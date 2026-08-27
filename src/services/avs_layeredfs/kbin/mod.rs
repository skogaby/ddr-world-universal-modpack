//! Minimal kbin binary XML decoder for layeredfs.
//! Ported from bemani-buddy's kbin crate.

pub mod reader;
pub mod sixbit;
pub mod types;

/// Magic byte that identifies a kbin payload.
pub const KBIN_MAGIC: u8 = 0xA0;

/// Errors during kbin decoding.
#[derive(Debug)]
pub enum KbinError {
    InvalidHeader(String),
    UnknownType(u8),
    UnexpectedEof,
    InvalidSixbit(String),
    Conversion(String),
}

impl std::fmt::Display for KbinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHeader(s) => write!(f, "invalid kbin header: {s}"),
            Self::UnknownType(id) => write!(f, "unknown type ID: {id}"),
            Self::UnexpectedEof => write!(f, "unexpected end of data"),
            Self::InvalidSixbit(s) => write!(f, "invalid sixbit: {s}"),
            Self::Conversion(s) => write!(f, "conversion error: {s}"),
        }
    }
}

/// Text encodings supported by kbin.
pub const ENCODINGS: &[&str] = &[
    "SHIFT_JIS",
    "ASCII",
    "ISO-8859-1",
    "EUC-JP",
    "SHIFT_JIS",
    "UTF-8",
];
