//! Simfile byte → text decoding.
//!
//! Community packs are a mix of UTF-8 (with and without BOM), Shift-JIS
//! (Japanese packs authored before StepMania 5 went UTF-8-only) and
//! Windows-1252 (older Western packs). The order is strict UTF-8 first
//! (a BOM is stripped), then Shift-JIS, then Windows-1252 — Shift-JIS is
//! tried before 1252 because 1252 decodes *any* byte sequence, so it can
//! only ever be the fallback. Shift-JIS decoding is accepted only when
//! `encoding_rs` reports no replacement; otherwise the bytes fall through.
//!
//! The 4 MiB input cap (design N7) is enforced by the caller before the
//! bytes reach this function.

use std::borrow::Cow;

/// Which decoder produced the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    ShiftJis,
    Windows1252,
}

/// Decode simfile bytes to text. Never fails: Windows-1252 maps every byte.
#[must_use]
pub fn decode(bytes: &[u8]) -> (Cow<'_, str>, Encoding) {
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(body) {
        return (Cow::Borrowed(s), Encoding::Utf8);
    }
    let (text, _, had_errors) = encoding_rs::SHIFT_JIS.decode(body);
    if !had_errors {
        return (Cow::Owned(text.into_owned()), Encoding::ShiftJis);
    }
    let (text, _, _) = encoding_rs::WINDOWS_1252.decode(body);
    (Cow::Owned(text.into_owned()), Encoding::Windows1252)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_borrows_and_strips_bom() {
        let (t, e) = decode("#TITLE:ü;".as_bytes());
        assert_eq!(e, Encoding::Utf8);
        assert!(matches!(t, Cow::Borrowed(_)));
        let (t, e) = decode(b"\xEF\xBB\xBF#TITLE:A;");
        assert_eq!(e, Encoding::Utf8);
        assert_eq!(t, "#TITLE:A;");
    }

    #[test]
    fn shift_jis_title_decodes() {
        // "予" = 0x97 0x5C in Shift-JIS (second byte is a backslash — the
        // classic MSD trap, which the tokenizer never sees as bytes).
        let bytes = b"#TITLE:\x97\x5C \x97\xF6;";
        let (t, e) = decode(bytes);
        assert_eq!(e, Encoding::ShiftJis);
        assert_eq!(t, "#TITLE:予 恋;");
    }

    #[test]
    fn invalid_shift_jis_falls_back_to_1252() {
        // 0xFF is not a valid Shift-JIS lead byte.
        let (t, e) = decode(b"#TITLE:caf\xE9 \xFF;");
        assert_eq!(e, Encoding::Windows1252);
        assert!(t.starts_with("#TITLE:café "));
    }
}
