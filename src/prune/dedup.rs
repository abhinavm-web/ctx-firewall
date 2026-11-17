use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub path: String,
    pub content_hash: [u8; 32],
    pub line_count: usize,
    pub byte_len: usize,
    pub turn_seen: usize,
    pub last_deduped_turn: Option<usize>,
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
        let matched_key = self
            .files
            .get(&norm_path)
            .filter(|r| r.content_hash == content_hash)
            .map(|r| r.path.clone())
            .or_else(|| self.by_hash.get(&content_hash).map(|r| r.path.clone()));

        if let Some(key_path) = matched_key {
            let pushback_retry = self
                .files
                .get(&key_path)
                .and_then(|r| r.last_deduped_turn)
                .is_some_and(|t| current_turn > 0 && t == current_turn.saturating_sub(1));

            if pushback_retry {
                if let Some(rec) = self.files.get_mut(&key_path) {
                    rec.last_deduped_turn = None;
                }
                if let Some(rec) = self.by_hash.get_mut(&content_hash) {
                    rec.last_deduped_turn = None;
                }
                return (content.to_string(), false, 0);
            }

            let line_cnt = self
                .files
                .get(&key_path)
                .map(|r| r.line_count)
                .unwrap_or(line_count);
            let tombstone = format!(
                "[ctx-firewall: content identical to previous read of '{}' ({} lines, sha256: {}). Retaining reference.]",
                key_path,
                line_cnt,
                &hex::encode(content_hash)[..8],
            );
            let saved_bytes = byte_len.saturating_sub(tombstone.len());
            if let Some(rec) = self.files.get_mut(&key_path) {
                rec.last_deduped_turn = Some(current_turn);
            }
            if let Some(rec) = self.by_hash.get_mut(&content_hash) {
                rec.last_deduped_turn = Some(current_turn);
            }
            return (tombstone, true, saved_bytes);
        }

        // New or modified file
        let record = FileRecord {
            path: norm_path.clone(),
            content_hash,
            line_count,
            byte_len,
            turn_seen: current_turn,
            last_deduped_turn: None,
        };

        self.files.insert(norm_path, record.clone());
        self.by_hash.insert(content_hash, record);

        (content.to_string(), false, 0)
    }

    /// Pre-registers a file read seen in historical turns without replacing it
    /// and without clobbering `last_deduped_turn` if the exact same content was already tracked.
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

        if self
            .files
            .get(&norm_path)
            .is_some_and(|r| r.content_hash == content_hash)
        {
            return;
        }

        let record = FileRecord {
            path: norm_path.clone(),
            content_hash,
            line_count: content.lines().count().max(1),
            byte_len: content.len(),
            turn_seen: turn,
            last_deduped_turn: None,
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

    #[test]
    fn test_reread_pushback_bypasses_dedup() {
        let mut tracker = FileReadTracker::new();
        let content = "// Important auth logic\n".repeat(25);
        let (_, d1, _) = tracker.process_file_read("src/auth.rs", &content, 1);
        assert!(!d1);

        // Turn 2: deduplicated into tombstone
        let (out2, d2, _) = tracker.process_file_read("src/auth.rs", &content, 2);
        assert!(d2);
        assert!(out2.contains("[ctx-firewall: content identical"));

        // Turn 3: agent immediately re-reads the exact same file after seeing the tombstone -> allow full read through
        let (out3, d3, saved3) = tracker.process_file_read("src/auth.rs", &content, 3);
        assert!(!d3);
        assert_eq!(saved3, 0);
        assert_eq!(out3, content);
    }
}
