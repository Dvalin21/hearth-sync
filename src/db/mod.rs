use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::Path;

pub type DbPool = Pool<SqliteConnectionManager>;

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
    Ok(())
}

pub fn conn(pool: &DbPool) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, rusqlite::Error> {
    pool.get().map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))
}
