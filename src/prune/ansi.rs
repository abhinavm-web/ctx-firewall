use std::borrow::Cow;

pub struct AnsiStripper;

impl AnsiStripper {
    /// Strips ANSI CSI and OSC escape sequences and collapses carriage-return progress bars.
    /// Returns `Cow::Borrowed` if no escape sequences or `\r` exist.
    pub fn strip(input: &str) -> Cow<'_, str> {
        let bytes = input.as_bytes();
        if !bytes.iter().any(|&b| b == 0x1b || b == b'\r') {
            return Cow::Borrowed(input);
        }
        Cow::Owned(Self::strip_to_string(input))
    }

    fn strip_to_string(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let lines = input.split('\n');
        let mut first = true;

        for line in lines {
            if !first {
                out.push('\n');
            }
            first = false;

            // Handle \r overwrite: keep only the text after the last \r on this line
            let effective_line = match line.rfind('\r') {
                Some(idx) => &line[idx + 1..],
                None => line,
            };

            // Strip ANSI escape sequences from effective_line
            let bytes = effective_line.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == 0x1b {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                        // CSI sequence: ESC [ ... final_byte (0x40..=0x7e)
                        i += 2;
                        while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                            i += 1;
                        }
                        if i < bytes.len() {
                            i += 1; // skip final byte
                        }
                        continue;
                    } else if i + 1 < bytes.len() && bytes[i + 1] == b']' {
                        // OSC sequence: ESC ] ... (BEL or ESC \)
                        i += 2;
                        while i < bytes.len() {
                            if bytes[i] == 0x07 {
                                i += 1;
                                break;
                            } else if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                        continue;
                    } else {
                        // Single char escape
                        i += 2;
                        continue;
                    }
                }
                out.push(bytes[i] as char);
                i += 1;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_ansi() {
        let s = "Hello, world!";
        assert_eq!(AnsiStripper::strip(s), Cow::Borrowed(s));
    }

    #[test]
    fn test_strip_color_codes() {
        let s = "\x1b[32mPASS\x1b[0m: all tests passed";
        let cleaned = AnsiStripper::strip(s);
        assert_eq!(cleaned, "PASS: all tests passed");
    }

    #[test]
    fn test_collapse_carriage_returns() {
        let s = "Building: 10%\rBuilding: 50%\rBuilding: 100%\nDone.";
        let cleaned = AnsiStripper::strip(s);
        assert_eq!(cleaned, "Building: 100%\nDone.");
    }
}
