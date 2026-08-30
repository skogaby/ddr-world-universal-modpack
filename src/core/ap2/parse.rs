//! AP2 document parser — `Ap2Doc::parse` over descrambled data.
//!
//! Transcribed from bemaniutils `bemani/format/afp/swf.py` (Unlicense):
//! file header in `__parse` (~line 2714), tag sections in `__parse_tags`
//! (~line 2536), per-tag decode in `__parse_tag` (~line 1178). Every read is
//! bounds-checked; any structural violation returns `None` — the caller is a
//! hook-adjacent patch function, so this path must never panic.
//!
//! Beyond decoding, the parser captures the layout metadata (region order,
//! gap bytes, tag padding, raw prefix/middle/suffix) that lets
//! `Ap2Doc::serialize` reproduce the input byte-for-byte when the model is
//! unmodified (strategy write-up in the module docs and the task plan).

use super::align4;
use super::model::{
    read_u16, read_u32, Ap2Doc, DefineSprite, FileOrder, FrameSpan, Label, OpaqueTag, PlaceObject,
    RegionKind, RegionSlot, SectionLayout, Shape, StringTable, Tag, TagSection, FILE_HEADER_MIN,
    FILE_HEADER_MIN_FLAG4, FRAME_COUNT_MAX, FRAME_START_MAX, SECTION_HEADER_LEN, STRING_TABLE_MAX,
    TAG_DEFINE_SPRITE, TAG_PLACE_OBJECT, TAG_SHAPE, TAG_SIZE_MAX,
};

impl Ap2Doc {
    /// Parse a descrambled AP2 image (the `afp_patcher` contract: BSI applied,
    /// string table plaintext). Total over malformed input: `None` on any
    /// structural violation, never panics.
    ///
    /// Accepted-input restrictions (mirrored by serialize-side limits so the
    /// round-trip property stays total): data version 0x200 only; header
    /// length must equal the buffer length; string table ≤ 64 KiB with
    /// 4-aligned size; section regions non-overlapping at/after the section
    /// header; known file regions non-overlapping at/after the fixed header.
    pub fn parse(data: &[u8]) -> Option<Ap2Doc> {
        // --- Fixed header (bemaniutils __parse ~2718: `<4sIHHIHHHH`) ---
        if data.len() < FILE_HEADER_MIN {
            return None;
        }
        // Magic: byte 0 = container version 7..=10; bytes 3..1 & 0x7F spell
        // "AP2" (bemaniutils ~2730).
        if (data[3] & 0x7F, data[2] & 0x7F, data[1] & 0x7F) != (b'A', b'P', b'2') {
            return None;
        }
        if !(7..=10).contains(&data[0]) {
            return None;
        }
        let total_len = read_u32(data, 4)? as usize;
        if total_len != data.len() {
            return None;
        }
        // Data version 0x100 is the old bit-packed tag format — our tag walk
        // would misread it (bemaniutils ~2738 rejects it too).
        if read_u16(data, 8)? != 0x200 {
            return None;
        }
        let name_offset = read_u16(data, 10)?;
        let header_flags = read_u32(data, 12)?;
        let header_min = if header_flags & 0x4 != 0 {
            // Imported-tag-initializers pointer occupies @56 (bemaniutils ~2780).
            FILE_HEADER_MIN_FLAG4
        } else {
            FILE_HEADER_MIN
        };
        if data.len() < header_min {
            return None;
        }

        // --- String table (@48/@52; input is already plaintext) ---
        let st_offset = read_u32(data, 48)? as usize;
        let st_size = read_u32(data, 52)? as usize;
        let st_end = st_offset.checked_add(st_size)?;
        if st_end > data.len() || st_size > STRING_TABLE_MAX {
            return None;
        }
        let strings = StringTable::from_plain_bytes(&data[st_offset..st_end])?;
        let exported_name = strings.get(name_offset)?.to_string();

        // --- Root tag section (@36) ---
        let tags_offset = read_u32(data, 36)? as usize;
        if tags_offset > data.len() {
            return None;
        }
        let (root, extent) = parse_section(&data[tags_offset..], &strings)?;

        // --- File region map (byte-identity carriage) ---
        let sec_iv = (tags_offset, tags_offset.checked_add(extent)?);
        let st_iv = (st_offset, st_end);
        let (first, second, order) = if sec_iv.0 <= st_iv.0 {
            (sec_iv, st_iv, FileOrder::RootFirst)
        } else {
            (st_iv, sec_iv, FileOrder::StringsFirst)
        };
        if first.0 < header_min || second.0 < first.1 || second.1 > data.len() {
            return None;
        }

        Some(Ap2Doc {
            root,
            strings,
            exported_name,
            prefix: data[..first.0].to_vec(),
            middle: data[first.1..second.0].to_vec(),
            suffix: data[second.1..].to_vec(),
            order,
            header_flags,
            orig_first: (first.0 as u32, first.1 as u32),
            orig_second: (second.0 as u32, second.1 as u32),
        })
    }
}

/// Parse one tag section from `s` (slice starting at the section base,
/// extending to the end of the enclosing scope). Returns the section plus its
/// extent — the end of its last non-empty region, relative to the base;
/// trailing bytes belong to the caller.
pub(super) fn parse_section(s: &[u8], strings: &StringTable) -> Option<(TagSection, usize)> {
    // Header `<HHIIIII` (bemaniutils __parse_tags ~2546).
    if s.len() < SECTION_HEADER_LEN {
        return None;
    }
    let name_reference_flags = read_u16(s, 0)?;
    let name_reference_count = read_u16(s, 2)? as usize;
    let frame_count = read_u32(s, 4)? as usize;
    let tags_count = read_u32(s, 8)? as usize;
    let name_reference_offset = read_u32(s, 12)? as usize;
    let frame_offset = read_u32(s, 16)? as usize;
    let tags_offset = read_u32(s, 20)? as usize;

    // --- Frames: packed u32, low 20 start / next 12 count (~2570) ---
    let frames_end = frame_offset.checked_add(frame_count.checked_mul(4)?)?;
    if frame_count > 0 && (frame_offset < SECTION_HEADER_LEN || frames_end > s.len()) {
        return None;
    }
    let mut frames = Vec::with_capacity(frame_count.min(4096));
    for i in 0..frame_count {
        let w = read_u32(s, frame_offset + i * 4)?;
        frames.push(FrameSpan {
            start_tag: w & FRAME_START_MAX,
            tag_count: (w >> 20) & FRAME_COUNT_MAX,
        });
    }

    // --- Tags: u32 header + payload, 4-aligned (~2590) ---
    if tags_count > 0 && tags_offset < SECTION_HEADER_LEN {
        return None;
    }
    let mut tags = Vec::with_capacity(tags_count.min(4096));
    let mut pos = tags_offset;
    for _ in 0..tags_count {
        let w = read_u32(s, pos)?;
        let tag_id = ((w >> 22) & 0x3FF) as u16;
        let size = (w & TAG_SIZE_MAX) as usize;
        let payload_start = pos.checked_add(4)?;
        let payload_end = payload_start.checked_add(size)?;
        let padded_end = payload_start.checked_add(align4(size))?;
        if padded_end > s.len() {
            // Includes truncated final-tag padding: the pad bytes must exist
            // in full so they can be carried for byte identity.
            return None;
        }
        let payload = &s[payload_start..payload_end];
        let pad = s[payload_end..padded_end].to_vec();
        tags.push(parse_tag(tag_id, payload, pad, strings)?);
        pos = padded_end;
    }
    let tags_end = pos;

    // --- Name references (frame labels): `<HH>` pairs (~2616) ---
    let nr_end = name_reference_offset.checked_add(name_reference_count.checked_mul(4)?)?;
    if name_reference_count > 0 && (name_reference_offset < SECTION_HEADER_LEN || nr_end > s.len())
    {
        return None;
    }
    let mut labels = Vec::with_capacity(name_reference_count.min(4096));
    for i in 0..name_reference_count {
        let at = name_reference_offset + i * 4;
        let frame = read_u16(s, at)?;
        let label_name_offset = read_u16(s, at + 2)?;
        let name = strings.get(label_name_offset)?.to_string();
        labels.push(Label {
            frame,
            name_offset: label_name_offset,
            name,
        });
    }

    // --- Region layout capture (byte-identity metadata) ---
    struct Region {
        kind: RegionKind,
        start: usize,
        end: usize,
    }
    let mut regions = [
        Region {
            kind: RegionKind::NameRefs,
            start: name_reference_offset,
            end: nr_end,
        },
        Region {
            kind: RegionKind::Frames,
            start: frame_offset,
            end: frames_end,
        },
        Region {
            kind: RegionKind::Tags,
            start: tags_offset,
            end: tags_end,
        },
    ];
    regions.sort_by_key(|r| (r.start, kind_rank(r.kind)));

    let mut cursor = SECTION_HEADER_LEN;
    let mut slots = Vec::with_capacity(3);
    for r in &regions {
        let gap_before = if r.end == r.start {
            // Empty region: consumes nothing; its (semantics-free) offset is
            // preserved verbatim via orig_offset.
            Vec::new()
        } else {
            if r.start < cursor {
                return None; // overlapping regions cannot round-trip cleanly
            }
            let gap = s.get(cursor..r.start)?.to_vec();
            cursor = r.end;
            gap
        };
        slots.push(RegionSlot {
            kind: r.kind,
            gap_before,
            orig_offset: Some(r.start as u32),
        });
    }

    Some((
        TagSection {
            frames,
            tags,
            labels,
            layout: SectionLayout {
                name_reference_flags,
                regions: slots,
            },
        },
        cursor,
    ))
}

/// Deterministic tie-break rank for regions sharing an offset (only possible
/// when at least one is empty, where relative order does not affect bytes).
pub(super) fn kind_rank(kind: RegionKind) -> usize {
    match kind {
        RegionKind::NameRefs => 0,
        RegionKind::Frames => 1,
        RegionKind::Tags => 2,
    }
}

/// Decode one tag (bemaniutils `__parse_tag` ~1178). Typed: DefineSprite
/// (recursive — a malformed nested section fails the whole document),
/// PlaceObject (payload carried opaquely, see `model::PlaceObject`), Shape
/// (fixed 4 bytes; other sizes fall back to opaque carriage). Everything
/// else: byte-exact `Opaque`.
fn parse_tag(tag_id: u16, payload: &[u8], pad: Vec<u8>, strings: &StringTable) -> Option<Tag> {
    match tag_id {
        TAG_DEFINE_SPRITE => {
            // `<HH>` sprite_flags, sprite_id; flags bit 0 → u32 relative
            // subtags pointer at +4, else subtags directly at +4
            // (bemaniutils ~1206).
            let flags = read_u16(payload, 0)?;
            let id = read_u16(payload, 2)?;
            let (header_len, subtags_rel) = if flags & 1 != 0 {
                let rel = read_u32(payload, 4)? as usize;
                if rel < 8 || rel > payload.len() {
                    return None;
                }
                (8usize, rel)
            } else {
                if payload.len() < 4 {
                    return None;
                }
                (4usize, 4usize)
            };
            let pre_section = payload[header_len..subtags_rel].to_vec();
            let (section, extent) = parse_section(&payload[subtags_rel..], strings)?;
            let post_section = payload[subtags_rel + extent..].to_vec();
            Some(Tag::DefineSprite(DefineSprite {
                flags,
                id,
                pre_section,
                section,
                post_section,
                pad,
            }))
        }
        TAG_PLACE_OBJECT => Some(Tag::PlaceObject(PlaceObject {
            data: payload.to_vec(),
            pad,
        })),
        TAG_SHAPE if payload.len() == 4 => Some(Tag::Shape(Shape {
            unknown: read_u16(payload, 0)?,
            id: read_u16(payload, 2)?,
        })),
        _ => Some(Tag::Opaque(OpaqueTag {
            tag_id,
            data: payload.to_vec(),
            pad,
        })),
    }
}
