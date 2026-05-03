use anyhow::Result;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;
use tracing::{info, debug};

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
            .build(manager)?;

        info!("Database pool created: {}", db_path.display());

        Ok(Self { pool })
    }

    /// Run migrations from the given SQL string.
    pub fn run_migrations(&self, sql: &str) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute_batch(sql)?;
        debug!("Migrations applied successfully");
        Ok(())
    }

    /// Get a connection from the pool.
    pub fn get(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        Ok(self.pool.get()?)
    }
}
