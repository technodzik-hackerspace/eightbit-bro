use std::fs;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bro_capture::Ring;
use bro_heart::ConsoleState;
use bro_store::EventRow;
use serde::Serialize;
use tokio::sync::broadcast;

/// What the pipeline shares with the dashboard.
pub struct Live {
    pub ring: Option<Arc<Ring>>,
    pub source: String,
    /// When this process started (unix ms).
    pub started: i64,
    /// Identifies the running binary: its Nix store hash, so every deploy shows up.
    pub build: String,
    state: Mutex<(ConsoleState, i64)>,
    pub stats: Stats,
    rates: Mutex<Rates>,
    tx: broadcast::Sender<Push>,
}

/// Counters, written by the pipeline threads.
#[derive(Default)]
pub struct Stats {
    pub captured: AtomicU64,
    pub processed: AtomicU64,
    /// Not perceived because perception was behind (live only; the video is unaffected).
    pub skipped: AtomicU64,
    pub corrupt: AtomicU64,
    pub perceive_us: AtomicU64,
    pub capture_errors: AtomicU64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Rates {
    pub capture_fps: f64,
    pub perceive_fps: f64,
    pub perceive_ms: f64,
    pub temp_c: Option<f64>,
    pub load: Option<String>,
    pub rss_mb: Option<f64>,
}

/// Pushed to dashboards over SSE.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Push {
    State { path: String, state: ConsoleState, since: i64 },
    Event(EventRow),
    Review { id: i64 },
}

impl Live {
    pub fn new(ring: Option<Arc<Ring>>, source: String) -> Live {
        Live {
            ring,
            source,
            started: now_ms(),
            build: build_id(),
            state: Mutex::new((ConsoleState::NoSignal, now_ms())),
            stats: Stats::default(),
            rates: Mutex::new(Rates::default()),
            tx: broadcast::channel(256).0,
        }
    }

    pub fn set_state(&self, state: &ConsoleState) {
        let since = now_ms();
        *self.state.lock().unwrap() = (state.clone(), since);
        self.push(Push::State { path: state.path(), state: state.clone(), since });
    }

    pub fn state(&self) -> (ConsoleState, i64) {
        self.state.lock().unwrap().clone()
    }

    pub fn push(&self, push: Push) {
        let _ = self.tx.send(push);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Push> {
        self.tx.subscribe()
    }

    pub fn rates(&self) -> Rates {
        self.rates.lock().unwrap().clone()
    }
}

/// Samples the counters once a second into rates for the health view.
pub fn spawn_rates(live: Arc<Live>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut prev = (0, 0, 0);
        for n in 0u64.. {
            tick.tick().await;
            let s = &live.stats;
            let now = (
                s.captured.load(Ordering::Relaxed),
                s.processed.load(Ordering::Relaxed),
                s.perceive_us.load(Ordering::Relaxed),
            );
            let processed = now.1 - prev.1;
            let rates = Rates {
                capture_fps: (now.0 - prev.0) as f64,
                perceive_fps: processed as f64,
                perceive_ms: match processed {
                    0 => 0.0,
                    n => (now.2 - prev.2) as f64 / n as f64 / 1000.0,
                },
                temp_c: read("/sys/class/thermal/thermal_zone0/temp")
                    .and_then(|t| t.trim().parse::<f64>().ok())
                    .map(|t| t / 1000.0),
                load: read("/proc/loadavg")
                    .map(|l| l.split_whitespace().take(3).collect::<Vec<_>>().join(" ")),
                rss_mb: read("/proc/self/status").and_then(|s| {
                    let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
                    Some(line.split_whitespace().nth(1)?.parse::<f64>().ok()? / 1024.0)
                }),
            };
            // Periodic stats in the journal (PRD O5).
            if n % 60 == 59 {
                tracing::info!(
                    capture_fps = rates.capture_fps,
                    perceive_fps = rates.perceive_fps,
                    perceive_ms = format!("{:.1}", rates.perceive_ms),
                    skipped = s.skipped.load(Ordering::Relaxed),
                    corrupt = s.corrupt.load(Ordering::Relaxed),
                    temp_c = rates.temp_c,
                    rss_mb = rates.rss_mb.map(|m| m.round()),
                    "stats"
                );
            }
            *live.rates.lock().unwrap() = rates;
            prev = now;
        }
    });
}

/// The hash part of the binary's Nix store path; outside the store (a dev push), "dev-" and
/// the binary's modification time, so each push shows up too.
fn build_id() -> String {
    let Ok(exe) = std::env::current_exe() else { return "dev".into() };
    if let Some(name) = exe.strip_prefix("/nix/store").ok().and_then(|p| p.iter().next()) {
        return name.to_string_lossy().chars().take(8).collect();
    }
    let modified = fs::metadata(&exe).and_then(|m| m.modified()).ok();
    let secs =
        modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
    secs.map_or("dev".into(), |s| format!("dev-{:06x}", s & 0xff_ffff))
}

fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}
