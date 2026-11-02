// Shared helpers for the dashboard pages. No build step: plain browser JS.

async function api(path, opts = {}) {
  const init = { headers: {}, ...opts };
  if (opts.json !== undefined) {
    init.method = init.method || "POST";
    init.headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(opts.json);
  }
  const res = await fetch("/api" + path, init);
  if (res.status === 401 && path !== "/login") { location.href = "/login"; throw new Error("login required"); }
  if (res.status === 409 && !location.pathname.startsWith("/setup")) {
    const body = await res.clone().json().catch(() => ({}));
    if (body.error === "setup required") { location.href = "/setup"; throw new Error(body.error); }
  }
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(body.error || res.statusText);
  return body;
}

function nav(active) {
  const links = [["/", "Live"], ["/events", "Events"], ["/review", "Review"], ["/settings", "Settings"]];
  const el = document.createElement("nav");
  el.innerHTML = `<span class="brand">👾 eightbit-bro</span>` +
    links.map(([href, name]) => `<a href="${href}" class="${href === active ? "on" : ""}">${name}</a>`).join("") +
    `<span class="spacer"></span><a href="#" id="logout" class="muted small">Log out</a>`;
  document.body.prepend(el);
  fetch("/api/auth").then((r) => r.json()).then((a) => {
    if (!a.login) el.querySelector("#logout").hidden = true;
  }).catch(() => {});
  el.querySelector("#logout").onclick = async (e) => {
    e.preventDefault();
    await api("/logout", { json: {} }).catch(() => {});
    location.href = "/login";
  };
}

function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

function when(ms) {
  if (!ms) return "";
  const d = new Date(ms);
  return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function ago(ms) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

function players(ps, layout) {
  const n = layout === "Single" ? 1 : 2;
  return ps.slice(0, n).map((p, i) =>
    `P${i + 1} <b>${p.score ?? "?"}</b> (${p.lines ?? "?"} lines${p.level != null ? `, L${p.level}` : ""})`).join(" · ");
}

function describe(kind) {
  if (kind.GameFinished) {
    const r = kind.GameFinished.result;
    const layout = { Single: "1P", Double: "2P", Coop: "coop" }[r.layout] ?? "";
    const level = r.start_level.filter((l) => l != null).join("–");
    const reason = { GameOver: "game over", NewGame: "abandoned", Left: "left the game" }[r.reason] ?? r.reason;
    const over = r.topped_out.map((t, i) => (t ? `P${i + 1} topped out` : null)).filter(Boolean).join(", ");
    return `🏁 <b>${esc(kind.GameFinished.game)} ${layout}</b> finished: ${reason} · ${r.session.toLowerCase()}` +
      `${level ? ` · start L${level}` : ""}<br>${players(r.players, r.layout)}${over ? ` · ${over}` : ""}`;
  }
  if (kind.Changed) return `${esc(kind.Changed.from)} → <b>${esc(kind.Changed.to)}</b>`;
  if (kind.UnknownScreen) return `unknown screen in ${esc(kind.UnknownScreen.state)} → <a href="/review/${kind.UnknownScreen.item}">review #${kind.UnknownScreen.item}</a>`;
  return esc(JSON.stringify(kind));
}

function stream(handlers) {
  const es = new EventSource("/api/stream");
  for (const [type, fn] of Object.entries(handlers)) {
    es.addEventListener(type, (e) => fn(JSON.parse(e.data)));
  }
  return es;
}
