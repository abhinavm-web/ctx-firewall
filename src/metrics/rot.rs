#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotLevel {
    Low,
    Medium,
    High,
}

impl RotLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            RotLevel::Low => "LOW",
            RotLevel::Medium => "MEDIUM",
            RotLevel::High => "HIGH",
        }
    }
}

pub struct RotScoreCalculator;

impl RotScoreCalculator {
    /// Computes Context Rot Score from 0 to 100 based on noise ratio in active context.
    pub fn calculate(
        stale_read_tokens: usize,
        ansi_tokens: usize,
        folded_log_tokens: usize,
        total_context_tokens: usize,
    ) -> u8 {
        if total_context_tokens == 0 {
            return 0;
        }

        let weighted_noise = (stale_read_tokens as f64 * 1.2)
            + (ansi_tokens as f64 * 1.0)
            + (folded_log_tokens as f64 * 0.8);

        let ratio = weighted_noise / (total_context_tokens as f64);
        let score = (ratio * 100.0).round() as u64;
        score.min(100) as u8
    }

    pub fn level(score: u8) -> RotLevel {
        if score <= 25 {
            RotLevel::Low
        } else if score <= 60 {
            RotLevel::Medium
        } else {
            RotLevel::High
        }
    }
}
