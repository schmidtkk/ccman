use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use tracing::info;

use crate::models::{ApiKey, Provider};
use crate::pool::DatabasePool;

// ---------------------------------------------------------------------------
// ProviderRepository
// ---------------------------------------------------------------------------
pub trait ProviderRepository {
    fn list(&self) -> Result<Vec<Provider>>;
    fn get_by_id(&self, id: i64) -> Result<Option<Provider>>;
    fn get_by_name(&self, name: &str) -> Result<Option<Provider>>;
    fn create(&self, provider: &Provider) -> Result<i64>;
    fn update(&self, provider: &Provider) -> Result<()>;
    fn delete(&self, id: i64) -> Result<()>;
}

pub struct SqliteProviderRepository {
    pool: DatabasePool,
}

impl SqliteProviderRepository {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }
}

impl ProviderRepository for SqliteProviderRepository {
    fn list(&self) -> Result<Vec<Provider>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT * FROM providers ORDER BY name")?;

        let providers = stmt
            .query_map([], Provider::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(providers)
    }

    fn get_by_id(&self, id: i64) -> Result<Option<Provider>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT * FROM providers WHERE id = ?1")?;

        let provider = stmt.query_row([id], Provider::from_row).optional()?;

        Ok(provider)
    }

    fn get_by_name(&self, name: &str) -> Result<Option<Provider>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT * FROM providers WHERE name = ?1")?;

        let provider = stmt.query_row([name], Provider::from_row).optional()?;

        Ok(provider)
    }

    fn create(&self, provider: &Provider) -> Result<i64> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO providers (name, display_name, base_url, model, auth_header, timeout_ms, requires_disable_traffic, usage_endpoint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &provider.name,
                &provider.display_name,
                &provider.base_url,
                &provider.model,
                &provider.auth_header,
                provider.timeout_ms,
                provider.requires_disable_traffic as i64,
                &provider.usage_endpoint,
            ],
        )?;

        let id = conn.last_insert_rowid();
        info!("Provider created: {} (id={})", provider.name, id);
        Ok(id)
    }

    fn update(&self, provider: &Provider) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE providers SET
                display_name = ?2,
                base_url = ?3,
                model = ?4,
                auth_header = ?5,
                timeout_ms = ?6,
                requires_disable_traffic = ?7,
                usage_endpoint = ?8,
                updated_at = datetime('now')
             WHERE id = ?1",
            params![
                provider.id,
                &provider.display_name,
                &provider.base_url,
                &provider.model,
                &provider.auth_header,
                provider.timeout_ms,
                provider.requires_disable_traffic as i64,
                &provider.usage_endpoint,
            ],
        )?;

        Ok(())
    }

    fn delete(&self, id: i64) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute("DELETE FROM providers WHERE id = ?1", [id])?;

        info!("Provider deleted: id={}", id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ApiKeyRepository
// ---------------------------------------------------------------------------
pub trait ApiKeyRepository {
    fn list_by_provider(&self, provider_id: i64) -> Result<Vec<ApiKey>>;
    fn get_by_id(&self, id: i64) -> Result<Option<ApiKey>>;
    fn get_best_key_for_provider(&self, provider_id: i64) -> Result<Option<ApiKey>>;
    fn create(&self, key: &ApiKey) -> Result<i64>;
    fn update(&self, key: &ApiKey) -> Result<()>;
    fn update_usage_stats(
        &self,
        key_id: i64,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<()>;
    fn delete(&self, id: i64) -> Result<()>;
    fn count_by_provider(&self, provider_id: i64) -> Result<i64>;
}

pub struct SqliteApiKeyRepository {
    pool: DatabasePool,
}

impl SqliteApiKeyRepository {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }
}

impl ApiKeyRepository for SqliteApiKeyRepository {
    fn list_by_provider(&self, provider_id: i64) -> Result<Vec<ApiKey>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM api_keys WHERE provider_id = ?1 ORDER BY priority ASC, created_at DESC",
        )?;

        let keys = stmt
            .query_map([provider_id], ApiKey::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(keys)
    }

    fn get_by_id(&self, id: i64) -> Result<Option<ApiKey>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT * FROM api_keys WHERE id = ?1")?;

        let key = stmt.query_row([id], ApiKey::from_row).optional()?;

        Ok(key)
    }

    fn get_best_key_for_provider(&self, provider_id: i64) -> Result<Option<ApiKey>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM api_keys
             WHERE provider_id = ?1
               AND is_active = 1
               AND (error_count < 3 OR last_error_at < datetime('now', '-24 hours'))
             ORDER BY priority ASC, error_count ASC, last_used_at ASC
             LIMIT 1",
        )?;

        let key = stmt.query_row([provider_id], ApiKey::from_row).optional()?;

        Ok(key)
    }

    fn create(&self, key: &ApiKey) -> Result<i64> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO api_keys (provider_id, key_value, key_label, is_active, priority, daily_limit_cents, monthly_limit_cents)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                key.provider_id,
                &key.key_value,
                &key.key_label,
                key.is_active as i64,
                key.priority,
                key.daily_limit_cents,
                key.monthly_limit_cents,
            ],
        )?;

        let id = conn.last_insert_rowid();
        info!(
            "API key created: id={} for provider_id={}",
            id, key.provider_id
        );
        Ok(id)
    }

    fn update(&self, key: &ApiKey) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE api_keys SET
                key_value = ?2,
                key_label = ?3,
                is_active = ?4,
                priority = ?5,
                daily_limit_cents = ?6,
                monthly_limit_cents = ?7,
                error_count = ?8,
                last_used_at = ?9,
                last_error_at = ?10
             WHERE id = ?1",
            params![
                key.id,
                &key.key_value,
                &key.key_label,
                key.is_active as i64,
                key.priority,
                key.daily_limit_cents,
                key.monthly_limit_cents,
                key.error_count,
                &key.last_used_at,
                &key.last_error_at,
            ],
        )?;

        Ok(())
    }

    fn update_usage_stats(
        &self,
        key_id: i64,
        success: bool,
        _error_message: Option<&str>,
    ) -> Result<()> {
        let conn = self.pool.get()?;

        if success {
            conn.execute(
                "UPDATE api_keys
                 SET last_used_at = datetime('now'),
                     error_count = 0,
                     last_error_at = NULL
                 WHERE id = ?1",
                [key_id],
            )?;
        } else {
            conn.execute(
                "UPDATE api_keys
                 SET last_error_at = datetime('now'),
                     error_count = error_count + 1
                 WHERE id = ?1",
                [key_id],
            )?;
        }

        Ok(())
    }

    fn delete(&self, id: i64) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute("DELETE FROM api_keys WHERE id = ?1", [id])?;

        info!("API key deleted: id={}", id);
        Ok(())
    }

    fn count_by_provider(&self, provider_id: i64) -> Result<i64> {
        let conn = self.pool.get()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM api_keys WHERE provider_id = ?1",
            [provider_id],
            |row| row.get(0),
        )?;

        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// SettingsRepository
// ---------------------------------------------------------------------------
pub trait SettingsRepository {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn get_active_provider_id(&self) -> Result<Option<i64>>;
    fn set_active_provider_id(&self, provider_id: Option<i64>) -> Result<()>;
}

pub struct SqliteSettingsRepository {
    pool: DatabasePool,
}

impl SqliteSettingsRepository {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }
}

impl SettingsRepository for SqliteSettingsRepository {
    fn get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.pool.get()?;
        let value = conn
            .query_row(
                "SELECT value FROM settings_metadata WHERE key = ?1",
                [key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        Ok(value)
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO settings_metadata (key, value, updated_at)
             VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at",
            params![key, value],
        )?;

        Ok(())
    }

    fn get_active_provider_id(&self) -> Result<Option<i64>> {
        let value = self.get("active_provider_id")?;
        match value {
            Some(v) if v != "NULL" && !v.is_empty() => v
                .parse::<i64>()
                .map(Some)
                .context("Invalid active_provider_id"),
            _ => Ok(None),
        }
    }

    fn set_active_provider_id(&self, provider_id: Option<i64>) -> Result<()> {
        let value = provider_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "NULL".to_string());
        self.set("active_provider_id", &value)
    }
}

// ---------------------------------------------------------------------------
// UsageLogRepository
// ---------------------------------------------------------------------------
pub trait UsageLogRepository {
    fn create(&self, log: &crate::models::UsageLog) -> Result<i64>;
    fn list_by_provider(
        &self,
        provider_id: i64,
        limit: i64,
    ) -> Result<Vec<crate::models::UsageLog>>;
    fn list_recent(&self, limit: i64) -> Result<Vec<crate::models::UsageLog>>;
    fn daily_stats(&self, start_date: &str, end_date: &str) -> Result<Vec<DailyStat>>;
    fn provider_stats(&self, start_date: &str, end_date: &str) -> Result<Vec<ProviderStat>>;
    fn total_cost(&self, start_date: &str, end_date: &str) -> Result<i64>;
    fn total_tokens(&self, start_date: &str, end_date: &str) -> Result<i64>;
}

pub struct SqliteUsageLogRepository {
    pool: DatabasePool,
}

impl SqliteUsageLogRepository {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }
}

impl UsageLogRepository for SqliteUsageLogRepository {
    fn create(&self, log: &crate::models::UsageLog) -> Result<i64> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO usage_logs
             (provider_id, api_key_id, model, prompt_tokens, completion_tokens, total_tokens,
              prompt_cost_cents, completion_cost_cents, total_cost_cents,
              usage_json, request_id, success, error_message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                log.provider_id,
                log.api_key_id,
                &log.model,
                log.prompt_tokens,
                log.completion_tokens,
                log.total_tokens,
                log.prompt_cost_cents,
                log.completion_cost_cents,
                log.total_cost_cents,
                &log.usage_json,
                &log.request_id,
                log.success as i64,
                &log.error_message,
            ],
        )?;
        let id = conn.last_insert_rowid();
        Ok(id)
    }

    fn list_by_provider(
        &self,
        provider_id: i64,
        limit: i64,
    ) -> Result<Vec<crate::models::UsageLog>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM usage_logs WHERE provider_id = ?1 ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let logs = stmt
            .query_map(
                params![provider_id, limit],
                crate::models::UsageLog::from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(logs)
    }

    fn list_recent(&self, limit: i64) -> Result<Vec<crate::models::UsageLog>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT * FROM usage_logs ORDER BY timestamp DESC LIMIT ?1")?;
        let logs = stmt
            .query_map([limit], crate::models::UsageLog::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(logs)
    }

    fn daily_stats(&self, start_date: &str, end_date: &str) -> Result<Vec<DailyStat>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT
                date(timestamp) as day,
                COUNT(*) as requests,
                SUM(prompt_tokens) as prompt_tokens,
                SUM(completion_tokens) as completion_tokens,
                SUM(total_tokens) as total_tokens,
                SUM(total_cost_cents) as cost_cents
             FROM usage_logs
             WHERE date(timestamp) BETWEEN date(?1) AND date(?2)
             GROUP BY day
             ORDER BY day DESC",
        )?;
        let stats = stmt
            .query_map(params![start_date, end_date], |row| {
                Ok(DailyStat {
                    day: row.get(0)?,
                    requests: row.get(1)?,
                    prompt_tokens: row.get(2)?,
                    completion_tokens: row.get(3)?,
                    total_tokens: row.get(4)?,
                    cost_cents: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(stats)
    }

    fn provider_stats(&self, start_date: &str, end_date: &str) -> Result<Vec<ProviderStat>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT
                p.name as provider_name,
                COUNT(*) as requests,
                SUM(ul.total_tokens) as total_tokens,
                SUM(ul.total_cost_cents) as cost_cents
             FROM usage_logs ul
             JOIN providers p ON ul.provider_id = p.id
             WHERE date(ul.timestamp) BETWEEN date(?1) AND date(?2)
             GROUP BY p.name
             ORDER BY cost_cents DESC",
        )?;
        let stats = stmt
            .query_map(params![start_date, end_date], |row| {
                Ok(ProviderStat {
                    provider_name: row.get(0)?,
                    requests: row.get(1)?,
                    total_tokens: row.get(2)?,
                    cost_cents: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(stats)
    }

    fn total_cost(&self, start_date: &str, end_date: &str) -> Result<i64> {
        let conn = self.pool.get()?;
        let cost: i64 = conn.query_row(
            "SELECT COALESCE(SUM(total_cost_cents), 0) FROM usage_logs
             WHERE date(timestamp) BETWEEN date(?1) AND date(?2)",
            params![start_date, end_date],
            |row| row.get(0),
        )?;
        Ok(cost)
    }

    fn total_tokens(&self, start_date: &str, end_date: &str) -> Result<i64> {
        let conn = self.pool.get()?;
        let tokens: i64 = conn.query_row(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM usage_logs
             WHERE date(timestamp) BETWEEN date(?1) AND date(?2)",
            params![start_date, end_date],
            |row| row.get(0),
        )?;
        Ok(tokens)
    }
}

#[derive(Debug, Clone)]
pub struct DailyStat {
    pub day: String,
    pub requests: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub cost_cents: i64,
}

#[derive(Debug, Clone)]
pub struct ProviderStat {
    pub provider_name: String,
    pub requests: i64,
    pub total_tokens: i64,
    pub cost_cents: i64,
}

// ---------------------------------------------------------------------------
// PricingRepository
// ---------------------------------------------------------------------------
pub trait PricingRepository {
    fn get_current_for_model(
        &self,
        provider_id: i64,
        model: &str,
    ) -> Result<Option<crate::models::Pricing>>;
    fn list_by_provider(&self, provider_id: i64) -> Result<Vec<crate::models::Pricing>>;
    fn upsert(&self, pricing: &crate::models::Pricing) -> Result<()>;
}

pub struct SqlitePricingRepository {
    pool: DatabasePool,
}

impl SqlitePricingRepository {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }
}

impl PricingRepository for SqlitePricingRepository {
    fn get_current_for_model(
        &self,
        provider_id: i64,
        model: &str,
    ) -> Result<Option<crate::models::Pricing>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM pricing
             WHERE provider_id = ?1 AND model = ?2 AND is_current = 1
             ORDER BY effective_date DESC
             LIMIT 1",
        )?;
        let pricing = stmt
            .query_row(
                params![provider_id, model],
                crate::models::Pricing::from_row,
            )
            .optional()?;
        Ok(pricing)
    }

    fn list_by_provider(&self, provider_id: i64) -> Result<Vec<crate::models::Pricing>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM pricing WHERE provider_id = ?1 ORDER BY model, effective_date DESC",
        )?;
        let pricings = stmt
            .query_map([provider_id], crate::models::Pricing::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(pricings)
    }

    fn upsert(&self, pricing: &crate::models::Pricing) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO pricing (provider_id, model, input_price_cents_per_million, output_price_cents_per_million, effective_date, is_current)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(provider_id, model, effective_date) DO UPDATE SET
                input_price_cents_per_million = excluded.input_price_cents_per_million,
                output_price_cents_per_million = excluded.output_price_cents_per_million,
                is_current = excluded.is_current",
            params![
                pricing.provider_id,
                &pricing.model,
                pricing.input_price_cents_per_million,
                pricing.output_price_cents_per_million,
                &pricing.effective_date,
                pricing.is_current as i64,
            ],
        )?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// HealthCheckRepository
// ---------------------------------------------------------------------------
pub trait HealthCheckRepository {
    fn create(&self, check: &crate::models::HealthCheck) -> Result<i64>;
    fn list_recent(&self, api_key_id: i64, limit: i64) -> Result<Vec<crate::models::HealthCheck>>;
    fn list_all_recent(&self, limit: i64) -> Result<Vec<crate::models::HealthCheck>>;
    fn latest_for_key(&self, api_key_id: i64) -> Result<Option<crate::models::HealthCheck>>;
}

pub struct SqliteHealthCheckRepository {
    pool: DatabasePool,
}

impl SqliteHealthCheckRepository {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }
}

impl HealthCheckRepository for SqliteHealthCheckRepository {
    fn create(&self, check: &crate::models::HealthCheck) -> Result<i64> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO health_checks (api_key_id, is_healthy, response_time_ms, error_message)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                check.api_key_id,
                check.is_healthy as i64,
                check.response_time_ms,
                &check.error_message,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    fn list_recent(&self, api_key_id: i64, limit: i64) -> Result<Vec<crate::models::HealthCheck>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM health_checks WHERE api_key_id = ?1 ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let checks = stmt
            .query_map(
                params![api_key_id, limit],
                crate::models::HealthCheck::from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(checks)
    }

    fn list_all_recent(&self, limit: i64) -> Result<Vec<crate::models::HealthCheck>> {
        let conn = self.pool.get()?;
        let mut stmt =
            conn.prepare("SELECT * FROM health_checks ORDER BY timestamp DESC LIMIT ?1")?;
        let checks = stmt
            .query_map([limit], crate::models::HealthCheck::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(checks)
    }

    fn latest_for_key(&self, api_key_id: i64) -> Result<Option<crate::models::HealthCheck>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM health_checks WHERE api_key_id = ?1 ORDER BY timestamp DESC LIMIT 1",
        )?;
        let check = stmt
            .query_row([api_key_id], crate::models::HealthCheck::from_row)
            .optional()?;
        Ok(check)
    }
}
