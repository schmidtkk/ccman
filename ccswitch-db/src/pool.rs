use anyhow::Result;
use r2d2::{CustomizeConnection, Pool};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::Path;
use tracing::info;

#[derive(Debug)]
struct ForeignKeysOn;

impl CustomizeConnection<Connection, rusqlite::Error> for ForeignKeysOn {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")
    }
}

#[derive(Debug, Clone)]
pub struct DatabasePool {
    pool: Pool<SqliteConnectionManager>,
}

impl DatabasePool {
    /// Create a new connection pool with the database at the given path.
    /// If the database file does not exist, it will be created.
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let db_path = db_path.as_ref();

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let manager = SqliteConnectionManager::file(db_path);
        let pool = Pool::builder()
            .max_size(10)
            .connection_customizer(Box::new(ForeignKeysOn))
            .build(manager)?;

        info!("Database pool created: {}", db_path.display());

        Ok(Self { pool })
    }

    /// Execute a batch of SQL statements.
    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute_batch(sql)?;
        Ok(())
    }

    /// Get a connection from the pool.
    pub fn get(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        Ok(self.pool.get()?)
    }
}
