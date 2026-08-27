//! kbin type registry — maps type IDs to names, sizes, and conversion logic.
//!
//! The kbin binary format uses numeric type IDs (2–56) to identify the data
//! type of each value. This module provides the complete mapping plus
//! byte↔string conversion for each type.

use super::KbinError;

// ---------------------------------------------------------------------------
// Control bytes
// ---------------------------------------------------------------------------

/// Node start control byte.
pub const NODE_START: u8 = 1;

/// Attribute control byte.
pub const ATTRIBUTE: u8 = 46;

/// Node end control byte (variant 1).
pub const NODE_END: u8 = 190;

/// File end control byte (variant 1).
pub const FILE_END: u8 = 191;

/// Bit mask for the array flag (bit 6 of the type byte).
pub const ARRAY_FLAG: u8 = 0x40;

// ---------------------------------------------------------------------------
// Special type IDs
// ---------------------------------------------------------------------------

/// Type ID for binary data (variable length).
pub const TYPE_BIN: u8 = 10;

/// Type ID for string data (variable length).
pub const TYPE_STR: u8 = 11;

// ---------------------------------------------------------------------------
// Primitive enum — determines how a single element is converted
// ---------------------------------------------------------------------------

/// The underlying primitive type used for byte↔string conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    S8,
    U8,
    S16,
    U16,
    S32,
    U32,
    S64,
    U64,
    Float,
    Double,
    Bool,
    Ip4,
    /// Variable-length types (str, bin) — handled specially by reader/writer.
    Variable,
}

// ---------------------------------------------------------------------------
// KbinType
// ---------------------------------------------------------------------------

/// Metadata for a single kbin type.
#[derive(Debug, Clone)]
pub struct KbinType {
    /// Numeric type ID (2–56).
    pub id: u8,
    /// Primary name (e.g. `"s32"`, `"vs8"`).
    pub name: &'static str,
    /// Alternate names (e.g. `"float"` has alias `"f"`).
    pub aliases: &'static [&'static str],
    /// Byte size of one element.
    pub element_size: usize,
    /// Number of elements per value (1 for scalars, 2+ for vectors).
    pub count: usize,
    /// Primitive type for conversion.
    pub primitive: Primitive,
}

impl KbinType {
    /// Total byte size for one complete value (`element_size * count`).
    pub fn total_size(&self) -> usize {
        self.element_size * self.count
    }

    /// Whether this type has variable length (str or bin).
    pub fn is_variable_length(&self) -> bool {
        self.primitive == Primitive::Variable
    }

    /// Convert raw bytes (one complete value) to its string representation.
    ///
    /// For vector types, elements are space-separated.
    /// Not valid for variable-length types (str/bin) — those are handled
    /// directly by the reader/writer with encoding context.
    pub fn bytes_to_string(&self, bytes: &[u8]) -> Result<String, KbinError> {
        if self.is_variable_length() {
            return Err(KbinError::Conversion(
                "variable-length types must be handled by reader/writer".into(),
            ));
        }
        if bytes.len() < self.total_size() {
            return Err(KbinError::Conversion(format!(
                "expected {} bytes for {}, got {}",
                self.total_size(),
                self.name,
                bytes.len()
            )));
        }
        let parts: Vec<String> = bytes
            .chunks_exact(self.element_size)
            .take(self.count)
            .map(|chunk| element_to_string(self.primitive, chunk))
            .collect::<Result<_, _>>()?;
        Ok(parts.join(" "))
    }

    /// Convert a string representation to raw bytes.
    ///
    /// For vector types, elements are space-separated.
    /// Not valid for variable-length types (str/bin).
    pub fn string_to_bytes(&self, text: &str) -> Result<Vec<u8>, KbinError> {
        if self.is_variable_length() {
            return Err(KbinError::Conversion(
                "variable-length types must be handled by reader/writer".into(),
            ));
        }
        let parts: Vec<&str> = if self.count == 1 {
            vec![text]
        } else {
            text.split(' ').collect()
        };
        if parts.len() != self.count {
            return Err(KbinError::Conversion(format!(
                "expected {} elements for {}, got {}",
                self.count,
                self.name,
                parts.len()
            )));
        }
        let mut out = Vec::with_capacity(self.total_size());
        for part in parts {
            out.extend(element_from_string(self.primitive, part)?);
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Single-element conversion helpers
// ---------------------------------------------------------------------------

fn element_to_string(prim: Primitive, bytes: &[u8]) -> Result<String, KbinError> {
    match prim {
        Primitive::S8 => Ok(i8::from_be_bytes([bytes[0]]).to_string()),
        Primitive::U8 => Ok(bytes[0].to_string()),
        Primitive::S16 => Ok(i16::from_be_bytes([bytes[0], bytes[1]]).to_string()),
        Primitive::U16 => Ok(u16::from_be_bytes([bytes[0], bytes[1]]).to_string()),
        Primitive::S32 => Ok(i32::from_be_bytes(bytes[..4].try_into().unwrap()).to_string()),
        Primitive::U32 => Ok(u32::from_be_bytes(bytes[..4].try_into().unwrap()).to_string()),
        Primitive::S64 => Ok(i64::from_be_bytes(bytes[..8].try_into().unwrap()).to_string()),
        Primitive::U64 => Ok(u64::from_be_bytes(bytes[..8].try_into().unwrap()).to_string()),
        Primitive::Float => {
            let v = f32::from_be_bytes(bytes[..4].try_into().unwrap());
            Ok(format!("{v:.6}"))
        }
        Primitive::Double => {
            let v = f64::from_be_bytes(bytes[..8].try_into().unwrap());
            Ok(format!("{v:.6}"))
        }
        Primitive::Bool => Ok((bytes[0] & 1).to_string()),
        Primitive::Ip4 => Ok(format!(
            "{}.{}.{}.{}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        )),
        Primitive::Variable => Err(KbinError::Conversion("variable-length type".into())),
    }
}

fn element_from_string(prim: Primitive, text: &str) -> Result<Vec<u8>, KbinError> {
    let err = |e: std::fmt::Arguments<'_>| KbinError::Conversion(format!("{e}"));
    match prim {
        Primitive::S8 => {
            let v: i8 = text.parse().map_err(|e| err(format_args!("{e}")))?;
            Ok(v.to_be_bytes().to_vec())
        }
        Primitive::U8 => {
            let v: u8 = text.parse().map_err(|e| err(format_args!("{e}")))?;
            Ok(vec![v])
        }
        Primitive::S16 => {
            let v: i16 = text.parse().map_err(|e| err(format_args!("{e}")))?;
            Ok(v.to_be_bytes().to_vec())
        }
        Primitive::U16 => {
            let v: u16 = text.parse().map_err(|e| err(format_args!("{e}")))?;
            Ok(v.to_be_bytes().to_vec())
        }
        Primitive::S32 => {
            let v: i32 = text.parse().map_err(|e| err(format_args!("{e}")))?;
            Ok(v.to_be_bytes().to_vec())
        }
        Primitive::U32 => {
            let v: u32 = text.parse().map_err(|e| err(format_args!("{e}")))?;
            Ok(v.to_be_bytes().to_vec())
        }
        Primitive::S64 => {
            let v: i64 = text.parse().map_err(|e| err(format_args!("{e}")))?;
            Ok(v.to_be_bytes().to_vec())
        }
        Primitive::U64 => {
            let v: u64 = text.parse().map_err(|e| err(format_args!("{e}")))?;
            Ok(v.to_be_bytes().to_vec())
        }
        Primitive::Float => {
            let v: f32 = text.parse().map_err(|e| err(format_args!("{e}")))?;
            Ok(v.to_be_bytes().to_vec())
        }
        Primitive::Double => {
            let v: f64 = text.parse().map_err(|e| err(format_args!("{e}")))?;
            Ok(v.to_be_bytes().to_vec())
        }
        Primitive::Bool => {
            let v: u8 = if text == "0" { 0 } else { 1 };
            Ok(vec![v])
        }
        Primitive::Ip4 => {
            let octets: Vec<u8> = text
                .split('.')
                .map(|s| s.parse::<u8>().map_err(|e| err(format_args!("{e}"))))
                .collect::<Result<_, _>>()?;
            if octets.len() != 4 {
                return Err(KbinError::Conversion(format!(
                    "expected 4 octets for ip4, got {}",
                    octets.len()
                )));
            }
            Ok(octets)
        }
        Primitive::Variable => Err(KbinError::Conversion("variable-length type".into())),
    }
}

// ---------------------------------------------------------------------------
// Type table — all 56+ kbin types
// ---------------------------------------------------------------------------

macro_rules! t {
    ($id:expr, $name:expr, $aliases:expr, $elem_size:expr, $count:expr, $prim:expr) => {
        KbinType {
            id: $id,
            name: $name,
            aliases: $aliases,
            element_size: $elem_size,
            count: $count,
            primitive: $prim,
        }
    };
}

use Primitive::*;

static TYPES: &[KbinType] = &[
    // Scalars
    t!(2, "s8", &[], 1, 1, S8),
    t!(3, "u8", &[], 1, 1, U8),
    t!(4, "s16", &[], 2, 1, S16),
    t!(5, "u16", &[], 2, 1, U16),
    t!(6, "s32", &[], 4, 1, S32),
    t!(7, "u32", &[], 4, 1, U32),
    t!(8, "s64", &[], 8, 1, S64),
    t!(9, "u64", &[], 8, 1, U64),
    t!(10, "bin", &["binary"], 1, 1, Variable),
    t!(11, "str", &["string"], 1, 1, Variable),
    t!(12, "ip4", &[], 4, 1, Ip4),
    t!(13, "time", &[], 4, 1, U32),
    t!(14, "float", &["f"], 4, 1, Float),
    t!(15, "double", &["d"], 8, 1, Double),
    // 2-element vectors
    t!(16, "2s8", &[], 1, 2, S8),
    t!(17, "2u8", &[], 1, 2, U8),
    t!(18, "2s16", &[], 2, 2, S16),
    t!(19, "2u16", &[], 2, 2, U16),
    t!(20, "2s32", &[], 4, 2, S32),
    t!(21, "2u32", &[], 4, 2, U32),
    t!(22, "2s64", &["vs64"], 8, 2, S64),
    t!(23, "2u64", &["vu64"], 8, 2, U64),
    t!(24, "2f", &[], 4, 2, Float),
    t!(25, "2d", &["vd"], 8, 2, Double),
    // 3-element vectors
    t!(26, "3s8", &[], 1, 3, S8),
    t!(27, "3u8", &[], 1, 3, U8),
    t!(28, "3s16", &[], 2, 3, S16),
    t!(29, "3u16", &[], 2, 3, U16),
    t!(30, "3s32", &[], 4, 3, S32),
    t!(31, "3u32", &[], 4, 3, U32),
    t!(32, "3s64", &[], 8, 3, S64),
    t!(33, "3u64", &[], 8, 3, U64),
    t!(34, "3f", &[], 4, 3, Float),
    t!(35, "3d", &[], 8, 3, Double),
    // 4-element vectors
    t!(36, "4s8", &[], 1, 4, S8),
    t!(37, "4u8", &[], 1, 4, U8),
    t!(38, "4s16", &[], 2, 4, S16),
    t!(39, "4u16", &[], 2, 4, U16),
    t!(40, "4s32", &["vs32"], 4, 4, S32),
    t!(41, "4u32", &["vu32"], 4, 4, U32),
    t!(42, "4s64", &[], 8, 4, S64),
    t!(43, "4u64", &[], 8, 4, U64),
    t!(44, "4f", &["vf"], 4, 4, Float),
    t!(45, "4d", &[], 8, 4, Double),
    // Large vectors
    t!(48, "vs8", &[], 1, 16, S8),
    t!(49, "vu8", &[], 1, 16, U8),
    t!(50, "vs16", &[], 2, 8, S16),
    t!(51, "vu16", &[], 2, 8, U16),
    // Bool variants
    t!(52, "bool", &["b"], 1, 1, Bool),
    t!(53, "2b", &[], 1, 2, Bool),
    t!(54, "3b", &[], 1, 3, Bool),
    t!(55, "4b", &[], 1, 4, Bool),
    t!(56, "vb", &[], 1, 16, Bool),
];

// ---------------------------------------------------------------------------
// Lookup functions
// ---------------------------------------------------------------------------

/// Look up a kbin type by its numeric ID.
pub fn type_by_id(id: u8) -> Option<&'static KbinType> {
    TYPES.iter().find(|t| t.id == id)
}

/// Look up a kbin type by name (checks primary name and aliases).
pub fn type_by_name(name: &str) -> Option<&'static KbinType> {
    TYPES
        .iter()
        .find(|t| t.name == name || t.aliases.contains(&name))
}

/// Returns `true` if `byte` (with array flag masked off) is a control byte
/// rather than a value type.
pub fn is_control_byte(byte: u8) -> bool {
    matches!(byte, NODE_START | ATTRIBUTE | NODE_END | FILE_END)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_all_scalar_types_by_id() {
        let cases: &[(u8, &str)] = &[
            (2, "s8"),
            (3, "u8"),
            (4, "s16"),
            (5, "u16"),
            (6, "s32"),
            (7, "u32"),
            (8, "s64"),
            (9, "u64"),
            (10, "bin"),
            (11, "str"),
            (12, "ip4"),
            (13, "time"),
            (14, "float"),
            (15, "double"),
            (52, "bool"),
        ];
        for &(id, expected_name) in cases {
            let t = type_by_id(id).unwrap_or_else(|| panic!("missing type ID {id}"));
            assert_eq!(t.name, expected_name, "type ID {id}");
        }
    }

    #[test]
    fn lookup_vector_types_by_id() {
        let t = type_by_id(40).unwrap();
        assert_eq!(t.name, "4s32");
        assert_eq!(t.count, 4);
        assert_eq!(t.element_size, 4);
        assert_eq!(t.total_size(), 16);

        let t = type_by_id(48).unwrap();
        assert_eq!(t.name, "vs8");
        assert_eq!(t.count, 16);
        assert_eq!(t.element_size, 1);
    }

    #[test]
    fn lookup_by_name_primary() {
        let t = type_by_name("s32").unwrap();
        assert_eq!(t.id, 6);
    }

    #[test]
    fn lookup_by_name_alias() {
        let t = type_by_name("f").unwrap();
        assert_eq!(t.id, 14);
        assert_eq!(t.name, "float");

        let t = type_by_name("binary").unwrap();
        assert_eq!(t.id, 10);

        let t = type_by_name("string").unwrap();
        assert_eq!(t.id, 11);

        let t = type_by_name("b").unwrap();
        assert_eq!(t.id, 52);

        let t = type_by_name("vs32").unwrap();
        assert_eq!(t.id, 40);

        let t = type_by_name("vf").unwrap();
        assert_eq!(t.id, 44);
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(type_by_id(0).is_none());
        assert!(type_by_id(1).is_none()); // control byte, not a type
        assert!(type_by_id(99).is_none());
        assert!(type_by_name("nonexistent").is_none());
    }

    #[test]
    fn scalar_bytes_to_string() {
        let t = type_by_id(6).unwrap(); // s32
        assert_eq!(t.bytes_to_string(&42i32.to_be_bytes()).unwrap(), "42");

        let t = type_by_id(7).unwrap(); // u32
        assert_eq!(
            t.bytes_to_string(&4000000000u32.to_be_bytes()).unwrap(),
            "4000000000"
        );

        let t = type_by_id(2).unwrap(); // s8
        assert_eq!(t.bytes_to_string(&(-1i8).to_be_bytes()).unwrap(), "-1");
    }

    #[test]
    fn scalar_string_to_bytes() {
        let t = type_by_id(6).unwrap(); // s32
        assert_eq!(t.string_to_bytes("42").unwrap(), 42i32.to_be_bytes());

        let t = type_by_id(8).unwrap(); // s64
        assert_eq!(t.string_to_bytes("-100").unwrap(), (-100i64).to_be_bytes());
    }

    #[test]
    fn float_round_trip() {
        let t = type_by_id(14).unwrap(); // float
        let bytes = t.string_to_bytes("3.140000").unwrap();
        let text = t.bytes_to_string(&bytes).unwrap();
        assert!(text.starts_with("3.14"));
    }

    #[test]
    fn bool_conversion() {
        let t = type_by_id(52).unwrap();
        assert_eq!(t.bytes_to_string(&[1]).unwrap(), "1");
        assert_eq!(t.bytes_to_string(&[0]).unwrap(), "0");
        assert_eq!(t.string_to_bytes("1").unwrap(), vec![1]);
        assert_eq!(t.string_to_bytes("0").unwrap(), vec![0]);
    }

    #[test]
    fn ip4_conversion() {
        let t = type_by_id(12).unwrap();
        assert_eq!(
            t.bytes_to_string(&[192, 168, 1, 100]).unwrap(),
            "192.168.1.100"
        );
        assert_eq!(t.string_to_bytes("10.0.0.1").unwrap(), vec![10, 0, 0, 1]);
    }

    #[test]
    fn vector_bytes_to_string() {
        let t = type_by_id(20).unwrap(); // 2s32
        let mut bytes = Vec::new();
        bytes.extend(100i32.to_be_bytes());
        bytes.extend((-200i32).to_be_bytes());
        assert_eq!(t.bytes_to_string(&bytes).unwrap(), "100 -200");
    }

    #[test]
    fn vector_string_to_bytes() {
        let t = type_by_id(20).unwrap(); // 2s32
        let bytes = t.string_to_bytes("100 -200").unwrap();
        let mut expected = Vec::new();
        expected.extend(100i32.to_be_bytes());
        expected.extend((-200i32).to_be_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn bool_vector() {
        let t = type_by_id(53).unwrap(); // 2b
        assert_eq!(t.bytes_to_string(&[1, 0]).unwrap(), "1 0");
        assert_eq!(t.string_to_bytes("0 1").unwrap(), vec![0, 1]);
    }

    #[test]
    fn variable_length_types_reject_conversion() {
        let t = type_by_id(10).unwrap(); // bin
        assert!(t.bytes_to_string(&[0]).is_err());
        assert!(t.string_to_bytes("00").is_err());
        assert!(t.is_variable_length());

        let t = type_by_id(11).unwrap(); // str
        assert!(t.is_variable_length());
    }

    #[test]
    fn time_uses_u32_conversion() {
        let t = type_by_id(13).unwrap();
        assert_eq!(t.name, "time");
        assert_eq!(t.primitive, U32);
        let ts: u32 = 1700000000;
        assert_eq!(
            t.bytes_to_string(&ts.to_be_bytes()).unwrap(),
            ts.to_string()
        );
    }

    #[test]
    fn control_byte_detection() {
        assert!(is_control_byte(NODE_START));
        assert!(is_control_byte(ATTRIBUTE));
        assert!(is_control_byte(NODE_END));
        assert!(is_control_byte(FILE_END));
        assert!(!is_control_byte(6)); // s32 type
        assert!(!is_control_byte(0));
    }

    #[test]
    fn all_type_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for t in TYPES {
            assert!(seen.insert(t.id), "duplicate type ID {}", t.id);
        }
    }

    #[test]
    fn all_primary_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for t in TYPES {
            assert!(seen.insert(t.name), "duplicate type name {}", t.name);
        }
    }

    #[test]
    fn no_gaps_in_scalar_ids() {
        // IDs 2–15 should all be present (scalars)
        for id in 2..=15 {
            assert!(type_by_id(id).is_some(), "missing scalar type ID {id}");
        }
    }
}
