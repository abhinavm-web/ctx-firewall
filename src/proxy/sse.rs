use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTokenMetrics {
    pub input_tokens: Option<usize>,
    pub output_tokens: Option<usize>,
    pub cache_read_tokens: Option<usize>,
    pub cache_write_tokens: Option<usize>,
    pub delta_content_chars: usize,
}

#[derive(Debug, Default)]
pub struct SseChunkParser {
    buffer: Vec<u8>,
}

impl SseChunkParser {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
        }
    }

    /// Feeds new raw bytes from upstream chunk, parses complete events,
    /// and leaves incomplete trailing bytes in the buffer.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<ParsedTokenMetrics> {
        self.buffer.extend_from_slice(chunk);
        let mut results = Vec::new();

        let mut start_idx = 0;
        while let Some(rel_end) = self.find_delimiter(&self.buffer[start_idx..]) {
            let event_end = start_idx + rel_end;
            let event_bytes = &self.buffer[start_idx..event_end];
            if let Some(metrics) = self.parse_event_block(event_bytes) {
                results.push(metrics);
            }
            let delim_len = self.delimiter_len(&self.buffer[event_end..]);
            start_idx = event_end + delim_len;
        }

        if start_idx > 0 {
            self.buffer.drain(0..start_idx);
        }

        results
    }

    fn find_delimiter(&self, slice: &[u8]) -> Option<usize> {
        for i in 0..slice.len().saturating_sub(1) {
            if slice[i] == b'\n' && slice[i + 1] == b'\n' {
                return Some(i);
            }
            if i + 3 < slice.len()
                && slice[i] == b'\r'
                && slice[i + 1] == b'\n'
                && slice[i + 2] == b'\r'
                && slice[i + 3] == b'\n'
            {
                return Some(i);
            }
        }
        None
    }

    fn delimiter_len(&self, slice: &[u8]) -> usize {
        if slice.starts_with(b"\r\n\r\n") {
            4
        } else if slice.starts_with(b"\n\n") {
            2
        } else {
            0
        }
    }

    fn parse_event_block(&self, block: &[u8]) -> Option<ParsedTokenMetrics> {
        let text = std::str::from_utf8(block).ok()?;
        let mut data_lines = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("data:") {
                let trimmed = rest.trim();
                if !trimmed.is_empty() && trimmed != "[DONE]" {
                    data_lines.push(trimmed);
                }
            }
        }

        if data_lines.is_empty() {
            return None;
        }

        let combined = data_lines.join("");
        let val: Value = serde_json::from_str(&combined).ok()?;

        // Anthropic: message_start with cache metrics
        if let Some(message) = val.get("message") {
            if let Some(usage) = message.get("usage") {
                let input = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let cache_read = usage
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let cache_write = usage
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                return Some(ParsedTokenMetrics {
                    input_tokens: input,
                    output_tokens: None,
                    cache_read_tokens: cache_read,
                    cache_write_tokens: cache_write,
                    delta_content_chars: 0,
                });
            }
        }

        // Anthropic: message_delta with final output tokens
        // OpenAI: chat.completion.chunk with usage
        if let Some(usage) = val.get("usage") {
            let output = usage
                .get("output_tokens")
                .or_else(|| usage.get("completion_tokens"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let input = usage
                .get("input_tokens")
                .or_else(|| usage.get("prompt_tokens"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let cached = usage
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);

            return Some(ParsedTokenMetrics {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cached,
                cache_write_tokens: None,
                delta_content_chars: 0,
            });
        }

        // Output delta text accumulation
        let mut delta_chars = 0;
        if let Some(delta) = val.get("delta") {
            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                delta_chars += text.len();
            }
        }
        if let Some(choices) = val.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                if let Some(content) = choice
                    .get("delta")
                    .and_then(|d| d.get("content"))
                    .and_then(|t| t.as_str())
                {
                    delta_chars += content.len();
                }
            }
        }

        if delta_chars > 0 {
            Some(ParsedTokenMetrics {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                delta_content_chars: delta_chars,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_anthropic_message_start() {
        let mut parser = SseChunkParser::new();
        let chunk = b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":1200,\"cache_read_input_tokens\":1000}}}\n\n";
        let metrics = parser.feed(chunk);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].input_tokens, Some(1200));
        assert_eq!(metrics[0].cache_read_tokens, Some(1000));
    }
}
