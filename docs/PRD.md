# eightbit-bro — Product Requirements Document

> He watches and knows all your 8-bit games.

| | |
|---|---|
| Status | Draft v0.4 (2026-11-01) |
| Owner | Omo Yauhen |
| Predecessor | `tetris-recorder` (v1, Python) |
| Next document | `docs/TECH_DESIGN.md` |

## 1. Summary

`eightbit-bro` watches the HDMI output of a **Retro-Genesis 8-bit console** (an NES-compatible
console with ~300 built-in games), and for every captured frame it knows **what the console
is doing right now**: no signal, browsing the game menu, or playing a specific game — and, for
games with deep recognition, exactly what is happening inside that game.

From the stream of state changes it produces **events**. Events drive actions: start/stop
recording, render a video on the fly, post it to Telegram with statistics, and store
everything in SQLite for future features (high scores, leaderboards, NFT minting).

v2 is a rewrite in **Rust**, **NixOS-native from day one**, replacing v1, which only
understood 2-player Tetris, recorded at ~10 fps, and was hard to extend.

## 2. Background: what v1 got wrong

These are explicit non-regressions — v2 acceptance tests must cover each of them.

| # | v1 problem | v2 requirement |
|---|---|---|
| B1 | Only 2-player versus Tetris; other modes treated as "menu" | All Tetris modes recognized (§6.1) |
| B2 | Start level ignored | Start level recognized and stored |
| B3 | Demo mode not distinguished from human play | Demo is recognized and never recorded |
| B4 | Final score read on the frame *after* leaving game over (often `None`) | Each player's score captured at the moment that player tops out, confirmed at session end |
| B5 | Output framerate = `frames / wall-clock duration` → varies per game | Constant output framerate (§8.3) |
| B6 | Capture, CV and PNG writing in one loop → silent frame drops | Capture never blocked by analysis or encoding; drops are measured and reported |
| B7 | Every frame written as PNG to SD card (23 GB accumulated) | Frames stay in RAM; only encoded video hits disk |
| B8 | Hard-coded `/dev/video0` (device re-enumerated → service dead) | Capture device selected by stable identity (USB id / by-id path) |
| B9 | Invalid secret crashes the whole service in a restart loop | Missing/invalid Telegram config degrades to "record locally, queue posts", never blocks recognition |
| B10 | Logic spread across ad-hoc booleans, hard to reason about | Single explicit state tree with deterministic transitions (§5) |

## 3. Goals and non-goals

### 3.1 Goals (v2.0)

- **G1. Always-correct console state.** For every frame, a deterministic state from a single
  state tree (§5). No probabilistic guesses.
- **G2. Recognize from a single frame, refine with history.** Any frame can be classified
  without prior state (cold start, joining mid-game); previous state is used to resolve
  ambiguous frames and to make processing cheaper.
- **G3. Game identification.** Recognize the menu and 5 games: **Tetris (deep)**,
  **Super Mario**, **Battle City**, **Dr. Mario**, **Jewelry** (identity only). Everything
  else is `Other`.
- **G4. Full Tetris parity and beyond** — all modes, start level, pause, bonus, per-player
  game over and final scores, demo detection.
- **G5. Automatic recording of human Tetris games** at a **stable 30 fps** (target on current
  hardware), rendered on the fly, posted to one Telegram channel with statistics.
- **G6. Event log in SQLite** — the stable interface for all future features.
- **G7. Learning loop.** Unknown screens are saved for review so recognition coverage grows
  over time (by an agent, with human help).
- **G8. Same pipeline for live capture, image folders and video files**, with a regression
  harness that runs in CI.
- **G9. NixOS-native:** flake, NixOS module, one-command deploy to the Pi.
- **G10. Web dashboard** showing the live console state, event log, errors and unknown
  frames, sessions/videos and settings (§12).

### 3.2 Non-goals (v2.0)

- High scores, leaderboards, NFT minting, player identity/registration. (The event model must
  make them possible later — §9.)
- Recording games other than Tetris. (The recording trigger is per-game, so adding one later
  is a small change.)
- Deep recognition of Mario / Battle City / Dr. Mario / Jewelry (planned for v2.1+).
- Audio in videos (the capture card supports it; Tetris doesn't need it — later).
- Physical cartridges in the slot (not used; would show up as `Other`).
- Splitting long videos to fit Telegram limits (Tetris games are short enough — revisit later).
- Home Assistant / MQTT integration (maybe later; the event log and dashboard API enable it).
- Public internet exposure of the dashboard.
- ML training. Recognition is hand-written, pattern based, and deterministic.

## 4. Users and scenarios

- **Players (hackerspace visitors).** Play on the console. They never touch the software;
  games appear in the Telegram channel automatically.
- **Maintainer (Omo, plus a coding agent).** Deploys new versions, reviews unknown frames,
  adds recognition for new screens/games, debugs from recorded inputs.

Key scenarios:

1. *Normal game.* Console on → menu → player picks Tetris → title → mode select → level
   select → play → both players game over → video + stats in Telegram.
2. *Joined mid-game.* Service restarts while a Tetris game is running. The first frame is
   recognized as `InGame{Tetris, Playing, …}`; the session is recorded and marked **dirty**.
3. *Idle console.* Tetris sits on its title screen and starts the built-in demo. Nothing is
   recorded.
4. *Pause.* A player pauses for 3 minutes; the video contains no pause frames.
5. *Reset.* A player holds Start for 10 s (or presses reset / power-cycles) → console returns
   to the menu → any open session ends with reason `reset`.
6. *New screen.* A screen inside Tetris that we've never seen (e.g. a rare animation). State
   stays `InGame{Tetris}`, a warning is logged, the frame goes to the review queue.
7. *Debug.* Maintainer replays a captured folder or video (dashboard Replay page, or a dev
   instance configured with that input) and gets exactly the same state timeline and events
   as on the Pi.

## 5. Console state model

### 5.1 The state tree

The console state is a **tree** that gets more detailed the deeper you go. Each level is
owned by a recognizer; a game without deep recognition simply has no subtree.

```
ConsoleState
├── NoSignal
├── Menu { page, cursor_index, highlighted_game }
└── InGame { game, detail }
      ├── game = Tetris        → detail = TetrisState (§6.1)
      ├── game = SuperMario    → detail = none (v2.0)
      ├── game = BattleCity    → detail = none (v2.0)
      ├── game = DrMario       → detail = none (v2.0)
      ├── game = Jewelry       → detail = none (v2.0)
      └── game = Other         → detail = none
```

There is no `Unknown`, `Boot`, or `Transition` state: it's an 8-bit console, so every frame
is the menu, a game, or no signal.

### 5.2 Determinism rules

- **R1. 100% or nothing.** A recognizer either positively identifies a frame (by patterns that
  are always visible on that screen) or doesn't claim it. There are no confidence scores.
- **R2. Same input → same output.** Given the same frame sequence, the state timeline and
  events are identical on every machine and every run. This holds for live capture, folders
  and video files.
- **R3. Stateless classification exists.** Every recognizer must be able to classify a frame
  with no prior state (cold start). History is an optimization and a tie-breaker, never a
  requirement for recognizable screens.
- **R4. Inheritance for unrecognized frames.** Given the previous state `P` and a frame that no
  recognizer claims:

  | Frame | Previous state | Result |
  |---|---|---|
  | Black | `InGame{g,…}` | stays `InGame{g,…}` (level transitions, etc.) |
  | Black | none / `NoSignal` | `NoSignal` |
  | Black | `Menu` | stays `Menu` (TBD after the menu capture session) |
  | Non-black, unrecognized | `InGame{g,…}` | stays `InGame{g,…}` + frame goes to the review queue (§7) |
  | Non-black, unrecognized | none / `NoSignal` / `Menu` | `InGame{Other}` + frame goes to the review queue |

- **R5. Positive recognition always wins.** If a recognizer claims a frame, that overrides
  inheritance (e.g. the menu appearing during "Tetris" means the player reset).
- **R6. Debouncing only where the console really flickers.** NES sprite flicker and 30-of-60 fps
  sampling can hide sprites on individual frames. Recognizers must use patterns robust to that
  (backgrounds, HUD, text), not single sprites. Any multi-frame confirmation needed must be
  explicit, per-recognizer, and covered by tests.

### 5.3 Menu

- The menu is a blue screen titled **"300 IN 1 VER2.0"** with a numbered, alphabetical list of
  **20 games per page** and a red arrow cursor (seen in the first capture: page 1 =
  1. Adventure Island … 20. Babel no Tou). The full catalog comes from the labeling session (§11).
- The menu recognizer reports `page`, `cursor_index` and `highlighted_game` (the title under
  the cursor, from a catalog of all ~300 titles built during the labeling session).
- When the console goes from `Menu` to `InGame`, the last `highlighted_game` is recorded as
  **menu selection** metadata on the session. If the in-game recognizer disagrees with it
  (e.g. menu said Tetris, screen says Battle City), log a warning; in-game recognition wins.
- If a game without a recognizer is launched from the menu, the state is
  `InGame{Other}`, but the session still carries the menu selection ("playing *Contra*").

## 6. Game coverage

### 6.1 Tetris (deep)

**Modes:** 1 Player, 2 Player (versus), Cooperative (one shared well; single score — to be
confirmed), Versus Computer, With Computer (CPU is always P2), Demo.

**TetrisState subtree (v2.0):**

```
TetrisState
├── Title
├── ModeSelect { highlighted_mode }
├── LevelSelect { mode, highlighted_level }
├── Playing { mode, start_level, players: [PlayerState], paused, bonus, demo }
│     PlayerState { score, lines, level, topped_out }
└── GameOver { mode, players: [PlayerState] }
```

(The exact sub-screens get adjusted after the fixture-capture session; the principle is that
everything a human can read off the screen is in the state.)

**Requirements:**

- **T1. Mode detection** for all modes above, from the playfield layout plus the
  ModeSelect history when available.
- **T2. Start level** read from LevelSelect *and* from the LEVEL counter on the first playing
  frame; if they disagree, log a warning and prefer the in-game counter.
- **T3. Demo detection.** A human always goes through the menu → Tetris → ModeSelect →
  LevelSelect. The demo starts straight from the Tetris title screen.
  `Title → Playing` (with no ModeSelect/LevelSelect in between) ⇒ `demo = true`.
- **T4. Clean vs dirty sessions.** `clean` = we saw the session start via ModeSelect /
  LevelSelect; `dirty` = we joined mid-game (cold start, or the capture was interrupted).
  A dirty session is treated as **human** and recorded.
- **T5. Pause** recognized for all modes; paused frames are not recorded.
- **T6. Bonus** screens recognized and recorded (they are part of the game).
- **T7. Per-player game over.** A player's `topped_out` is sticky for the session, and their
  score/lines/level are captured from the frames at that moment (fix B4).
- **T8. End of game:** 1P / coop / with-computer: the (shared) well tops out; 2P versus and
  versus computer: **both** players topped out. Also ends on reset to menu, no signal, or a
  new game start (scores reset).
- **T9. Score/lines/level OCR** must be exact on all fixtures, including large scores
  (e.g. 5+ digits, `score_big.png`).

### 6.2 Identity-only games (v2.0)

Super Mario, Battle City, Dr. Mario, Jewelry: the recognizer reports `InGame{game}` from any
in-game frame (title or gameplay) using always-visible elements. No subtree, no recording.

Deeper recognition (worlds/stages, lives, score, kills) is v2.1+, and must be possible by
adding a subtree, without changing the console-level model.

### 6.3 Adding a game

Adding a game (or deepening one) must be a local change: one recognizer module plus its
fixtures, registered in one place. No changes to the capture, recording, or event code.

## 7. The learning loop

- **L1.** Every unrecognized frame (R4), in any state including `InGame{Other}`, is offered to
  the **review queue** on disk. A frame that becomes a new item logs an `UnknownScreen`
  warning with the frame number and the current state.
- **L2.** Near-duplicate frames are deduplicated (perceptual hash): a repeat only bumps the
  item's count and last-seen time. The queue holds at most **100 items**; when it is full, the
  oldest already-reviewed item makes room, and if every item is still unreviewed new screens
  are counted but not saved until the owner reviews or deletes some.
- **L3.** Each saved item carries context: the previous N recognized states, timestamp, input
  source.
- **L4.** The dashboard (§12) lets the owner browse queued frames, **annotate** them and mark
  them reviewed. This is for quality control and for explaining screens to the agent; the agent
  then turns annotated frames into fixtures and extends recognition in code.
- **L4a. POI annotations.** On a queued frame the owner draws one or more **points of interest**:
  a rectangle `(x1, y1, x2, y2)` around the part of the screen that tells this frame apart
  from others (e.g. the "HIGH SCORE" header of the name-entry screen), plus a **comment for the
  agent** ("high-score entry screen, Tetris 1P; the name is typed here"). The system may
  **optimize** the region (trim it to what is actually drawn inside it and align it to the
  console's 8×8 tiles) so the resulting check is small and fast; the dashboard shows both the
  drawn and the optimized rectangle. The agent turns each POI into a recognizer probe.
- **L5.** The running system never changes its own recognition behavior (keeps R2).
- **L6.** Queued frames (and recorded sessions' raw frames, when kept) can be downloaded as an
  archive from the dashboard (replaces v1's "download the last game" scripts).

## 8. Recording and video

### 8.1 Triggers (v2.0)

- **Start:** a human Tetris session enters `Playing` (clean or dirty). **The video starts at
  the first gameplay frame** — no menu, title, mode select or level select in the video.
  If the start decision is made a few frames late, the frames in between must not be lost.
- **Exclude:** paused frames (cut out completely); demo sessions (never recorded).
- **Include:** bonus screens; the game-over screen (a short tail, a couple of seconds, so the
  result is visible).
- **Stop:** session end (T8).
- Triggers are defined per game, so adding recording for other games later is configuration
  plus a small rule, not new plumbing.

### 8.2 On-the-fly rendering

- Frames are held in **RAM**, never written as individual images.
- While a session records, frames are **encoded into the video as they arrive**, in parallel
  with recognition of the following frames. When the session ends, the video is finalized
  within seconds — no post-processing pass over thousands of images.
- Only the encoded video is written to disk. SD-card writes during idle and menu time ≈ 0.
- If a session turns out not to be worth keeping (e.g. discovered to be a demo, or shorter
  than a minimum length), the partial video is discarded.

### 8.3 Frame rate and stability

- **F1. Decided: constant 30 fps** output (v1 was ~10 fps). Capture and recognition still run
  at the card's 60 fps (tech design §2); the video takes every other frame.
- **F2. Constant, not computed.** The output timeline is built from capture timestamps; gaps
  are filled by repeating frames and excess frames are dropped, so playback speed is
  always real-time.
- **F3. Measured.** Per session we log and store captured, dropped and duplicated frame counts.
  Success: **< 0.5 %** dropped or duplicated frames for a full game.
- **F4. Capture is never blocked** by recognition, encoding, disk or network.
- **F5. Visual quality. Decided:** the console picture cropped from the capture and passed
  through at **896×720** (look A). The re-rendered NES-canvas upscales (B: 768×720, C: 512×480)
  looked blurry in the samples.
- **F6. Path to 60 fps.** The architecture must not cap at 30 fps; going to 60 fps output is a
  configuration change (and CPU budget), not a rewrite.

### 8.4 Where processing happens

- **Preferred: everything on the Pi** (capture, recognition, encoding, posting).
- **Optional split:** the Pi captures and streams frames; another NixOS machine (fenek)
  recognizes and renders. This is the same binary in a different mode, used only if
  measurements show the Pi can't hold F1–F4.
- Which topology to ship is decided by measurements in the tech design.

## 9. Events and storage

- **E1.** Every state change produces an **event**, at every level of the tree
  (e.g. `Menu → InGame{Tetris}`, `Tetris.LevelSelect → Tetris.Playing`, `P2 topped out`,
  `paused`, `resumed`).
- **E2.** Higher-level events: `SessionStarted`, `SessionEnded{reason}`, `RecordingStarted`,
  `RecordingFinished{video, frame stats}`, `UnknownScreen`, `PostQueued/Posted/Failed`.
- **E3.** A **session** record holds: game, menu selection, mode, start level, clean/dirty,
  demo flag, start/end time and frame numbers, end reason, per-player results
  (score/lines/level/topped-out moment), CPU flag, video path.
- **E4.** Stored in **SQLite** on the device. The schema is versioned and migrated
  automatically.
- **E5.** The events and sessions tables are the **public contract** for future features
  (high scores, leaderboards, NFT evidence). Each event references frame numbers and
  timestamps so it can be traced back to the video.
- **E6.** Retention: keep the database forever; rotate videos by disk budget.

## 10. Telegram

- **TG1.** One channel. After a recorded session ends, post the video with a caption: game,
  mode, start level, per-player score/lines/level, winner, duration (excluding pauses),
  clean/dirty, "vs CPU" when applicable.
- **TG2.** Posting goes through a **persistent queue** in SQLite; it retries with backoff and
  survives restarts and network outages.
- **TG3.** Missing or invalid credentials never stop capture or recognition (fix B9);
  the service starts, records locally, and logs a clear error.
- **TG4.** Telegram can be turned off entirely for development (`--no-post`).

## 11. Inputs, tooling and testing

- **I1. Inputs:** live V4L2 device (chosen by stable identity), a folder of images
  (sorted by name), a video file. All go through the same pipeline.
- **I2. Offline runs are deterministic:** frame timestamps come from the input (file order ×
  nominal fps, or the video's PTS), not the wall clock.
- **I3. No CLI tools.** Operations happen in the web dashboard: classify an uploaded image
  (shows the state tree), replay a folder/video, capture raw frames for fixtures, review
  queue. The binary only takes a config file; developers use `cargo test` and the dashboard.
- **I4. Regression harness:** labeled frame folders with an **expected state timeline**
  (e.g. `frames 0–119: Menu`, `120–4300: Tetris/Playing/2P`) and expected events; runs in
  CI and locally; a mismatch shows the first differing frame.
- **I5. Fixture coverage:** at least one fixture per recognized screen, mode and transition,
  plus every v1 fixture ported (score fixtures keep the `{P1}_{P2}.png` convention).
- **I6. Labeling session (one-off, with the owner):** scroll through every menu page; enter and
  play/idle each of the 5 recognized games; Tetris: every mode, several start levels, pause,
  bonus, game over per player, demo. The output is the fixtures and the menu catalog.

## 12. Web dashboard

A small web UI served by the service itself, for watching, debugging and configuring
eightbit-bro without SSH.

### 12.1 Pages

- **W1. Live.** The current console state tree (e.g. `InGame › Tetris › Playing › 2P versus,
  start level 9 › P1 12,283 / P2 2,680 (topped out)`), updated live. A **live preview** of the
  latest captured frame at low rate (≈ 1–2 fps, downscaled), optionally with the recognized
  regions drawn over it. Recording indicator (on/off, duration, frames, drop %).
- **W2. Events.** The event log, live-tailing and searchable, filtered by level of the state
  tree, event type, session and time range. Clicking an event shows the frame it came from
  when that frame is still available (recorded video or review queue).
- **W3. Sessions.** Past sessions with their metadata (§9 E3), the video player, frame stats,
  and Telegram post status (queued / posted / failed + a retry button).
- **W4. Errors and unknown frames.** Warnings/errors (Telegram failures, capture problems,
  frame drops, recognizer disagreements) and the **review queue** (§7): thumbnails grouped by
  similarity, with context (previous states, timestamp). Actions: label, mark as known
  non-issue, delete, download a batch for fixture work, and draw POIs with comments for the
  agent (§7 L4a).
- **W5. Health.** Input fps, output fps, dropped/duplicated frames, per-stage timings, CPU,
  RAM, temperature, disk usage and budget, capture device status, version/commit.
- **W6. Settings.** See §12.2.

### 12.2 Settings ownership

NixOS stays the source of truth for the deployment; the dashboard must not fight it.

- **Declarative (Nix, read-only in the dashboard):** capture device identity, paths, ports,
  memory/disk budgets, and which features are enabled. The dashboard shows them.
- **Runtime (stored in SQLite, editable in the dashboard, take effect without a restart):**
  e.g. pause Telegram posting, enable/disable recording per game, review-queue size, log level,
  live preview on/off. Each change is logged as an event (who/when/old → new).
- **Secrets are entered in the dashboard and are write-only:** on first boot a setup page asks
  for the dashboard password and the Telegram bot token and channel. Afterwards the UI shows
  only "set / not set" with a way to replace them; it never displays them. This is what makes
  the device a drop-in: flash, plug in, open the dashboard.

### 12.3 Constraints

- **W7. Never hurts capture.** The dashboard runs at a lower priority than capture,
  recognition and encoding; with N browsers open, F1–F4 (§8.3) still hold. The preview uses
  frames that already exist; it never triggers extra decoding at full rate.
- **W8. Access:** LAN/Tailscale only, behind a simple shared login (static password set on
  the setup page). Not exposed to the internet.
- **W9. Lightweight:** served by the same binary, no separate database or Node runtime on the
  Pi; works on a phone screen.
- **W10. API first:** everything the UI shows comes from a documented HTTP/JSON (+ live
  stream) API, so scripts, the agent and future integrations (HA, NFT minting) can use it.

## 13. Deployment and operations

- **O1. Raspberry Pi drop-in.** The repo is a Nix flake providing the package, a dev shell, the
  service module (`services.eightbit-bro`), a Pi 4 **appliance** module and a flashable **SD
  image**. A new box = flash, plug in the capture card, boot, finish setup in the dashboard.
  Building for aarch64 must not require compiling on the Pi.
- **O2.** One command deploys a new version to the Pi (Colmena, push-based during
  development; GitHub CI/CD later), with an easy rollback. Real host configs (address, SSH
  keys) are gitignored and live on the deploying machine; an example host is committed.
- **O3.** Fast dev loop: try a local change on the Pi without committing or pushing.
- **O4.** Configuration: device identity, paths, disk budgets, log level in Nix, with sensible
  defaults. Telegram credentials and the dashboard password are set in the dashboard (§12.2)
  and never stored in the Nix store or git.
- **O5.** Observability: structured logs with frame number and state path; periodic stats
  (fps in/out, drops, per-stage timing, queue depth, RAM); visible via `journalctl`.
- **O6.** Resource limits on the Pi 4 (1.8 GB RAM, no swap, SD card): the service runs within
  a fixed memory budget (exact number in the tech design), and writes to the SD card only for
  videos, the database, the review queue and logs.
- **O7.** v1 is retired: the old NixOS module is removed from the Pi's config when v2 is
  deployed.

## 14. Success criteria for v2.0

1. On the regression suite: 100 % of frames get the expected state; 0 false recordings
   (demo, menu); 0 missed human Tetris games; exact scores/lines/levels/start level.
2. Live, over a week of hackerspace use: every human Tetris game is posted with correct stats;
   no demo posted; each UnknownScreen gets reviewed.
3. Videos at a constant 30 fps with < 0.5 % dropped/duplicated frames, real-time speed.
4. Cold start mid-game: the correct state within the first recognizable frame.
5. Deploying a change to the Pi takes one command and a few minutes.
6. The current state, events and unknown frames can be followed from a phone via the
   dashboard, with no measurable impact on capture fps.

## 15. Milestones (indicative)

1. **M0. Measurements and skeleton** — capture-card modes, decode/encode benchmarks on the Pi,
   Rust workspace, Nix flake, folder/video input, single-frame classification.
2. **M1. Labeling session** — fixtures and menu catalog.
3. **M2. Console state** — NoSignal / Menu / InGame identification for the 5 games + Other;
   inheritance rules; review queue; regression harness.
4. **M3. Tetris deep** — full TetrisState, all modes, demo, clean/dirty, OCR.
5. **M4. Recording** — in-RAM buffer, on-the-fly encoding, constant fps, Telegram queue, SQLite.
6. **M5. Dashboard** — live state, events, sessions, review queue, health, settings.
7. **M6. Deploy** — NixOS module, Pi rollout, v1 removal, one week of live validation.

## 16. Risks and open questions

| # | Item | Notes / plan |
|---|---|---|
| Q1 | **MS2109 capture card** (USB 2.0, MJPEG) | Measured: 720p60 clean at ~59 fps with 60 unique frames/s; 1080p60 sends every other frame corrupt; 1080p30 clean |
| Q2 | 30-of-60 fps sampling + NES sprite flicker can hide sprites | Recognize by HUD/background/text only (R6) |
| Q3 | Pi 4 CPU budget for MJPEG decode + recognition + H.264 encode at 30 fps | M0 benchmarks; HW codec options; fall back to the fenek split (§8.4) |
| Q4 | Demo detection when joining mid-demo (cold start during the demo) → would be recorded as a dirty human game | Accept for v2.0; investigate demo fingerprints (fixed piece sequence?) |
| Q5 | Coop scoring (one shared score?) | Confirm in the labeling session |
| Q6 | Menu behavior on black frames, and whether the menu cursor is restored after reset | Labeling session |
| Q7 | What the MS2109 outputs with the console off (black / blue / frozen frame) | Test with the owner |
| Q9 | The card occasionally emits corrupt JPEG frames (seen right after a mode switch, and on every other frame at 1080p60) | Corrupt frames are counted and skipped, never crash or change state |
| Q8 | Dashboard load on the Pi (live preview, websocket clients) | W7; measure in M5 |
