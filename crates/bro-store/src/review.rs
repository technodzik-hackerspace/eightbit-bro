//! The review queue (PRD §7): unrecognized frames, deduplicated by perceptual hash and capped,
//! with the owner's POIs and comments for the agent.

use std::fs;

use anyhow::{Context, Result, bail};
use bro_vision::{Rect, hamming};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::{Store, now_ms};

#[derive(Debug, Clone, Copy)]
pub struct ReviewConfig {
    /// At most this many items (PRD L2).
    pub max_items: usize,
    /// Frames whose hashes differ in at most this many bits are the same screen.
    pub max_distance: u32,
    /// Repeats of a screen are counted in memory and written at most this often per item,
    /// to keep SD-card writes low.
    pub flush_ms: i64,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        ReviewConfig { max_items: 100, max_distance: 6, flush_ms: 5_000 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    New,
    Reviewed,
    /// A known non-issue.
    Ignored,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::New => "new",
            Status::Reviewed => "reviewed",
            Status::Ignored => "ignored",
        }
    }

    fn parse(s: &str) -> Status {
        match s {
            "reviewed" => Status::Reviewed,
            "ignored" => Status::Ignored,
            _ => Status::New,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewItem {
    pub id: i64,
    pub first_seen: i64,
    pub last_seen: i64,
    /// How many frames matched this screen.
    pub count: i64,
    pub frame_no: u64,
    /// State path when the frame was first seen.
    pub state: String,
    pub status: Status,
    pub note: String,
    pub pois: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Poi {
    pub id: i64,
    pub item: i64,
    /// As drawn by the owner, in capture pixels.
    pub drawn: Rect,
    /// After `optimize_poi`, in NES canvas pixels; `None` when outside the console picture.
    pub optimized: Option<Rect>,
    pub comment: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Saved as a new item.
    New(i64),
    /// Same screen as an existing item.
    Duplicate(i64),
    /// The queue is full of unreviewed items: not saved.
    Full,
}

pub(crate) struct Cache {
    cfg: ReviewConfig,
    entries: Vec<Entry>,
    /// New screens not saved because the queue was full, since start.
    skipped: u64,
}

struct Entry {
    id: i64,
    phash: u64,
    evictable: bool,
    pending: i64,
    last_seen: i64,
    flushed_at: i64,
}

impl Cache {
    pub fn load(db: &Connection, cfg: ReviewConfig) -> Result<Cache> {
        let mut stmt = db.prepare("SELECT id, phash, status FROM review_items ORDER BY id")?;
        let entries = stmt
            .query_map([], |r| {
                Ok(Entry {
                    id: r.get(0)?,
                    phash: r.get::<_, i64>(1)? as u64,
                    evictable: Status::parse(&r.get::<_, String>(2)?) != Status::New,
                    pending: 0,
                    last_seen: 0,
                    flushed_at: 0,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Cache { cfg, entries, skipped: 0 })
    }
}

impl Store {
    /// Offers an unrecognized frame. `canvas_png` is only called for a new item.
    pub fn review_offer(
        &self,
        phash: u64,
        frame_no: u64,
        state: &str,
        jpeg: &[u8],
        canvas_png: impl FnOnce() -> Result<Vec<u8>>,
    ) -> Result<Offer> {
        let now = now_ms();
        let mut guard = self.lock();
        let inner = &mut *guard;
        let cfg = inner.review.cfg;

        let nearest = inner
            .review
            .entries
            .iter_mut()
            .map(|e| (hamming(e.phash, phash), e))
            .min_by_key(|(d, _)| *d)
            .filter(|(d, _)| *d <= cfg.max_distance);
        if let Some((_, e)) = nearest {
            e.pending += 1;
            e.last_seen = now;
            if now - e.flushed_at >= cfg.flush_ms {
                inner.db.execute(
                    "UPDATE review_items SET count = count + ?1, last_seen = ?2 WHERE id = ?3",
                    params![e.pending, now, e.id],
                )?;
                e.pending = 0;
                e.flushed_at = now;
            }
            return Ok(Offer::Duplicate(e.id));
        }

        if inner.review.entries.len() >= cfg.max_items {
            match inner.review.entries.iter().find(|e| e.evictable).map(|e| e.id) {
                Some(oldest) => {
                    delete_item(&inner.db, &mut inner.review, &self.review_dir(), oldest)?
                }
                None => {
                    inner.review.skipped += 1;
                    return Ok(Offer::Full);
                }
            }
        }

        inner.db.execute(
            "INSERT INTO review_items (first_seen, last_seen, count, phash, frame_no, state)
             VALUES (?1, ?1, 1, ?2, ?3, ?4)",
            params![now, phash as i64, frame_no as i64, state],
        )?;
        let id = inner.db.last_insert_rowid();
        let dir = self.review_dir();
        fs::write(dir.join(format!("{id}.jpg")), jpeg)?;
        fs::write(dir.join(format!("{id}.png")), canvas_png()?)?;
        inner.review.entries.push(Entry {
            id,
            phash,
            evictable: false,
            pending: 0,
            last_seen: now,
            flushed_at: now,
        });
        Ok(Offer::New(id))
    }

    /// Newest first.
    pub fn review_items(&self) -> Result<Vec<ReviewItem>> {
        let inner = self.lock();
        let mut stmt = inner.db.prepare(&format!("{SELECT_ITEM} ORDER BY i.id DESC"))?;
        let items = stmt.query_map([], item_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(items.into_iter().map(|i| with_pending(&inner.review, i)).collect())
    }

    pub fn review_item(&self, id: i64) -> Result<Option<ReviewItem>> {
        let inner = self.lock();
        let item = inner
            .db
            .query_row(&format!("{SELECT_ITEM} WHERE i.id = ?1"), [id], item_from_row)
            .optional()?;
        Ok(item.map(|i| with_pending(&inner.review, i)))
    }

    /// Items, the maximum, and new screens skipped because the queue was full.
    pub fn review_stats(&self) -> (usize, usize, u64) {
        let inner = self.lock();
        (inner.review.entries.len(), inner.review.cfg.max_items, inner.review.skipped)
    }

    pub fn review_update(&self, id: i64, status: Option<Status>, note: Option<&str>) -> Result<()> {
        let mut inner = self.lock();
        if let Some(status) = status {
            inner.db.execute(
                "UPDATE review_items SET status = ?1 WHERE id = ?2",
                params![status.as_str(), id],
            )?;
            if let Some(e) = inner.review.entries.iter_mut().find(|e| e.id == id) {
                e.evictable = status != Status::New;
            }
        }
        if let Some(note) = note {
            inner
                .db
                .execute("UPDATE review_items SET note = ?1 WHERE id = ?2", params![note, id])?;
        }
        Ok(())
    }

    pub fn review_delete(&self, id: i64) -> Result<()> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        delete_item(&inner.db, &mut inner.review, &self.review_dir(), id)
    }

    /// Deletes every item, its frames and POIs. Returns how many were deleted.
    pub fn review_delete_all(&self) -> Result<usize> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let ids: Vec<i64> = inner.review.entries.iter().map(|e| e.id).collect();
        for &id in &ids {
            delete_item(&inner.db, &mut inner.review, &self.review_dir(), id)?;
        }
        inner.review.skipped = 0;
        Ok(ids.len())
    }

    pub fn poi_add(
        &self,
        item: i64,
        drawn: Rect,
        optimized: Option<Rect>,
        comment: &str,
    ) -> Result<Poi> {
        let inner = self.lock();
        let exists: bool = inner.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM review_items WHERE id = ?1)",
            [item],
            |r| r.get(0),
        )?;
        if !exists {
            bail!("no review item {item}");
        }
        let created_at = now_ms();
        let o = optimized.map(|r| [r.x1, r.y1, r.x2, r.y2]);
        inner.db.execute(
            "INSERT INTO review_pois (item, x1, y1, x2, y2, cx1, cy1, cx2, cy2, comment, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                item, drawn.x1, drawn.y1, drawn.x2, drawn.y2,
                o.map(|o| o[0]), o.map(|o| o[1]), o.map(|o| o[2]), o.map(|o| o[3]),
                comment, created_at
            ],
        )?;
        let id = inner.db.last_insert_rowid();
        Ok(Poi { id, item, drawn, optimized, comment: comment.into(), created_at })
    }

    pub fn pois(&self, item: i64) -> Result<Vec<Poi>> {
        let inner = self.lock();
        let mut stmt = inner.db.prepare(
            "SELECT id, item, x1, y1, x2, y2, cx1, cy1, cx2, cy2, comment, created_at
             FROM review_pois WHERE item = ?1 ORDER BY id",
        )?;
        let pois = stmt.query_map([item], |r| {
            let optimized = match r.get::<_, Option<u32>>(6)? {
                Some(x1) => Some(Rect { x1, y1: r.get(7)?, x2: r.get(8)?, y2: r.get(9)? }),
                None => None,
            };
            Ok(Poi {
                id: r.get(0)?,
                item: r.get(1)?,
                drawn: Rect { x1: r.get(2)?, y1: r.get(3)?, x2: r.get(4)?, y2: r.get(5)? },
                optimized,
                comment: r.get(10)?,
                created_at: r.get(11)?,
            })
        })?;
        Ok(pois.collect::<rusqlite::Result<_>>()?)
    }

    pub fn poi_delete(&self, item: i64, id: i64) -> Result<()> {
        let inner = self.lock();
        inner
            .db
            .execute("DELETE FROM review_pois WHERE id = ?1 AND item = ?2", params![id, item])?;
        Ok(())
    }
}

const SELECT_ITEM: &str = "SELECT i.id, i.first_seen, i.last_seen, i.count, i.frame_no, i.state,
    i.status, i.note, (SELECT COUNT(*) FROM review_pois p WHERE p.item = i.id)
    FROM review_items i";

fn item_from_row(r: &Row) -> rusqlite::Result<ReviewItem> {
    Ok(ReviewItem {
        id: r.get(0)?,
        first_seen: r.get(1)?,
        last_seen: r.get(2)?,
        count: r.get(3)?,
        frame_no: r.get::<_, i64>(4)? as u64,
        state: r.get(5)?,
        status: Status::parse(&r.get::<_, String>(6)?),
        note: r.get(7)?,
        pois: r.get(8)?,
    })
}

/// Adds repeats not written to the database yet.
fn with_pending(cache: &Cache, mut item: ReviewItem) -> ReviewItem {
    if let Some(e) = cache.entries.iter().find(|e| e.id == item.id) {
        item.count += e.pending;
        item.last_seen = item.last_seen.max(e.last_seen);
    }
    item
}

fn delete_item(db: &Connection, cache: &mut Cache, dir: &std::path::Path, id: i64) -> Result<()> {
    db.execute("DELETE FROM review_items WHERE id = ?1", [id])?;
    cache.entries.retain(|e| e.id != id);
    for ext in ["jpg", "png"] {
        let path = dir.join(format!("{id}.{ext}"));
        if path.exists() {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::temp_store;

    fn offer(store: &Store, phash: u64) -> Offer {
        store.review_offer(phash, 1, "InGame/Other", b"jpeg", || Ok(b"png".to_vec())).unwrap()
    }

    #[test]
    fn near_duplicates_are_counted_not_saved() {
        let (store, dir) = temp_store(ReviewConfig { flush_ms: 0, ..Default::default() });
        let Offer::New(id) = offer(&store, 0) else { panic!() };
        assert_eq!(offer(&store, 0b111), Offer::Duplicate(id), "3 bits apart");
        assert!(matches!(offer(&store, u64::MAX), Offer::New(_)), "a different screen");
        assert_eq!(store.review_item(id).unwrap().unwrap().count, 2);
        assert!(dir.join(format!("review/{id}.jpg")).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repeats_are_counted_in_memory_between_flushes() {
        let (store, dir) = temp_store(ReviewConfig { flush_ms: 60_000, ..Default::default() });
        let Offer::New(id) = offer(&store, 0) else { panic!() };
        for _ in 0..10 {
            offer(&store, 0);
        }
        assert_eq!(store.review_items().unwrap()[0].count, 11);
        fs::remove_dir_all(dir).unwrap();
        let _ = id;
    }

    #[test]
    fn full_queue_evicts_the_oldest_reviewed_item_or_skips() {
        let (store, dir) = temp_store(ReviewConfig { max_items: 3, ..Default::default() });
        // Hashes 64 bits apart from each other's neighbours: all distinct screens.
        let screens = [0u64, u64::MAX, 0xFFFF_FFFF, 0xFFFF_FFFF_0000_0000, 0x5555_5555_5555_5555];
        let ids: Vec<i64> = screens[..3]
            .iter()
            .map(|&h| match offer(&store, h) {
                Offer::New(id) => id,
                other => panic!("{other:?}"),
            })
            .collect();

        assert_eq!(offer(&store, screens[3]), Offer::Full);
        assert_eq!(store.review_stats(), (3, 3, 1));

        store.review_update(ids[1], Some(Status::Reviewed), None).unwrap();
        assert!(matches!(offer(&store, screens[3]), Offer::New(_)));
        assert!(store.review_item(ids[1]).unwrap().is_none(), "reviewed item made room");
        assert!(!dir.join(format!("review/{}.jpg", ids[1])).exists());
        assert_eq!(offer(&store, screens[4]), Offer::Full);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pois_round_trip_and_go_with_their_item() {
        let (store, dir) = temp_store(ReviewConfig::default());
        let Offer::New(id) = offer(&store, 0) else { panic!() };
        let drawn = Rect::new(365, 48, 516, 72);
        let poi =
            store.poi_add(id, drawn, Some(Rect::new(104, 16, 152, 24)), "SCORE label").unwrap();
        store.poi_add(id, drawn, None, "outside").unwrap();
        let pois = store.pois(id).unwrap();
        assert_eq!(pois.len(), 2);
        assert_eq!(pois[0].optimized, poi.optimized);
        assert_eq!(pois[1].optimized, None);
        assert_eq!(store.review_item(id).unwrap().unwrap().pois, 2);

        store.poi_delete(id, pois[1].id).unwrap();
        assert_eq!(store.pois(id).unwrap().len(), 1);
        store.review_delete(id).unwrap();
        assert!(store.pois(id).unwrap().is_empty());
        assert!(store.poi_add(id, drawn, None, "gone").is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn delete_all_empties_the_queue() {
        let (store, dir) = temp_store(ReviewConfig { max_items: 2, ..Default::default() });
        let Offer::New(id) = offer(&store, 0) else { panic!() };
        store.poi_add(id, Rect::new(0, 0, 10, 10), None, "x").unwrap();
        offer(&store, u64::MAX);
        assert_eq!(offer(&store, 0xFFFF_FFFF), Offer::Full);
        assert_eq!(store.review_delete_all().unwrap(), 2);
        assert_eq!(store.review_stats(), (0, 2, 0));
        assert!(store.pois(id).unwrap().is_empty());
        assert!(fs::read_dir(dir.join("review")).unwrap().next().is_none());
        assert!(matches!(offer(&store, 0xFFFF_FFFF), Offer::New(_)), "room again");
        fs::remove_dir_all(dir).unwrap();
    }
}
