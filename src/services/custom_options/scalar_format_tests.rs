//! Host tests for [`api::format_scalar_value`] — the pure value→display
//! formatter behind every scalar row's value TextLayer. Pins each
//! [`ScalarFormat`] variant's byte output (the function returns raw bytes
//! because `SignedUnit`'s zero case embeds the Shift-JIS `±` glyph, which is
//! not valid UTF-8) and the 15-byte SSO-inline budget for the composed
//! strings the shipped registrations can produce (a longer string heap
//! promotes inside the game's `string::assign` and leaks per push — see
//! `destruct_sso_string`).

use super::api::format_scalar_value;
use super::api::ScalarFormat;

/// Every pre-existing variant ignores the option id; the pins below pass a
/// placeholder so the byte assertions stay id-agnostic.
fn fmt(value: i32, format: ScalarFormat) -> Vec<u8> {
    format_scalar_value("any", value, format)
}

#[test]
fn integer_and_fixed_point() {
    assert_eq!(fmt(490, ScalarFormat::Integer), b"490");
    assert_eq!(fmt(-3, ScalarFormat::Integer), b"-3");
    assert_eq!(fmt(150, ScalarFormat::FixedPoint { decimals: 2 }), b"1.50");
    assert_eq!(fmt(-5, ScalarFormat::FixedPoint { decimals: 2 }), b"-0.05");
}

#[test]
fn offset_integer() {
    let f = ScalarFormat::OffsetInteger { display_offset: 1 };
    assert_eq!(fmt(0, f), b"1");
    assert_eq!(fmt(41, f), b"42");
}

#[test]
fn signed_unit_stock_convention() {
    let f = ScalarFormat::SignedUnit { unit: "ms" };
    assert_eq!(fmt(10, f), b"+10ms");
    assert_eq!(fmt(-41, f), b"-41ms");
    // Zero renders the Shift-JIS plus-minus glyph (0x81 0x7D).
    assert_eq!(fmt(0, f), &[0x81, 0x7D, b'0', b'm', b's']);
}

#[test]
fn unit_is_unsigned() {
    assert_eq!(fmt(12, ScalarFormat::Unit { unit: "ms" }), b"12ms");
    assert_eq!(fmt(100, ScalarFormat::Unit { unit: "%" }), b"100%");
    assert_eq!(fmt(70, ScalarFormat::Unit { unit: "kg" }), b"70kg");
    assert_eq!(fmt(0, ScalarFormat::Unit { unit: "%" }), b"0%");
}

#[test]
fn minutes_seconds() {
    assert_eq!(fmt(0, ScalarFormat::MinutesSeconds), b"0:00");
    assert_eq!(fmt(5, ScalarFormat::MinutesSeconds), b"0:05");
    assert_eq!(fmt(90, ScalarFormat::MinutesSeconds), b"1:30");
    assert_eq!(fmt(200, ScalarFormat::MinutesSeconds), b"3:20");
    assert_eq!(fmt(600, ScalarFormat::MinutesSeconds), b"10:00");
    // Defensive clamp — no shipped row can go negative.
    assert_eq!(fmt(-1, ScalarFormat::MinutesSeconds), b"0:00");
}

#[test]
fn prefixed_index() {
    let f = ScalarFormat::PrefixedIndex {
        prefix: "Char #",
        display_offset: 1,
    };
    assert_eq!(fmt(0, f), b"Char #1");
    assert_eq!(fmt(9, f), b"Char #10");
}

/// Every prefix shipped by the WebUI cosmetic pickers must stay inside the
/// 15-byte SSO inline buffer even at a 5-digit displayed position.
#[test]
fn shipped_prefixes_fit_sso_at_five_digits() {
    for prefix in ["Board #", "BG #", "Char #", "Lane #", "Cover #"] {
        let f = ScalarFormat::PrefixedIndex {
            prefix,
            display_offset: 1,
        };
        let rendered = fmt(99998, f); // displays 99999
        assert!(
            rendered.len() <= 15,
            "{prefix}99999 is {} bytes (> 15-byte SSO inline)",
            rendered.len()
        );
    }
}

/// `Labeled`: the enum-like scalar (Multiplayer Bot level row) renders
/// `"{prefix}{value}"` everywhere except the terminal value, which shows its
/// label; every shipped string fits the SSO inline buffer.
#[test]
fn labeled_prefix_and_terminal() {
    let f = ScalarFormat::Labeled {
        prefix: "Level ",
        terminal_value: 11,
        terminal_label: "Target Score",
    };
    assert_eq!(fmt(1, f), b"Level 1");
    assert_eq!(fmt(10, f), b"Level 10");
    assert_eq!(fmt(11, f), b"Target Score");
    // Out-of-range values (a clamp bug upstream) still render readably.
    assert_eq!(fmt(12, f), b"Level 12");
    assert_eq!(fmt(0, f), b"Level 0");
    for v in 1..=11 {
        assert!(fmt(v, f).len() <= 15, "value {v} exceeds the SSO budget");
    }
    // The overlay's UTF-8 view is the same text.
    assert_eq!(
        super::api::format_scalar_value_utf8("any", 11, f),
        "Target Score"
    );
    assert_eq!(super::api::format_scalar_value_utf8("any", 7, f), "Level 7");
}

/// `Dynamic`: the owner-supplied labeler (the Background Dancer / Stage
/// rows' catalog names) renders per-value text keyed on the OPTION ID, and a
/// `None` falls back to the plain integer so a catalog hole never blanks the
/// row.
#[test]
fn dynamic_labeler_and_fallback() {
    fn labeler(id: &str, value: i32) -> Option<String> {
        match (id, value) {
            (_, 0) => Some("RANDOM".to_string()),
            ("background_dancer", 2) => Some("EMI #2".to_string()),
            ("background_stage", 2) => Some("BOOM #3".to_string()),
            _ => None,
        }
    }
    let f = ScalarFormat::Dynamic(labeler);
    assert_eq!(format_scalar_value("background_dancer", 0, f), b"RANDOM");
    assert_eq!(format_scalar_value("background_dancer", 2, f), b"EMI #2");
    // Id dispatch: the same value names a different entry on the other row.
    assert_eq!(format_scalar_value("background_stage", 2, f), b"BOOM #3");
    // `None` ⇒ the integer text (an out-of-catalog value stays readable).
    assert_eq!(format_scalar_value("background_dancer", 99, f), b"99");
    assert_eq!(format_scalar_value("unknown_row", 2, f), b"2");
    // The overlay's UTF-8 view is the same text.
    assert_eq!(
        super::api::format_scalar_value_utf8("background_stage", 2, f),
        "BOOM #3"
    );
    assert_eq!(
        super::api::format_scalar_value_utf8("background_dancer", 99, f),
        "99"
    );
}
