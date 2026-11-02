use anyhow::Result;
use rusqlite::Connection;

/// Append-only: each entry migrates from `user_version` = its index.
const MIGRATIONS: &[&str] = &[
    "
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
",
    "
    -- Item ids are never reused: a new item after \"delete all\" got id 1 again, and
    -- browsers and old UnknownScreen events still pointed at the deleted one.
    CREATE TABLE review_items_new (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        first_seen INTEGER NOT NULL,
        last_seen INTEGER NOT NULL,
        count INTEGER NOT NULL,
        phash INTEGER NOT NULL,
        frame_no INTEGER NOT NULL,
        state TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'new',
        note TEXT NOT NULL DEFAULT ''
    );
    INSERT INTO review_items_new SELECT * FROM review_items;
    DROP TABLE review_items;
    ALTER TABLE review_items_new RENAME TO review_items;
    DELETE FROM sqlite_sequence WHERE name = 'review_items';
    INSERT INTO sqlite_sequence (name, seq) SELECT 'review_items', max(
        coalesce((SELECT max(id) FROM review_items), 0),
        coalesce((SELECT max(json_extract(kind, '$.UnknownScreen.item')) FROM events), 0)
    );
",
];

pub fn migrate(db: &mut Connection) -> Result<()> {
    db.pragma_update(None, "journal_mode", "WAL")?;
    // Off while migrating: rebuilding a table must not cascade-delete its children.
    db.pragma_update(None, "foreign_keys", false)?;
    let version: usize = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = db.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    db.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_ids_continue_past_deleted_and_referenced_ones() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(MIGRATIONS[0]).unwrap();
        db.pragma_update(None, "user_version", 1).unwrap();
        db.execute_batch(
            "INSERT INTO review_items (id, first_seen, last_seen, count, phash, frame_no, state)
                 VALUES (3, 0, 0, 1, 0, 0, 'x');
             INSERT INTO review_pois (item, x1, y1, x2, y2, comment, created_at)
                 VALUES (3, 0, 0, 1, 1, 'kept', 0);
             INSERT INTO events (ts, frame_no, state, kind)
                 VALUES (0, 0, 'x', '{\"UnknownScreen\":{\"item\":40,\"state\":\"x\"}}');",
        )
        .unwrap();
        migrate(&mut db).unwrap();

        let pois: i64 = db.query_row("SELECT count(*) FROM review_pois", [], |r| r.get(0)).unwrap();
        assert_eq!(pois, 1, "children survive the rebuild");
        db.execute("DELETE FROM review_items", []).unwrap();
        db.execute(
            "INSERT INTO review_items (first_seen, last_seen, count, phash, frame_no, state)
             VALUES (0, 0, 1, 0, 0, 'y')",
            [],
        )
        .unwrap();
        assert_eq!(db.last_insert_rowid(), 41);
    }
}
