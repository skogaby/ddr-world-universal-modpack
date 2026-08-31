//! AP2 document model types and read accessors.
//!
//! Layout provenance: bemaniutils `bemani/format/afp/swf.py` (Unlicense) —
//! header/string table in `__parse` (~line 2714), tag sections in
//! `__parse_tags` (~line 2536), per-tag decode in `__parse_tag` (~line 1178,
//! PlaceObject at ~line 1281); tag id constants from
//! `bemani/format/afp/types/ap2.py:192-203`.
//!
//! Byte-identity design: the model recomputes every offset/length at
//! serialize time but carries parse-time layout metadata (region order, gap
//! bytes, tag padding, raw header/string-table carriage) so recomputation
//! reproduces the original bytes for unmodified documents. See the plan
//! write-up in
//! `.agents/scratchpad/2026-08-29-s-marvelous-judgement/ap2-model-parser/plan.md`.

use super::align4;

// --- Tag ids (bemani/format/afp/types/ap2.py:192-203) ----------------------

/// `AP2_DEFINE_FONT` — carried opaquely; named for the definition-tag
/// classification in `edit.rs` (`is_definition_tag_id`).
pub const TAG_DEFINE_FONT: u16 = 0x78;
/// `AP2_DEFINE_SPRITE` — recursive nested tag section.
pub const TAG_DEFINE_SPRITE: u16 = 0x79;

/// `AP2_DO_ACTION` — frame bytecode (gotoAndPlay loops etc.); carried
/// opaquely, with a spliceable string-offset table header (edit.rs
/// `retarget_do_action_strings`).
pub const TAG_DO_ACTION: u16 = 0x7A;
/// `AP2_DEFINE_TEXT` — carried opaquely; named for the definition-tag
/// classification in `edit.rs`.
pub const TAG_DEFINE_TEXT: u16 = 0x7D;
/// `AP2_DEFINE_EDIT_TEXT` — carried opaquely (named here for future use).
pub const TAG_DEFINE_EDIT_TEXT: u16 = 0x7E;
/// `AP2_PLACE_OBJECT` — flag-driven placement record.
pub const TAG_PLACE_OBJECT: u16 = 0x7F;
/// `AP2_REMOVE_OBJECT` — carried opaquely (named here for future use).
pub const TAG_REMOVE_OBJECT: u16 = 0x80;
/// `AP2_DEFINE_MORPH_SHAPE` — carried opaquely; named for the definition-tag
/// classification in `edit.rs`.
pub const TAG_DEFINE_MORPH_SHAPE: u16 = 0x82;
/// `AP2_IMAGE` — carried opaquely (named here for future use).
pub const TAG_IMAGE: u16 = 0x83;
/// `AP2_SHAPE` — 4-byte record binding geo `{exported_name}_shape{id}`.
pub const TAG_SHAPE: u16 = 0x84;

// --- Packed-field widths (bemaniutils swf.py __parse_tags ~2570/~2590) -----

/// Frame word low 20 bits: start index into the tag list.
pub const FRAME_START_MAX: u32 = 0xFFFFF;
/// Frame word next 12 bits: tag count executed that frame.
pub const FRAME_COUNT_MAX: u32 = 0xFFF;
/// Tag header low 22 bits: payload size.
pub const TAG_SIZE_MAX: u32 = 0x3FFFFF;
/// Tag header high 10 bits: tag id.
pub const TAG_ID_MAX: u16 = 0x3FF;

/// String table hard cap: offsets are u16 and the table is 4-aligned, so
/// anything past 64 KiB is unreachable (design §4.1 serialization limits).
pub const STRING_TABLE_MAX: usize = 0x10000;

/// Tag section header size: `<HHIIIII` (bemaniutils swf.py ~2546).
pub const SECTION_HEADER_LEN: usize = 24;

/// Fixed file-header extent when flags bit 0x4 (imported tag initializers,
/// field at @56) is clear; 60 when set (bemaniutils swf.py `__parse` ~2780).
pub const FILE_HEADER_MIN: usize = 56;
pub const FILE_HEADER_MIN_FLAG4: usize = 60;

// --- String table -----------------------------------------------------------

/// Plaintext (descrambled) AP2 string table.
///
/// Stored as the raw bytes exactly as parsed — appends are the only mutation,
/// so every existing table-relative offset (labels, exported name, PlaceObject
/// movie names inside opaque payloads, exported-asset entries) stays valid
/// forever. Offset 0 is the null string by convention: real tables never start
/// a string at 0 (bemaniutils `__descramble_stringtable` ~2698 raises on it),
/// and `__get_string(0)` returns `""`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringTable {
    raw: Vec<u8>,
}

impl StringTable {
    /// Wrap plaintext table bytes. Rejects tables the serializer could not
    /// re-emit: size > 64 KiB or size not 4-aligned (misalignment is a
    /// live-game FATAL — `docs/afp_system.md` §2).
    pub fn from_plain_bytes(raw: &[u8]) -> Option<StringTable> {
        if raw.len() > STRING_TABLE_MAX || raw.len() % 4 != 0 {
            return None;
        }
        Some(StringTable { raw: raw.to_vec() })
    }

    /// A minimal fresh table: 4 NUL bytes so offset 0 stays the null string
    /// and the first interned string lands 4-aligned.
    pub fn new_minimal() -> StringTable {
        StringTable { raw: vec![0; 4] }
    }

    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Read the NUL-terminated UTF-8 string starting at `offset`.
    /// Offset 0 is the null string (`Some("")`) per the bemaniutils
    /// convention (`__get_string` ~2703). Returns `None` on out-of-bounds
    /// offsets, unterminated runs, or invalid UTF-8.
    pub fn get(&self, offset: u16) -> Option<&str> {
        if offset == 0 {
            return Some("");
        }
        let start = offset as usize;
        if start >= self.raw.len() {
            return None;
        }
        let rel_end = self.raw[start..].iter().position(|b| *b == 0)?;
        std::str::from_utf8(&self.raw[start..start + rel_end]).ok()
    }

    /// Find the offset of an existing string (exact match at a string start:
    /// position 0 or right after a NUL). `""` maps to offset 0.
    pub fn find(&self, s: &str) -> Option<u16> {
        if s.is_empty() {
            return Some(0);
        }
        let needle = s.as_bytes();
        let mut at_start = true;
        for i in 0..self.raw.len() {
            if at_start
                && i != 0
                && self.raw[i..].starts_with(needle)
                && self.raw.get(i + needle.len()) == Some(&0)
            {
                return Some(i as u16);
            }
            at_start = self.raw[i] == 0;
        }
        None
    }

    /// Return the offset of `s`, appending it (NUL-terminated, padded to 4)
    /// if absent. `None` when the append would push the table past 64 KiB —
    /// the table is left untouched in that case.
    pub fn intern(&mut self, s: &str) -> Option<u16> {
        if let Some(off) = self.find(s) {
            return Some(off);
        }
        let off = self.raw.len();
        let appended = align4(s.len() + 1);
        if off + appended > STRING_TABLE_MAX || off > u16::MAX as usize {
            return None;
        }
        self.raw.extend_from_slice(s.as_bytes());
        self.raw.push(0);
        self.raw.resize(off + appended, 0);
        Some(off as u16)
    }

    /// Enumerate `(offset, string)` for every string start in the table.
    pub fn strings(&self) -> Vec<(u16, &str)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.raw.len() {
            if self.raw[i] == 0 {
                i += 1;
                continue;
            }
            let start = i;
            while i < self.raw.len() && self.raw[i] != 0 {
                i += 1;
            }
            if i < self.raw.len() {
                if let Ok(s) = std::str::from_utf8(&self.raw[start..i]) {
                    // Offset 0 is reserved for the null string; a table whose
                    // first byte starts a string is rejected at parse.
                    out.push((start as u16, s));
                }
            }
            i += 1;
        }
        out
    }
}

// --- Frames / labels --------------------------------------------------------

/// One frame's packed span into the section tag list
/// (bemaniutils swf.py `__parse_tags` ~2570: `start = w & 0xFFFFF`,
/// `count = (w >> 20) & 0xFFF`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSpan {
    pub start_tag: u32,
    pub tag_count: u32,
}

/// One name-reference (frame label) entry: `<HH>` frame number + string
/// offset (bemaniutils swf.py ~2620). `name` is the parse-time resolution of
/// `name_offset` kept for ergonomics; `name_offset` is authoritative at
/// serialize time (string-table appends never move existing offsets).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub frame: u16,
    pub name_offset: u16,
    pub name: String,
}

// --- Section layout metadata -------------------------------------------------

/// The three data regions a tag section header points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionKind {
    NameRefs,
    Frames,
    Tags,
}

/// One region's emission slot: parse-time gap bytes that preceded it plus its
/// original offset (re-emitted verbatim when the region serializes to zero
/// bytes — the value is semantics-free at count 0 but must reproduce).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionSlot {
    pub kind: RegionKind,
    /// Raw filler bytes between the previous region's end (or the header)
    /// and this region's start, re-emitted verbatim.
    pub gap_before: Vec<u8>,
    /// Original header offset; `None` for sections built fresh.
    pub orig_offset: Option<u32>,
}

/// Parse-time layout metadata that lets the serializer's offset recomputation
/// reproduce the original section image byte-for-byte when unmodified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionLayout {
    /// Header `name_reference_flags` — semantics unknown (bemaniutils ignores
    /// it); preserved verbatim, never recomputed.
    pub name_reference_flags: u16,
    /// Emission order of the three regions (ascending original offset).
    /// Always exactly one slot per `RegionKind`.
    pub regions: Vec<RegionSlot>,
}

impl Default for SectionLayout {
    /// Canonical layout for freshly built sections: frames, then tags, then
    /// name references, no gaps (matches the shape
    /// `core/afp.rs::make_empty_define_sprite` emits).
    fn default() -> SectionLayout {
        SectionLayout {
            name_reference_flags: 0,
            regions: vec![
                RegionSlot {
                    kind: RegionKind::Frames,
                    gap_before: Vec::new(),
                    orig_offset: None,
                },
                RegionSlot {
                    kind: RegionKind::Tags,
                    gap_before: Vec::new(),
                    orig_offset: None,
                },
                RegionSlot {
                    kind: RegionKind::NameRefs,
                    gap_before: Vec::new(),
                    orig_offset: None,
                },
            ],
        }
    }
}

// --- Tags ---------------------------------------------------------------------

/// A tag section: frames, tags, and the section's own frame-label map
/// (frame labels are NOT tags — bemaniutils swf.py ~2616). The root movie and
/// every DefineSprite carry one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagSection {
    pub frames: Vec<FrameSpan>,
    pub tags: Vec<Tag>,
    pub labels: Vec<Label>,
    pub layout: SectionLayout,
}

impl TagSection {
    pub fn new() -> TagSection {
        TagSection {
            frames: Vec::new(),
            tags: Vec::new(),
            labels: Vec::new(),
            layout: SectionLayout::default(),
        }
    }

    /// Frame number a label maps to, if present in this section.
    pub fn label_frame(&self, name: &str) -> Option<u16> {
        self.labels.iter().find(|l| l.name == name).map(|l| l.frame)
    }

    /// This section's label map as `(name, frame)` pairs in file order.
    pub fn label_map(&self) -> Vec<(&str, u16)> {
        self.labels
            .iter()
            .map(|l| (l.name.as_str(), l.frame))
            .collect()
    }
}

impl Default for TagSection {
    fn default() -> TagSection {
        TagSection::new()
    }
}

/// `AP2_DEFINE_SPRITE (0x79)` — recursive nested tag section
/// (bemaniutils swf.py `__parse_tag` ~1206).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefineSprite {
    /// Sprite flags; bit 0 set = new-style (u32 relative subtags pointer at
    /// payload +4), clear = old-style (subtags directly at payload +4).
    pub flags: u16,
    pub id: u16,
    /// New-style slack between the 8-byte header and the nested section
    /// (payload bytes `[8, pointer)`); always empty for old-style.
    pub pre_section: Vec<u8>,
    pub section: TagSection,
    /// Payload bytes past the nested section's extent (usually empty).
    pub post_section: Vec<u8>,
    /// Parse-time tag padding bytes (see `OpaqueTag::pad`).
    pub pad: Vec<u8>,
}

/// `AP2_PLACE_OBJECT (0x7F)`.
///
/// The payload is carried **opaquely** — the flag-conditional field order
/// interleaves unmodeled fields between the modeled ones and skips
/// content-unspecified realignment bytes, so re-encoding from typed fields
/// cannot guarantee byte identity (decision record in the task plan).
/// Reads go through [`PlaceObject::view`]; new tags are built from scratch
/// via [`PlaceObject::build`]. Never mutated in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceObject {
    /// Exact payload bytes (flags word onward), re-emitted verbatim.
    pub data: Vec<u8>,
    /// Parse-time tag padding bytes (see `OpaqueTag::pad`).
    pub pad: Vec<u8>,
}

/// `AP2_SHAPE (0x84)` — fixed 4-byte record `<HH>` unknown + shape id; binds
/// geo `{exported_name}_shape{id}` (bemaniutils swf.py `__parse_tag` ~1190).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// Leading u16 — not parsed by the games bemaniutils examined; preserved.
    pub unknown: u16,
    pub id: u16,
}

/// Any tag the model does not type: byte-exact payload carriage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpaqueTag {
    pub tag_id: u16,
    pub data: Vec<u8>,
    /// Parse-time padding bytes (`align4(len) - len`, 0..=3; content is
    /// unspecified in real files so it must be carried, not assumed zero).
    /// Re-emitted verbatim while the payload length still matches; zeros
    /// otherwise.
    pub pad: Vec<u8>,
}

/// One tag. Every byte of the input is either typed-decoded or carried
/// opaquely — nothing dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tag {
    DefineSprite(DefineSprite),
    PlaceObject(PlaceObject),
    Shape(Shape),
    Opaque(OpaqueTag),
}

impl Tag {
    /// The wire tag id this variant serializes as.
    pub fn tag_id(&self) -> u16 {
        match self {
            Tag::DefineSprite(_) => TAG_DEFINE_SPRITE,
            Tag::PlaceObject(_) => TAG_PLACE_OBJECT,
            Tag::Shape(_) => TAG_SHAPE,
            Tag::Opaque(o) => o.tag_id,
        }
    }
}

// --- PlaceObject view / builder ------------------------------------------------

/// Read-only decode of the PlaceObject fields the feature consumes, plus the
/// unmodeled pre-matrix fields that must be walked to reach them. Field read
/// ORDER (not bit order!) transcribed from bemaniutils swf.py `__parse_tag`
/// PlaceObject branch (~line 1281): prefix `<IHH>` flags/depth/object_id;
/// flag 0x80000000 → u32 `more_flags` extending flags to 64 bits; then
/// 0x2 src_tag u16, 0x10 label u16, 0x20 movie_name offset u16, 0x40 unk3
/// u16, 0x20000 blend u8, realign-to-4, 0x100 scale i32 a/d, 0x200 rotate
/// i32 b/c, 0x400 translate i32 tx/ty. Everything after 0x400 (colors,
/// events, filters, 3D…) is left in the opaque payload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaceObjectView {
    pub flags: u64,
    pub depth: u16,
    pub object_id: u16,
    /// Flag 0x2 — source character id (sprite/shape id, NOT a tag index).
    pub source_tag_id: Option<u16>,
    /// Flag 0x10 — frame label id (decoded because it precedes movie_name).
    pub label_id: Option<u16>,
    /// Flag 0x20 — named instance, string-table-relative offset.
    pub movie_name_offset: Option<u16>,
    /// Flag 0x40 — unknown u16 (bemaniutils "unk3").
    pub unk3: Option<u16>,
    /// Flag 0x20000 — blend mode byte.
    pub blend: Option<u8>,
    /// Flag 0x100 — matrix a/d, stored s32, fixed-point /1024.
    pub scale: Option<(i32, i32)>,
    /// Flag 0x200 — matrix b/c, stored s32, fixed-point /1024.
    pub rotate: Option<(i32, i32)>,
    /// Flag 0x400 — matrix tx/ty, stored s32, fixed-point /20.
    pub translate: Option<(i32, i32)>,
}

/// Inputs for [`PlaceObject::build`] — the modeled flags only. Field values
/// are the raw fixed-point integers (scale/rotate /1024, translate /20).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaceObjectParams {
    pub depth: u16,
    pub object_id: u16,
    pub source_tag_id: Option<u16>,
    pub movie_name_offset: Option<u16>,
    pub scale: Option<(i32, i32)>,
    pub rotate: Option<(i32, i32)>,
    pub translate: Option<(i32, i32)>,
}

impl PlaceObject {
    /// Decode the modeled fields. `None` if the payload is too short for the
    /// fields its flag word claims. Bounds-checked throughout; never panics.
    pub fn view(&self) -> Option<PlaceObjectView> {
        let d = &self.data;
        let flags32 = read_u32(d, 0)?;
        let depth = read_u16(d, 4)?;
        let object_id = read_u16(d, 6)?;
        let mut p = 8usize;

        let mut flags = flags32 as u64;
        if flags32 & 0x8000_0000 != 0 {
            // Second flag word (bemaniutils ~1290) — read before any field.
            let more = read_u32(d, p)?;
            p += 4;
            flags |= (more as u64) << 32;
        }

        let mut view = PlaceObjectView {
            flags,
            depth,
            object_id,
            ..PlaceObjectView::default()
        };

        if flags & 0x2 != 0 {
            view.source_tag_id = Some(read_u16(d, p)?);
            p += 2;
        }
        if flags & 0x10 != 0 {
            view.label_id = Some(read_u16(d, p)?);
            p += 2;
        }
        if flags & 0x20 != 0 {
            view.movie_name_offset = Some(read_u16(d, p)?);
            p += 2;
        }
        if flags & 0x40 != 0 {
            view.unk3 = Some(read_u16(d, p)?);
            p += 2;
        }
        if flags & 0x20000 != 0 {
            view.blend = Some(*d.get(p)?);
            p += 1;
        }
        // Realign to 4 (bemaniutils ~1355: "Due to possible misalignment").
        p = align4(p);

        if flags & 0x100 != 0 {
            view.scale = Some((read_i32(d, p)?, read_i32(d, p + 4)?));
            p += 8;
        }
        if flags & 0x200 != 0 {
            view.rotate = Some((read_i32(d, p)?, read_i32(d, p + 4)?));
            p += 8;
        }
        if flags & 0x400 != 0 {
            view.translate = Some((read_i32(d, p)?, read_i32(d, p + 4)?));
            p += 8;
        }
        let _ = p; // Tail (colors/events/filters/…) stays opaque.
        Some(view)
    }
}

// Little bounds-checked readers shared by the module (parse.rs uses them too).
pub(super) fn read_u16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(at)?, *d.get(at + 1)?]))
}

pub(super) fn read_u32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *d.get(at)?,
        *d.get(at + 1)?,
        *d.get(at + 2)?,
        *d.get(at + 3)?,
    ]))
}

pub(super) fn read_i32(d: &[u8], at: usize) -> Option<i32> {
    read_u32(d, at).map(|v| v as i32)
}

// --- Sprite paths -----------------------------------------------------------

/// Path to a tag section: tag indices of nested `DefineSprite`s walked from
/// the root. Empty = the root section itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpritePath {
    pub tag_indices: Vec<usize>,
}

// --- Document -----------------------------------------------------------------

/// Whether the root tag section or the string table comes first in the file
/// body (both orders observed-legal; recorded at parse, reproduced at
/// serialize).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FileOrder {
    RootFirst,
    StringsFirst,
}

/// A parsed AP2 document. Public mutation surface: `root` (tags/frames/labels)
/// and `strings` (append-only interning). The private fields carry the raw
/// bytes and original boundaries the serializer needs for byte identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ap2Doc {
    pub root: TagSection,
    pub strings: StringTable,
    pub(super) exported_name: String,
    /// Raw bytes `[0, first_region_start)` — file header plus anything before
    /// the first known region. Recomputed fields are patched into a copy at
    /// serialize time; everything else is carried verbatim.
    pub(super) prefix: Vec<u8>,
    /// Raw bytes between the two known regions.
    pub(super) middle: Vec<u8>,
    /// Raw bytes after the second known region to EOF.
    pub(super) suffix: Vec<u8>,
    pub(super) order: FileOrder,
    /// Header flags u32 @12 (bit 0x4 = initializers pointer at @56 exists).
    pub(super) header_flags: u32,
    /// Original absolute boundaries of the two known regions, for the
    /// zone-delta pointer fixups (@40/@44/@56).
    pub(super) orig_first: (u32, u32),
    pub(super) orig_second: (u32, u32),
}

impl Ap2Doc {
    /// Build a fresh minimal document (canonical 56-byte header, empty root
    /// section, minimal string table). `None` if the exported name cannot be
    /// interned (pathologically long).
    pub fn new(exported_name: &str) -> Option<Ap2Doc> {
        let mut strings = StringTable::new_minimal();
        let name_offset = strings.intern(exported_name)?;

        // Canonical fresh header (56 bytes, flags 0 → float FPS @24, no
        // background color, no initializers field). Recomputed fields (@4,
        // @36, @48, @52) are placeholders — serialize() always rewrites them.
        let mut prefix = vec![0u8; FILE_HEADER_MIN];
        // Magic: container version 8, "AP2" with high bits set
        // (docs/afp_system.md §2: 08 B2 D0 C1 descrambled).
        prefix[0..4].copy_from_slice(&[0x08, 0xB2, 0xD0, 0xC1]);
        prefix[8..10].copy_from_slice(&0x200u16.to_le_bytes()); // data version
        prefix[10..12].copy_from_slice(&name_offset.to_le_bytes());
        prefix[24..28].copy_from_slice(&30.0f32.to_le_bytes()); // FPS (float)

        Some(Ap2Doc {
            root: TagSection::new(),
            strings,
            exported_name: exported_name.to_string(),
            prefix,
            middle: Vec::new(),
            suffix: Vec::new(),
            order: FileOrder::RootFirst,
            header_flags: 0,
            orig_first: (FILE_HEADER_MIN as u32, FILE_HEADER_MIN as u32),
            orig_second: (FILE_HEADER_MIN as u32, FILE_HEADER_MIN as u32),
        })
    }

    /// The movie's exported name (header name offset @10 resolved through the
    /// string table at parse time).
    pub fn exported_name(&self) -> &str {
        &self.exported_name
    }

    /// The section a path addresses (empty path = root).
    pub fn section(&self, path: &SpritePath) -> Option<&TagSection> {
        let mut sec = &self.root;
        for &idx in &path.tag_indices {
            match sec.tags.get(idx)? {
                Tag::DefineSprite(s) => sec = &s.section,
                _ => return None,
            }
        }
        Some(sec)
    }

    /// Mutable variant of [`Ap2Doc::section`].
    pub fn section_mut(&mut self, path: &SpritePath) -> Option<&mut TagSection> {
        let mut sec = &mut self.root;
        for &idx in &path.tag_indices {
            match sec.tags.get_mut(idx)? {
                Tag::DefineSprite(s) => sec = &mut s.section,
                _ => return None,
            }
        }
        Some(sec)
    }

    /// Depth-first search (root first, then tags in file order) for the
    /// section carrying `label` in its label map.
    pub fn find_sprite_by_label(&self, label: &str) -> Option<SpritePath> {
        fn walk(sec: &TagSection, label: &str, path: &mut Vec<usize>) -> bool {
            if sec.labels.iter().any(|l| l.name == label) {
                return true;
            }
            for (i, tag) in sec.tags.iter().enumerate() {
                if let Tag::DefineSprite(s) = tag {
                    path.push(i);
                    if walk(&s.section, label, path) {
                        return true;
                    }
                    path.pop();
                }
            }
            false
        }
        let mut path = Vec::new();
        if walk(&self.root, label, &mut path) {
            Some(SpritePath { tag_indices: path })
        } else {
            None
        }
    }

    /// Highest character id defined by any `DefineSprite`/`Shape` tag,
    /// root and nested (0 when none — mirrors the
    /// `core/afp.rs::patch_inject_children` id-allocation precedent).
    pub fn max_character_id(&self) -> u16 {
        fn walk(sec: &TagSection, max: &mut u16) {
            for tag in &sec.tags {
                match tag {
                    Tag::DefineSprite(s) => {
                        *max = (*max).max(s.id);
                        walk(&s.section, max);
                    }
                    Tag::Shape(s) => *max = (*max).max(s.id),
                    _ => {}
                }
            }
        }
        let mut max = 0u16;
        walk(&self.root, &mut max);
        max
    }
}
