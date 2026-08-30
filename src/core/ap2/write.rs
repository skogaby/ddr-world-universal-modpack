//! AP2 document serializer — `Ap2Doc::serialize` with full offset
//! recomputation, plus the from-scratch `PlaceObject::build` encoder.
//!
//! Every offset, length, count, and alignment is recomputed here; the
//! parse-time layout metadata (region order, gap bytes, empty-region
//! offsets, tag padding, raw prefix/middle/suffix, raw string-table bytes)
//! makes the recomputation land on the original values for unmodified
//! documents — byte identity is a property of the emission algorithm, not a
//! cached-bytes shortcut. Limit violations (design §4.1: string table
//! ≤ 64 KiB / 4-aligned, frame start ≤ 20 bits, frame count ≤ 12 bits, tag
//! size ≤ 22 bits, tag id ≤ 10 bits, label count ≤ u16) return `None` —
//! never panic; this runs inside hook-adjacent patch functions.

use super::align4;
use super::model::{
    Ap2Doc, FileOrder, PlaceObject, PlaceObjectParams, RegionKind, Tag, TagSection,
    FILE_HEADER_MIN, FILE_HEADER_MIN_FLAG4, FRAME_COUNT_MAX, FRAME_START_MAX, SECTION_HEADER_LEN,
    STRING_TABLE_MAX, TAG_DEFINE_SPRITE, TAG_ID_MAX, TAG_PLACE_OBJECT, TAG_SHAPE, TAG_SIZE_MAX,
};
use super::parse::kind_rank;

impl Ap2Doc {
    /// Serialize the document. `None` on any limit violation; never panics.
    pub fn serialize(&self) -> Option<Vec<u8>> {
        let root_bytes = serialize_section(&self.root)?;
        let st = self.strings.raw();
        // Defensive re-check (the StringTable API already enforces both):
        // misaligned tables are a live-game FATAL (docs/afp_system.md §2).
        if st.len() > STRING_TABLE_MAX || st.len() % 4 != 0 {
            return None;
        }
        if self.prefix.len() < FILE_HEADER_MIN {
            return None;
        }

        let mut out = self.prefix.clone();
        let (first_bytes, second_bytes): (&[u8], &[u8]) = match self.order {
            FileOrder::RootFirst => (&root_bytes, st),
            FileOrder::StringsFirst => (st, &root_bytes),
        };
        let new_first_start = out.len(); // == orig_first.0 (prefix carried)
        out.extend_from_slice(first_bytes);
        let new_first_end = out.len();
        out.extend_from_slice(&self.middle);
        let new_second_start = out.len();
        out.extend_from_slice(second_bytes);
        let new_second_end = out.len();
        out.extend_from_slice(&self.suffix);

        // --- Recomputed header fields ---
        let total = u32::try_from(out.len()).ok()?;
        out[4..8].copy_from_slice(&total.to_le_bytes());
        let (sec_start, st_start) = match self.order {
            FileOrder::RootFirst => (new_first_start, new_second_start),
            FileOrder::StringsFirst => (new_second_start, new_first_start),
        };
        out[36..40].copy_from_slice(&u32::try_from(sec_start).ok()?.to_le_bytes());
        out[48..52].copy_from_slice(&u32::try_from(st_start).ok()?.to_le_bytes());
        out[52..56].copy_from_slice(&u32::try_from(st.len()).ok()?.to_le_bytes());

        // --- Zone-delta fixups for the opaque-region pointers ---
        // @40 exported assets, @44 imported tags, @56 initializers (iff
        // flags&0x4) point into carried-verbatim zones that shift when the
        // known regions change size (all deltas are 0 for unmodified docs).
        let zones = ZoneMap {
            orig_first: self.orig_first,
            orig_second: self.orig_second,
            new_first: (new_first_start as u32, new_first_end as u32),
            new_second: (new_second_start as u32, new_second_end as u32),
        };
        for at in [40usize, 44] {
            remap_pointer_field(&mut out, at, &zones)?;
        }
        if self.header_flags & 0x4 != 0 {
            if out.len() < FILE_HEADER_MIN_FLAG4 {
                return None;
            }
            remap_pointer_field(&mut out, 56, &zones)?;
        }
        Some(out)
    }
}

struct ZoneMap {
    orig_first: (u32, u32),
    orig_second: (u32, u32),
    new_first: (u32, u32),
    new_second: (u32, u32),
}

impl ZoneMap {
    /// Shift a file-offset pointer by the delta of the zone it points into.
    /// Pointers into the fixed header stay put; values inside a known region
    /// or past EOF are semantics-free but shifted consistently.
    fn remap(&self, p: u32) -> u32 {
        if p >= self.orig_second.1 {
            p.wrapping_add(self.new_second.1)
                .wrapping_sub(self.orig_second.1)
        } else if p >= self.orig_second.0 {
            p.wrapping_add(self.new_second.0)
                .wrapping_sub(self.orig_second.0)
        } else if p >= self.orig_first.1 {
            p.wrapping_add(self.new_first.1)
                .wrapping_sub(self.orig_first.1)
        } else if p >= self.orig_first.0 {
            p.wrapping_add(self.new_first.0)
                .wrapping_sub(self.orig_first.0)
        } else {
            p
        }
    }
}

fn remap_pointer_field(out: &mut [u8], at: usize, zones: &ZoneMap) -> Option<()> {
    let field = out.get(at..at + 4)?;
    let v = u32::from_le_bytes([field[0], field[1], field[2], field[3]]);
    out.get_mut(at..at + 4)?
        .copy_from_slice(&zones.remap(v).to_le_bytes());
    Some(())
}

/// Serialize one tag section (recursive through DefineSprite). The offset
/// fixup logic lives here and nowhere else: region payloads are built first,
/// then emitted in the layout's recorded order with recorded gaps, and the
/// header is patched with the landing offsets.
pub(super) fn serialize_section(sec: &TagSection) -> Option<Vec<u8>> {
    // --- Frames: packed u32, low 20 start / next 12 count ---
    let mut frames_bytes = Vec::with_capacity(sec.frames.len() * 4);
    for f in &sec.frames {
        if f.start_tag > FRAME_START_MAX || f.tag_count > FRAME_COUNT_MAX {
            return None;
        }
        frames_bytes.extend_from_slice(&((f.tag_count << 20) | f.start_tag).to_le_bytes());
    }

    // --- Tags: header + payload + pad ---
    let mut tags_bytes = Vec::new();
    for tag in &sec.tags {
        append_tag(&mut tags_bytes, tag)?;
    }

    // --- Name references: `<HH>` frame + string offset ---
    if sec.labels.len() > u16::MAX as usize {
        return None;
    }
    let mut nr_bytes = Vec::with_capacity(sec.labels.len() * 4);
    for l in &sec.labels {
        nr_bytes.extend_from_slice(&l.frame.to_le_bytes());
        nr_bytes.extend_from_slice(&l.name_offset.to_le_bytes());
    }

    // --- Region emission in recorded order ---
    let layout = &sec.layout;
    if layout.regions.len() != 3 {
        return None;
    }
    let mut seen = [false; 3];
    let mut offsets = [0u32; 3]; // indexed by kind_rank
    let mut body = Vec::new();
    for slot in &layout.regions {
        let rank = kind_rank(slot.kind);
        if seen[rank] {
            return None; // malformed layout: duplicate region kind
        }
        seen[rank] = true;
        body.extend_from_slice(&slot.gap_before);
        let bytes: &[u8] = match slot.kind {
            RegionKind::NameRefs => &nr_bytes,
            RegionKind::Frames => &frames_bytes,
            RegionKind::Tags => &tags_bytes,
        };
        let cursor = u32::try_from(SECTION_HEADER_LEN + body.len()).ok()?;
        offsets[rank] = if bytes.is_empty() {
            // Empty region: nothing lands; re-emit the original
            // (semantics-free) offset so unmodified docs stay byte-identical.
            slot.orig_offset.unwrap_or(cursor)
        } else {
            cursor
        };
        body.extend_from_slice(bytes);
    }

    // --- Header `<HHIIIII` ---
    let mut out = Vec::with_capacity(SECTION_HEADER_LEN + body.len());
    out.extend_from_slice(&layout.name_reference_flags.to_le_bytes());
    out.extend_from_slice(&(sec.labels.len() as u16).to_le_bytes());
    out.extend_from_slice(&u32::try_from(sec.frames.len()).ok()?.to_le_bytes());
    out.extend_from_slice(&u32::try_from(sec.tags.len()).ok()?.to_le_bytes());
    out.extend_from_slice(&offsets[kind_rank(RegionKind::NameRefs)].to_le_bytes());
    out.extend_from_slice(&offsets[kind_rank(RegionKind::Frames)].to_le_bytes());
    out.extend_from_slice(&offsets[kind_rank(RegionKind::Tags)].to_le_bytes());
    out.extend_from_slice(&body);
    Some(out)
}

/// Encode a tag's payload. DefineSprite recurses; PlaceObject/Opaque emit
/// their carried bytes verbatim; Shape re-encodes its fixed 4 bytes.
fn encode_tag(tag: &Tag) -> Option<(u16, Vec<u8>, Vec<u8>)> {
    match tag {
        Tag::Opaque(o) => Some((o.tag_id, o.data.clone(), o.pad.clone())),
        Tag::PlaceObject(p) => Some((TAG_PLACE_OBJECT, p.data.clone(), p.pad.clone())),
        Tag::Shape(s) => {
            let mut d = Vec::with_capacity(4);
            d.extend_from_slice(&s.unknown.to_le_bytes());
            d.extend_from_slice(&s.id.to_le_bytes());
            Some((TAG_SHAPE, d, Vec::new()))
        }
        Tag::DefineSprite(sp) => {
            let mut d = Vec::new();
            d.extend_from_slice(&sp.flags.to_le_bytes());
            d.extend_from_slice(&sp.id.to_le_bytes());
            if sp.flags & 1 != 0 {
                // New-style: relative subtags pointer, reproduced from the
                // carried slack length.
                d.extend_from_slice(&u32::try_from(8 + sp.pre_section.len()).ok()?.to_le_bytes());
                d.extend_from_slice(&sp.pre_section);
            } else if !sp.pre_section.is_empty() {
                return None; // old-style sprites have no header slack
            }
            d.extend_from_slice(&serialize_section(&sp.section)?);
            d.extend_from_slice(&sp.post_section);
            Some((TAG_DEFINE_SPRITE, d, sp.pad.clone()))
        }
    }
}

/// Emit one tag: `(tag_id << 22) | size` header, payload, 4-byte padding.
/// The parse-time pad bytes are re-emitted while the payload length still
/// matches (real files may pad with non-zero bytes); zeros otherwise.
fn append_tag(out: &mut Vec<u8>, tag: &Tag) -> Option<()> {
    let (tag_id, payload, pad) = encode_tag(tag)?;
    if tag_id > TAG_ID_MAX {
        return None;
    }
    let size = u32::try_from(payload.len()).ok()?;
    if size > TAG_SIZE_MAX {
        return None;
    }
    out.extend_from_slice(&(((tag_id as u32) << 22) | size).to_le_bytes());
    out.extend_from_slice(&payload);
    let needed = align4(payload.len()) - payload.len();
    if pad.len() == needed {
        out.extend_from_slice(&pad);
    } else {
        out.extend(std::iter::repeat(0u8).take(needed));
    }
    Some(())
}

impl PlaceObject {
    /// Encode a new PlaceObject from scratch — modeled flags only (0x2
    /// source_tag_id, 0x20 movie_name, 0x100 scale, 0x200 rotate, 0x400
    /// translate), emitted in the bemaniutils read order (swf.py ~1281) with
    /// the mid-payload realign-to-4. Generalizes the fixed 0x22 shape
    /// `core/afp.rs::make_place_object` emits.
    pub fn build(params: &PlaceObjectParams) -> Option<PlaceObject> {
        let mut flags: u32 = 0;
        if params.source_tag_id.is_some() {
            flags |= 0x2;
        }
        if params.movie_name_offset.is_some() {
            flags |= 0x20;
        }
        if params.scale.is_some() {
            flags |= 0x100;
        }
        if params.rotate.is_some() {
            flags |= 0x200;
        }
        if params.translate.is_some() {
            flags |= 0x400;
        }

        let mut d = Vec::with_capacity(40);
        d.extend_from_slice(&flags.to_le_bytes());
        d.extend_from_slice(&params.depth.to_le_bytes());
        d.extend_from_slice(&params.object_id.to_le_bytes());
        if let Some(v) = params.source_tag_id {
            d.extend_from_slice(&v.to_le_bytes());
        }
        if let Some(v) = params.movie_name_offset {
            d.extend_from_slice(&v.to_le_bytes());
        }
        while d.len() % 4 != 0 {
            d.push(0); // realign catchup (bemaniutils ~1355)
        }
        for (a, b) in [params.scale, params.rotate, params.translate]
            .into_iter()
            .flatten()
        {
            d.extend_from_slice(&a.to_le_bytes());
            d.extend_from_slice(&b.to_le_bytes());
        }
        Some(PlaceObject {
            data: d,
            pad: Vec::new(),
        })
    }
}
