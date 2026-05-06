use anyhow::Result;
use std::path::Path;
use tracing::info;

use crate::pool::DatabasePool;

/// Migrations compiled into the binary. Ordered by filename.
const EMBEDDED_MIGRATIONS: &[(&str, &str)] = &[
    ("001_initial.sql", include_str!("../../migrations/001_initial.sql")),
    ("002_fix_pricing.sql", include_str!("../../migrations/002_fix_pricing.sql")),
];

/// Run all migrations embedded in the binary at compile time.
pub fn run_embedded_migrations(pool: &DatabasePool) -> Result<()> {
    for (name, sql) in EMBEDDED_MIGRATIONS {
        pool.execute_batch(sql)?;
        info!("Migration applied: {}", name);
    }
    Ok(())
}

/// Run all SQL migrations from a directory.
pub fn run_migrations_from_dir<P: AsRef<Path>>(
    pool: &DatabasePool,
    migrations_dir: P,
) -> Result<()> {
    let migrations_dir = migrations_dir.as_ref();

    if !migrations_dir.exists() {
        anyhow::bail!(
            "Migrations directory not found: {}",
            migrations_dir.display()
        );
    }

    let mut entries: Vec<_> = std::fs::read_dir(migrations_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            path.is_file() && path.extension().is_some_and(|ext| ext == "sql")
        })
        .collect();

    // Sort by filename to ensure order
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let sql = std::fs::read_to_string(&path)?;

        pool.execute_batch(&sql)?;
        info!(
            "Migration applied: {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
    }

    Ok(())
}
