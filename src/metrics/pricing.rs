#[derive(Debug, Clone)]
pub struct ModelPricing {
    pub name: String,
    pub input_per_million: f64,
    pub cached_read_per_million: f64,
    pub output_per_million: f64,
}

impl Default for ModelPricing {
    fn default() -> Self {
        Self::claude_3_5_sonnet()
    }
}

impl ModelPricing {
    pub fn claude_3_5_sonnet() -> Self {
        Self {
            name: "claude-3-5-sonnet".to_string(),
            input_per_million: 3.00,
            cached_read_per_million: 0.30,
            output_per_million: 15.00,
        }
    }

    pub fn gpt_4o() -> Self {
        Self {
            name: "gpt-4o".to_string(),
            input_per_million: 2.50,
            cached_read_per_million: 1.25,
            output_per_million: 10.00,
        }
    }

    pub fn for_model(model_name: &str) -> Self {
        let lower = model_name.to_lowercase();
        if lower.contains("gpt") || lower.contains("o1") || lower.contains("o3") {
            Self::gpt_4o()
        } else {
            Self::claude_3_5_sonnet()
        }
    }

    /// Calculates USD saved by pruning tokens before submission.
    pub fn calculate_prune_savings(&self, tokens_saved: usize) -> f64 {
        (tokens_saved as f64 / 1_000_000.0) * self.input_per_million
    }

    /// Calculates USD saved by hitting the KV prompt cache instead of uncached input.
    pub fn calculate_cache_savings(&self, cached_tokens: usize) -> f64 {
        let differential = (self.input_per_million - self.cached_read_per_million).max(0.0);
        (cached_tokens as f64 / 1_000_000.0) * differential
    }
}
