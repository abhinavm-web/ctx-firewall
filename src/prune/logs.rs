use std::borrow::Cow;

pub struct LogFolder {
    pub head_lines: usize,
    pub tail_lines: usize,
    pub threshold: usize,
}

impl Default for LogFolder {
    fn default() -> Self {
        Self {
            head_lines: 20,
            tail_lines: 30,
            threshold: 60,
        }
    }
}

impl LogFolder {
    pub fn new(head_lines: usize, tail_lines: usize, threshold: usize) -> Self {
        Self {
            head_lines,
            tail_lines,
            threshold,
        }
    }

    /// Folds repetitive middle lines if line count exceeds threshold.
    pub fn fold<'a>(&self, input: &'a str) -> Cow<'a, str> {
        let total_lines = input.lines().count();
        if total_lines <= self.threshold {
            return Cow::Borrowed(input);
        }

        let mut lines = input.lines();
        let mut head = Vec::with_capacity(self.head_lines);
        for _ in 0..self.head_lines {
            if let Some(l) = lines.next() {
                head.push(l);
            }
        }

        let remaining: Vec<&str> = lines.collect();
        if remaining.len() <= self.tail_lines {
            return Cow::Borrowed(input);
        }

        let folded_count = remaining.len() - self.tail_lines;
        let tail = &remaining[folded_count..];

        let mut out = String::with_capacity(input.len());
        for line in head {
            out.push_str(line);
            out.push('\n');
        }

        out.push_str(&format!(
            "\n[... ctx-firewall: folded {} lines of repetitive build/test output ...]\n\n",
            folded_count
        ));

        for (idx, line) in tail.iter().enumerate() {
            out.push_str(line);
            if idx + 1 < tail.len() || input.ends_with('\n') {
                out.push('\n');
            }
        }

        Cow::Owned(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_logs_not_folded() {
        let folder = LogFolder::default();
        let short = "Line 1\nLine 2\nLine 3\n";
        assert_eq!(folder.fold(short), Cow::Borrowed(short));
    }

    #[test]
    fn test_long_logs_folded() {
        let folder = LogFolder::new(2, 2, 5);
        let long = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10";
        let folded = folder.fold(long);
        assert!(folded.contains("folded 6 lines"));
        assert!(folded.starts_with("1\n2\n"));
        assert!(folded.ends_with("9\n10"));
    }
}
