//! The SQLite store (tech design §5): the event log and the review queue with its POIs.
//! One connection behind a mutex; writes are rare (state changes, new review items).

mod review;
mod schema;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use bro_heart::EventKind;
use rusqlite::{Connection, params};
use serde::Serialize;

pub use review::{Offer, Poi, ReviewConfig, ReviewItem, Status};

pub struct Store {
    dir: PathBuf,
    inner: Mutex<Inner>,
}

struct Inner {
    db: Connection,
    review: review::Cache,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub id: i64,
    /// Unix milliseconds.
    pub ts: i64,
    pub frame_no: u64,
    pub state: String,
    pub kind: EventKind,
}

impl Store {
    /// Opens (and migrates) `<dir>/bro.sqlite`; review frames live in `<dir>/review/`.
    pub fn open(dir: &Path, review: ReviewConfig) -> Result<Store> {
        fs::create_dir_all(dir.join("review"))
            .with_context(|| format!("creating {}", dir.display()))?;
        let mut db = Connection::open(dir.join("bro.sqlite"))?;
        schema::migrate(&mut db)?;
        let review = review::Cache::load(&db, review)?;
        Ok(Store { dir: dir.to_owned(), inner: Mutex::new(Inner { db, review }) })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap()
    }

    pub fn insert_event(&self, frame_no: u64, state: &str, kind: &EventKind) -> Result<EventRow> {
        let ts = now_ms();
        let inner = self.lock();
        inner.db.execute(
            "INSERT INTO events (ts, frame_no, state, kind) VALUES (?1, ?2, ?3, ?4)",
            params![ts, frame_no as i64, state, serde_json::to_string(kind)?],
        )?;
        let id = inner.db.last_insert_rowid();
        Ok(EventRow { id, ts, frame_no, state: state.into(), kind: kind.clone() })
    }

    /// Newest first; `before` pages backwards by id.
    pub fn events(&self, limit: u32, before: Option<i64>) -> Result<Vec<EventRow>> {
        let inner = self.lock();
        let mut stmt = inner.db.prepare(
            "SELECT id, ts, frame_no, state, kind FROM events WHERE id < ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![before.unwrap_or(i64::MAX), limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)?, r.get(3)?, r.get::<_, String>(4)?))
        })?;
        rows.map(|row| {
            let (id, ts, frame_no, state, kind) = row?;
            Ok(EventRow {
                id,
                ts,
                frame_no: frame_no as u64,
                state,
                kind: serde_json::from_str(&kind)?,
            })
        })
        .collect()
    }

    pub fn review_dir(&self) -> PathBuf {
        self.dir.join("review")
    }
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

#[cfg(test)]
pub(crate) fn temp_store(review: ReviewConfig) -> (Store, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "bro-store-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    (Store::open(&dir, review).unwrap(), dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_page_newest_first() {
        let (store, dir) = temp_store(ReviewConfig::default());
        for n in 0..5 {
            let kind = EventKind::Changed { from: "NoSignal".into(), to: format!("x{n}") };
            store.insert_event(n, "NoSignal", &kind).unwrap();
        }
        let page = store.events(2, None).unwrap();
        assert_eq!(page.iter().map(|e| e.frame_no).collect::<Vec<_>>(), [4, 3]);
        let next = store.events(10, Some(page[1].id)).unwrap();
        assert_eq!(next.iter().map(|e| e.frame_no).collect::<Vec<_>>(), [2, 1, 0]);
        fs::remove_dir_all(dir).unwrap();
    }
}
