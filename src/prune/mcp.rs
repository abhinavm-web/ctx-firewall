use serde_json::{json, Value};
use std::collections::HashSet;

pub struct McpOptimizer {
    pub active_tools: HashSet<String>,
}

impl Default for McpOptimizer {
    fn default() -> Self {
        let core_tools = &[
            "read_file",
            "view_file",
            "edit_file",
            "write_file",
            "run_command",
            "bash",
            "grep_search",
            "find_by_name",
        ];
        Self::new(core_tools)
    }
}

impl McpOptimizer {
    pub fn new(core_tools: &[&str]) -> Self {
        let active_tools = core_tools.iter().map(|s| s.to_string()).collect();
        Self { active_tools }
    }

    pub fn mark_active(&mut self, tool_name: &str) {
        self.active_tools.insert(tool_name.to_string());
    }

    /// Optimizes an array of tool schema JSON values.
    /// Dormant tools have descriptions summarized and input schemas emptied.
    pub fn optimize_tools(&self, tools: &mut [Value]) -> usize {
        let mut stubbed_count = 0;

        for tool in tools.iter_mut() {
            if let Some(obj) = tool.as_object_mut() {
                let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                if !self.active_tools.contains(name) {
                    let short_desc = format!(
                        "[MCP tool stubbed by ctx-firewall until invoked] {}",
                        obj.get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .lines()
                            .next()
                            .unwrap_or("")
                    );
                    obj.insert("description".to_string(), json!(short_desc));
                    obj.insert(
                        "input_schema".to_string(),
                        json!({ "type": "object", "properties": {} }),
                    );
                    stubbed_count += 1;
                }
            }
        }
        stubbed_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_stubbing() {
        let optimizer = McpOptimizer::default();
        let mut tools = vec![
            json!({
                "name": "read_file",
                "description": "Reads a file from disk",
                "input_schema": { "type": "object", "properties": { "path": { "type": "string" } } }
            }),
            json!({
                "name": "dormant_mcp_analyzer",
                "description": "Extremely verbose description of deep static analysis.\nSecond line.\nThird line.",
                "input_schema": { "type": "object", "properties": { "ast": { "type": "object" }, "rules": { "type": "array" } } }
            }),
        ];

        let stubbed = optimizer.optimize_tools(&mut tools);
        assert_eq!(stubbed, 1);
        // read_file was preserved
        assert_eq!(tools[0]["description"], "Reads a file from disk");
        assert!(tools[0]["input_schema"]["properties"].get("path").is_some());

        // dormant tool was stubbed
        assert!(tools[1]["description"].as_str().unwrap().contains("stubbed by ctx-firewall"));
        assert_eq!(tools[1]["input_schema"]["properties"], json!({}));
    }
}
