use ctx_firewall::prune::{PruneConfig, PruningEngine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Instant;

fn sha256_prefix(val: &Value) -> String {
    let bytes = serde_json::to_vec(val).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(&bytes);
    hex::encode(h.finalize())
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== ctx-firewall Multi-Turn A/B & Safety Benchmark (15-Turn Session) ===");

    let engine = PruningEngine::new(PruneConfig::default());
    let source_file = "pub fn verify_session(token: &str) -> bool {\n    token.len() == 64\n}\n".repeat(40);
    let noisy_build = (1..=140)
        .map(|i| format!("\x1b[32mCompiling\x1b[0m crate_{i} v0.1.0\r\nrunning test_{i} ... ok"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut tools = Vec::new();
    for i in 0..20 {
        let name = if i == 0 {
            "read_file".to_string()
        } else if i == 1 {
            "bash".to_string()
        } else {
            format!("dormant_mcp_tool_{i}")
        };
        tools.push(json!({
            "name": name,
            "description": format!("Detailed MCP tool specification for {name}.\nSupports advanced flags."),
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Target path argument" },
                    "mode": { "type": "string", "description": "Execution mode" }
                }
            }
        }));
    }

    let mut history: Vec<Value> = vec![json!({
        "role": "user",
        "content": "Audit auth.rs and fix tests."
    })];

    let mut baseline_input_bytes = 0usize;
    let mut firewall_input_bytes = 0usize;
    let mut prefix_hits = 0usize;
    let mut prev_prefix_hash: Option<String> = None;
    let mut reread_recovered = false;

    let start = Instant::now();

    for turn in 1..=15usize {
        let tool_id = format!("toolu_{turn}");
        let (tool_name, input_json, raw_result) = if turn == 1 || turn == 5 || turn == 6 {
            // Turn 1: initial read; Turn 5: duplicate read (deduped); Turn 6: immediate re-read pushback (recovered!)
            (
                "read_file",
                json!({ "path": "src/auth.rs" }),
                source_file.clone(),
            )
        } else if turn % 3 == 0 {
            (
                "bash",
                json!({ "command": "cargo test" }),
                noisy_build.clone(),
            )
        } else {
            (
                "read_file",
                json!({ "path": format!("src/mod_{turn}.rs") }),
                format!("pub fn mod_{turn}() {{}}\n").repeat(20),
            )
        };

        history.push(json!({
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": tool_id,
                "name": tool_name,
                "input": input_json
            }]
        }));
        history.push(json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": tool_id,
                "content": raw_result
            }]
        }));

        let payload = json!({
            "model": "claude-sonnet-4-20250514",
            "tools": tools,
            "messages": history
        });

        baseline_input_bytes += serde_json::to_vec(&payload)?.len();

        let (pruned, _report) = engine.prune_anthropic_payload(payload, turn as u64)?;
        firewall_input_bytes += serde_json::to_vec(&pruned)?.len();

        if turn == 6 {
            let tail_content = pruned["messages"]
                .as_array()
                .and_then(|arr| arr.last())
                .and_then(|m| m["content"].as_array())
                .and_then(|c| c.first())
                .and_then(|b| b["content"].as_str())
                .unwrap_or("");
            reread_recovered = tail_content == source_file;
        }

        // Verify prefix (messages[0..len-2]) matches the previous turn's prefix hash after stabilization
        if let Some(msgs) = pruned["messages"].as_array() {
            if msgs.len() >= 4 {
                let current_prefix = sha256_prefix(&Value::Array(msgs[..msgs.len() - 2].to_vec()));
                let stable_prev = prev_prefix_hash.as_ref().is_some_and(|p| p == &current_prefix);
                if turn > 2 && stable_prev {
                    prefix_hits += 1;
                }
            }
            if msgs.len() >= 2 {
                prev_prefix_hash = Some(sha256_prefix(&Value::Array(msgs.to_vec())));
            }
        }
    }

    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let baseline_tokens = (baseline_input_bytes as f64 / 3.8).round() as usize;
    let firewall_tokens = (firewall_input_bytes as f64 / 3.8).round() as usize;
    let savings_pct =
        ((baseline_tokens.saturating_sub(firewall_tokens)) as f64 / baseline_tokens as f64) * 100.0;

    println!("Turns simulated:          15");
    println!("Unpruned billed tokens:   {} tokens", baseline_tokens);
    println!("Firewall billed tokens:   {} tokens (-{:.1}%)", firewall_tokens, savings_pct);
    println!("KV-cache prefix hits:     {}/13 eligible follow-up turns", prefix_hits);
    println!("Immediate re-read bypass: {}", if reread_recovered { "VERIFIED (full file restored on retry)" } else { "FAILED" });
    println!("Total proxy latency:      {:.2} ms ({:.2} ms/turn)", elapsed_ms, elapsed_ms / 15.0);

    assert!(firewall_tokens < baseline_tokens);
    assert!(reread_recovered);
    Ok(())
}
