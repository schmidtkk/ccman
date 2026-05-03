use anyhow::Result;
use chrono::Utc;
use tracing::warn;

use ccswitch_db::models::ApiKey;

/// Key rotation strategy for selecting the best API key
pub struct KeyRotationStrategy {
    max_consecutive_errors: i32,
    error_reset_duration_hours: i64,
}

impl KeyRotationStrategy {
    pub fn new() -> Self {
        Self {
            max_consecutive_errors: 3,
            error_reset_duration_hours: 24,
        }
    }

    /// Select the best API key from a list of candidates
    pub fn select_best_key(
        &self,
        keys: Vec<ApiKey>,
    ) -> Result<Option<ApiKey>> {
        if keys.is_empty() {
            return Ok(None);
        }

        let now = Utc::now().fixed_offset();

        let eligible_keys: Vec<_> = keys
            .into_iter()
            .filter(|key| {
                // Must be active
                if !key.is_active {
                    return false;
                }

                // Check error count
                if key.error_count >= self.max_consecutive_errors {
                    // Check if cooldown has expired
                    if let Some(last_error) = key
                        .last_error_at
                        .as_ref()
                        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                    {
                        let hours_since_error = (now - last_error).num_hours();
                        if hours_since_error < self.error_reset_duration_hours {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }

                true
            })
            .collect();

        if eligible_keys.is_empty() {
            warn!("No eligible API keys found (all failed or over budget)");
            return Ok(None);
        }

        // Sort by priority (lower = higher priority), then by error_count, then by last_used_at
        let mut sorted_keys = eligible_keys;
        sorted_keys.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.error_count.cmp(&b.error_count))
                .then_with(|| a.last_used_at.cmp(&b.last_used_at))
        });

        Ok(sorted_keys.into_iter().next())
    }

    pub fn with_max_errors(
        mut self,
        max: i32,
    ) -> Self {
        self.max_consecutive_errors = max;
        self
    }

    pub fn with_error_reset_duration(
        mut self,
        hours: i64,
    ) -> Self {
        self.error_reset_duration_hours = hours;
        self
    }
}

impl Default for KeyRotationStrategy {
    fn default() -> Self {
        Self::new()
    }
}
