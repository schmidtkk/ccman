use anyhow::{Context, Result};
use reqwest::blocking::Client;
use std::time::Instant;
use tracing::{info, warn};

use ccswitch_db::models::{ApiKey, HealthCheck, Provider};
use ccswitch_db::repositories::{ApiKeyRepository, HealthCheckRepository, ProviderRepository};

/// Health check service pings provider endpoints to verify key validity
pub struct HealthCheckService<P, A, H>
where
    P: ProviderRepository,
    A: ApiKeyRepository,
    H: HealthCheckRepository,
{
    provider_repo: P,
    api_key_repo: A,
    health_repo: H,
    client: Client,
    timeout_secs: u64,
}

impl<P, A, H> HealthCheckService<P, A, H>
where
    P: ProviderRepository,
    A: ApiKeyRepository,
    H: HealthCheckRepository,
{
    pub fn new(provider_repo: P, api_key_repo: A, health_repo: H) -> Self {
        Self {
            provider_repo,
            api_key_repo,
            health_repo,
            client: Client::new(),
            timeout_secs: 30,
        }
    }

    /// Check health of a single API key
    pub fn check_key(&self, key: &ApiKey, provider: &Provider) -> Result<HealthCheckResult> {
        let start = Instant::now();

        let url = if provider.base_url.is_empty() {
            // Native Claude - skip health check
            return Ok(HealthCheckResult {
                is_healthy: true,
                response_time_ms: Some(0),
                error_message: None,
            });
        } else {
            format!("{}/v1/models", provider.base_url.trim_end_matches('/'))
        };

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", key.key_value))
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .send();

        let latency = start.elapsed().as_millis() as i64;

        match response {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    Ok(HealthCheckResult {
                        is_healthy: true,
                        response_time_ms: Some(latency),
                        error_message: None,
                    })
                } else {
                    let _body = resp.text().unwrap_or_default();
                    Ok(HealthCheckResult {
                        is_healthy: false,
                        response_time_ms: Some(latency),
                        error_message: Some(format!("HTTP {}", status)),
                    })
                }
            }
            Err(e) => {
                warn!("Health check failed for key {}: {}", key.id, e);
                Ok(HealthCheckResult {
                    is_healthy: false,
                    response_time_ms: None,
                    error_message: Some(e.to_string()),
                })
            }
        }
    }

    /// Run health check for all keys of a provider, record results
    pub fn check_provider(&self, provider_name: &str) -> Result<Vec<KeyCheckResult>> {
        let provider = self
            .provider_repo
            .get_by_name(provider_name)?
            .with_context(|| format!("Provider not found: {}", provider_name))?;

        let keys = self.api_key_repo.list_by_provider(provider.id)?;
        let mut results = Vec::new();

        for key in keys {
            let check_result = self.check_key(&key, &provider)?;

            // Record in DB
            let health_check = HealthCheck {
                id: 0,
                api_key_id: key.id,
                timestamp: chrono::Utc::now().to_rfc3339(),
                is_healthy: check_result.is_healthy,
                response_time_ms: check_result.response_time_ms,
                error_message: check_result.error_message.clone(),
            };
            self.health_repo.create(&health_check)?;

            // Update key error count if unhealthy
            if !check_result.is_healthy {
                self.api_key_repo.update_usage_stats(
                    key.id,
                    false,
                    check_result.error_message.as_deref(),
                )?;
                warn!(
                    "Key {} for provider {} is unhealthy: {:?}",
                    key.id, provider_name, check_result.error_message
                );
            } else {
                // Reset error count on success
                let mut updated_key = key.clone();
                updated_key.error_count = 0;
                updated_key.last_error_at = None;
                self.api_key_repo.update(&updated_key)?;
            }

            results.push(KeyCheckResult {
                key_id: key.id,
                key_label: key.key_label,
                is_healthy: check_result.is_healthy,
                response_time_ms: check_result.response_time_ms,
                error_message: check_result.error_message,
            });

            info!(
                "Health check key={} provider={} healthy={} latency={:?}ms",
                key.id, provider_name, check_result.is_healthy, check_result.response_time_ms
            );
        }

        Ok(results)
    }

    /// Check all providers
    pub fn check_all(&self) -> Result<Vec<(Provider, Vec<KeyCheckResult>)>> {
        let providers = self.provider_repo.list()?;
        let mut all_results = Vec::new();

        for provider in providers {
            if provider.name == "claude" || provider.base_url.is_empty() {
                continue;
            }

            let results = self.check_provider(&provider.name)?;
            if !results.is_empty() {
                all_results.push((provider, results));
            }
        }

        Ok(all_results)
    }

    /// Get latest health status for all keys
    pub fn latest_status(&self) -> Result<Vec<KeyHealthStatus>> {
        let providers = self.provider_repo.list()?;
        let mut statuses = Vec::new();

        for provider in providers {
            let keys = self.api_key_repo.list_by_provider(provider.id)?;
            for key in keys {
                let latest = self.health_repo.latest_for_key(key.id)?;
                statuses.push(KeyHealthStatus {
                    provider_name: provider.name.clone(),
                    provider_display: provider.display_name.clone(),
                    key_id: key.id,
                    key_label: key.key_label.clone(),
                    is_active: key.is_active,
                    error_count: key.error_count,
                    latest_check: latest,
                });
            }
        }

        Ok(statuses)
    }
}

#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub is_healthy: bool,
    pub response_time_ms: Option<i64>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KeyCheckResult {
    pub key_id: i64,
    pub key_label: Option<String>,
    pub is_healthy: bool,
    pub response_time_ms: Option<i64>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KeyHealthStatus {
    pub provider_name: String,
    pub provider_display: String,
    pub key_id: i64,
    pub key_label: Option<String>,
    pub is_active: bool,
    pub error_count: i32,
    pub latest_check: Option<HealthCheck>,
}
