pub mod cost_calculator;
pub mod health_check;
pub mod provider;
pub mod settings;
pub mod usage_tracker;

#[cfg(test)]
mod tests {
    use super::*;
    use ccswitch_db::models::Pricing;
    use cost_calculator::{CostCalculator, TokenUsage};
    use provider::normalize_provider_name;

    #[test]
    fn test_normalize_provider_name() {
        assert_eq!(normalize_provider_name("cc"), "claude");
        assert_eq!(normalize_provider_name("zz"), "zhongzhuan");
        assert_eq!(normalize_provider_name("kimi"), "kimi");
        assert_eq!(normalize_provider_name("glm"), "glm");
        assert_eq!(normalize_provider_name("openrouter"), "openrouter");
    }

    #[test]
    fn test_cost_calculator_basic() {
        let calc = CostCalculator::new();
        let usage = TokenUsage {
            prompt_tokens: 1_000_000,
            completion_tokens: 500_000,
            total_tokens: 1_500_000,
        };
        let pricing = Pricing {
            id: 1,
            provider_id: 1,
            model: "test".to_string(),
            input_price_cents_per_million: 100,
            output_price_cents_per_million: 200,
            effective_date: "2026-01-01".to_string(),
            is_current: true,
        };

        let cost = calc.calculate(&usage, &pricing);
        assert_eq!(cost.prompt_cost_cents, 100);
        assert_eq!(cost.completion_cost_cents, 100);
        assert_eq!(cost.total_cost_cents, 200);
    }

    #[test]
    fn test_cost_calculator_rounding() {
        let calc = CostCalculator::new();
        // 500 tokens at 100 cents per million = 0.05 cents, rounds to 0
        let usage = TokenUsage {
            prompt_tokens: 500,
            completion_tokens: 0,
            total_tokens: 500,
        };
        let pricing = Pricing {
            id: 1,
            provider_id: 1,
            model: "test".to_string(),
            input_price_cents_per_million: 100,
            output_price_cents_per_million: 100,
            effective_date: "2026-01-01".to_string(),
            is_current: true,
        };

        let cost = calc.calculate(&usage, &pricing);
        // (500 * 100 + 500_000) / 1_000_000 = (50_000 + 500_000) / 1_000_000 = 550_000 / 1_000_000 = 0
        assert_eq!(cost.prompt_cost_cents, 0);
    }

    #[test]
    fn test_cost_calculator_unknown() {
        let calc = CostCalculator::new();
        let usage = TokenUsage {
            prompt_tokens: 1_000_000,
            completion_tokens: 1_000_000,
            total_tokens: 2_000_000,
        };
        let cost = calc.calculate_unknown(&usage);
        assert_eq!(cost.total_cost_cents, 0);
    }

    #[test]
    fn test_provider_service_provider_crud() {
        use ccswitch_db::pool::DatabasePool;
        use ccswitch_db::repositories::{
            SqliteApiKeyRepository, SqliteProviderRepository, SqliteSettingsRepository,
        };

        use std::sync::atomic::{AtomicU64, Ordering};
        static DB_COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = DB_COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp = std::env::temp_dir().join(format!(
            "ccswitch_core_test_{}_{}.db",
            std::process::id(),
            n
        ));
        let pool = DatabasePool::new(&tmp).unwrap();

        let sql = include_str!("../../migrations/001_initial.sql");
        pool.execute_batch(sql).unwrap();
        let sql = include_str!("../../migrations/002_fix_pricing.sql");
        pool.execute_batch(sql).unwrap();

        let provider_repo = SqliteProviderRepository::new(pool.clone());
        let api_key_repo = SqliteApiKeyRepository::new(pool.clone());
        let settings_repo = SqliteSettingsRepository::new(pool);

        let service = provider::ProviderService::new(provider_repo, api_key_repo, settings_repo);

        // Add provider
        let p = ccswitch_db::models::Provider {
            id: 0,
            name: "testsvc".to_string(),
            display_name: "Test Service".to_string(),
            base_url: "https://test.example.com".to_string(),
            model: Some("model-v1".to_string()),
            auth_header: "X-Api-Key".to_string(),
            timeout_ms: 120000,
            requires_disable_traffic: true,
            usage_endpoint: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        let id = service.add_provider(p.clone()).unwrap();
        assert!(id > 0);

        // List
        let providers = service.list_providers().unwrap();
        assert!(providers.iter().any(|p| p.name == "testsvc"));

        // Get
        let fetched = service.get_provider("testsvc").unwrap().unwrap();
        assert_eq!(fetched.display_name, "Test Service");
        assert_eq!(fetched.timeout_ms, 120000);

        // Update
        service
            .update_provider(
                "testsvc",
                Some("Updated Service".to_string()),
                Some("https://updated.example.com".to_string()),
                None,
                None,
                Some(30000),
                Some(false),
            )
            .unwrap();

        let updated = service.get_provider("testsvc").unwrap().unwrap();
        assert_eq!(updated.display_name, "Updated Service");
        assert_eq!(updated.base_url, "https://updated.example.com");
        assert_eq!(updated.timeout_ms, 30000);
        assert!(!updated.requires_disable_traffic);

        // Remove
        service.remove_provider("testsvc").unwrap();
        assert!(service.get_provider("testsvc").unwrap().is_none());

        std::fs::remove_file(&tmp).ok();
    }
}
