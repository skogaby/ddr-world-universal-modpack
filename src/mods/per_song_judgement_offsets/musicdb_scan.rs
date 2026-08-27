//! Pure string-level `<basename>` scanner for musicdb XML (Per-Song
//! Judgement Offsets, design §Components → musicdb crawl).
//!
//! The boot crawl feeds this the merged musicdb text (LayeredFS merge cache,
//! whole-file mod override, or the stock file served by AVS) and gets back
//! the ordered, deduplicated basename list used to append missing codes into
//! `judgement_offsets.csv`. Entries are flat `<basename>xxxx</basename>`
//! tags (optionally attributed); a string scan mirrors the tolerance of the
//! LayeredFS XML merger — no XML parser dependency.
//!
//! Dependency-free on purpose: host-tested through the harness mount
//! (`scripts/validate_judgement_offsets.sh`).

use std::collections::HashSet;

/// Extract every `<basename>` value, in document order, first occurrence
/// wins. Values are whitespace-trimmed; empty values are skipped. Tolerates
/// attributes on the opening tag and CRLF content.
pub fn scan_basenames(xml: &str) -> Vec<String> {
    const OPEN: &str = "<basename";
    const CLOSE: &str = "</basename>";

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = 0usize;
    while let Some(rel) = xml[cursor..].find(OPEN) {
        let tag_start = cursor + rel;
        let after_name = tag_start + OPEN.len();
        // Must be the whole tag name: next char is '>' or whitespace/attrs
        // (rejects e.g. `<basename_yomi>`).
        let Some(gt_rel) = xml[after_name..].find('>') else {
            break;
        };
        let value_start = after_name + gt_rel + 1;
        let head = &xml[after_name..after_name + gt_rel];
        if !head.is_empty() && !head.starts_with(|c: char| c.is_ascii_whitespace() || c == '/') {
            cursor = value_start;
            continue;
        }
        let Some(close_rel) = xml[value_start..].find(CLOSE) else {
            break;
        };
        let value = xml[value_start..value_start + close_rel].trim();
        cursor = value_start + close_rel + CLOSE.len();
        if value.is_empty() {
            continue;
        }
        if seen.insert(value.to_string()) {
            out.push(value.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_shape_in_document_order() {
        let xml = "<mdb>\n  <music>\n    <mcode __type=\"u32\">10</mcode>\n    <basename>puty</basename>\n    <title>PUT YOUR FAITH IN ME</title>\n  </music>\n  <music><basename>bril</basename></music>\n  <music><basename>aaaa</basename></music>\n</mdb>";
        assert_eq!(scan_basenames(xml), ["puty", "bril", "aaaa"]);
    }

    #[test]
    fn merged_shape_dedupes_union() {
        // Stock entries plus fragment entries appended before </mdb>, one
        // duplicate.
        let xml = "<mdb><music><basename>puty</basename></music>\n<music><basename>cust</basename></music><music><basename>puty</basename></music></mdb>";
        assert_eq!(scan_basenames(xml), ["puty", "cust"]);
    }

    #[test]
    fn tolerance_cases() {
        let xml = "<music><basename>  padded \r\n</basename></music>\n<music><basename></basename></music>\n<music><basename __type=\"str\">attr</basename></music>\n<music><basename_yomi>nope</basename_yomi></music>";
        assert_eq!(scan_basenames(xml), ["padded", "attr"]);
    }

    #[test]
    fn empty_and_tagless_inputs() {
        assert!(scan_basenames("").is_empty());
        assert!(scan_basenames("<mdb></mdb>").is_empty());
        // Unterminated tag doesn't loop or panic.
        assert!(scan_basenames("<basename>trailing").is_empty());
    }
}
