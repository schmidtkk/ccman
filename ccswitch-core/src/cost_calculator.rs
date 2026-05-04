use ccswitch_db::models::Pricing;

/// Parsed token usage from an API response
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

/// Cost breakdown in cents
#[derive(Debug, Clone, Default)]
pub struct CostBreakdown {
    pub prompt_cost_cents: i64,
    pub completion_cost_cents: i64,
    pub total_cost_cents: i64,
}

/// Calculates cost based on token usage and pricing
pub struct CostCalculator;

impl CostCalculator {
    pub fn new() -> Self {
        Self
    }

    /// Calculate cost given token counts and per-million pricing.
    /// Uses integer arithmetic to avoid floating-point rounding issues.
    pub fn calculate(&self, usage: &TokenUsage, pricing: &Pricing) -> CostBreakdown {
        // cost = tokens * price_per_million / 1_000_000
        // Use i128 for intermediate to prevent overflow, then round-half-up
        let prompt_cost = ((usage.prompt_tokens as i128
            * pricing.input_price_cents_per_million as i128
            + 500_000)
            / 1_000_000) as i64;
        let completion_cost = ((usage.completion_tokens as i128
            * pricing.output_price_cents_per_million as i128
            + 500_000)
            / 1_000_000) as i64;

        CostBreakdown {
            prompt_cost_cents: prompt_cost,
            completion_cost_cents: completion_cost,
            total_cost_cents: prompt_cost + completion_cost,
        }
    }

    /// Calculate cost without pricing (returns zero cost)
    pub fn calculate_unknown(&self, _usage: &TokenUsage) -> CostBreakdown {
        CostBreakdown {
            prompt_cost_cents: 0,
            completion_cost_cents: 0,
            total_cost_cents: 0,
        }
    }

    /// Format cents as dollars for display
    pub fn format_cents(cents: i64) -> String {
        format!("${:.2}", cents as f64 / 100.0)
    }
}

impl Default for CostCalculator {
    fn default() -> Self {
        Self::new()
    }
}
