# eightbit-bro

> He watches and knows all your 8-bit games.

eightbit-bro watches the HDMI output of a **Retro-Genesis 8-bit console** (NES-compatible,
"300 IN 1 VER2.0") at a hackerspace and knows, frame by frame, what the console is doing: no
signal, the game menu (which page, which game is highlighted), or a game. For Tetris it knows
the screen, the mode, start levels, pause, bonus, each player's score, lines and level, and who
topped out, and reports every finished game. Battle City and Dr. Mario are recognized by
screen.

It is a Rust rewrite of `tetris-recorder` (v1), NixOS-native, running on a Raspberry Pi 4
next to the console, with a web dashboard. Recording videos and posting them to Telegram are
next (see [Status](#status)).

- Product requirements: [`docs/PRD.md`](docs/PRD.md)
- Technical design: [`docs/TECH_DESIGN.md`](docs/TECH_DESIGN.md)

## How it works

```
 console ─HDMI─► MS2109 capture card ─USB─► Raspberry Pi 4
                                              │
   ffmpeg (MJPEG 1280×720 @ 30, copied as-is) │
                                              ▼
   capture ──► ring buffer (~4 s of JPEGs) ──► perception ──► reducer ──► events
                     │                        (pure)        (pure)       │
                     └──► dashboard preview                              ├─► SQLite
                                                                         ├─► review queue
                                                                         └─► dashboard (SSE)
```

1. **Capture** never waits for anything: it moves the card's JPEGs into a ring buffer.
2. **Perception** decodes a frame at ½ scale, maps it onto the console's own 256×240 pixel
   grid (the *NES canvas*) and asks the recognizers what it shows. Recognition is
   deterministic and "100 % or nothing": a frame is claimed by patterns that are always
   visible on that screen, or not at all. There are no confidence scores and no ML.
3. The **reducer** turns observations into the console state tree, e.g.
   `Menu/p14/c6/TETRIS II` or `InGame/Tetris/Playing/2P/clean/L3-5/P2 over`, and emits
   events when it changes. Frames nobody recognizes keep the previous state and go to the
   **review queue**.
4. The **dashboard** shows the live state, the events, the review queue and the device's
   health.

### What it recognizes

| | |
|---|---|
| Console | no signal; black and half-black transition frames |
| Menu | page, cursor row, highlighted game (static catalog of the menu's games: `Title` enum, 204 of 300 read so far) |
| Tetris | splash, game select (mode), level select (per player), handicap and music select, 1P / 2P / coop layout, pause, bonus and game over per player, score / lines / level per player, clean / dirty / demo sessions, a `GameFinished` event per game |
| Battle City | title, playing, base destroyed, score screen |
| Dr. Mario | menu, settings, 1P game (game over, stage clear), 2P game (P1 / P2 over) |
| Anything else | `InGame/Other`, carrying the title highlighted in the menu when it was launched |

### Teaching it new screens

Unknown screens land in the dashboard's **Review** page, deduplicated by similarity (at most
100 items). On any of them, or on a frame you capture from the **Live** page with
📸 *Capture frame for review*, draw a **POI**: a rectangle around what tells this screen
apart, with a comment. The dashboard shows how the region is optimized (trimmed and snapped to
the console's 8×8 tiles). POIs are turned into recognizer probes in code, with the frame
added as a test fixture (see [Adding recognition](#adding-recognition)).

## Hardware

- Raspberry Pi 4 (the current one has 1.8 GB RAM) with an SD card.
- MacroSilicon **MS2109** USB HDMI capture card (`534d:2109`), used in MJPEG 1280×720 at
  30 fps (it also does 60 and 50). It is opened by its stable id,
  `/dev/v4l/by-id/usb-MACROSILICON_USB3._0_capture-video-index0`.
- The console's picture spans x = 188…1098 of the 720p frame: 3.555 capture pixels per
  console pixel. Other capture setups need their own `geometry` (see
  [Configuration](#configuration)).

On the Pi the service uses about 0.6 of a core at 30 fps (~20 ms per frame) and 35–60 MB of
RAM.

## Repository

```
crates/
  bro-heart/     state tree, observations, events, reducer, Tetris state, menu titles. Pure.
  bro-vision/    NES canvas, geometry, probes, POI optimization
  bro-capture/   sources (capture card and video files via ffmpeg, image folders), ring
                 buffer, JPEG decoding (libjpeg-turbo, ½ scale)
  bro-games/     perception: menu reader and font, Tetris (screens, HUD digits), screen
                 tables (Battle City, Dr. Mario)
  bro-store/     SQLite: events, review queue, POIs
  bro-record/    constant-frame-rate timeline for recording (encoder not wired yet)
  bro-web/       dashboard and JSON API (axum, static pages, SSE)
  eightbit-bro/  the binary: config, capture and perception threads, pipeline
fixtures/frames/ real captured frames with expected results (expected.toml per folder)
docs/            PRD and technical design
nix/             package, NixOS module (services.eightbit-bro), Pi 4 appliance
deploy/          colmena hive, deploy script, hosts (real hosts are gitignored)
```

## Development

Everything comes from the flake:

```sh
nix develop                # Rust toolchain, clippy, rustfmt, ffmpeg, libjpeg-turbo, sqlite
cargo test                 # unit tests and the fixture tests
cargo clippy --all-targets -- -D warnings
cargo fmt
```

The fixture tests (`bro-games`) run every frame in `fixtures/frames/*/` through perception and
compare with its `expected.toml`: what it is (menu, Tetris, …), menu rows and cursor, Tetris
screen, layout, cursors, flags and HUD numbers. They also check that no frame is claimed by
two recognizers.

### Running locally

The binary takes one argument, a TOML config. To replay a folder of frames (sorted by name)
with the dashboard:

```toml
# dev.toml
data_dir = "/tmp/bro"

[input]
dir = "fixtures/frames/pi-2026-11-02"   # or: video = "game.mp4", or: device = "/dev/v4l/by-id/…"
fps = 2                                  # nominal rate of the folder

[web]
listen = "127.0.0.1:8080"
login = false
```

```sh
cargo run --release -p eightbit-bro -- dev.toml
```

The log shows every state change; the dashboard is at <http://127.0.0.1:8080>.

## Deployment

The Pi is a NixOS appliance (`nix/appliance-rpi4.nix`): mainline kernel, zram, Tailscale,
firewall, and the service with the capture card. Hosts live in `deploy/hosts/<name>.nix`:
copy `example.nix`; real host files are gitignored. Builds happen on the deploying machine,
which needs aarch64 emulation (`boot.binfmt.emulatedSystems = [ "aarch64-linux" ]`).

```sh
deploy/deploy.sh           # full NixOS deploy with colmena (~10 min, rollback via generations)
deploy/deploy.sh --dev     # dev push: incremental build, copy the binary, restart (~15 s – 1 min)
```

- Both report their progress to the dashboard (the *Deploy* panel on the Live page).
- A **dev push** builds with cargo in the aarch64 dev shell (cache in `target/pi`), copies the
  binary to `/var/lib/eightbit-bro-dev/` and runs it through a systemd override in `/run`. A
  reboot or the next full deploy goes back to the Nix-built version, so do a full deploy
  before leaving the Pi alone.
- A fresh box: `nix build .#packages.aarch64-linux.sd-image-rpi4`, flash it, plug in the
  capture card, boot, open the dashboard.

Logs: `journalctl -fu eightbit-bro` on the Pi (state changes, finished games, unknown
screens, and stats every minute).

### Configuration

`/etc/eightbit-bro/config.toml` is generated from `services.eightbit-bro.settings`:

| Key | Default | |
|---|---|---|
| `data_dir` | `/var/lib/eightbit-bro` | database, review queue, secrets |
| `input.device` / `input.dir` / `input.video` | | exactly one |
| `input.fps` | 60 | capture card mode (60 / 50 / 30), or the nominal rate of a folder or video |
| `geometry.picture` | `{ x1 = 188, y1 = 0, x2 = 1098, y2 = 720 }` | where the console picture is in the frame |
| `review.max_items` | 100 | review queue size |
| `web.listen` | `0.0.0.0:8080` (NixOS module; required otherwise) | without `[web]` the binary replays its input and exits |
| `web.login` | `true` | ask for the dashboard password (set on the first-boot setup page) |

The Pi runs with `input.fps = 30` and `web.login = false`. Keep the dashboard on the LAN /
Tailscale: it is not meant for the internet. Secrets (dashboard password, Telegram bot token
and channel) are entered in the dashboard, stored in `data_dir/secrets.json` (mode 0600),
and never shown again.

## Dashboard

- **Live:** console state, players' numbers during Tetris, a 10 fps preview, *Capture frame
  for review*, health (capture and perception fps, CPU temperature, memory, uptime, build),
  deploy progress, recent events.
- **Events:** the event log, live, filterable; finished games show their results.
- **Review:** unknown and captured screens; POIs with comments; mark reviewed / known
  non-issue; delete; delete all.
- **Settings:** secrets (write-only) and the declarative config.

Everything comes from a JSON API under `/api/` (`state`, `stream` (SSE), `events`, `review`,
`capture`, `preview.jpg`, `deploy`, …), so scripts can use it too.

## Adding recognition

1. Collect frames: from the review queue, or capture them on the Live page, and draw POIs with
   comments.
2. Copy the frames into a fixture folder (`fixtures/frames/<source>/`) and write what each
   must be recognized as in its `expected.toml`.
3. Cut probes from the reference frames at the POI rectangles: a probe compares which pixels
   are *drawn* (bright, lit or Battle City grey), tolerates a small share of differing pixels
   and a shift of ±2 pixels, because colours suffer from JPEG's halved chroma and the picture
   moves by a pixel between screens.
   - Tetris: `crates/bro-games/src/tetris.rs`.
   - Other games: a screen table in `crates/bro-games/src/screens.rs` (a few lines per
     screen), registered in `Perception::new`.
4. `cargo test`, then deploy.

Menu titles still missing from the catalog are logged by the service when it sees a complete
menu page (`menu page … unknown=…`); scrolling through the menu once fills them in.

## Status

Done: capture, perception and state for the menu, Tetris (including HUD numbers and game
results), Battle City and Dr. Mario; event log; review queue with POIs; dashboard; NixOS
appliance and deploys.

Next (PRD milestones M4–M6): recording the games to video (the timeline is built; the ffmpeg
encoder and its wiring are not), posting to Telegram, Super Mario and Jewelry, the rest of
the menu catalog, and faster full deploys (caching dependencies in Nix).
