# eightbit-bro — Technical Design

| | |
|---|---|
| Status | Draft v0.3 (2026-11-01) |
| Implements | `docs/PRD.md` v0.4 |

## 1. Overview

One Rust binary, `eightbit-bro`, running as a NixOS service on the Raspberry Pi 4 next to the
console. It captures MJPEG frames from the HDMI capture card, turns each frame into a small
**NES canvas**, runs **pure** recognition on it to get the console state tree, and publishes
state-change events. The recorder, SQLite store, Telegram poster and web dashboard all consume
those events.

```
            ┌──────────────── eightbit-bro (one process) ────────────────┐
 MS2109 ──► │ capture ──► ring buffer (JPEG, ~4 s) ──────────► recorder ──┼─► ffmpeg ─► video.mkv
 /dev/v4l   │    │            │ latest frame                     ▲       │
            │    ▼            ▼                                  │       │
            │ perceive ──► reduce (pure) ──► events ──┬──────────┘       │
            │ (pure)        prev state                ├─► store (SQLite)  │
            │                                         ├─► telegram queue  │
            │                                         └─► web (SSE)       │
            └────────────────────────────────────────────────────────────┘
```

Key ideas:

1. **Capture is sacred.** It only moves compressed JPEG buffers into a ring buffer. It never
   waits for anyone.
2. **Recording doesn't depend on recognition speed.** The recorder reads frames straight from
   the ring buffer. Recognition only decides *which frame numbers* are in the video (start,
   pause ranges, end). If recognition falls behind or skips frames, the video still gets every
   frame.
3. **Perception and state are pure functions.** `perceive(frame) → Observation` and
   `reduce(prev, observation) → (state, events)`. No IO, no clocks. That gives the
   determinism the PRD asks for (R2) and makes everything testable from files.
4. **Recognize on the NES canvas, not on HD pixels.** The console draws 256×240 pixels from a
   fixed palette and fixed 8×8 tiles. If we map the capture back to that grid, recognition
   becomes near-exact tile and colour matching, which is fast, independent of resolution,
   and fits the "100% or nothing" rule (R1).

## 2. Measurements (2026-11-01, on the Pi)

Hardware: Raspberry Pi 4 Model B Rev 1.4, 4× Cortex-A72, 1.8 GB RAM (~1 GB free), no swap,
118 GB SD card. NixOS 24.11, kernel 6.6.31, ffmpeg with `libx264` and `h264_v4l2m2m`.
Capture card: MacroSilicon **MS2109** (`534d:2109`), runs on USB 2.0 (480 Mbit), UVC, with
audio (ALSA card `MS2109`).

**Capture device.** After a USB re-authorize (`authorized` 0→1) the stable path is
`/dev/v4l/by-id/usb-MACROSILICON_USB3._0_capture-video-index0`. v1 died because `/dev/video0`
went stale after a re-enumeration. v2 opens the device by id, and re-opens it when it
disappears.

**Delivered frame rate** (`v4l2-ctl --stream-mmap`, 240 frames, console showing the menu / Jewelry):

| Mode (MJPEG) | Delivered fps | Avg JPEG size | Corrupt frames |
|---|---|---|---|
| 1920×1080 @60 | 59.0 | 92 KB | **every other frame** (300/600) → effectively 30 fps |
| 1920×1080 @30 | 29.8 | 219 KB | 0 |
| **1280×720 @60** | **59.3** | 140 KB | **0** |
| 720×480 @60 | 56.9 | 88 KB | not checked |
| 640×480 @60 | 57.8 | 88 KB | not checked |

YUYV (raw) is useless: ≤10 fps at 720p.

At 720p the console picture spans **x = 188…1098 (910 px), full height**: exactly
**3 capture lines per NES line** (720 / 240) and **3.555 px per NES pixel** horizontally,
nearest-neighbour scaled (columns 3 or 4 px wide). Measured from the pixel edges of menu text
(D7); the visible pillarbox edge (≈192…1088) is not the picture edge. → **Chosen capture mode:
MJPEG 1280×720 @60.**

The very first frame after a mode switch can be garbage (two half-images glued together),
so frames whose decode fails are counted and dropped.

**Frame uniqueness at 60 fps: confirmed.** A 20 s capture of the Tetris demo (1P layout,
level 13) at 60.05 fps had 0 corrupt frames out of 1,200. The playfield changes every 4 frames
(the falling piece's gravity at that level), and some changes land on *consecutive* frames
(e.g. frames 23 and 24), which a card duplicating 30→60 fps couldn't produce. So the card
really delivers 60 unique frames per second at 720p.

**CPU cost on the Pi** (ffmpeg, 600 frames = 10 s of 720p60 capture):

| Operation | Wall time for 10 s of video | CPU-seconds per 10 s | ≈ cores at 60 fps |
|---|---|---|---|
| MJPEG decode, full 1280×720, 1 thread | 5.1 s | 5.2 | 0.5 |
| MJPEG decode at ¼ scale (DCT scaling, 320×180) | 2.8 s | 3.3 | 0.33 |
| Decode + `libx264 ultrafast` 1280×720 | 6.0 s | 19.1 | 1.9 |
| Decode + `libx264 veryfast` 1280×720 | 8.0 s | 26.9 | 2.7 |
| Decode + `h264_v4l2m2m` (HW encoder) | 6.1 s | 18.8 | 1.9 (the conversion in software dominates) |
| Remux MJPEG → MKV (no transcode) | 0.8 s | 1.3 | 0.13 |

Temperature after the run: 62.8 °C.

**Estimated budget** (to confirm with the Rust prototype in M0):

| Stage | 60 fps output | 30 fps output |
|---|---|---|
| Capture (UVC, memcpy) | ~0.05 | ~0.05 |
| Perception: ½-scale decode + canvas + recognizers | ~0.35 | ~0.35 (perception always runs at 60) |
| Encode: decode + x264 ultrafast, cropped 896×720 (70% of the pixels) | ~1.3 | ~0.7 |
| Web, SQLite, Telegram | ~0.1 | ~0.1 |
| **Total (of 4 cores)** | **~1.8** | **~1.2** |

**Conclusion: everything fits on the Pi.** The chosen 30 fps output (D1) has a lot of headroom;
60 fps would also fit with about half the CPU free. The fenek split (PRD §8.4) stays a
fallback; the design keeps it possible (§10) but we don't build it in v2.0.

## 3. Frame pipeline

### 3.1 Frames and sources

```rust
pub struct RawFrame {
    pub no: u64,             // monotonically increasing from source start
    pub ts: Duration,        // capture timestamp (V4L2 buffer time, or synthetic for files)
    pub data: FrameData,
}
pub enum FrameData { Jpeg(Bytes), Rgb(Arc<RgbImage>) }

pub trait Source: Send { fn next(&mut self) -> Result<Option<RawFrame>>; }
```

| Source | How | Timestamps |
|---|---|---|
| `FfmpegSource::device` | `ffmpeg -f v4l2 -input_format mjpeg … -c:v copy -f mjpeg -`: the card's own JPEGs, split at SOI/EOI markers (~0.13 core); re-opened by id after errors, frame numbers keep increasing | arrival time (a direct V4L2 reader with kernel timestamps can replace it if jitter matters) |
| `DirSource` | sorted `*.jpg` / `*.png` | `no / nominal_fps` |
| `FfmpegSource::video` | `ffmpeg -i file -c:v mjpeg -f mjpeg -` (JPEG stream) | `no / video_fps` |

File sources use synthetic timestamps, so offline runs are byte-for-byte reproducible (PRD I2).
The same code path also handles v1's PNG captures (`downloaded_games/`).

**NoSignal.** The source reports `SignalLost` when the device disappears or delivers no frames
for 1 s. What the MS2109 outputs with the console off (black, blue, frozen) is still to check
(PRD Q7). If it's a recognizable pattern, the console-level perception classifies it as
`NoSignal` directly.

### 3.2 Ring buffer

A ring of the last **~4 s of JPEG frames** (240 × ~140 KB ≈ 34 MB) of `Arc` frames behind a
mutex held only for a push or a lookup (no copying, no IO under the lock), so the recorder, perception and the dashboard preview share frames without copying. When the
reducer says "a session started at frame N" a few frames late, the recorder starts from
frame N out of the ring. This is how the PRD requirement "the video starts at the first
gameplay frame" is met.

### 3.3 NES canvas (normalization)

```rust
pub struct NesCanvas {           // 256 × 240
    pub rgb: Box<[[u8; 3]]>,     // colour at the centre of each console pixel
}
```

1. Decode the JPEG at **½ scale** with libjpeg-turbo DCT scaling (640×360 → the picture area is
   455×360, i.e. 1.78 × 1.5 px per NES pixel).
2. Use the **geometry calibration** (picture rectangle per capture mode, in config, verified by
   fixtures) to sample the centre of each NES pixel.

Cost on the Pi: ~11 ms per frame in total, 60 fps with ~0.7 core.

**Match shapes on brightness, not colour.** JPEG 4:2:2 halves the colour resolution, and the
console's scaler shifts the picture by fractions of a line between screens, so colours at
edges are unreliable. Probes compare *which pixels are bright* (luma > 150), allow a small
share of differing pixels and a vertical shift of up to 2 lines. There is no palette
quantization. Every recognizer is tested against real captured JPEGs.

### 3.4 Perception (stateless)

```rust
pub struct Observation {
    pub kind: Seen,                // Black | Menu(MenuObs) | Game(GameId, GameObs) | Unrecognized
    pub frame_no: u64,
}
pub trait GameRecognizer: Send + Sync {
    fn id(&self) -> GameId;
    /// Cheap identity check: patterns that are always visible in this game.
    fn claims(&self, c: &NesCanvas) -> bool;
    /// Game-specific observation (for games with deep recognition).
    fn observe(&self, c: &NesCanvas) -> GameObs { GameObs::None }
}
```

- **Black:** almost all pixels are black, **or blanked**: a run of ≥ 48 fully black lines.
  Games switch rendering off while loading the next screen; a frame caught while rendering
  switches on/off partway down is part picture, part black (seen between menu and Tetris).
- **Menu:** the "300 IN 1 VER2.0" header (bright mask, ≤ 12 % off) and ≥ 50 % of the list area
  in the menu's blue. The scenery below scrolls and the windmills turn, so they are not used.
  The menu is then **read on a full-resolution decode** (at ½ scale some glyphs differ by one
  pixel): text cells at x = 7 + 8j, y = 32 + 8·row plus a per-frame offset (the menu scrolls by
  a line between screens), each matched against the 42-glyph menu font (`bro-games/src/font.rs`,
  ≤ 1 pixel off and unambiguous, else `?`), and the red arrow in cell column 2. The page comes
  from the number on the cursor's row; `highlighted` is the catalog `Title` when number and
  name both match (`bro-heart/src/title.rs`). Pages are redrawn row by row, so a frame can show
  two pages; only rows whose number fits their position count. The full decode runs only when
  the list area changes.
- **Games:** each recognizer has signature probes (Tetris: the "SCORE"/"LINES"/"LEVEL" labels,
  identical in gameplay, game select and level select). `claims()` must hold on **every**
  screen of that game we know about, or the recognizer adds more signatures.
- **Hint for speed:** `perceive(canvas, hint: Option<GameId>)` tries the hinted game first.
  **Invariant, tested:** the hint changes speed, never the result. `perceive(c, Some(g)) ==
  perceive(c, None)` for every fixture. That's how PRD R3 ("stateless classification
  exists") is enforced.
- **Exclusivity, tested:** on every fixture at most one recognizer claims the frame. If two
  claim, that's a test failure, not a runtime tie-break.

### 3.5 Reducer (state machine)

```rust
pub enum ConsoleState {
    NoSignal,
    Menu { page: u8, cursor: u8, highlighted: Option<CatalogId> },
    InGame { game: GameId, detail: GameDetail, session: SessionMeta },
}
pub fn reduce(prev: &ConsoleState, obs: &Observation, games: &Games)
    -> (ConsoleState, Vec<Event>);
```

- Console-level rules come straight from PRD §5.2 R4/R5 (the inheritance table), in one
  `match` with one test per row.
- Each deep game has its own `reduce_game(prev_detail, game_obs) -> (detail, events)`, e.g.
  Tetris `Title → ModeSelect → LevelSelect → Playing → GameOver`, with demo detection
  (`Title → Playing` without ModeSelect) and clean/dirty (cold start into `Playing` is dirty).
- Events are generated by **diffing** the old and new trees, level by level, so every change
  becomes an event without hand-writing each one. Session events (`SessionStarted`,
  `PlayerToppedOut`, `SessionEnded{reason}`, …) are emitted by the game reducers.
- Unrecognized frames go to the **review queue** (`bro-store`), not into the reducer's
  events. The queue deduplicates by a 64-bit dHash of the canvas (≤ 6 bits apart = same
  screen; repeats are counted in memory and written at most every 5 s), holds at most 100
  items, evicts the oldest reviewed/ignored item when full and otherwise skips (counted).
  Each new item emits one `UnknownScreen{item, state}` event.

### 3.6 POI annotations (review queue → probes)

The owner draws a **POI** on a review-queue frame in the dashboard: a rectangle `(x1, y1, x2,
y2)` in the frame's capture pixels (half-open: `x1 ≤ x < x2`), the part of the screen that
tells this frame apart, plus a free-text comment for the agent. A frame can have several POIs.

On save the server **optimizes** the rectangle (`bro_vision::optimize_poi`):

1. Map it to the NES canvas through the geometry calibration, rounding outwards.
2. Trim it to the bounding box of the content inside it: pixels that differ from the region's
   dominant (background) colour. A uniform region is kept as drawn: a plain area can be a
   signature too.
3. Snap it outwards to the **8×8 tile grid**. Backgrounds and text are drawn in tiles, so a
   tile-aligned probe compares whole tiles and stays small.

Both rectangles are stored and drawn over the frame, so the owner sees what will be checked.
The agent reads the annotations through the API (`GET /api/review/{id}`: frame JPEG, canvas
PNG, POIs, comments) and turns each optimized POI into a `Probe`: the bright mask inside it,
cut from a reference fixture, with a capped share of differing pixels (R1).

### 3.6b Screen tables (Battle City, Dr. Mario)

Games without a dedicated module are a **table of screens** (`bro-games/src/screens.rs`):
each screen is one or more probes that must all match, plus flags checked on that screen;
the game is claimed by identity probes or any screen. The state is the last known screen and
its flags (`InGame/DrMario/Playing 2P/P1 over`); an unknown screen keeps it and goes to the
review queue. Adding a game or a screen is a few rows, from the owner's POIs.

- **Battle City:** Title (settled; it scrolls up first), Score, Playing / Base destroyed. The
  game is identified by the dark grey frame around the playfield (a `Grey` ink: (63,63,63)
  ±22), and the eagle at the bottom tells playing from lost by its grey shape.
- **Dr. Mario:** Menu, Settings, Playing 1P (TOP/SCORE clipboard, bright, so the coloured
  checkerboard doesn't count, plus the bottle) with "game over", Playing 2P with "P1 over" /
  "P2 over". Its PAUSE screens are black frames.

### 3.7 Tetris module

Status: screens, flags, HUD numbers and game results done.

- **Identity:** the SCORE / LINES / LEVEL labels (owner's POI), identical in gameplay, game
  select and level select, in both layouts.
- **Screens** (`TetrisObs.screen`), in the console's order: the splash ("1993 TETRIS GAME",
  also claims Tetris: the demo starts here); game select (mode under the arrow); level select
  (level 0–9 per player: P1's arrow left of the digits, P2's right); handicap select
  (0/3/6/9/12 per player); music select; playing (layout found); otherwise `Unknown`, and the
  frame goes to the review queue. All from the owner's POIs on live frames.
- **Recognition before "blanked":** the splash is mostly black, so a frame is treated as a
  blanked transition only when no recognizer claims it.
- **Layout:** STATS (POI) means one player or the demo; NEXT on the right means two wells.
- **Flags:** PAUSE box; GAME OVER box per well; BONUS per well. An element can have several
  reference probes (live 720p and v1 1080p) and shows if any matches: the 2P layout's right
  NEXT is 8 px further right on the live capture than on v1's, BONUS ~6 px left.
- **Probes** compare the *lit* mask (any channel > 100) on the black background, so red
  (NEXT, BONUS) and dim blue text (GAME SELECT) count; ≤ 10 % differing pixels within ±2 px.
  The labels allow 20 %: probes cut from the Pi differ by ~11 % on v1's captures.
- **State** (`bro_heart::tetris`): the mode and start level highlighted when leaving the
  select screens; the session is **clean** after the select screens, **dirty** on a cold
  start, **demo** when play starts without them (PRD T3, T4). Game over is sticky per player
  (T7). Unknown screens keep the state.
- **HUD numbers** (`bro-games/src/hud.rs`): score, lines and level per player, read on the
  full-resolution canvas (at ½ scale some digits blur together) only when the HUD area of the
  ½-scale canvas changes. The HUD has its own font (its "1" has a base serif): 10 digit
  glyphs, learned from the Pi frames (466 cells, all exact). Cells are 8 px at x ≡ 7 (mod 8):
  P1's numbers end at x = 95, P2's start at x = 151; rows y = 16 / 32 / 48. P2 is read only
  in 2P and coop (in 1P those cells hold HIGH SCORE / STATS); coop has no level row. A cell
  that isn't within one pixel of exactly one digit makes the number `None`, never guessed.
- **Results** (PRD T7, T8, E3): numbers freeze when a player tops out; the game ends when the
  well tops out (1P, coop) or both do (2P), or when the console goes back to the select
  screens / splash, the menu, another game or no signal. Each game is reported once as a
  `GameFinished` event: reason, session (clean / dirty / demo), layout, mode, start levels,
  handicaps, each player's score / lines / level and top-out.
- **v1 fixtures** (`fixtures/frames/v1-1080p`, JPEG) carry v1's assertions in
  `expected.toml`: Tetris or not, 2P, pause, bonus, game over per player. Not ported: the
  scores (v1's 1080p capture renders the digits differently, with drifting spacing; the live
  720p frames carry the score tests instead) and the 1,500-frame captured sequence.

## 4. Recorder

### 4.1 Decisions from events

The recorder subscribes to events and keeps a **cut list** in frame numbers:
`start = first Playing frame`, `exclude [pause_start, pause_end)`, `end = game over + 2 s tail`.
It pulls frames from the ring buffer in order and feeds the encoder. Because recognition may
run slightly behind, the recorder waits until the reducer has processed frame N before deciding
whether frame N is in the video (a small lag, bounded by the ring size).

### 4.2 Constant frame rate

The output has a fixed tick of 1/30 s (D1). For each tick the recorder takes the newest
included frame with `ts ≤ tick` (after subtracting cut-out pause time). No new frame →
duplicate the previous one; several → drop the extras. Counts of captured, dropped and
duplicated frames go into the session record (PRD F2/F3). At a 30 fps output this simply takes
every other frame of the 60 fps capture; those unused frames are expected and counted
separately. Frames lost before the recorder (V4L2 sequence gaps, corrupt JPEGs) are counted by
capture. Paused time is cut out by shifting the timestamps after a pause back by its length, so
a pause produces neither duplicates nor a jump.

### 4.3 Encoder

v2.0 uses an **ffmpeg subprocess**, fed JPEG bytes on stdin:

```
ffmpeg -f mjpeg -framerate 30 -i pipe:0 \
       -vf crop=896:720:192:0 -c:v libx264 -preset ultrafast -crf 20 -g 120 \
       -pix_fmt yuv420p session.mkv
```

- Runs at `nice 5`, so it can never starve capture or perception.
- MKV while recording (survives a crash), then a quick remux to MP4 with `+faststart` for
  Telegram (~1 s).
- If the encoder can't keep up, the queue between recorder and ffmpeg is bounded by the ring
  buffer. On overflow we drop frames (counted) instead of blocking capture.
- **Output look: A**, the cropped pass-through at 896×720 (D2). The canvas re-renders B (3×,
  768×720) and C (2×, 512×480) were blurry in the samples.
- Later: in-process libav, sharing decoded frames with perception (saves one decode), or the
  Pi's HW encoder with zero-copy, but only if profiling says we need it.
- Demo / too-short sessions: the partial file is deleted.

## 5. Storage

SQLite (`rusqlite`, WAL), one writer thread fed by a channel, embedded migrations.

| Table | Contents |
|---|---|
| `events` | id, ts, frame_no, kind, state_path, payload (JSON) |
| `sessions` | game, menu_selection, mode, start_level, clean, demo, cpu, started/ended (ts + frame), end_reason, video_path, frame stats |
| `session_players` | session, slot, score, lines, level, topped_out_frame |
| `posts` | session, status (queued/posted/failed), attempts, next_attempt, last_error, message_id |
| `review_items` | id, first/last seen, count, phash, state context (JSON), image path, status, note |
| `review_pois` | id, item, drawn rect (capture px), optimized rect (canvas px), comment, created_at |
| `settings` | key, value, updated_at (runtime settings only) |

Data dir `/var/lib/eightbit-bro/`: `bro.sqlite`, `videos/`, `review/` (JPEG plus a canvas PNG
per item, capped by size), `captures/` (raw frame dumps from the dashboard, capped).
Videos rotate by a disk budget (default 20 GB). The database is never rotated.

## 6. Telegram

`reqwest` multipart `sendVideo` to the Bot API (no bot framework needed: we only post).
The poster loop reads due rows from `posts`, retries with exponential backoff (1 min … 6 h),
and records the `message_id`. Missing or invalid token → the poster is disabled with an error
on the dashboard; everything else runs (PRD TG3). The caption comes from the session row.

## 7. Web dashboard

- **Stack:** `axum` serving static HTML pages with a little vanilla JS over the JSON API,
  plus Server-Sent Events. Assets are embedded in the binary (`include_str!`). No JS build,
  no Node, works on a phone.
- **Live:** the state tree via SSE on each change. The preview shows the **latest JPEG from the
  ring buffer as-is**, so there's no decoding. 1–2 fps by default, with an optional overlay
  image of the probe regions rendered on demand.
- **Pages:** Live, Events (paged queries plus SSE tail), Sessions (video via HTTP range
  requests, post status, retry), Errors and Review (thumbnails, draw POIs with
  comments (§3.6), annotate, mark reviewed, download as zip), Health, Settings, Tools:
  classify an uploaded image, replay a folder/video, capture N raw frames for fixtures.
- **Replay** runs a second, offline pipeline in a sandbox (separate in-memory state and DB,
  no Telegram, no effect on live recording) at reduced priority, and shows the resulting
  timeline.
- **API:** every page is backed by JSON endpoints under `/api/` (state, events, sessions,
  review, settings, health) plus `/api/stream` (SSE). The HTML is a thin layer over it.
- **Auth:** one static password → signed session cookie. On first boot (no password yet) the
  dashboard shows a **setup page** that sets the password and the Telegram bot token and
  channel. Listens on LAN/Tailscale (firewall: only `tailscale0` plus the LAN interface). No
  internet exposure.
- **Secrets** (dashboard password hash, bot token) live in a root-only file in the data dir,
  written by the dashboard, never in the Nix store or git. They are **write-only**: the UI shows
  "set / not set" and lets you replace them, never displays them.
- **Settings:** declarative values from Nix are shown read-only; runtime settings live in the
  `settings` table, apply live, and every change is logged as an event.

## 8. Concurrency and resource limits

| Thread / task | Priority | Notes |
|---|---|---|
| capture (std thread) | highest (process `Nice=-5`) | only dequeues buffers into the ring |
| perception + reducer (std thread) | normal | takes the newest frame; if it falls behind it skips frames and reports it (the video is unaffected, §1) |
| recorder (std thread) + ffmpeg child | ffmpeg `nice 5` | bounded queue |
| tokio runtime (2 workers): web, telegram, SQLite writer | normal | blocking work on the blocking pool |

systemd: `MemoryMax=600M`, `CPUWeight` for the service, `ProtectSystem=strict`, `DeviceAllow`
for the capture device by id. Target RSS < 300 MB: ring 34 MB, ffmpeg ~110 MB, rest small.

Per-stage timings, queue depths, drops, CPU, RAM and temperature are sampled every second for
the Health page and logged every minute.

## 9. Testing

1. **Unit tests** per recognizer on single fixtures: real captured JPEGs (~140 KB each, kept
   in git), plus the ported v1 PNGs.
2. **Invariant tests** over all fixtures: hint-independence and exclusivity (§3.4), and
   "positive frames classify the same with and without previous state".
3. **Reducer tests:** table-driven, one per PRD R4 row and each Tetris transition, including the
   v1 bugs B1–B4 as named regression tests.
4. **Timeline tests:** sequences with an expected-timeline YAML:
   ```yaml
   source: tetris_2p_full_game/
   expect:
     - frames: 0-118      state: Menu
     - frames: 119-410    state: InGame/Tetris/ModeSelect
     - frames: 411-9300   state: InGame/Tetris/Playing  {mode: versus2p, start_level: 9}
   events:
     - SessionEnded {reason: game_over, p1: 12283, p2: 2680}
   ```
   Full raw games are big (a 5-minute game ≈ 2.5 GB of JPEG), so timeline tests store
   **canvas sequences** (256×240 palette indices, zstd-compressed, a few KB per frame) in git,
   generated from the raw capture. A few short raw-JPEG sequences cover the normalization
   step end to end. Raw full-game captures live outside git (NAS).
5. **Recorder tests:** synthetic timestamp streams (jitter, gaps, pauses) → assert the
   CFR output frame list and drop/duplicate counts.
6. **CI:** GitHub Actions with Nix: `cargo test`, clippy, fmt, and the aarch64 cross build.

## 10. Repository and Nix

```
eightbit-bro/
├── Cargo.toml                 # workspace
├── crates/
│   ├── bro-heart/             # state types, reducer, events — pure, no IO
│   ├── bro-vision/            # NES canvas, geometry, probes, POI
│   ├── bro-games/             # menu, tetris (deep), mario, battle_city, dr_mario, jewelry
│   ├── bro-capture/           # sources, ring buffer
│   ├── bro-record/            # cut list, CFR timeline, ffmpeg encoder
│   ├── bro-store/             # SQLite
│   ├── bro-telegram/
│   ├── bro-web/               # axum dashboard + API
│   └── eightbit-bro/          # binary: config + wiring
├── fixtures/                  # JPEG fixtures, canvas sequences, timeline YAMLs
├── docs/
├── flake.nix                  # nixos-26.05; packages, modules, sd image
├── nix/
│   ├── package.nix            # rustPlatform build of the workspace
│   ├── module.nix             # services.eightbit-bro (added in M0)
│   └── appliance-rpi4.nix     # turns a Pi 4 into an eightbit-bro box
└── deploy/
    ├── hive.nix               # colmena hive (plain, pinned via flake.lock)
    └── hosts/
        ├── example.nix        # committed template
        └── <hostname>.nix     # real hosts: gitignored, live on the deploying machine
```

The repo is a **Raspberry Pi drop-in appliance**, in three layers:

1. **Generic flake outputs:** the package, `nixosModules.default` (just the service, usable on
   any NixOS machine, e.g. fenek for replays), `nixosModules.appliance-rpi4`, and
   `packages.aarch64-linux.sd-image-rpi4`.
2. **The appliance module** (`nix/appliance-rpi4.nix`): **mainline kernel** (`linux-rpi` is
   deprecated in nixpkgs and no longer cached by Hydra; mainline is cached and is what the
   official NixOS aarch64 SD image uses), u-boot + extlinux, zram swap, `/tmp` in RAM, capped
   journald, key-only SSH, Tailscale, firewall, weekly nix GC, and the service
   (`nix/module.nix`, dashboard on port 8080) with the capture card found by USB id.
3. **Hosts:** `deploy/hosts/<hostname>.nix` adds the hostname, target address, SSH keys and
   `stateVersion`. Real host files are gitignored and stay on the deploying machine; only
   `example.nix` is committed. Flakes can't see untracked files, so `deploy/hive.nix` is a
   plain colmena hive that pins nixpkgs from `flake.lock`.

**Drop-in flow:** flash the SD image, plug in the capture card, boot, open the dashboard, set the
password and Telegram details on the setup page. No secrets to prepare.

- **More flake outputs:** `packages.default` (the binary; the build runs the tests) and
  `devShells.default` (Rust toolchain, clippy, rustfmt, ffmpeg, sqlite).
- **Building for the Pi:** fenek has `boot.binfmt.emulatedSystems = [ "aarch64-linux" ]`, so
  colmena builds the Pi's system on fenek (`buildOnTarget = false`) and copies it over; almost
  everything is substituted from cache.nixos.org. The Rust package itself will be
  **cross-compiled** (fast, no emulation). v1 compiled on the Pi, which is too slow and risks
  OOM for Rust with 1.8 GB and no swap.
- **Deploy (push-based during development):** `deploy/deploy.sh` (wraps `colmena apply -f
  deploy/hive.nix --on nix-tetris-recorder`) from the local checkout, so there's no need to
  commit or push to test (fixes v1's `rev = "main"` + placeholder hash problem). NixOS
  generations give rollback. It writes its progress (phase, crates compiled, commit, result)
  to `deploy.json` on the host over SSH, and the dashboard's Live page shows it. A full
  deploy rebuilds the whole package under emulation: ~10 min.
- **Dev push (PRD O3):** `deploy/deploy.sh --dev` builds with cargo in the aarch64 dev shell
  (emulated, cache in `target/pi`, profile `pi-dev`: release without LTO, incremental), copies
  the binary to `/var/lib/eightbit-bro-dev/` and points the service at it with a systemd
  override in `/run`. ~15 s for a change in one or two crates (the first build ~9 min). The
  binary links to the same pinned nixpkgs as the Pi, so its libraries are there already
  (`nix copy` sends any that aren't). A reboot or the next full deploy returns to the
  Nix-built version; the dashboard shows the build as `dev-…`.
- **Pi config moves** from `tetris-recorder/nix/` into this repo. The v1 service and module
  are removed in the same change (PRD O7).
- **Config:** a TOML file generated by the NixOS module (`/etc/eightbit-bro/config.toml`) for
  the declarative settings. Secrets come from the dashboard setup page (§7), not from Nix.
- **Future split mode (not v2.0):** `Source` and the event bus are traits, so a
  `NetworkSource` (JPEG frames over TCP from a capture-only instance) can be added without
  touching recognition or the recorder.

## 11. Milestones

| | Deliverable | Exit criterion |
|---|---|---|
| **M0** | Workspace, flake, sources, ring buffer, NES canvas + geometry calibration, POI optimization, CFR timeline, ffmpeg recorder prototype on the Pi | Pi CPU at 30 fps output measured with the real binary |
| **M1** | Labeling session with the owner (dashboard Tools → capture) | Fixtures for menu pages, 5 games, all Tetris screens/modes, demo |
| **M2** | Console perception + reducer + review queue + timeline harness | All console-level fixtures and timelines green |
| **M3** | Tetris deep | Tetris fixtures/timelines green, v1 bugs B1–B4 covered |
| **M4** | Recorder + SQLite + Telegram queue | Full replayed games produce correct videos and posts (sandbox channel) |
| **M5** | Dashboard | All PRD §12 pages; no measurable fps impact with 3 clients |
| **M6** | Deploy to the Pi, remove v1, one week live | PRD §14 criteria |

## 12. Open decisions

| # | Question | Proposal |
|---|---|---|
| D1 | Output fps: 30 or 60? | **Decided: 30.** Capture too, since 2026-11-02 (`[input] fps = 30`, the card's 720p30 mode): it halves the work. The card also does 60 and 50 |
| D2 | Output look (A/B/C, §4.3) | **Decided: A**, cropped pass-through 896×720; B and C were blurry |
| D3 | aarch64 build strategy | **Done:** binfmt on fenek (`fl` 89d0f55, rebuilt) |
| D4 | Pi NixOS 24.11 (EOL) → 26.05 | **Done (2026-11-01):** appliance layout (§10) deployed, rebooted into mainline 6.18.60; capture still 720p at 59.6 fps |
| D5 | Dashboard stack | **Done:** static pages + vanilla JS over the JSON API (no JS build); the POI editor needed client-side drawing anyway |
| D6 | Raw full-game captures for tests | Stored on the NAS, canvas sequences in git |
| D7 | Geometry calibration | **Done:** x = 188…1098 fitted to the pixel edges of menu text; menu text now sits on the 8×8 grid. Tetris draws its HUD 3 px right of the grid (its own scroll), so POIs there snap one tile wider |
