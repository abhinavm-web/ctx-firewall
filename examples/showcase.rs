use ctx_firewall::prune::{PruneConfig, PruningEngine};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== ctx-firewall: Cache-Safe In-Flight Context Pruning Showcase ===");

    let engine = PruningEngine::new(PruneConfig::default());
    let repeated_source = "pub struct BillingEngine {\n    pub rate_cents: u64,\n}\n".repeat(10);
    let noisy_test_log = (1..=120)
        .map(|i| format!("\x1b[32mCompiling\x1b[0m crate_{i} v0.1.0\r\nrunning test_case_{i} ... ok"))
        .collect::<Vec<_>>()
        .join("\n");

    let payload = json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [
            {
                "role": "user",
                "content": "Inspect src/billing.rs and run the test suite."
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_read_1",
                        "name": "read_file",
                        "input": { "path": "src/billing.rs" }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_read_1",
                        "content": repeated_source
                    }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_read_2",
                        "name": "read_file",
                        "input": { "path": "src/billing.rs" }
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_bash_1",
                        "name": "bash",
                        "input": { "command": "cargo test" }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_read_2",
                        "content": repeated_source
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_bash_1",
                        "content": noisy_test_log
                    }
                ]
            }
        ]
    });

    let (_pruned, report) = engine.prune_anthropic_payload(payload, 5)?;

    println!("Turn ID:                 {}", report.turn_id);
    println!("Original tail bytes:     {} B", report.original_tail_bytes);
    println!("Pruned tail bytes:       {} B", report.pruned_tail_bytes);
    println!("Estimated tokens saved:  {} tokens", report.estimated_tokens_saved);
    println!("Active pruning rules:    {}", report.rules_applied.join(", "));
    println!("Sealed KV-Prefix SHA256: {} (verified intact)", report.sealed_prefix_hash);

    Ok(())
}
