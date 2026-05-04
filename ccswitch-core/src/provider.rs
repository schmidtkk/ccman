use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

use ccswitch_db::models::{ApiKey, Provider};
use ccswitch_db::repositories::{ApiKeyRepository, ProviderRepository, SettingsRepository};

/// Environment variables that ccswitch manages in ~/.claude/settings.json
pub const CCSWITCH_ENV_KEYS: [&str; 5] = [
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_MODEL",
    "API_TIMEOUT_MS",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
];

/// Environment variable representation for shell export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVars {
    pub anthropic_base_url: Option<String>,
    pub anthropic_auth_token: Option<String>,
    pub anthropic_model: Option<String>,
    pub api_timeout_ms: Option<String>,
    pub claude_code_disable_nonessential_traffic: Option<String>,
}

impl EnvVars {
    pub fn to_export_pairs(&self) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        if let Some(v) = &self.anthropic_base_url {
            pairs.push(("ANTHROPIC_BASE_URL".to_string(), v.clone()));
        }
        if let Some(v) = &self.anthropic_auth_token {
            pairs.push(("ANTHROPIC_AUTH_TOKEN".to_string(), v.clone()));
        }
        if let Some(v) = &self.anthropic_model {
            pairs.push(("ANTHROPIC_MODEL".to_string(), v.clone()));
        }
        if let Some(v) = &self.api_timeout_ms {
            pairs.push(("API_TIMEOUT_MS".to_string(), v.clone()));
        }
        if let Some(v) = &self.claude_code_disable_nonessential_traffic {
            pairs.push((
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
                v.clone(),
            ));
        }
        pairs
    }

    pub fn is_empty(&self) -> bool {
        self.anthropic_base_url.is_none()
            && self.anthropic_auth_token.is_none()
            && self.anthropic_model.is_none()
            && self.api_timeout_ms.is_none()
            && self.claude_code_disable_nonessential_traffic.is_none()
    }
}

/// Normalize provider name aliases (e.g., "cc" -> "claude", "zz" -> "zhongzhuan")
pub fn normalize_provider_name(name: &str) -> &str {
    match name {
        "cc" => "claude",
        "zz" => "zhongzhuan",
        other => other,
    }
}

/// Provider service handles provider management and switching
pub struct ProviderService<P, A, S>
where
    P: ProviderRepository,
    A: ApiKeyRepository,
    S: SettingsRepository,
{
    provider_repo: P,
    api_key_repo: A,
    settings_repo: S,
}

impl<P, A, S> ProviderService<P, A, S>
where
    P: ProviderRepository,
    A: ApiKeyRepository,
    S: SettingsRepository,
{
    pub fn new(provider_repo: P, api_key_repo: A, settings_repo: S) -> Self {
        Self {
            provider_repo,
            api_key_repo,
            settings_repo,
        }
    }

    /// List all providers
    pub fn list_providers(&self) -> Result<Vec<Provider>> {
        self.provider_repo.list()
    }

    /// Get provider by name
    pub fn get_provider(&self, name: &str) -> Result<Option<Provider>> {
        let name = normalize_provider_name(name);
        self.provider_repo.get_by_name(name)
    }

    /// Get environment variables for a provider (shell env mode)
    pub fn get_env_vars(&self, provider_name: &str) -> Result<EnvVars> {
        let provider_name = normalize_provider_name(provider_name);
        let provider = self
            .provider_repo
            .get_by_name(provider_name)?
            .with_context(|| format!("Provider not found: {}", provider_name))?;

        let key = self
            .api_key_repo
            .get_best_key_for_provider(provider.id)?
            .with_context(|| format!("No API key available for provider: {}", provider_name))?;

        let env_vars = EnvVars {
            anthropic_base_url: if provider.base_url.is_empty() {
                None
            } else {
                Some(provider.base_url.clone())
            },
            anthropic_auth_token: Some(key.key_value.clone()),
            anthropic_model: provider.model.clone(),
            api_timeout_ms: if provider.timeout_ms > 0 {
                Some(provider.timeout_ms.to_string())
            } else {
                None
            },
            claude_code_disable_nonessential_traffic: if provider.requires_disable_traffic {
                Some("1".to_string())
            } else {
                None
            },
        };

        Ok(env_vars)
    }

    /// Switch to a provider (update active provider in DB)
    pub fn switch_to_provider(&self, provider_name: &str) -> Result<EnvVars> {
        let provider_name = normalize_provider_name(provider_name);
        let provider = self
            .provider_repo
            .get_by_name(provider_name)?
            .with_context(|| format!("Provider not found: {}", provider_name))?;

        // Native Claude mode: clear active provider
        if provider_name == "claude" {
            self.settings_repo.set_active_provider_id(None)?;
            info!("Switched to native Claude mode");
            return Ok(EnvVars {
                anthropic_base_url: None,
                anthropic_auth_token: None,
                anthropic_model: None,
                api_timeout_ms: None,
                claude_code_disable_nonessential_traffic: None,
            });
        }

        let key = self
            .api_key_repo
            .get_best_key_for_provider(provider.id)?
            .with_context(|| format!("No API key available for provider: {}", provider_name))?;

        let env_vars = EnvVars {
            anthropic_base_url: if provider.base_url.is_empty() {
                None
            } else {
                Some(provider.base_url.clone())
            },
            anthropic_auth_token: Some(key.key_value.clone()),
            anthropic_model: provider.model.clone(),
            api_timeout_ms: if provider.timeout_ms > 0 {
                Some(provider.timeout_ms.to_string())
            } else {
                None
            },
            claude_code_disable_nonessential_traffic: if provider.requires_disable_traffic {
                Some("1".to_string())
            } else {
                None
            },
        };

        // Update active provider in settings
        self.settings_repo
            .set_active_provider_id(Some(provider.id))?;

        // Update key usage stats
        self.api_key_repo.update_usage_stats(key.id, true, None)?;

        info!(
            "Switched to provider: {} using key: {}",
            provider_name,
            key.key_label.as_deref().unwrap_or(&key.id.to_string())
        );

        Ok(env_vars)
    }

    /// Get current active provider
    pub fn get_active_provider(&self) -> Result<Option<(Provider, ApiKey)>> {
        let active_id = self.settings_repo.get_active_provider_id()?;

        match active_id {
            Some(id) => {
                let provider = self.provider_repo.get_by_id(id)?;
                match provider {
                    Some(p) => {
                        let key = self.api_key_repo.get_best_key_for_provider(p.id)?;
                        Ok(key.map(|k| (p, k)))
                    }
                    None => Ok(None),
                }
            }
            None => Ok(None),
        }
    }

    // -----------------------------------------------------------------------
    // Key management
    // -----------------------------------------------------------------------

    /// Add an API key for a provider
    pub fn add_key(
        &self,
        provider_name: &str,
        key_value: String,
        label: Option<String>,
        priority: Option<i32>,
    ) -> Result<i64> {
        let provider = self
            .get_provider(provider_name)?
            .with_context(|| format!("Provider not found: {}", provider_name))?;

        let key = ApiKey {
            id: 0,
            provider_id: provider.id,
            key_value,
            key_label: label,
            is_active: true,
            priority: priority.unwrap_or(0),
            daily_limit_cents: None,
            monthly_limit_cents: None,
            error_count: 0,
            last_used_at: None,
            last_error_at: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let id = self.api_key_repo.create(&key)?;
        info!("Added key id={} for provider {}", id, provider_name);
        Ok(id)
    }

    /// List keys for a specific provider
    pub fn list_keys(&self, provider_name: &str) -> Result<Vec<ApiKey>> {
        let provider = self
            .get_provider(provider_name)?
            .with_context(|| format!("Provider not found: {}", provider_name))?;

        self.api_key_repo.list_by_provider(provider.id)
    }

    /// List keys across all providers (returns (Provider, Vec<ApiKey>) pairs)
    pub fn list_all_keys(&self) -> Result<Vec<(Provider, Vec<ApiKey>)>> {
        let providers = self.provider_repo.list()?;
        let mut result = Vec::new();
        for p in providers {
            let keys = self.api_key_repo.list_by_provider(p.id)?;
            if !keys.is_empty() {
                result.push((p, keys));
            }
        }
        Ok(result)
    }

    /// Remove an API key by ID
    pub fn remove_key(&self, key_id: i64) -> Result<()> {
        self.api_key_repo.delete(key_id)?;
        info!("Removed key id={}", key_id);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Provider CRUD
    // -----------------------------------------------------------------------

    /// Add a new provider
    pub fn add_provider(&self, provider: Provider) -> Result<i64> {
        let id = self.provider_repo.create(&provider)?;
        info!("Added provider: {} (id={})", provider.name, id);
        Ok(id)
    }

    /// Update an existing provider by name
    #[allow(clippy::too_many_arguments)]
    pub fn update_provider(
        &self,
        name: &str,
        display_name: Option<String>,
        base_url: Option<String>,
        model: Option<String>,
        auth_header: Option<String>,
        timeout_ms: Option<i64>,
        requires_disable_traffic: Option<bool>,
    ) -> Result<()> {
        let mut provider = self
            .get_provider(name)?
            .with_context(|| format!("Provider not found: {}", name))?;

        if let Some(v) = display_name {
            provider.display_name = v;
        }
        if let Some(v) = base_url {
            provider.base_url = v;
        }
        if let Some(v) = model {
            provider.model = Some(v);
        }
        if let Some(v) = auth_header {
            provider.auth_header = v;
        }
        if let Some(v) = timeout_ms {
            provider.timeout_ms = v;
        }
        if let Some(v) = requires_disable_traffic {
            provider.requires_disable_traffic = v;
        }

        self.provider_repo.update(&provider)?;
        info!("Updated provider: {}", name);
        Ok(())
    }

    /// Remove a provider by name
    pub fn remove_provider(&self, name: &str) -> Result<()> {
        let provider = self
            .get_provider(name)?
            .with_context(|| format!("Provider not found: {}", name))?;
        self.provider_repo.delete(provider.id)?;
        info!("Removed provider: {}", name);
        Ok(())
    }
}
