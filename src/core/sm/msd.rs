//! MSD tokenizer — shared by the SSC and SM parsers.
//!
//! Ported from `ddr-chart-tools` `src/ssc/msd.rs` (a733404) with one
//! change: the tool walked bytes and pushed each as a `char`, garbling
//! anything non-ASCII; this version walks `char`s so decoded Shift-JIS /
//! UTF-8 titles survive. Grammar is unchanged:
//!
//! - `#` starts a value, `:` separates params, `;` terminates;
//! - `//` begins a line comment;
//! - a `#` at the start of a line while a value is open recovers a
//!   missing `;` (StepMania tolerates this; real files rely on it);
//! - backslash escapes the next character.
//!
//! Every loop advances through the input once, so work is linear in the
//! input size the caller already capped.

/// One parsed MSD value. `params[0]` is conventionally the tag name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MsdValue {
    pub params: Vec<String>,
}

impl MsdValue {
    /// Tag name as written, or empty if the value had no params.
    #[must_use]
    pub fn tag(&self) -> &str {
        self.params.first().map(String::as_str).unwrap_or("")
    }

    /// Nth sub-field of the value, or `""` if absent.
    #[must_use]
    pub fn param(&self, n: usize) -> &str {
        self.params.get(n).map(String::as_str).unwrap_or("")
    }
}

/// Tokenize the full input into a list of MSD values.
#[must_use]
pub fn tokenize(input: &str) -> Vec<MsdValue> {
    let mut out: Vec<MsdValue> = Vec::new();
    let mut chars = input.chars().peekable();
    let mut reading_value = false;
    // Current param being accumulated; `None` means "no param started
    // yet since the last control character".
    let mut cur: Option<String> = None;

    while let Some(c) = chars.next() {
        // Line comment — skip to end of line (the newline itself stays,
        // as in the tool, so a comment never glues two lines together).
        if c == '/' && chars.peek() == Some(&'/') {
            while let Some(&n) = chars.peek() {
                if n == '\n' {
                    break;
                }
                chars.next();
            }
            continue;
        }

        if !reading_value {
            if c == '#' {
                out.push(MsdValue::default());
                reading_value = true;
                cur = None;
            }
            // Everything outside a value is ignored.
            continue;
        }

        let Some(value) = out.last_mut() else {
            // Unreachable: `reading_value` is only set after a push.
            reading_value = false;
            continue;
        };

        // Missing-`;` recovery: a `#` at the start of a line while still
        // inside a value closes the current value and opens a new one.
        if c == '#' && is_first_non_whitespace_on_line(&cur) {
            finalize_param(value, &mut cur);
            trim_trailing_whitespace_from_last_param(value);
            out.push(MsdValue::default());
            cur = None;
            continue;
        }

        if c == ':' {
            finalize_param(value, &mut cur);
            continue;
        }

        if c == ';' {
            finalize_param(value, &mut cur);
            reading_value = false;
            continue;
        }

        // Backslash escape: consume the next character verbatim.
        if c == '\\' {
            if let Some(n) = chars.next() {
                cur.get_or_insert_with(String::new).push(n);
            }
            continue;
        }

        cur.get_or_insert_with(String::new).push(c);
    }

    // Flush any unterminated value.
    if reading_value {
        if let Some(value) = out.last_mut() {
            if cur.is_some() {
                finalize_param(value, &mut cur);
            }
        }
    }

    out
}

fn finalize_param(value: &mut MsdValue, cur: &mut Option<String>) {
    value.params.push(cur.take().unwrap_or_default());
}

/// True when the param being built is empty or holds only whitespace
/// since the last newline — the condition for missing-`;` recovery.
fn is_first_non_whitespace_on_line(cur: &Option<String>) -> bool {
    let Some(s) = cur else { return true };
    for c in s.chars().rev() {
        if c == '\n' || c == '\r' {
            return true;
        }
        if c.is_whitespace() {
            continue;
        }
        return false;
    }
    true
}

fn trim_trailing_whitespace_from_last_param(value: &mut MsdValue) {
    if let Some(last) = value.params.last_mut() {
        let trimmed_len = last.trim_end().len();
        last.truncate(trimmed_len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_tag_value() {
        let v = tokenize("#TITLE:Example;");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].tag(), "TITLE");
        assert_eq!(v[0].param(1), "Example");
    }

    #[test]
    fn multiple_params_separated_by_colon() {
        let v = tokenize("#NOTES:dance-single::Beginner:3:0.5,0.5,0.5:0000\n0000\n;");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].param(1), "dance-single");
        assert_eq!(v[0].param(2), "");
        assert_eq!(v[0].param(3), "Beginner");
        assert_eq!(v[0].param(4), "3");
        assert_eq!(v[0].param(5), "0.5,0.5,0.5");
        assert_eq!(v[0].param(6), "0000\n0000\n");
    }

    #[test]
    fn multiple_top_level_values_and_crlf() {
        let v = tokenize("#TITLE:A;\r\n#ARTIST:B;\r\n");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].param(1), "A");
        assert_eq!(v[1].tag(), "ARTIST");
        assert_eq!(v[1].param(1), "B");
    }

    #[test]
    fn line_comment_is_stripped() {
        let v = tokenize("#TITLE:Hello // this is a comment\nWorld;");
        assert_eq!(v[0].param(1), "Hello \nWorld");
    }

    #[test]
    fn backslash_escapes_special_chars() {
        let v = tokenize(r"#TITLE:a\:b\;c;");
        assert_eq!(v[0].param(1), "a:b;c");
    }

    #[test]
    fn missing_semicolon_recovers_on_hash_at_line_start() {
        let v = tokenize("#TITLE:A\n#ARTIST:B;");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].param(1), "A");
        assert_eq!(v[1].tag(), "ARTIST");
        assert_eq!(v[1].param(1), "B");
    }

    #[test]
    fn hash_mid_value_is_literal() {
        let v = tokenize("#TITLE:A#B;");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].param(1), "A#B");
    }

    #[test]
    fn non_ascii_survives() {
        let v = tokenize("#TITLE:恋のメロディ;#ARTIST:Ünïcödé;");
        assert_eq!(v[0].param(1), "恋のメロディ");
        assert_eq!(v[1].param(1), "Ünïcödé");
    }

    #[test]
    fn empty_and_unterminated_input() {
        assert!(tokenize("").is_empty());
        let v = tokenize("#TITLE:abc");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].param(1), "abc");
        let v = tokenize("#A:1;\n\n   \n#B:2;");
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn trailing_backslash_does_not_panic() {
        let v = tokenize("#TITLE:a\\");
        assert_eq!(v[0].param(1), "a");
    }
}
