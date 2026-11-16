pub mod pricing;
pub mod rot;

pub use pricing::ModelPricing;
pub use rot::{RotLevel, RotScoreCalculator};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextBreakdown {
    pub system_tokens: usize,
    pub tools_tokens: usize,
    pub file_content_tokens: usize,
    pub logs_tokens: usize,
    pub history_tokens: usize,
    pub total_tokens: usize,
}

pub struct TokenEstimator;

impl TokenEstimator {
    /// In-flight token estimation: calibrated ~3.8 bytes per token for code & English prose.
    pub fn estimate_tokens(text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }
        (text.len() * 10 / 38).max(1)
    }

    /// Analyzes an Anthropic Messages request JSON payload and decomposes it into component categories.
    pub fn analyze_anthropic_request(body: &serde_json::Value) -> ContextBreakdown {
        let mut breakdown = ContextBreakdown::default();

        // System prompt
        if let Some(sys) = body.get("system") {
            if let Some(s) = sys.as_str() {
                breakdown.system_tokens = Self::estimate_tokens(s);
            } else if let Some(arr) = sys.as_array() {
                breakdown.system_tokens = arr
                    .iter()
                    .filter_map(|v| v.get("text").and_then(|t| t.as_str()))
                    .map(Self::estimate_tokens)
                    .sum();
            }
        }

        // Tools schemas
        if let Some(tools) = body.get("tools") {
            if let Ok(serialized) = serde_json::to_string(tools) {
                breakdown.tools_tokens = Self::estimate_tokens(&serialized);
            }
        }

        // Messages breakdown
        if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
            for msg in messages {
                if let Some(content) = msg.get("content") {
                    if let Some(s) = content.as_str() {
                        breakdown.history_tokens += Self::estimate_tokens(s);
                    } else if let Some(blocks) = content.as_array() {
                        for block in blocks {
                            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            if block_type == "tool_result" {
                                let c = block.get("content").and_then(|c| c.as_str()).unwrap_or("");
                                let est = Self::estimate_tokens(c);
                                if is_source_file_content(c) {
                                    breakdown.file_content_tokens += est;
                                } else if is_log_output(c) {
                                    breakdown.logs_tokens += est;
                                } else {
                                    breakdown.history_tokens += est;
                                }
                            } else if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                breakdown.history_tokens += Self::estimate_tokens(t);
                            }
                        }
                    }
                }
            }
        }

        breakdown.total_tokens = breakdown.system_tokens
            + breakdown.tools_tokens
            + breakdown.file_content_tokens
            + breakdown.logs_tokens
            + breakdown.history_tokens;

        breakdown
    }

    /// Analyzes an OpenAI Chat Completions request JSON payload and decomposes it into component categories.
    pub fn analyze_openai_request(body: &serde_json::Value) -> ContextBreakdown {
        let mut breakdown = ContextBreakdown::default();

        // Tools
        if let Some(tools) = body.get("tools") {
            if let Ok(serialized) = serde_json::to_string(tools) {
                breakdown.tools_tokens = Self::estimate_tokens(&serialized);
            }
        }

        // Messages
        if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
            for msg in messages {
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
                let content_str = match msg.get("content") {
                    Some(serde_json::Value::String(s)) => s.as_str(),
                    _ => "",
                };
                let est = Self::estimate_tokens(content_str);

                if role == "system" || role == "developer" {
                    breakdown.system_tokens += est;
                } else if role == "tool" {
                    if is_source_file_content(content_str) {
                        breakdown.file_content_tokens += est;
                    } else if is_log_output(content_str) {
                        breakdown.logs_tokens += est;
                    } else {
                        breakdown.history_tokens += est;
                    }
                } else {
                    breakdown.history_tokens += est;
                }
            }
        }

        breakdown.total_tokens = breakdown.system_tokens
            + breakdown.tools_tokens
            + breakdown.file_content_tokens
            + breakdown.logs_tokens
            + breakdown.history_tokens;

        breakdown
    }
}

fn is_source_file_content(text: &str) -> bool {
    text.contains("fn ")
        || text.contains("import ")
        || text.contains("class ")
        || text.contains("pub ")
        || text.contains("const ")
        || text.contains("let ")
        || text.contains("#include")
}

fn is_log_output(text: &str) -> bool {
    text.contains("passed")
        || text.contains("failed")
        || text.contains("error:")
        || text.contains("warning:")
        || text.contains("Compiling")
        || text.contains("Finished")
        || text.contains("test result:")
}
