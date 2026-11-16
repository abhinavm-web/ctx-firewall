use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub path: String,
    pub content_hash: [u8; 32],
    pub line_count: usize,
    pub byte_len: usize,
    pub turn_seen: usize,
}

#[derive(Default, Debug)]
pub struct FileReadTracker {
    files: HashMap<String, FileRecord>,
    by_hash: HashMap<[u8; 32], FileRecord>,
}

impl FileReadTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates a file read tool result. If content matches an earlier read, returns a deduplicated tombstone.
    pub fn process_file_read(
        &mut self,
        path: &str,
        content: &str,
        current_turn: usize,
    ) -> (String, bool, usize) {
        // Only skip completely empty reads
        let byte_len = content.len();
        if byte_len == 0 {
            return (content.to_string(), false, 0);
        }

        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&hasher.finalize());

        let norm_path = if path.is_empty() {
            "unnamed_file".to_string()
        } else {
            path.replace('\\', "/")
        };

        let line_count = content.lines().count().max(1);

        // Check by path or by content hash
        let duplicate_match = self.files.get(&norm_path).filter(|r| r.content_hash == content_hash)
            .or_else(|| self.by_hash.get(&content_hash));

        if let Some(record) = duplicate_match {
            let tombstone = format!(
                "[ctx-firewall: content identical to previous read of '{}' ({} lines, sha256: {}). Retaining reference.]",
                record.path,
                record.line_count,
                &hex::encode(record.content_hash)[..8],
            );
            let saved_bytes = byte_len.saturating_sub(tombstone.len());
            return (tombstone, true, saved_bytes);
        }

        // New or modified file
        let record = FileRecord {
            path: norm_path.clone(),
            content_hash,
            line_count,
            byte_len,
            turn_seen: current_turn,
        };

        self.files.insert(norm_path, record.clone());
        self.by_hash.insert(content_hash, record);

        (content.to_string(), false, 0)
    }

    /// Pre-registers a file read seen in historical turns without replacing it.
    pub fn register_history_read(&mut self, path: &str, content: &str, turn: usize) {
        if content.is_empty() {
            return;
        }
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&hasher.finalize());

        let norm_path = if path.is_empty() {
            "unnamed_file".to_string()
        } else {
            path.replace('\\', "/")
        };

        let record = FileRecord {
            path: norm_path.clone(),
            content_hash,
            line_count: content.lines().count().max(1),
            byte_len: content.len(),
            turn_seen: turn,
        };

        self.files.insert(norm_path, record.clone());
        self.by_hash.insert(content_hash, record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_second_read() {
        let mut tracker = FileReadTracker::new();
        let content = "// Line of code in src/main.rs\n".repeat(25);
        let (out1, dedup1, _) = tracker.process_file_read("src/main.rs", &content, 1);
        assert!(!dedup1);
        assert_eq!(out1, content);

        let (out2, dedup2, saved) = tracker.process_file_read("src/main.rs", &content, 2);
        assert!(dedup2);
        assert!(saved > 0);
        assert!(out2.contains("[ctx-firewall: content identical"));
    }
}
