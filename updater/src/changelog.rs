//! Release-notes preview for the console (design §4.13): the first lines of
//! the GitHub release body, cleaned of Markdown emphasis and code fences,
//! capped with an ellipsis. Pure.

/// Non-empty lines of `body`, cleaned, at most `max_lines` (+ a trailing `…`
/// line when truncated).
pub fn preview(body: &str, max_lines: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut truncated = false;
    for raw in body.lines() {
        let line = raw.trim_end_matches('\r').trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || trimmed.is_empty() {
            continue;
        }
        if out.len() >= max_lines {
            truncated = true;
            break;
        }
        out.push(clean_line(line));
    }
    if truncated {
        out.push("…".to_string());
    }
    out
}

fn clean_line(line: &str) -> String {
    let indent = line.len() - line.trim_start().len();
    let body = line.trim_start();
    // Bullets → "• " so nested lists still read as lists in a plain console.
    let (prefix, rest) =
        if let Some(r) = body.strip_prefix("* ").or_else(|| body.strip_prefix("- ")) {
            ("• ", r)
        } else {
            ("", body)
        };
    let cleaned: String = rest.replace("**", "").replace("__", "").replace('`', "");
    format!("{}{prefix}{cleaned}", " ".repeat(indent))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Excerpt of the live v1.2 release body (2026-09-13).
    const V12: &str = "Here's another quick-turnaround release. Changelog below:\r\n\r\n**New features and mods:**\r\n* Automatically discover and load split SSQs on songs with different BPMs per difficulty.\r\n\r\n**Quality-of-life improvements:**\r\n* S-Marvelous changes:\r\n    * Added an option under `GLOBAL SETTINGS` to toggle the texture\r\n\r\n```\r\nacef - ACE FOR ACES - TAG×U1\r\naceo - Ace out\r\n```\r\n* Fixed an issue with the playback speed adjustment mod";

    #[test]
    fn strips_emphasis_and_fences_keeps_bullets_and_indent() {
        let lines = preview(V12, 20);
        assert_eq!(
            lines[0],
            "Here's another quick-turnaround release. Changelog below:"
        );
        assert_eq!(lines[1], "New features and mods:");
        assert!(lines[2].starts_with("• Automatically discover"));
        assert_eq!(lines[3], "Quality-of-life improvements:");
        assert_eq!(lines[4], "• S-Marvelous changes:");
        assert_eq!(
            lines[5],
            "    • Added an option under GLOBAL SETTINGS to toggle the texture"
        );
        assert!(
            lines.iter().all(|l| !l.contains("acef")),
            "fenced block skipped: {lines:?}"
        );
        assert_eq!(
            lines[6],
            "• Fixed an issue with the playback speed adjustment mod"
        );
        assert_eq!(lines.len(), 7);
    }

    #[test]
    fn cap_adds_ellipsis_and_empty_body_is_empty() {
        let lines = preview(V12, 5);
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[5], "…");
        assert!(preview("", 10).is_empty());
        assert!(preview("\r\n\r\n", 10).is_empty());
        // Exactly at the cap: no ellipsis.
        assert_eq!(preview("a\nb", 2), vec!["a", "b"]);
    }
}
