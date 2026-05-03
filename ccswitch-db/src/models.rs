use rusqlite::{Row, Result as SqliteResult};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: i64,
    pub name: String,
    pub display_name: String,
    pub base_url: String,
    pub model: Option<String>,
    pub auth_header: String,
    pub timeout_ms: i64,
    pub requires_disable_traffic: bool,
    pub usage_endpoint: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Provider {
    pub fn from_row(row: &Row) -> SqliteResult<Self> {
        Ok(Self {
            id: row.get("id")?,
            name: row.get("name")?,
            display_name: row.get("display_name")?,
            base_url: row.get("base_url")?,
            model: row.get("model")?,
            auth_header: row.get("auth_header")?,
            timeout_ms: row.get("timeout_ms")?,
            requires_disable_traffic: row.get::<_, i64>("requires_disable_traffic")? != 0,
            usage_endpoint: row.get("usage_endpoint")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

// ---------------------------------------------------------------------------
// ApiKey
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: i64,
    pub provider_id: i64,
    pub key_value: String,
    pub key_label: Option<String>,
    pub is_active: bool,
    pub priority: i32,
    pub daily_limit_cents: Option<i64>,
    pub monthly_limit_cents: Option<i64>,
    pub error_count: i32,
    pub last_used_at: Option<String>,
    pub last_error_at: Option<String>,
    pub created_at: String,
}

impl ApiKey {
    pub fn from_row(row: &Row) -> SqliteResult<Self> {
        Ok(Self {
            id: row.get("id")?,
            provider_id: row.get("provider_id")?,
            key_value: row.get("key_value")?,
            key_label: row.get("key_label")?,
            is_active: row.get::<_, i64>("is_active")? != 0,
            priority: row.get("priority")?,
            daily_limit_cents: row.get("daily_limit_cents")?,
            monthly_limit_cents: row.get("monthly_limit_cents")?,
            error_count: row.get("error_count")?,
            last_used_at: row.get("last_used_at")?,
            last_error_at: row.get("last_error_at")?,
            created_at: row.get("created_at")?,
        })
    }
}

// ---------------------------------------------------------------------------
// UsageLog
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLog {
    pub id: i64,
    pub provider_id: i64,
    pub api_key_id: Option<i64>,
    pub timestamp: String,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub prompt_cost_cents: i64,
    pub completion_cost_cents: i64,
    pub total_cost_cents: i64,
    pub usage_json: Option<String>,
    pub request_id: Option<String>,
    pub success: bool,
    pub error_message: Option<String>,
}

impl UsageLog {
    pub fn from_row(row: &Row) -> SqliteResult<Self> {
        Ok(Self {
            id: row.get("id")?,
            provider_id: row.get("provider_id")?,
            api_key_id: row.get("api_key_id")?,
            timestamp: row.get("timestamp")?,
            model: row.get("model")?,
            prompt_tokens: row.get("prompt_tokens")?,
            completion_tokens: row.get("completion_tokens")?,
            total_tokens: row.get("total_tokens")?,
            prompt_cost_cents: row.get("prompt_cost_cents")?,
            completion_cost_cents: row.get("completion_cost_cents")?,
            total_cost_cents: row.get("total_cost_cents")?,
            usage_json: row.get("usage_json")?,
            request_id: row.get("request_id")?,
            success: row.get::<_, i64>("success")? != 0,
            error_message: row.get("error_message")?,
        })
    }
}

// ---------------------------------------------------------------------------
// Pricing
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pricing {
    pub id: i64,
    pub provider_id: i64,
    pub model: String,
    pub input_price_cents_per_million: i64,
    pub output_price_cents_per_million: i64,
    pub effective_date: String,
    pub is_current: bool,
}

impl Pricing {
    pub fn from_row(row: &Row) -> SqliteResult<Self> {
        Ok(Self {
            id: row.get("id")?,
            provider_id: row.get("provider_id")?,
            model: row.get("model")?,
            input_price_cents_per_million: row.get("input_price_cents_per_million")?,
            output_price_cents_per_million: row.get("output_price_cents_per_million")?,
            effective_date: row.get("effective_date")?,
            is_current: row.get::<_, i64>("is_current")? != 0,
        })
    }
}

// ---------------------------------------------------------------------------
// HealthCheck
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub id: i64,
    pub api_key_id: i64,
    pub timestamp: String,
    pub is_healthy: bool,
    pub response_time_ms: Option<i64>,
    pub error_message: Option<String>,
}

impl HealthCheck {
    pub fn from_row(row: &Row) -> SqliteResult<Self> {
        Ok(Self {
            id: row.get("id")?,
            api_key_id: row.get("api_key_id")?,
            timestamp: row.get("timestamp")?,
            is_healthy: row.get::<_, i64>("is_healthy")? != 0,
            response_time_ms: row.get("response_time_ms")?,
            error_message: row.get("error_message")?,
        })
    }
}

// ---------------------------------------------------------------------------
// SettingsMetadata
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsMetadata {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// ProviderConfig (joined view)
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: Provider,
    pub api_key: ApiKey,
}
