use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::Path;

pub type DbPool = Pool<SqliteConnectionManager>;

/// First value handed out by [`next_seq`].
///
/// Deliberately larger than any Unix-seconds value: older clients stored
/// `updated_at` (seconds) as their cursor, and a fresh sequence that started
/// at 1 would compare below those cursors and silently stop syncing forever.
/// Seeding above them makes every legacy client do one full, idempotent
/// resync and then continue on the new cursor.
pub const SYNC_SEQ_BASE: i64 = 1_000_000_000_000;

/// Allocate the next cursor value for a user.
///
/// Strictly increasing, never repeats, and independent of wall-clock
/// resolution — the property `updated_at` (seconds) does not have.
pub fn next_seq(conn: &Connection, user_id: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "INSERT INTO sync_seq (user_id, next_seq) VALUES (?1, ?2)
         ON CONFLICT(user_id) DO UPDATE SET next_seq = sync_seq.next_seq + 1
         RETURNING next_seq",
        rusqlite::params![user_id, SYNC_SEQ_BASE],
        |r| r.get(0),
    )
}

pub fn init_pool(db_path: &Path) -> Result<DbPool, rusqlite::Error> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let manager = SqliteConnectionManager::file(db_path);
    let pool = Pool::new(manager).map_err(|e| {
        // convert
        rusqlite::Error::InvalidParameterName(e.to_string())
    })?;
    // run migrations
    {
        let conn = pool.get().map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
        migrate(&conn)?;
    }
    Ok(pool)
}

fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    conn.execute_batch(include_str!("schema.sql"))?;
    // Idempotent column additions for existing DBs
    let _ = conn.execute("ALTER TABLE albums ADD COLUMN is_shared INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE photos ADD COLUMN person_ids TEXT NOT NULL DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE photos ADD COLUMN ocr_text TEXT", []);

    // Sync cursor column. Only backfill when the column is genuinely new —
    // re-running the backfill on a populated DB would hand already-synced rows
    // brand new sequence numbers below the live counter.
    if conn.execute("ALTER TABLE photos ADD COLUMN sync_seq INTEGER NOT NULL DEFAULT 0", []).is_ok() {
        conn.execute(
            "UPDATE photos SET sync_seq = ?1 + rowid WHERE sync_seq = 0",
            rusqlite::params![SYNC_SEQ_BASE],
        )?;
    }
    if conn.execute("ALTER TABLE tombstones ADD COLUMN sync_seq INTEGER NOT NULL DEFAULT 0", []).is_ok() {
        conn.execute(
            "UPDATE tombstones SET sync_seq = ?1 + rowid WHERE sync_seq = 0",
            rusqlite::params![SYNC_SEQ_BASE],
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_photos_sync_seq ON photos(user_id, sync_seq);
         CREATE INDEX IF NOT EXISTS idx_tomb_sync_seq ON tombstones(user_id, sync_seq);",
    )?;
    // Continue allocating above whatever the backfill produced.
    conn.execute(
        "INSERT INTO sync_seq (user_id, next_seq)
         SELECT user_id, MAX(seq) FROM (
             SELECT user_id, MAX(sync_seq) AS seq FROM photos GROUP BY user_id
             UNION ALL
             SELECT user_id, MAX(sync_seq) AS seq FROM tombstones GROUP BY user_id
         ) GROUP BY user_id
         ON CONFLICT(user_id) DO UPDATE SET next_seq = MAX(sync_seq.next_seq, excluded.next_seq)",
        [],
    )?;
    Ok(())
}

pub fn conn(pool: &DbPool) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, rusqlite::Error> {
    pool.get().map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))
}
