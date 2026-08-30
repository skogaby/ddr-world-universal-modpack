//! Synthetic AP2 fixture builders — test / dev-validation infrastructure.
//!
//! Two layers, deliberately independent of each other:
//!
//! 1. **Raw byte-image assemblers** (`raw_*`, `plain_string_table`) — build
//!    AP2 images by hand, byte-for-byte, WITHOUT going through the serializer.
//!    These validate the parser against layouts the serializer would never
//!    produce (inter-region gaps, non-zero tag padding, string table placed
//!    before the tag section) and anchor the round-trip identity tests.
//! 2. **`FixtureBuilder`** — programmatic `Ap2Doc` construction through the
//!    public model API (N sprites, labels, opaque tags, PlaceObjects), used
//!    by the serializer round-trip matrix and available to later dev tooling
//!    (plan Step 2 task-02 requirement; lift the `#[cfg(test)]` gate on this
//!    module if task-03 needs it outside the harness).
//!
//! No Konami bytes anywhere — every image is synthetic.

use super::model::{
    Ap2Doc, FrameSpan, Label, OpaqueTag, PlaceObject, PlaceObjectParams, Shape, SpritePath, Tag,
    TagSection,
};
use super::{align4, model::DefineSprite};

/// Pack a frame word: low 20 bits start, next 12 bits count.
pub fn frame_word(start: u32, count: u32) -> [u8; 4] {
    ((count << 20) | (start & 0xFFFFF)).to_le_bytes()
}

/// Pack a tag header word: `(id << 22) | size`.
pub fn tag_header(id: u16, size: u32) -> [u8; 4] {
    (((id as u32) << 22) | (size & 0x3FFFFF)).to_le_bytes()
}

/// Assemble one raw tag: header + payload + explicit pad bytes. The pad must
/// bring the payload to 4-byte alignment (asserted — fixture bug otherwise).
pub fn raw_tag(id: u16, payload: &[u8], pad: &[u8]) -> Vec<u8> {
    assert_eq!(pad.len(), align4(payload.len()) - payload.len());
    let mut out = Vec::with_capacity(4 + payload.len() + pad.len());
    out.extend_from_slice(&tag_header(id, payload.len() as u32));
    out.extend_from_slice(payload);
    out.extend_from_slice(pad);
    out
}

/// Zero-padded convenience wrapper over [`raw_tag`].
pub fn raw_tag_zero_pad(id: u16, payload: &[u8]) -> Vec<u8> {
    raw_tag(
        id,
        payload,
        &vec![0u8; align4(payload.len()) - payload.len()],
    )
}

/// Spec for a hand-assembled tag-section image.
#[derive(Default)]
pub struct RawSectionSpec {
    pub name_reference_flags: u16,
    /// `(start, count)` frame words.
    pub frames: Vec<(u32, u32)>,
    /// Junk bytes inserted between the frame array and the tag array.
    pub gap_before_tags: Vec<u8>,
    /// Pre-assembled raw tags (see [`raw_tag`]).
    pub tags: Vec<Vec<u8>>,
    /// Junk bytes inserted between the tag array and the label array.
    pub gap_before_labels: Vec<u8>,
    /// `(frame, string_offset)` name-reference entries.
    pub labels: Vec<(u16, u16)>,
}

/// Assemble a section image: 24-byte header, then frames / tags / name-refs
/// in that order with the spec's gap bytes between them, header offsets
/// computed to match.
pub fn raw_section(spec: &RawSectionSpec) -> Vec<u8> {
    let mut body = Vec::new();
    let frame_offset = 24u32;
    for (start, count) in &spec.frames {
        body.extend_from_slice(&frame_word(*start, *count));
    }
    body.extend_from_slice(&spec.gap_before_tags);
    let tags_offset = 24 + body.len() as u32;
    for t in &spec.tags {
        body.extend_from_slice(t);
    }
    body.extend_from_slice(&spec.gap_before_labels);
    let name_ref_offset = 24 + body.len() as u32;
    for (frame, string_offset) in &spec.labels {
        body.extend_from_slice(&frame.to_le_bytes());
        body.extend_from_slice(&string_offset.to_le_bytes());
    }

    let mut out = Vec::with_capacity(24 + body.len());
    out.extend_from_slice(&spec.name_reference_flags.to_le_bytes());
    out.extend_from_slice(&(spec.labels.len() as u16).to_le_bytes());
    out.extend_from_slice(&(spec.frames.len() as u32).to_le_bytes());
    out.extend_from_slice(&(spec.tags.len() as u32).to_le_bytes());
    out.extend_from_slice(&name_ref_offset.to_le_bytes());
    out.extend_from_slice(&frame_offset.to_le_bytes());
    out.extend_from_slice(&tags_offset.to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// New-style DefineSprite payload: flags(bit0 set) + id + relative pointer
/// (`8 + pre.len()`) + pre-section slack + nested section image + trailing
/// bytes.
pub fn raw_sprite_payload_newstyle(id: u16, pre: &[u8], section: &[u8], post: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_le_bytes()); // sprite_flags: new-style
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&((8 + pre.len()) as u32).to_le_bytes());
    out.extend_from_slice(pre);
    out.extend_from_slice(section);
    out.extend_from_slice(post);
    out
}

/// Build a plaintext string table from a string list: leading 4 NUL bytes
/// (offset 0 = null string, first real string 4-aligned), each entry
/// NUL-terminated and padded to 4. Returns `(bytes, offsets)`.
pub fn plain_string_table(strings: &[&str]) -> (Vec<u8>, Vec<u16>) {
    let mut raw = vec![0u8; 4];
    let mut offsets = Vec::with_capacity(strings.len());
    for s in strings {
        offsets.push(raw.len() as u16);
        raw.extend_from_slice(s.as_bytes());
        raw.push(0);
        while raw.len() % 4 != 0 {
            raw.push(0);
        }
    }
    (raw, offsets)
}

/// Spec for a hand-assembled AP2 file image (56-byte header form).
pub struct RawFileSpec {
    /// Header name offset @10 (string-table-relative).
    pub exported_name_offset: u16,
    /// Header flags u32 @12 (keep 0x4 clear — the 60-byte header form is not
    /// assembled here).
    pub flags: u32,
    /// Plaintext string-table bytes.
    pub strings: Vec<u8>,
    /// Raw root-section image (see [`raw_section`]).
    pub section: Vec<u8>,
    /// Emit the string table before the tag section.
    pub strings_first: bool,
    /// Junk bytes between the two regions.
    pub gap_middle: Vec<u8>,
    /// Junk bytes after the second region to EOF.
    pub suffix: Vec<u8>,
    /// Header @40 (exported-assets offset) override; 0 otherwise.
    pub asset_offset: Option<u32>,
    /// Header @44 (imported-tags offset) override; 0 otherwise.
    pub imported_offset: Option<u32>,
}

impl Default for RawFileSpec {
    fn default() -> RawFileSpec {
        RawFileSpec {
            exported_name_offset: 0,
            flags: 0,
            strings: plain_string_table(&[]).0,
            section: raw_section(&RawSectionSpec::default()),
            strings_first: false,
            gap_middle: Vec::new(),
            suffix: Vec::new(),
            asset_offset: None,
            imported_offset: None,
        }
    }
}

/// Assemble a complete descrambled AP2 file image.
pub fn raw_file(spec: &RawFileSpec) -> Vec<u8> {
    assert_eq!(
        spec.flags & 0x4,
        0,
        "60-byte header form not supported here"
    );
    let mut out = vec![0u8; 56];
    // Magic: container version 8, "AP2" letters with high bits set
    // (docs/afp_system.md §2: 08 B2 D0 C1 descrambled).
    out[0..4].copy_from_slice(&[0x08, 0xB2, 0xD0, 0xC1]);
    out[8..10].copy_from_slice(&0x200u16.to_le_bytes());
    out[10..12].copy_from_slice(&spec.exported_name_offset.to_le_bytes());
    out[12..16].copy_from_slice(&spec.flags.to_le_bytes());
    out[24..28].copy_from_slice(&30.0f32.to_le_bytes());

    let (section_off, strings_off);
    if spec.strings_first {
        strings_off = out.len() as u32;
        out.extend_from_slice(&spec.strings);
        out.extend_from_slice(&spec.gap_middle);
        section_off = out.len() as u32;
        out.extend_from_slice(&spec.section);
    } else {
        section_off = out.len() as u32;
        out.extend_from_slice(&spec.section);
        out.extend_from_slice(&spec.gap_middle);
        strings_off = out.len() as u32;
        out.extend_from_slice(&spec.strings);
    }
    out.extend_from_slice(&spec.suffix);

    let total = out.len() as u32;
    out[4..8].copy_from_slice(&total.to_le_bytes());
    out[36..40].copy_from_slice(&section_off.to_le_bytes());
    out[40..44].copy_from_slice(&spec.asset_offset.unwrap_or(0).to_le_bytes());
    out[44..48].copy_from_slice(&spec.imported_offset.unwrap_or(0).to_le_bytes());
    out[48..52].copy_from_slice(&strings_off.to_le_bytes());
    out[52..56].copy_from_slice(&(spec.strings.len() as u32).to_le_bytes());
    out
}

// ---------------------------------------------------------------------------
// Model-level fixture builder.
// ---------------------------------------------------------------------------

/// Programmatic `Ap2Doc` construction through the public model API.
/// `path` arguments are tag-index chains into nested sprites (empty = root).
/// Panics on misuse (tests may unwrap) — this is test infrastructure, not a
/// hook-path API.
pub struct FixtureBuilder {
    doc: Ap2Doc,
}

impl FixtureBuilder {
    pub fn new(exported_name: &str) -> FixtureBuilder {
        FixtureBuilder {
            doc: Ap2Doc::new(exported_name).expect("fresh doc"),
        }
    }

    fn section_mut(&mut self, path: &[usize]) -> &mut TagSection {
        self.doc
            .section_mut(&SpritePath {
                tag_indices: path.to_vec(),
            })
            .expect("fixture path")
    }

    pub fn intern(&mut self, s: &str) -> u16 {
        self.doc.strings.intern(s).expect("string table space")
    }

    pub fn push_frame(&mut self, path: &[usize], start: u32, count: u32) -> &mut Self {
        self.section_mut(path).frames.push(FrameSpan {
            start_tag: start,
            tag_count: count,
        });
        self
    }

    pub fn push_opaque(&mut self, path: &[usize], tag_id: u16, data: &[u8]) -> &mut Self {
        self.section_mut(path).tags.push(Tag::Opaque(OpaqueTag {
            tag_id,
            data: data.to_vec(),
            pad: Vec::new(),
        }));
        self
    }

    pub fn push_shape(&mut self, path: &[usize], unknown: u16, id: u16) -> &mut Self {
        self.section_mut(path)
            .tags
            .push(Tag::Shape(Shape { unknown, id }));
        self
    }

    /// Append an empty new-style sprite; returns its tag index in the parent.
    pub fn push_sprite(&mut self, path: &[usize], id: u16) -> usize {
        let sec = self.section_mut(path);
        sec.tags.push(Tag::DefineSprite(DefineSprite {
            flags: 1,
            id,
            pre_section: Vec::new(),
            section: TagSection::new(),
            post_section: Vec::new(),
            pad: Vec::new(),
        }));
        sec.tags.len() - 1
    }

    /// Append a PlaceObject built from scratch; `movie_name` is interned.
    pub fn push_place(
        &mut self,
        path: &[usize],
        mut params: PlaceObjectParams,
        movie_name: Option<&str>,
    ) -> &mut Self {
        if let Some(name) = movie_name {
            params.movie_name_offset = Some(self.intern(name));
        }
        let po = PlaceObject::build(&params).expect("place object encode");
        self.section_mut(path).tags.push(Tag::PlaceObject(po));
        self
    }

    /// Add a frame label to a section, interning the name.
    pub fn add_label(&mut self, path: &[usize], name: &str, frame: u16) -> &mut Self {
        let name_offset = self.intern(name);
        self.section_mut(path).labels.push(Label {
            frame,
            name_offset,
            name: name.to_string(),
        });
        self
    }

    pub fn doc_mut(&mut self) -> &mut Ap2Doc {
        &mut self.doc
    }

    pub fn finish(self) -> Ap2Doc {
        self.doc
    }
}
