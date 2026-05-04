use anyhow::Result;
use std::path::Path;
use tracing::info;

use crate::pool::DatabasePool;

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
