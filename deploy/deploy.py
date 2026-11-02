#!/usr/bin/env python3
"""Deploy to a host and show the progress on that host's dashboard.

    deploy/deploy.py [host] [colmena apply args...]   full deploy with colmena (~10 min)
    deploy/deploy.py --dev [host]                     dev push: incremental build, copy, restart

Full: runs `colmena apply -f deploy/hive.nix --on <host>` from this checkout; the system and
the package are built by Nix (aarch64 under emulation), with rollback via NixOS generations.

Dev: builds only what changed with cargo in the aarch64 dev shell (cache in target/pi),
copies the binary to /var/lib/eightbit-bro-dev/ and points the service at it with a systemd
override in /run, so a reboot or the next full deploy returns to the Nix-built version. The
binary links to the same pinned nixpkgs as the host, so its libraries are already there;
`nix copy` sends any that are missing.

Both write their progress to /var/lib/eightbit-bro/deploy.json on the host over SSH (the
access colmena uses); the dashboard's Live page shows it.
"""
import json, os, re, subprocess, sys, threading, time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
STATUS_PATH = "/var/lib/eightbit-bro/deploy.json"
DEV_DIR = "/var/lib/eightbit-bro-dev"
DROP_IN = "/run/systemd/system/eightbit-bro.service.d/dev.conf"
CACHE = Path.home() / ".cache" / "eightbit-bro"


def git(*args):
    return subprocess.run(["git", "-C", str(REPO), *args], capture_output=True, text=True).stdout.strip()


def target_host(host):
    expr = f'(import {REPO}/deploy/hosts/{host}.nix {{ pkgs = null; lib = null; config = null; }}).deployment.targetHost'
    out = subprocess.run(["nix-instantiate", "--eval", "--json", "-E", expr], capture_output=True, text=True)
    return json.loads(out.stdout) if out.returncode == 0 else host


class Deploy:
    """Progress of one deploy, published to the host every few seconds."""

    def __init__(self, host, kind):
        self.host, self.target = host, target_host(host)
        self.lock = threading.Lock()
        self.count_file = CACHE / f"{host}.{kind}.crates"
        self.status = {
            "host": host,
            "kind": kind,
            "commit": git("rev-parse", "--short", "HEAD"),
            "subject": git("log", "-1", "--format=%s"),
            "dirty": bool(git("status", "--porcelain")),
            "by": f"{os.environ.get('USER', '?')}@{os.uname().nodename}",
            "started": int(time.time() * 1000),
            "updated": 0,
            "phase": "evaluating",
            "compiled": 0,
            # From the last deploy of this kind; there is no cheap way to know in advance.
            "total": int(self.count_file.read_text()) if self.count_file.exists() else None,
            "last_line": "",
            "result": None,
            "finished": None,
        }
        self.done = threading.Event()
        threading.Thread(target=self._loop, daemon=True).start()
        self.publish()

    def ssh(self, command, **kw):
        return subprocess.run(["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", f"root@{self.target}", command], **kw)

    def publish(self):
        with self.lock:
            self.status["updated"] = int(time.time() * 1000)
            data = json.dumps(self.status)
        # Write then rename, so the dashboard never reads half a file. Best effort: the host
        # may be restarting.
        self.ssh(f"cat > {STATUS_PATH}.tmp && chmod 644 {STATUS_PATH}.tmp && mv {STATUS_PATH}.tmp {STATUS_PATH}",
                 input=data, text=True, capture_output=True)

    def _loop(self):
        while not self.done.wait(3):
            self.publish()

    def set(self, **fields):
        with self.lock:
            self.status.update(fields)

    def run(self, cmd, cwd=REPO):
        """Runs a command, echoing and parsing its output. Returns the exit code."""
        proc = subprocess.Popen(cmd, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
        for line in proc.stdout:
            sys.stdout.write(line)
            text = line.split("|", 1)[-1].strip()
            with self.lock:
                if "Compiling " in text:
                    self.status["phase"] = "building"
                    self.status["compiled"] += 1
                elif "Building" in text or "building '" in text:
                    self.status["phase"] = "building"
                elif "Pushing system closure" in text:
                    self.status["phase"] = "copying to the Pi"
                elif "Activating system profile" in text:
                    self.status["phase"] = "activating"
                if text and "copying path" not in text:
                    self.status["last_line"] = text[:200]
        return proc.wait()

    def finish(self, code):
        self.done.set()
        with self.lock:
            self.status["result"] = "ok" if code == 0 else f"failed (exit {code})"
            self.status["phase"] = "done" if code == 0 else "failed"
            self.status["finished"] = int(time.time() * 1000)
            if code == 0 and self.status["compiled"]:
                CACHE.mkdir(parents=True, exist_ok=True)
                self.count_file.write_text(str(self.status["compiled"]))
        self.publish()
        sys.exit(code)


def full(host, extra):
    d = Deploy(host, "full")
    code = d.run(["colmena", "apply", "-f", str(REPO / "deploy" / "hive.nix"), "--on", host, *extra])
    if code == 0:
        # Drop a dev push's override, so the Nix-built version runs.
        d.ssh(f"if [ -e {DROP_IN} ]; then rm {DROP_IN} && systemctl daemon-reload && systemctl restart eightbit-bro; fi")
    d.finish(code)


def dev(host):
    d = Deploy(host, "dev")
    shell = ["nix", "develop", f"{REPO}#devShells.aarch64-linux.default", "-c"]
    code = d.run([*shell, "env", "CARGO_TARGET_DIR=target/pi", "cargo", "build", "--profile", "pi-dev", "-p", "eightbit-bro"])
    if code != 0:
        d.finish(code)
    binary = REPO / "target" / "pi" / "pi-dev" / "eightbit-bro"

    d.set(phase="copying to the Pi", last_line="libraries")
    elf = subprocess.run(["nix", "shell", "nixpkgs#patchelf", "-c", "sh", "-c",
                          f"patchelf --print-interpreter {binary}; patchelf --print-rpath {binary}"],
                         capture_output=True, text=True).stdout
    paths = sorted(set(re.findall(r"/nix/store/[a-z0-9]{32}-[^/:\s]+", elf)))
    code = d.run(["nix", "copy", "--to", f"ssh-ng://root@{d.target}", *paths])
    if code != 0:
        d.finish(code)

    d.set(last_line="binary")
    with open(binary, "rb") as f:
        r = d.ssh(f"mkdir -p {DEV_DIR} && cat > {DEV_DIR}/eightbit-bro.new && chmod 755 {DEV_DIR}/eightbit-bro.new "
                  f"&& mv {DEV_DIR}/eightbit-bro.new {DEV_DIR}/eightbit-bro", stdin=f)
    if r.returncode != 0:
        d.finish(r.returncode)

    d.set(phase="activating", last_line="restarting with the dev binary")
    override = f"[Service]\\nExecStart=\\nExecStart={DEV_DIR}/eightbit-bro /etc/eightbit-bro/config.toml\\n"
    r = d.ssh(f"mkdir -p {os.path.dirname(DROP_IN)} && printf '{override}' > {DROP_IN} "
              "&& systemctl daemon-reload && systemctl restart eightbit-bro && sleep 2 && systemctl is-active eightbit-bro")
    d.set(last_line="dev binary running" if r.returncode == 0 else "service failed to start")
    d.finish(r.returncode)


def main():
    args = sys.argv[1:]
    if args and args[0] == "--dev":
        dev(args[1] if len(args) > 1 else "nix-tetris-recorder")
    else:
        full(args[0] if args else "nix-tetris-recorder", args[1:])


if __name__ == "__main__":
    main()
