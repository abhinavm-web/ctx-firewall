pub mod ansi;
pub mod cache_guard;
pub mod dedup;
pub mod logs;
pub mod mcp;

pub use ansi::AnsiStripper;
pub use cache_guard::{InvariantViolation, PrefixHash, SealedPrefixGuard};
pub use dedup::{FileReadTracker, FileRecord};
pub use logs::LogFolder;
pub use mcp::McpOptimizer;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneConfig {
    pub enable_ansi_strip: bool,
    pub enable_log_folding: bool,
    pub enable_dedup: bool,
    pub enable_lazy_mcp: bool,
    pub head_lines: usize,
    pub tail_lines: usize,
    pub log_threshold: usize,
}

impl Default for PruneConfig {
    fn default() -> Self {
        Self {
            enable_ansi_strip: true,
            enable_log_folding: true,
            enable_dedup: true,
            enable_lazy_mcp: true,
            head_lines: 20,
            tail_lines: 30,
            log_threshold: 60,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PruningReport {
    pub turn_id: u64,
    pub original_tail_bytes: usize,
    pub pruned_tail_bytes: usize,
    pub estimated_tokens_saved: usize,
    pub rules_applied: Vec<String>,
    pub sealed_prefix_hash: String,
    pub total_tokens: usize,
}

pub struct PruningEngine {
    pub config: PruneConfig,
    pub log_folder: LogFolder,
    pub file_tracker: Arc<Mutex<FileReadTracker>>,
    pub mcp_optimizer: Arc<Mutex<McpOptimizer>>,
}

impl Default for PruningEngine {
    fn default() -> Self {
        Self::new(PruneConfig::default())
    }
}

impl PruningEngine {
    pub fn new(config: PruneConfig) -> Self {
        let log_folder = LogFolder::new(config.head_lines, config.tail_lines, config.log_threshold);
        Self {
            config,
            log_folder,
            file_tracker: Arc::new(Mutex::new(FileReadTracker::new())),
            mcp_optimizer: Arc::new(Mutex::new(McpOptimizer::default())),
        }
    }

    /// Prunes an Anthropic request payload while verifying KV-cache prefix immutability.
    pub fn prune_anthropic_payload(
        &self,
        mut payload: Value,
        turn_id: u64,
    ) -> Result<(Value, PruningReport), Box<dyn std::error::Error + Send + Sync>> {
        let mut report = PruningReport {
            turn_id,
            ..Default::default()
        };

        // Extract tool_use ID to path map from assistant messages across the entire conversation
        let mut tool_use_paths: HashMap<String, String> = HashMap::new();
        if let Some(messages) = payload.get("messages").and_then(|m| m.as_array()) {
            for msg in messages {
                if msg.get("role").and_then(|r| r.as_str()) == Some("assistant") {
                    if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                        for block in content {
                            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                                    if let Some(input) = block.get("input") {
                                        if let Some(path) = input.get("path").and_then(|p| p.as_str()) {
                                            tool_use_paths.insert(id.to_string(), path.to_string());
                                        } else if let Some(file) = input.get("file").and_then(|f| f.as_str()) {
                                            tool_use_paths.insert(id.to_string(), file.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Register file reads already present in historical messages into file_tracker
        if let Some(messages) = payload.get("messages").and_then(|m| m.as_array()) {
            if messages.len() > 1 {
                let history_messages = &messages[0..messages.len() - 1];
                let mut tracker = self.file_tracker.lock().unwrap();
                for msg in history_messages {
                    if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                        for block in content {
                            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                                let path = block
                                    .get("tool_use_id")
                                    .and_then(|id| id.as_str())
                                    .and_then(|id| tool_use_paths.get(id))
                                    .cloned()
                                    .unwrap_or_default();
                                if let Some(content_str) = block.get("content").and_then(|c| c.as_str()) {
                                    tracker.register_history_read(&path, content_str, turn_id as usize);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Identify prefix boundary
        let messages = payload
            .get_mut("messages")
            .and_then(|v| v.as_array_mut())
            .ok_or("Missing or invalid messages array")?;

        if messages.is_empty() {
            return Ok((payload, report));
        }

        // Multi-turn boundary: messages[0 .. len-1] is sealed prefix; last message is tail
        let last_idx = messages.len() - 1;
        let (sealed_slice, tail_slice) = messages.split_at_mut(last_idx);

        // Canonicalize and seal prefix
        let sealed_canonical = serde_json::to_vec(&sealed_slice)?;
        let guard = SealedPrefixGuard::seal(sealed_canonical);
        report.sealed_prefix_hash = guard.hash().to_hex();

        // Prune tail message
        let tail_msg = &mut tail_slice[0];
        let mut original_bytes = 0usize;
        let mut pruned_bytes = 0usize;

        if let Some(content_array) = tail_msg.get_mut("content").and_then(|c| c.as_array_mut()) {
            for block in content_array.iter_mut() {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    let tool_id = block
                        .get("tool_use_id")
                        .and_then(|id| id.as_str())
                        .unwrap_or_default();
                    let file_path = tool_use_paths.get(tool_id).cloned().unwrap_or_default();

                    if let Some(content_val) = block.get_mut("content") {
                        if let Some(content_str) = content_val.as_str() {
                            original_bytes += content_str.len();
                            let mut current_text = content_str.to_string();

                            // 1. ANSI Stripping
                            if self.config.enable_ansi_strip {
                                let stripped = AnsiStripper::strip(&current_text);
                                if stripped.len() < current_text.len() {
                                    report.rules_applied.push("ansi_strip".to_string());
                                    current_text = stripped.into_owned();
                                }
                            }

                            // 2. Log Folding
                            if self.config.enable_log_folding {
                                let folded = self.log_folder.fold(&current_text);
                                if folded.len() < current_text.len() {
                                    report.rules_applied.push("log_folding".to_string());
                                    current_text = folded.into_owned();
                                }
                            }

                            // 3. File Read Deduplication
                            if self.config.enable_dedup {
                                let (deduped_text, is_deduped, _bytes_saved) = {
                                    let mut tracker = self.file_tracker.lock().unwrap();
                                    tracker.process_file_read(
                                        &file_path,
                                        &current_text,
                                        turn_id as usize,
                                    )
                                };
                                if is_deduped {
                                    report.rules_applied.push("file_dedup".to_string());
                                    current_text = deduped_text;
                                }
                            }

                            pruned_bytes += current_text.len();
                            *content_val = Value::String(current_text);
                        }
                    }
                }
            }
        }

        // Verify KV-cache prefix immutability
        let candidate_sealed = serde_json::to_vec(&sealed_slice)?;
        guard.verify_immutability(&candidate_sealed)?;

        // Deduplicate rule names in report
        report.rules_applied.sort();
        report.rules_applied.dedup();

        report.original_tail_bytes = original_bytes;
        report.pruned_tail_bytes = pruned_bytes;
        let bytes_saved = original_bytes.saturating_sub(pruned_bytes);
        report.estimated_tokens_saved = (bytes_saved as f64 / 3.8).round() as usize;

        Ok((payload, report))
    }

    /// Prunes an OpenAI chat completion payload while verifying KV-cache prefix immutability.
    pub fn prune_openai_payload(
        &self,
        mut payload: Value,
        turn_id: u64,
    ) -> Result<(Value, PruningReport), Box<dyn std::error::Error + Send + Sync>> {
        let mut report = PruningReport {
            turn_id,
            ..Default::default()
        };

        let messages = payload
            .get_mut("messages")
            .and_then(|v| v.as_array_mut())
            .ok_or("Missing or invalid messages array")?;

        if messages.is_empty() {
            return Ok((payload, report));
        }

        // Build tool_call_id to path map
        let mut tool_call_paths: HashMap<String, String> = HashMap::new();
        for msg in messages.iter() {
            if let Some(tool_calls) = msg.get("tool_calls").and_then(|tc| tc.as_array()) {
                for tc in tool_calls {
                    if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                        if let Some(func) = tc.get("function") {
                            if let Some(args_str) = func.get("arguments").and_then(|a| a.as_str()) {
                                if let Ok(args_json) = serde_json::from_str::<Value>(args_str) {
                                    if let Some(p) = args_json.get("path").and_then(|p| p.as_str()) {
                                        tool_call_paths.insert(id.to_string(), p.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Register historical reads
        if messages.len() > 1 {
            let history_messages = &messages[0..messages.len() - 1];
            let mut tracker = self.file_tracker.lock().unwrap();
            for msg in history_messages {
                if msg.get("role").and_then(|r| r.as_str()) == Some("tool") {
                    let path = msg
                        .get("tool_call_id")
                        .and_then(|id| id.as_str())
                        .and_then(|id| tool_call_paths.get(id))
                        .cloned()
                        .unwrap_or_default();
                    if let Some(content_str) = msg.get("content").and_then(|c| c.as_str()) {
                        tracker.register_history_read(&path, content_str, turn_id as usize);
                    }
                }
            }
        }

        let last_idx = messages.len() - 1;
        let (sealed_slice, tail_slice) = messages.split_at_mut(last_idx);

        let sealed_canonical = serde_json::to_vec(&sealed_slice)?;
        let guard = SealedPrefixGuard::seal(sealed_canonical);
        report.sealed_prefix_hash = guard.hash().to_hex();

        let tail_msg = &mut tail_slice[0];
        let mut original_bytes = 0usize;
        let mut pruned_bytes = 0usize;

        if tail_msg.get("role").and_then(|r| r.as_str()) == Some("tool") {
            let tool_id = tail_msg
                .get("tool_call_id")
                .and_then(|id| id.as_str())
                .unwrap_or_default();
            let file_path = tool_call_paths.get(tool_id).cloned().unwrap_or_default();

            if let Some(content_val) = tail_msg.get_mut("content") {
                if let Some(content_str) = content_val.as_str() {
                    original_bytes += content_str.len();
                    let mut current_text = content_str.to_string();

                    if self.config.enable_ansi_strip {
                        let stripped = AnsiStripper::strip(&current_text);
                        if stripped.len() < current_text.len() {
                            report.rules_applied.push("ansi_strip".to_string());
                            current_text = stripped.into_owned();
                        }
                    }

                    if self.config.enable_log_folding {
                        let folded = self.log_folder.fold(&current_text);
                        if folded.len() < current_text.len() {
                            report.rules_applied.push("log_folding".to_string());
                            current_text = folded.into_owned();
                        }
                    }

                    if self.config.enable_dedup {
                        let (deduped_text, is_deduped, _bytes_saved) = {
                            let mut tracker = self.file_tracker.lock().unwrap();
                            tracker.process_file_read(&file_path, &current_text, turn_id as usize)
                        };
                        if is_deduped {
                            report.rules_applied.push("file_dedup".to_string());
                            current_text = deduped_text;
                        }
                    }

                    pruned_bytes += current_text.len();
                    *content_val = Value::String(current_text);
                }
            }
        }

        let candidate_sealed = serde_json::to_vec(&sealed_slice)?;
        guard.verify_immutability(&candidate_sealed)?;

        report.rules_applied.sort();
        report.rules_applied.dedup();
        report.original_tail_bytes = original_bytes;
        report.pruned_tail_bytes = pruned_bytes;
        let bytes_saved = original_bytes.saturating_sub(pruned_bytes);
        report.estimated_tokens_saved = (bytes_saved as f64 / 3.8).round() as usize;

        Ok((payload, report))
    }
}
