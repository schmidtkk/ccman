-- ccswitch database schema v1

-- Provider configurations
CREATE TABLE IF NOT EXISTS providers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    model TEXT,
    auth_header TEXT NOT NULL DEFAULT 'Authorization: Bearer',
    timeout_ms INTEGER NOT NULL DEFAULT 60000,
    requires_disable_traffic INTEGER NOT NULL DEFAULT 0,
    usage_endpoint TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- API keys (supports multiple keys per provider)
CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id INTEGER NOT NULL,
    key_value TEXT NOT NULL,
    key_label TEXT,
    is_active INTEGER NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 0,
    daily_limit_cents INTEGER,
    monthly_limit_cents INTEGER,
    error_count INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT,
    last_error_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(provider_id, key_value),
    FOREIGN KEY (provider_id) REFERENCES providers(id) ON DELETE CASCADE
);

-- Usage logs
CREATE TABLE IF NOT EXISTS usage_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id INTEGER NOT NULL,
    api_key_id INTEGER,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    model TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL,
    prompt_cost_cents INTEGER NOT NULL,
    completion_cost_cents INTEGER NOT NULL,
    total_cost_cents INTEGER NOT NULL,
    usage_json TEXT,
    request_id TEXT,
    success INTEGER NOT NULL DEFAULT 1,
    error_message TEXT,
    FOREIGN KEY (provider_id) REFERENCES providers(id) ON DELETE CASCADE,
    FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_usage_provider_ts ON usage_logs(provider_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_usage_key_ts ON usage_logs(api_key_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_usage_model_ts ON usage_logs(model, timestamp);

-- Pricing table
CREATE TABLE IF NOT EXISTS pricing (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id INTEGER NOT NULL,
    model TEXT NOT NULL,
    input_price_cents_per_million INTEGER NOT NULL,
    output_price_cents_per_million INTEGER NOT NULL,
    effective_date TEXT NOT NULL DEFAULT (datetime('now')),
    is_current INTEGER NOT NULL DEFAULT 1,
    UNIQUE(provider_id, model, effective_date),
    FOREIGN KEY (provider_id) REFERENCES providers(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_pricing_provider_model ON pricing(provider_id, model);

-- Health checks
CREATE TABLE IF NOT EXISTS health_checks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    api_key_id INTEGER NOT NULL,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    is_healthy INTEGER NOT NULL DEFAULT 1,
    response_time_ms INTEGER,
    error_message TEXT,
    FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_health_key_ts ON health_checks(api_key_id, timestamp);

-- Settings metadata
CREATE TABLE IF NOT EXISTS settings_metadata (
    key TEXT NOT NULL UNIQUE PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Insert default active provider (none)
INSERT OR IGNORE INTO settings_metadata (key, value) VALUES ('active_provider_id', 'NULL');
