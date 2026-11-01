use anyhow::Result;
use rusqlite::Connection;

/// Append-only: each entry migrates from `user_version` = its index.
const MIGRATIONS: &[&str] = &["
    CREATE TABLE events (
        id INTEGER PRIMARY KEY,
        ts INTEGER NOT NULL,
        frame_no INTEGER NOT NULL,
        state TEXT NOT NULL,
        kind TEXT NOT NULL
    );
    CREATE TABLE review_items (
        id INTEGER PRIMARY KEY,
        first_seen INTEGER NOT NULL,
        last_seen INTEGER NOT NULL,
        count INTEGER NOT NULL,
        phash INTEGER NOT NULL,
        frame_no INTEGER NOT NULL,
        state TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'new',
        note TEXT NOT NULL DEFAULT ''
    );
    CREATE TABLE review_pois (
        id INTEGER PRIMARY KEY,
        item INTEGER NOT NULL REFERENCES review_items(id) ON DELETE CASCADE,
        x1 INTEGER NOT NULL, y1 INTEGER NOT NULL, x2 INTEGER NOT NULL, y2 INTEGER NOT NULL,
        cx1 INTEGER, cy1 INTEGER, cx2 INTEGER, cy2 INTEGER,
        comment TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );
"];

pub fn migrate(db: &mut Connection) -> Result<()> {
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.pragma_update(None, "foreign_keys", true)?;
    let version: usize = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = db.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    Ok(())
}
