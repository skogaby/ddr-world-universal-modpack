//! Sixbit encoding for kbin node names.
//!
//! Konami's kbin format optionally compresses XML node/attribute names by
//! encoding each character from a 64-character alphabet into 6 bits, packed
//! into bytes. This is signaled by the kbin compress flag byte being `0x42`.

use super::KbinError;

/// The 64-character alphabet used by sixbit encoding.
/// Index 0–9: `0`–`9`, 10: `:`, 11–36: `A`–`Z`, 37: `_`, 38–63: `a`–`z`.
const ALPHABET: &[u8; 64] = b"0123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";

/// kbin compress flag indicating sixbit-compressed node names.
pub const COMPRESSED: u8 = 0x42;

/// kbin compress flag indicating uncompressed (raw) node names.
pub const UNCOMPRESSED: u8 = 0x45;

/// Decode a sixbit-encoded byte slice into a string of `length` characters.
pub fn decode(input: &[u8], length: usize) -> Result<String, KbinError> {
    let total_bits = length * 6;
    let needed_bytes = total_bits.div_ceil(8);
    if input.len() < needed_bytes {
        return Err(KbinError::InvalidSixbit(format!(
            "need {needed_bytes} bytes for {length} chars, got {}",
            input.len()
        )));
    }

    let mut result = Vec::with_capacity(length);
    for char_idx in 0..length {
        let mut value: u8 = 0;
        for bit in 0..6 {
            let global_bit = char_idx * 6 + bit;
            let byte_idx = global_bit / 8;
            let bit_idx = 7 - (global_bit % 8);
            if (input[byte_idx] >> bit_idx) & 1 == 1 {
                value |= 1 << (5 - bit);
            }
        }
        if value as usize >= ALPHABET.len() {
            return Err(KbinError::InvalidSixbit(format!(
                "sixbit value {value} out of range"
            )));
        }
        result.push(ALPHABET[value as usize]);
    }

    String::from_utf8(result).map_err(|e| KbinError::InvalidSixbit(format!("invalid UTF-8: {e}")))
}

/// Encode a string into sixbit-packed bytes.
///
/// Every character must be in the sixbit alphabet (`0-9:A-Za-z_`).
pub fn encode(input: &str) -> Result<Vec<u8>, KbinError> {
    let char_values: Vec<u8> = input
        .bytes()
        .map(|b| {
            ALPHABET
                .iter()
                .position(|&a| a == b)
                .map(|p| p as u8)
                .ok_or_else(|| {
                    KbinError::InvalidSixbit(format!(
                        "character '{}' not in sixbit alphabet",
                        b as char
                    ))
                })
        })
        .collect::<Result<_, _>>()?;

    let total_bits = char_values.len() * 6;
    let out_len = total_bits.div_ceil(8);
    let mut output = vec![0u8; out_len];

    for (char_idx, &value) in char_values.iter().enumerate() {
        for bit in 0..6 {
            if (value >> (5 - bit)) & 1 == 1 {
                let global_bit = char_idx * 6 + bit;
                let byte_idx = global_bit / 8;
                let bit_idx = 7 - (global_bit % 8);
                output[byte_idx] |= 1 << bit_idx;
            }
        }
    }

    Ok(output)
}

/// Number of bytes needed to sixbit-encode a string of `char_count` characters.
pub fn encoded_length(char_count: usize) -> usize {
    (char_count * 6).div_ceil(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_simple() {
        let names = ["call", "playerdata", "retrycnt", "info", "version"];
        for name in names {
            let encoded = encode(name).unwrap();
            let decoded = decode(&encoded, name.len()).unwrap();
            assert_eq!(decoded, name, "round-trip failed for '{name}'");
        }
    }

    #[test]
    fn round_trip_all_alphabet_chars() {
        let all: String = std::str::from_utf8(ALPHABET).unwrap().to_string();
        let encoded = encode(&all).unwrap();
        let decoded = decode(&encoded, all.len()).unwrap();
        assert_eq!(decoded, all);
    }

    #[test]
    fn round_trip_single_char() {
        for &ch in ALPHABET {
            let s = String::from(ch as char);
            let encoded = encode(&s).unwrap();
            let decoded = decode(&encoded, 1).unwrap();
            assert_eq!(decoded, s, "round-trip failed for '{s}'");
        }
    }

    #[test]
    fn empty_string() {
        let encoded = encode("").unwrap();
        assert!(encoded.is_empty());
        let decoded = decode(&[], 0).unwrap();
        assert_eq!(decoded, "");
    }

    #[test]
    fn encoded_length_calculation() {
        assert_eq!(encoded_length(0), 0);
        assert_eq!(encoded_length(1), 1); // 6 bits → 1 byte
        assert_eq!(encoded_length(4), 3); // 24 bits → 3 bytes
        assert_eq!(encoded_length(8), 6); // 48 bits → 6 bytes
    }

    #[test]
    fn invalid_character_rejected() {
        assert!(encode("hello world").is_err()); // space not in alphabet
        assert!(encode("foo-bar").is_err()); // hyphen not in alphabet
    }

    #[test]
    fn decode_insufficient_bytes() {
        assert!(decode(&[0x00], 4).is_err()); // need 3 bytes for 4 chars
    }

    #[test]
    fn known_encoding() {
        // "call" = c(40) a(38) l(49) l(49)
        // Binary: 101000 100110 110001 110001
        // Packed: 10100010 01101100 01110001
        //       = 0xA2     0x6C     0x71
        let encoded = encode("call").unwrap();
        assert_eq!(encoded, vec![0xA2, 0x6C, 0x71]);
    }
}
