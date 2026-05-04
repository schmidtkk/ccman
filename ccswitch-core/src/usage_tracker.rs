use anyhow::{Context, Result};
use ccswitch_db::models::UsageLog;
use ccswitch_db::repositories::{DailyStat, PricingRepository, ProviderStat, UsageLogRepository};
use chrono::{Datelike, Local, NaiveDate};
use serde_json::Value;

use crate::cost_calculator::{CostCalculator, TokenUsage};

/// Tracks API usage: parses responses, calculates costs, and persists logs
pub struct UsageTracker<U, P>
where
    U: UsageLogRepository,
    P: PricingRepository,
{
    usage_repo: U,
    pricing_repo: P,
    calculator: CostCalculator,
}

impl<U, P> UsageTracker<U, P>
where
    U: UsageLogRepository,
    P: PricingRepository,
{
    pub fn new(usage_repo: U, pricing_repo: P) -> Self {
        Self {
            usage_repo,
            pricing_repo,
            calculator: CostCalculator::new(),
        }
    }

    /// Parse usage from an OpenAI-compatible API response JSON
    pub fn parse_usage(response: &str) -> Result<TokenUsage> {
        let json: Value = serde_json::from_str(response)
            .with_context(|| "Failed to parse API response as JSON")?;

        let usage = json.get("usage").context("No usage field in response")?;

        let prompt_tokens = usage["prompt_tokens"].as_i64().unwrap_or(0);
        let completion_tokens = usage["completion_tokens"].as_i64().unwrap_or(0);
        let total_tokens = usage["total_tokens"]
            .as_i64()
            .unwrap_or(prompt_tokens + completion_tokens);

        Ok(TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        })
    }

    /// Manually log usage with known token counts
    pub fn record_manual(
        &self,
        provider_id: i64,
        api_key_id: Option<i64>,
        model: &str,
        prompt_tokens: i64,
        completion_tokens: i64,
        request_id: Option<&str>,
    ) -> Result<i64> {
        let total_tokens = prompt_tokens + completion_tokens;
        let token_usage = TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        };

        let cost = match self.pricing_repo.get_current_for_model(provider_id, model) {
            Ok(Some(pricing)) => self.calculator.calculate(&token_usage, &pricing),
            _ => self.calculator.calculate_unknown(&token_usage),
        };

        let log = UsageLog {
            id: 0,
            provider_id,
            api_key_id,
            timestamp: chrono::Utc::now().to_rfc3339(),
            model: model.to_string(),
            prompt_tokens,
            completion_tokens,
            total_tokens,
            prompt_cost_cents: cost.prompt_cost_cents,
            completion_cost_cents: cost.completion_cost_cents,
            total_cost_cents: cost.total_cost_cents,
            usage_json: None,
            request_id: request_id.map(|s| s.to_string()),
            success: true,
            error_message: None,
        };

        self.usage_repo.create(&log)
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// Get today's stats
    pub fn today_stats(&self) -> Result<Vec<DailyStat>> {
        let today = Local::now().format("%Y-%m-%d").to_string();
        self.usage_repo.daily_stats(&today, &today)
    }

    /// Get this month's daily stats
    pub fn month_stats(&self) -> Result<Vec<DailyStat>> {
        let now = Local::now();
        let start = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
            .unwrap()
            .format("%Y-%m-%d")
            .to_string();
        let end = now.format("%Y-%m-%d").to_string();
        self.usage_repo.daily_stats(&start, &end)
    }

    /// Get provider stats for a date range
    pub fn provider_stats(&self, start: &str, end: &str) -> Result<Vec<ProviderStat>> {
        self.usage_repo.provider_stats(start, end)
    }

    /// Get total cost for a date range
    pub fn total_cost(&self, start: &str, end: &str) -> Result<i64> {
        self.usage_repo.total_cost(start, end)
    }

    /// Get total tokens for a date range
    pub fn total_tokens(&self, start: &str, end: &str) -> Result<i64> {
        self.usage_repo.total_tokens(start, end)
    }

    /// Get recent usage logs
    pub fn recent_logs(&self, limit: i64) -> Result<Vec<UsageLog>> {
        self.usage_repo.list_recent(limit)
    }
}
