//! GE2D geo-file label primitives — read and rewrite the texture-region
//! name strings a GE2D shape binds to.
//!
//! Promoted from `mods/folder_expansion.rs::patch_ge2d_labels` (which
//! rewrote labels strictly in place and silently TRUNCATED replacements
//! longer than the original) and extended with length-changing rebuilds:
//! a replacement that fits the original slot is written in place (byte-
//! identical to the shipped folder_expansion behavior for its equal/shorter
//! cases); a longer one is appended at end-of-file (4-aligned), the label's
//! pointer re-aimed at the appended copy, and the header file-size field
//! updated. Label pointers are absolute file offsets, so appending never
//! disturbs the vertex/tex/color/render-param sections.
//!
//! Format (transcribed from the bemaniutils project's `geo.py`, Unlicense;
//! layout notes in `docs/afp_texture_pipeline.md`):
//!
//! - magic @0: `D2EG` (little-endian file) or `GE2D` (big-endian file) —
//!   all multi-byte fields follow the magic's endianness.
//! - two version u32s @4/@8 (opaque here).
//! - **file size u32 @12** — bemaniutils validates it against the real
//!   length, so it must be kept correct across appends.
//! - file flags u32 @16 (opaque here).
//! - counts @20..32, six u16: vertex, tex, color, **label**, render_params,
//!   padding.
//! - offsets @32..52, five u32: vertex, tex, color, **label**,
//!   render_params.
//! - label section: `label_count` u32 ABSOLUTE file offsets, each pointing
//!   at a null-terminated string. Strings are optionally obfuscated by
//!   adding 0x80 to every byte (self-inverse mod 256); detection is
//!   bemaniutils-exact: a first byte `>= 0xA0` marks the string obfuscated.
//!   (The real dance_judge geo labels are PLAIN; folder geo labels are
//!   obfuscated — both occur in shipping data.)
//!
//! This module is deliberately **std-only** (no `crate::` imports, no
//! logging) so `scripts/validate_s_marvelous.sh` can mount it via `#[path]`
//! into the host-test harness — plain `cargo test` cannot compile the
//! `retour` dependency on non-x86 hosts. Callers that want per-label logs
//! emit them from the rewrite closure.

/// Byte offsets of the header fields this module touches.
const OFF_FILESIZE: usize = 12;
const OFF_LABEL_COUNT: usize = 26; // counts[3]
const OFF_LABEL_OFFSET: usize = 44; // offsets[3]
/// Minimum bytes a parseable header needs (through the offsets array).
const HEADER_LEN: usize = 52;

/// Endianness of a GE2D file's multi-byte fields, keyed by the magic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Endian {
    Little,
    Big,
}

fn detect_endian(data: &[u8]) -> Option<Endian> {
    match data.get(0..4)? {
        b"D2EG" => Some(Endian::Little),
        b"GE2D" => Some(Endian::Big),
        _ => None,
    }
}

fn read_u16(data: &[u8], at: usize, e: Endian) -> Option<u16> {
    let b = [*data.get(at)?, *data.get(at + 1)?];
    Some(match e {
        Endian::Little => u16::from_le_bytes(b),
        Endian::Big => u16::from_be_bytes(b),
    })
}

fn read_u32(data: &[u8], at: usize, e: Endian) -> Option<u32> {
    let b = [
        *data.get(at)?,
        *data.get(at + 1)?,
        *data.get(at + 2)?,
        *data.get(at + 3)?,
    ];
    Some(match e {
        Endian::Little => u32::from_le_bytes(b),
        Endian::Big => u32::from_be_bytes(b),
    })
}

fn write_u32(data: &mut [u8], at: usize, v: u32, e: Endian) {
    let b = match e {
        Endian::Little => v.to_le_bytes(),
        Endian::Big => v.to_be_bytes(),
    };
    data[at..at + 4].copy_from_slice(&b);
}

/// One label's parse-time facts, shared by [`labels`] and [`rewrite_labels`].
struct RawLabel {
    /// Byte offset of this label's u32 pointer slot.
    ptr_slot: usize,
    /// The pointer value: absolute offset of the string.
    str_off: usize,
    /// String bytes as stored (excluding the NUL).
    raw_len: usize,
    /// Whether the stored bytes are obfuscated (+0x80 codec).
    obfuscated: bool,
    /// Decoded plaintext.
    text: String,
}

/// Validate the header and walk the label section. `None` on any structural
/// inconsistency: bad magic, header truncation, a file-size field that does
/// not match the real length, a label section outside the file, a pointer
/// outside the file, an unterminated string, or non-UTF-8 plaintext.
fn parse_labels(data: &[u8]) -> Option<Vec<RawLabel>> {
    let e = detect_endian(data)?;
    if data.len() < HEADER_LEN {
        return None;
    }
    // A stale size field means we do not understand this file — refuse
    // rather than compound the inconsistency.
    if read_u32(data, OFF_FILESIZE, e)? as usize != data.len() {
        return None;
    }
    let label_count = read_u16(data, OFF_LABEL_COUNT, e)? as usize;
    let label_offset = read_u32(data, OFF_LABEL_OFFSET, e)? as usize;
    if label_count == 0 || label_offset == 0 {
        return None;
    }
    label_offset.checked_add(label_count.checked_mul(4)?)?;
    if label_offset + label_count * 4 > data.len() {
        return None;
    }

    let mut out = Vec::with_capacity(label_count);
    for i in 0..label_count {
        let ptr_slot = label_offset + i * 4;
        let str_off = read_u32(data, ptr_slot, e)? as usize;
        if str_off >= data.len() {
            return None;
        }
        let mut end = str_off;
        while *data.get(end)? != 0 {
            end += 1;
        }
        let raw = &data[str_off..end];
        if raw.is_empty() {
            return None;
        }
        // bemaniutils `descramble_text`: obfuscated iff the first byte is
        // >= 0xA0 (python `(b - 0x20) > 0x7F` without wraparound).
        let obfuscated = raw[0] >= 0xA0;
        let decoded: Vec<u8> = if obfuscated {
            raw.iter().map(|b| b.wrapping_add(0x80)).collect()
        } else {
            raw.to_vec()
        };
        let text = String::from_utf8(decoded).ok()?;
        out.push(RawLabel {
            ptr_slot,
            str_off,
            raw_len: raw.len(),
            obfuscated,
            text,
        });
    }
    Some(out)
}

/// Decoded label texts (texture-region names) in file order. `None` on any
/// structural inconsistency (see [`parse_labels`]).
pub fn labels(data: &[u8]) -> Option<Vec<String>> {
    Some(parse_labels(data)?.into_iter().map(|l| l.text).collect())
}

/// Rewrite label strings through `rewrite` (`Some(new)` = replace this
/// label, `None` = leave it). Returns the rebuilt file, or `None` when the
/// file is structurally unrecognized, a replacement is invalid (empty /
/// non-ASCII), or **no label was rewritten** — the same "None = nothing to
/// patch" contract the folder_expansion original had, so callers can fall
/// back to the donor bytes verbatim.
///
/// Replacements that fit the original slot are written in place with NUL
/// fill (byte-identical to the shipped equal/shorter behavior). Longer
/// replacements — and replacements of a string another label also points
/// at — are appended at end-of-file (zero-padded to 4-byte alignment first),
/// the label pointer re-aimed, and the file-size header field updated. Each
/// stored string keeps its original obfuscation state.
pub fn rewrite_labels(data: &[u8], rewrite: impl Fn(&str) -> Option<String>) -> Option<Vec<u8>> {
    let e = detect_endian(data)?;
    let parsed = parse_labels(data)?;

    let mut out = data.to_vec();
    let mut changed = false;

    for (i, label) in parsed.iter().enumerate() {
        let Some(new_text) = rewrite(&label.text) else {
            continue;
        };
        // The stored form must survive the obfuscation round trip and the
        // plain form must not false-positive the obfuscation probe.
        if new_text.is_empty() || !new_text.is_ascii() {
            return None;
        }
        let new_bytes: Vec<u8> = if label.obfuscated {
            new_text.bytes().map(|b| b.wrapping_add(0x80)).collect()
        } else {
            new_text.into_bytes()
        };

        // In-place is only safe when the slot is big enough AND no other
        // label aliases the same string bytes.
        let aliased = parsed
            .iter()
            .enumerate()
            .any(|(j, other)| j != i && other.str_off == label.str_off);
        if new_bytes.len() <= label.raw_len && !aliased {
            for j in 0..label.raw_len {
                out[label.str_off + j] = if j < new_bytes.len() { new_bytes[j] } else { 0 };
            }
        } else {
            // Append: pad to 4-byte alignment, write string + NUL, repoint.
            while out.len() % 4 != 0 {
                out.push(0);
            }
            let new_off = u32::try_from(out.len()).ok()?;
            out.extend_from_slice(&new_bytes);
            out.push(0);
            write_u32(&mut out, label.ptr_slot, new_off, e);
        }
        changed = true;
    }

    if !changed {
        return None;
    }
    let new_size = u32::try_from(out.len()).ok()?;
    write_u32(&mut out, OFF_FILESIZE, new_size, e);
    Some(out)
}

// ---------------------------------------------------------------------------
// Tests (std-only; also run by the validate_s_marvelous.sh harness mount).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal-but-honest GE2D fixture: header, label pointer array
    /// at 52, strings after it, then a 4-aligned dummy "vertex" region so
    /// the label section is not the last thing in the file (mirrors the real
    /// dance_judge_shape32 layout, where labels sit between the header and
    /// the vertex floats).
    fn build_geo(e: Endian, labels: &[(&str, bool)]) -> Vec<u8> {
        let w16 = |v: u16| -> [u8; 2] {
            match e {
                Endian::Little => v.to_le_bytes(),
                Endian::Big => v.to_be_bytes(),
            }
        };
        let w32 = |v: u32| -> [u8; 4] {
            match e {
                Endian::Little => v.to_le_bytes(),
                Endian::Big => v.to_be_bytes(),
            }
        };

        let mut out = Vec::new();
        out.extend_from_slice(match e {
            Endian::Little => b"D2EG",
            Endian::Big => b"GE2D",
        });
        out.extend_from_slice(&w32(1)); // version a
        out.extend_from_slice(&w32(2)); // version b
        out.extend_from_slice(&w32(0)); // filesize (patched below)
        out.extend_from_slice(&w32(0)); // fileflags
        out.extend_from_slice(&w16(1)); // vertex_count
        out.extend_from_slice(&w16(0)); // tex_count
        out.extend_from_slice(&w16(0)); // color_count
        out.extend_from_slice(&w16(labels.len() as u16)); // label_count
        out.extend_from_slice(&w16(0)); // render_params_count
        out.extend_from_slice(&w16(0)); // padding
        debug_assert_eq!(out.len(), 32);
        // Offsets patched after layout below.
        out.extend_from_slice(&[0u8; 20]);
        debug_assert_eq!(out.len(), HEADER_LEN);

        // Label pointer array.
        let label_offset = out.len();
        out.extend_from_slice(&vec![0u8; labels.len() * 4]);

        // Strings.
        let mut ptrs = Vec::new();
        for (text, obf) in labels {
            ptrs.push(out.len() as u32);
            let bytes: Vec<u8> = if *obf {
                text.bytes().map(|b| b.wrapping_add(0x80)).collect()
            } else {
                text.as_bytes().to_vec()
            };
            out.extend_from_slice(&bytes);
            out.push(0);
        }
        while out.len() % 4 != 0 {
            out.push(0);
        }

        // Dummy vertex region (one 8-byte point).
        let vertex_offset = out.len() as u32;
        out.extend_from_slice(&[0u8; 8]);

        // Patch offsets, pointers, filesize.
        let mut patch32 = |at: usize, v: u32| {
            let b = w32(v);
            out[at..at + 4].copy_from_slice(&b);
        };
        patch32(32, vertex_offset); // vertex_offset
        patch32(44, label_offset as u32); // label_offset
        for (i, p) in ptrs.iter().enumerate() {
            patch32(label_offset + i * 4, *p);
        }
        let total = out.len() as u32;
        let b = w32(total);
        out[OFF_FILESIZE..OFF_FILESIZE + 4].copy_from_slice(&b);
        out
    }

    /// The pre-promotion folder_expansion algorithm, kept verbatim as the
    /// byte-identity oracle for the in-place (equal/shorter) path.
    fn legacy_inplace(data: &[u8], old: &str, new: &str) -> Vec<u8> {
        let mut patched = data.to_vec();
        let e = detect_endian(data).unwrap();
        let label_count = read_u16(data, OFF_LABEL_COUNT, e).unwrap() as usize;
        let label_offset = read_u32(data, OFF_LABEL_OFFSET, e).unwrap() as usize;
        for i in 0..label_count {
            let str_off = read_u32(data, label_offset + i * 4, e).unwrap() as usize;
            let mut end = str_off;
            while patched[end] != 0 {
                end += 1;
            }
            let raw = patched[str_off..end].to_vec();
            let obf = (raw[0].wrapping_sub(0x20)) > 0x7F;
            let decoded: Vec<u8> = if obf {
                raw.iter().map(|b| b.wrapping_add(0x80)).collect()
            } else {
                raw.clone()
            };
            let label = String::from_utf8(decoded).unwrap();
            if !label.contains(old) {
                continue;
            }
            let new_label = label.replace(old, new);
            let new_bytes: Vec<u8> = if obf {
                new_label.bytes().map(|b| b.wrapping_add(0x80)).collect()
            } else {
                new_label.into_bytes()
            };
            for j in 0..raw.len() {
                patched[str_off + j] = if j < new_bytes.len() { new_bytes[j] } else { 0 };
            }
        }
        patched
    }

    fn substr_rewriter(old: &'static str, new: &'static str) -> impl Fn(&str) -> Option<String> {
        move |label| {
            if label.contains(old) {
                Some(label.replace(old, new))
            } else {
                None
            }
        }
    }

    #[test]
    fn geo_labels_reads_plain_and_obfuscated_both_endians() {
        for e in [Endian::Little, Endian::Big] {
            let data = build_geo(
                e,
                &[("dance_judge0000_marvelous", false), ("mufo_x_on", true)],
            );
            let got = labels(&data).expect("parses");
            assert_eq!(got, vec!["dance_judge0000_marvelous", "mufo_x_on"]);
        }
    }

    #[test]
    fn geo_rewrite_equal_length_matches_legacy_bytes() {
        for e in [Endian::Little, Endian::Big] {
            for obf in [false, true] {
                let data = build_geo(e, &[("card_firststep_on", obf), ("card_shared", obf)]);
                let out = rewrite_labels(&data, substr_rewriter("firststep", "ninestepz"))
                    .expect("rewrites");
                assert_eq!(out, legacy_inplace(&data, "firststep", "ninestepz"));
                assert_eq!(out.len(), data.len(), "in-place keeps length");
                assert_eq!(
                    labels(&out).unwrap(),
                    vec!["card_ninestepz_on", "card_shared"]
                );
            }
        }
    }

    #[test]
    fn geo_rewrite_shorter_matches_legacy_bytes() {
        let data = build_geo(Endian::Big, &[("card_firststep_on", true)]);
        let out = rewrite_labels(&data, substr_rewriter("firststep", "dogs")).expect("rewrites");
        assert_eq!(out, legacy_inplace(&data, "firststep", "dogs"));
        assert_eq!(labels(&out).unwrap(), vec!["card_dogs_on"]);
    }

    #[test]
    fn geo_rewrite_longer_appends_repoints_and_updates_filesize() {
        for e in [Endian::Little, Endian::Big] {
            for obf in [false, true] {
                let data = build_geo(e, &[("dance_judge0000_marvelous", obf)]);
                let out = rewrite_labels(&data, substr_rewriter("marvelous", "smarvelous"))
                    .expect("rewrites");
                assert!(out.len() > data.len(), "appended");
                // Everything before the append is untouched EXCEPT the
                // filesize field and the label pointer slot.
                let label_offset = read_u32(&data, OFF_LABEL_OFFSET, e).unwrap() as usize;
                for i in 0..data.len() {
                    if (OFF_FILESIZE..OFF_FILESIZE + 4).contains(&i)
                        || (label_offset..label_offset + 4).contains(&i)
                    {
                        continue;
                    }
                    assert_eq!(out[i], data[i], "byte {} changed unexpectedly", i);
                }
                // Header filesize matches the real new length.
                assert_eq!(read_u32(&out, OFF_FILESIZE, e).unwrap() as usize, out.len());
                // Pointer aims at a 4-aligned appended copy.
                let new_ptr = read_u32(&out, label_offset, e).unwrap() as usize;
                assert!(new_ptr >= data.len());
                assert_eq!(new_ptr % 4, 0);
                // Round-trips through the parser with the new name.
                assert_eq!(
                    labels(&out).unwrap(),
                    vec!["dance_judge0000_smarvelous".to_string()]
                );
            }
        }
    }

    #[test]
    fn geo_rewrite_real_shape32_layout_shape() {
        // Mirror of the real dance_judge_shape32 facts (dev probe): BE file,
        // one PLAIN label between the header and the data sections.
        let data = build_geo(Endian::Big, &[("dance_judge0000_marvelous", false)]);
        let out = rewrite_labels(&data, |l| {
            let stem = l.strip_suffix("marvelous")?;
            Some(format!("{}smarvelous", stem))
        })
        .expect("rewrites");
        assert_eq!(
            labels(&out).unwrap(),
            vec!["dance_judge0000_smarvelous".to_string()]
        );
    }

    #[test]
    fn geo_rewrite_multi_label_partial_and_mixed_lengths() {
        let data = build_geo(
            Endian::Little,
            &[("aaa_word", false), ("bbb_keep", true), ("ccc_word", true)],
        );
        // First grows (append), third shrinks (in place), second untouched.
        let out = rewrite_labels(&data, |l| match l {
            "aaa_word" => Some("aaa_wordier".to_string()),
            "ccc_word" => Some("ccc_w".to_string()),
            _ => None,
        })
        .expect("rewrites");
        assert_eq!(
            labels(&out).unwrap(),
            vec!["aaa_wordier", "bbb_keep", "ccc_w"]
        );
        assert_eq!(
            read_u32(&out, OFF_FILESIZE, Endian::Little).unwrap() as usize,
            out.len()
        );
    }

    #[test]
    fn geo_rewrite_aliased_string_appends_instead_of_mutating() {
        // Two labels pointing at the SAME string bytes: rewriting one must
        // not change the other's text.
        let mut data = build_geo(
            Endian::Little,
            &[("shared_word", false), ("shared_word", false)],
        );
        let label_offset = read_u32(&data, OFF_LABEL_OFFSET, Endian::Little).unwrap() as usize;
        let first_ptr = read_u32(&data, label_offset, Endian::Little).unwrap();
        write_u32(&mut data, label_offset + 4, first_ptr, Endian::Little);

        // Rewrite the shared text (hits BOTH labels; the replacement is
        // SHORTER, so without alias detection it would go in place and
        // corrupt the sibling's view of the shared bytes mid-loop).
        let out = rewrite_labels(&data, |l| {
            if l == "shared_word" {
                Some("short".to_string())
            } else {
                None
            }
        })
        .expect("rewrites");
        // Both labels were rewritten (same text), both via the append path,
        // and both decode to the new name.
        assert_eq!(labels(&out).unwrap(), vec!["short", "short"]);
        // The original shared bytes are untouched (dead data).
        let sp = first_ptr as usize;
        assert_eq!(&out[sp..sp + "shared_word".len()], b"shared_word");
    }

    #[test]
    fn geo_rewrite_failures_are_none() {
        let good = build_geo(Endian::Little, &[("word", false)]);

        // No label rewritten.
        assert!(rewrite_labels(&good, |_| None).is_none());
        // Empty / non-ASCII replacements.
        assert!(rewrite_labels(&good, |_| Some(String::new())).is_none());
        assert!(rewrite_labels(&good, |_| Some("wörd".to_string())).is_none());

        // Bad magic.
        let mut bad = good.clone();
        bad[0] = b'X';
        assert!(labels(&bad).is_none());
        assert!(rewrite_labels(&bad, |_| Some("x".into())).is_none());

        // Stale filesize field.
        let mut bad = good.clone();
        bad[OFF_FILESIZE] ^= 0xFF;
        assert!(labels(&bad).is_none());
        assert!(rewrite_labels(&bad, |_| Some("x".into())).is_none());

        // Truncated header.
        assert!(labels(&good[..40]).is_none());

        // Label pointer outside the file.
        let mut bad = good.clone();
        let label_offset = read_u32(&good, OFF_LABEL_OFFSET, Endian::Little).unwrap() as usize;
        write_u32(&mut bad, label_offset, 0xFFFF, Endian::Little);
        // filesize still matches, magic ok — the pointer itself is bad.
        assert!(labels(&bad).is_none());

        // Label section past EOF.
        let mut bad = good.clone();
        write_u32(&mut bad, OFF_LABEL_OFFSET, 0xFFF0, Endian::Little);
        assert!(labels(&bad).is_none());
    }
}
