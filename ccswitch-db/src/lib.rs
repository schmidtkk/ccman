pub mod migrations;
pub mod models;
pub mod pool;
pub mod repositories;

#[cfg(test)]
mod tests {
    use super::*;
    use repositories::{ApiKeyRepository, ProviderRepository};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_db() -> (pool::DatabasePool, PathBuf) {
        let n = DB_COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp =
            std::env::temp_dir().join(format!("ccswitch_test_{}_{}.db", std::process::id(), n));
        let pool = pool::DatabasePool::new(&tmp).unwrap();
        (pool, tmp)
    }

    fn run_migrations(pool: &pool::DatabasePool) {
        let sql = include_str!("../../migrations/001_initial.sql");
        pool.execute_batch(sql).unwrap();
        let sql = include_str!("../../migrations/002_fix_pricing.sql");
        pool.execute_batch(sql).unwrap();
    }

    #[test]
    fn test_pool_creates_db() {
        let (pool, tmp) = temp_db();
        assert!(tmp.exists());
        // Verify we can get a connection
        let _ = pool.get().unwrap();
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_provider_crud() {
        let (pool, tmp) = temp_db();
        run_migrations(&pool);

        let repo = repositories::SqliteProviderRepository::new(pool);

        // Create
        let provider = models::Provider {
            id: 0,
            name: "test".to_string(),
            display_name: "Test Provider".to_string(),
            base_url: "https://test.example.com".to_string(),
            model: Some("test-model".to_string()),
            auth_header: "Authorization: Bearer".to_string(),
            timeout_ms: 30000,
            requires_disable_traffic: false,
            usage_endpoint: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        let id = repo.create(&provider).unwrap();
        assert!(id > 0);

        // Read
        let fetched = repo.get_by_id(id).unwrap().unwrap();
        assert_eq!(fetched.name, "test");
        assert_eq!(fetched.display_name, "Test Provider");

        // Update
        let mut updated = fetched.clone();
        updated.display_name = "Updated".to_string();
        updated.base_url = "https://new.example.com".to_string();
        repo.update(&updated).unwrap();

        let after_update = repo.get_by_id(id).unwrap().unwrap();
        assert_eq!(after_update.display_name, "Updated");
        assert_eq!(after_update.base_url, "https://new.example.com");

        // Delete
        repo.delete(id).unwrap();
        assert!(repo.get_by_id(id).unwrap().is_none());

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_api_key_crud() {
        let (pool, tmp) = temp_db();
        run_migrations(&pool);

        let provider_repo = repositories::SqliteProviderRepository::new(pool.clone());
        let key_repo = repositories::SqliteApiKeyRepository::new(pool);

        // Need a provider first
        let provider = models::Provider {
            id: 0,
            name: "keytest".to_string(),
            display_name: "Key Test".to_string(),
            base_url: "https://key.example.com".to_string(),
            model: None,
            auth_header: "Authorization: Bearer".to_string(),
            timeout_ms: 60000,
            requires_disable_traffic: false,
            usage_endpoint: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        let provider_id = provider_repo.create(&provider).unwrap();

        // Create key
        let key = models::ApiKey {
            id: 0,
            provider_id,
            key_value: "sk-test123".to_string(),
            key_label: Some("test-key".to_string()),
            is_active: true,
            priority: 1,
            daily_limit_cents: Some(100),
            monthly_limit_cents: Some(1000),
            error_count: 0,
            last_used_at: None,
            last_error_at: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let key_id = key_repo.create(&key).unwrap();
        assert!(key_id > 0);

        // Read
        let fetched = key_repo.get_by_id(key_id).unwrap().unwrap();
        assert_eq!(fetched.key_value, "sk-test123");
        assert_eq!(fetched.key_label, Some("test-key".to_string()));

        // List by provider
        let keys = key_repo.list_by_provider(provider_id).unwrap();
        assert_eq!(keys.len(), 1);

        // Delete
        key_repo.delete(key_id).unwrap();
        assert!(key_repo.get_by_id(key_id).unwrap().is_none());

        std::fs::remove_file(&tmp).ok();
    }
}
