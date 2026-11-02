#!/usr/bin/env python3
"""Deploy to a host with colmena and show the progress on that host's dashboard.

    deploy/deploy.py [host] [colmena apply args...]     (default host: nix-tetris-recorder)

Runs `colmena apply -f deploy/hive.nix --on <host>` from this checkout, prints its output,
and every few seconds writes a status file onto the host over SSH (the same access colmena
uses). The dashboard's Live page shows it: phase, crates compiled, elapsed time, commit,
result. The build itself runs here (aarch64 under emulation), so the host can't see it
otherwise.
"""
import json, os, subprocess, sys, threading, time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
STATUS_PATH = "/var/lib/eightbit-bro/deploy.json"
CACHE = Path.home() / ".cache" / "eightbit-bro"


def git(*args):
    return subprocess.run(["git", "-C", str(REPO), *args], capture_output=True, text=True).stdout.strip()


def target_host(host):
    expr = f'(import {REPO}/deploy/hosts/{host}.nix {{ pkgs = null; lib = null; config = null; }}).deployment.targetHost'
    out = subprocess.run(["nix-instantiate", "--eval", "--json", "-E", expr], capture_output=True, text=True)
    return json.loads(out.stdout) if out.returncode == 0 else host


def main():
    host = sys.argv[1] if len(sys.argv) > 1 else "nix-tetris-recorder"
    extra = sys.argv[2:]
    target = target_host(host)
    count_file = CACHE / f"{host}.crates"
    status = {
        "host": host,
        "commit": git("rev-parse", "--short", "HEAD"),
        "subject": git("log", "-1", "--format=%s"),
        "dirty": bool(git("status", "--porcelain")),
        "by": f"{os.environ.get('USER', '?')}@{os.uname().nodename}",
        "started": int(time.time() * 1000),
        "updated": 0,
        "phase": "evaluating",
        "compiled": 0,
        # From the last deploy; there is no cheap way to know it in advance.
        "total": int(count_file.read_text()) if count_file.exists() else None,
        "last_line": "",
        "result": None,
        "finished": None,
    }
    lock = threading.Lock()

    def publish():
        with lock:
            status["updated"] = int(time.time() * 1000)
            data = json.dumps(status)
        # Write then rename, so the dashboard never reads half a file. Best effort: the host
        # may be restarting.
        subprocess.run(
            ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", f"root@{target}",
             f"cat > {STATUS_PATH}.tmp && chmod 644 {STATUS_PATH}.tmp && mv {STATUS_PATH}.tmp {STATUS_PATH}"],
            input=data, text=True, capture_output=True)

    done = threading.Event()

    def publisher():
        while not done.wait(3):
            publish()

    threading.Thread(target=publisher, daemon=True).start()
    publish()

    cmd = ["colmena", "apply", "-f", str(REPO / "deploy" / "hive.nix"), "--on", host, *extra]
    proc = subprocess.Popen(cmd, cwd=REPO, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
    for line in proc.stdout:
        sys.stdout.write(line)
        text = line.split("|", 1)[-1].strip()
        with lock:
            if "Compiling " in text:
                status["phase"] = "building"
                status["compiled"] += 1
            elif "Building" in text or "building '" in text:
                status["phase"] = "building"
            elif "Pushing system closure" in text:
                status["phase"] = "copying to the Pi"
            elif "Activating system profile" in text:
                status["phase"] = "activating"
            if text and "copying path" not in text:
                status["last_line"] = text[:200]
    code = proc.wait()
    done.set()
    with lock:
        status["result"] = "ok" if code == 0 else f"failed (exit {code})"
        status["phase"] = "done" if code == 0 else "failed"
        status["finished"] = int(time.time() * 1000)
        if code == 0 and status["compiled"]:
            CACHE.mkdir(parents=True, exist_ok=True)
            count_file.write_text(str(status["compiled"]))
    publish()
    sys.exit(code)


if __name__ == "__main__":
    main()
