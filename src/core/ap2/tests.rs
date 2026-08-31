//! Host test suites for `core/ap2` (run via `scripts/validate_s_marvelous.sh`).
//!
//! Task-01 (parser): structural parse, opaque carriage, malformed-input
//! totality, accessors, cipher. Task-02 (serializer): byte-identity round
//! trips (builder fixtures AND hand-assembled layouts the serializer would
//! never produce), mutation stability, header pointer fixups, packed-field
//! limits, PlaceObject encode/decode. Step-3 task-01 (editing primitives,
//! `edit_*` tests): add_label / add_shape / clone_labeled_segment /
//! add_place_object_named / adjust_placements — clone correctness, failure
//! atomicity (doc byte-identical after any `None`), round-trips of every
//! edited doc. Step-4 task-01 (definition-aware cloning):
//! clone_sprite_definition / clone_labeled_segment_placements_only on a
//! fixture mirroring the real dance_judge template's
//! frame-0-carries-the-dictionary shape. Tests may unwrap; production paths
//! may not.

use super::fixtures::*;
use super::model::*;
use super::*;

// ---------------------------------------------------------------------------
// Shared full fixture: 2 root frames, 3 root tags (opaque, DefineSprite with
// nested label + own frame, opaque), 1 root label (task-01 AC1 shape).
// ---------------------------------------------------------------------------

struct FullFixture {
    bytes: Vec<u8>,
    name_off: u16,
    seg_off: u16,
    nested_off: u16,
}

fn full_fixture() -> FullFixture {
    let (strings, offs) = plain_string_table(&["my_movie", "seg_label", "nested_lab"]);
    let nested = raw_section(&RawSectionSpec {
        frames: vec![(0, 1)],
        tags: vec![raw_tag_zero_pad(0x50, &[1, 2, 3, 4])],
        labels: vec![(0, offs[2])],
        ..Default::default()
    });
    let sprite_payload = raw_sprite_payload_newstyle(5, &[], &nested, &[]);
    let section = raw_section(&RawSectionSpec {
        frames: vec![(0, 2), (2, 1)],
        tags: vec![
            raw_tag_zero_pad(0x10, &[0xAA; 4]),
            raw_tag_zero_pad(TAG_DEFINE_SPRITE, &sprite_payload),
            raw_tag_zero_pad(0x12, &[0xBB; 8]),
        ],
        labels: vec![(1, offs[1])],
        ..Default::default()
    });
    let bytes = raw_file(&RawFileSpec {
        exported_name_offset: offs[0],
        strings,
        section,
        ..Default::default()
    });
    FullFixture {
        bytes,
        name_off: offs[0],
        seg_off: offs[1],
        nested_off: offs[2],
    }
}

// ---------------------------------------------------------------------------
// Cipher (local copy — semantics must match core/afp.rs).
// ---------------------------------------------------------------------------

#[test]
fn cipher_round_trip() {
    let plain: Vec<u8> = (0..=255u8).cycle().take(1024).collect();
    let enc = encode_string_table(&plain);
    assert_ne!(enc, plain);
    assert_eq!(decode_string_table(&enc), plain);
}

#[test]
fn cipher_known_vector() {
    // "AB\0" with key starting at 128: 0x41+128=0xC1, 0x42+129=0xC3, 0+130=0x82
    // — same rolling cipher core/afp.rs::encode_stringtable implements.
    assert_eq!(encode_string_table(b"AB\0"), vec![0xC1, 0xC3, 0x82]);
    assert_eq!(decode_string_table(&[0xC1, 0xC3, 0x82]), b"AB\0".to_vec());
}

// ---------------------------------------------------------------------------
// Task-01: structural parse.
// ---------------------------------------------------------------------------

#[test]
fn structural_parse_full_fixture() {
    let fx = full_fixture();
    let doc = Ap2Doc::parse(&fx.bytes).expect("fixture parses");

    assert_eq!(doc.exported_name(), "my_movie");
    assert_eq!(
        doc.root.frames,
        vec![
            FrameSpan {
                start_tag: 0,
                tag_count: 2
            },
            FrameSpan {
                start_tag: 2,
                tag_count: 1
            },
        ]
    );
    assert_eq!(doc.root.tags.len(), 3);
    match &doc.root.tags[0] {
        Tag::Opaque(o) => {
            assert_eq!(o.tag_id, 0x10);
            assert_eq!(o.data, vec![0xAA; 4]);
        }
        other => panic!("tag 0 should be opaque, got {other:?}"),
    }
    match &doc.root.tags[2] {
        Tag::Opaque(o) => {
            assert_eq!(o.tag_id, 0x12);
            assert_eq!(o.data, vec![0xBB; 8]);
        }
        other => panic!("tag 2 should be opaque, got {other:?}"),
    }
    assert_eq!(doc.root.label_map(), vec![("seg_label", 1)]);
    assert_eq!(doc.root.labels[0].name_offset, fx.seg_off);

    match &doc.root.tags[1] {
        Tag::DefineSprite(s) => {
            assert_eq!(s.id, 5);
            assert_eq!(s.flags & 1, 1);
            assert!(s.pre_section.is_empty());
            assert!(s.post_section.is_empty());
            assert_eq!(
                s.section.frames,
                vec![FrameSpan {
                    start_tag: 0,
                    tag_count: 1
                }]
            );
            assert_eq!(s.section.label_map(), vec![("nested_lab", 0)]);
            assert_eq!(s.section.labels[0].name_offset, fx.nested_off);
            match &s.section.tags[0] {
                Tag::Opaque(o) => {
                    assert_eq!(o.tag_id, 0x50);
                    assert_eq!(o.data, vec![1, 2, 3, 4]);
                }
                other => panic!("nested tag should be opaque, got {other:?}"),
            }
        }
        other => panic!("tag 1 should be a sprite, got {other:?}"),
    }
    let _ = fx.name_off;
}

#[test]
fn opaque_carriage_odd_ids_and_padding() {
    let (strings, offs) = plain_string_table(&["m"]);
    let section = raw_section(&RawSectionSpec {
        frames: vec![(0, 2)],
        tags: vec![
            raw_tag(0x3FE, &[9, 9, 9], &[0xAA]), // non-zero pad byte
            raw_tag_zero_pad(0x155, &[]),        // empty payload
        ],
        ..Default::default()
    });
    let bytes = raw_file(&RawFileSpec {
        exported_name_offset: offs[0],
        strings,
        section,
        ..Default::default()
    });
    let doc = Ap2Doc::parse(&bytes).expect("parses");
    match &doc.root.tags[0] {
        Tag::Opaque(o) => {
            assert_eq!(o.tag_id, 0x3FE);
            assert_eq!(o.data, vec![9, 9, 9]);
            assert_eq!(o.pad, vec![0xAA]);
        }
        other => panic!("expected opaque, got {other:?}"),
    }
    match &doc.root.tags[1] {
        Tag::Opaque(o) => {
            assert_eq!(o.tag_id, 0x155);
            assert!(o.data.is_empty());
            assert!(o.pad.is_empty());
        }
        other => panic!("expected opaque, got {other:?}"),
    }
}

#[test]
fn shape_tag_typed_decode() {
    let (strings, offs) = plain_string_table(&["m"]);
    let mut shape_payload = Vec::new();
    shape_payload.extend_from_slice(&7u16.to_le_bytes()); // unknown
    shape_payload.extend_from_slice(&9u16.to_le_bytes()); // shape id
    let section = raw_section(&RawSectionSpec {
        frames: vec![(0, 1)],
        tags: vec![raw_tag_zero_pad(TAG_SHAPE, &shape_payload)],
        ..Default::default()
    });
    let bytes = raw_file(&RawFileSpec {
        exported_name_offset: offs[0],
        strings,
        section,
        ..Default::default()
    });
    let doc = Ap2Doc::parse(&bytes).expect("parses");
    assert_eq!(doc.root.tags[0], Tag::Shape(Shape { unknown: 7, id: 9 }));
    assert_eq!(doc.max_character_id(), 9);
}

#[test]
fn accessors_find_sprite_and_max_char_id() {
    let fx = full_fixture();
    let doc = Ap2Doc::parse(&fx.bytes).unwrap();

    // Root label → empty path.
    assert_eq!(
        doc.find_sprite_by_label("seg_label"),
        Some(SpritePath {
            tag_indices: vec![]
        })
    );
    // Nested label → path through tag index 1.
    let nested = doc.find_sprite_by_label("nested_lab").expect("found");
    assert_eq!(nested.tag_indices, vec![1]);
    assert_eq!(
        doc.section(&nested).unwrap().label_frame("nested_lab"),
        Some(0)
    );
    assert_eq!(doc.find_sprite_by_label("absent"), None);
    assert_eq!(doc.max_character_id(), 5);
}

#[test]
fn exported_name_offset_zero_is_null_string() {
    let bytes = raw_file(&RawFileSpec::default());
    let doc = Ap2Doc::parse(&bytes).expect("parses");
    assert_eq!(doc.exported_name(), "");
}

// ---------------------------------------------------------------------------
// Task-01: malformed-input totality (all None, no panics).
// ---------------------------------------------------------------------------

#[test]
fn malformed_truncated_header() {
    let fx = full_fixture();
    for cut in [0usize, 4, 23, 40, 55] {
        assert!(Ap2Doc::parse(&fx.bytes[..cut]).is_none(), "cut {cut}");
    }
}

#[test]
fn malformed_bad_magic_and_version() {
    let fx = full_fixture();
    let mut b = fx.bytes.clone();
    b[1] = 0x00; // magic letter '2' destroyed
    assert!(Ap2Doc::parse(&b).is_none());

    let mut b = fx.bytes.clone();
    b[8..10].copy_from_slice(&0x100u16.to_le_bytes()); // old bit-packed format
    assert!(Ap2Doc::parse(&b).is_none());
}

#[test]
fn malformed_length_mismatch() {
    let fx = full_fixture();
    let total = read_len(&fx.bytes);
    for delta in [-1i64, 1] {
        let mut b = fx.bytes.clone();
        b[4..8].copy_from_slice(&(((total as i64) + delta) as u32).to_le_bytes());
        assert!(Ap2Doc::parse(&b).is_none(), "delta {delta}");
    }
}

fn read_len(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[4], b[5], b[6], b[7]])
}

#[test]
fn malformed_string_table_bounds_and_alignment() {
    let fx = full_fixture();

    // Offset out of range.
    let mut b = fx.bytes.clone();
    let len = b.len() as u32;
    b[48..52].copy_from_slice(&len.to_le_bytes());
    assert!(Ap2Doc::parse(&b).is_none());

    // Size overrunning EOF.
    let mut b = fx.bytes.clone();
    b[52..56].copy_from_slice(&len.to_le_bytes());
    assert!(Ap2Doc::parse(&b).is_none());

    // Misaligned size (game-fatal layout — rejected symmetrically).
    let mut b = fx.bytes.clone();
    let st_size = u32::from_le_bytes([b[52], b[53], b[54], b[55]]);
    b[52..56].copy_from_slice(&(st_size - 1).to_le_bytes());
    assert!(Ap2Doc::parse(&b).is_none());
}

#[test]
fn malformed_string_table_over_64k() {
    let (mut strings, offs) = plain_string_table(&["m"]);
    strings.resize(0x10004, 0); // 4-aligned but past the u16-offset horizon
    let bytes = raw_file(&RawFileSpec {
        exported_name_offset: offs[0],
        strings,
        ..Default::default()
    });
    assert!(Ap2Doc::parse(&bytes).is_none());
}

#[test]
fn malformed_section_pointer_and_header() {
    let fx = full_fixture();

    // Tag-section pointer out of range.
    let mut b = fx.bytes.clone();
    let len = b.len() as u32;
    b[36..40].copy_from_slice(&(len - 2).to_le_bytes());
    assert!(Ap2Doc::parse(&b).is_none());

    // Pointer inside the fixed header.
    let mut b = fx.bytes.clone();
    b[36..40].copy_from_slice(&8u32.to_le_bytes());
    assert!(Ap2Doc::parse(&b).is_none());
}

#[test]
fn malformed_tag_size_overrun() {
    let fx = full_fixture();
    let mut b = fx.bytes.clone();
    let sec = section_offset(&b);
    // Root section: frames = 2 words → tags array at +32; rewrite the first
    // tag header with a size far past EOF (still within the 22-bit field).
    let th = sec + 32;
    b[th..th + 4].copy_from_slice(&tag_header(0x10, 0x3FFF));
    assert!(Ap2Doc::parse(&b).is_none());
}

fn section_offset(b: &[u8]) -> usize {
    u32::from_le_bytes([b[36], b[37], b[38], b[39]]) as usize
}

#[test]
fn malformed_frame_and_label_arrays_out_of_range() {
    let fx = full_fixture();

    // frame_offset (section header +16) → far out of range.
    let mut b = fx.bytes.clone();
    let sec = section_offset(&b);
    b[sec + 16..sec + 20].copy_from_slice(&0xFFFFu32.to_le_bytes());
    assert!(Ap2Doc::parse(&b).is_none());

    // name_reference_offset (section header +12) → far out of range.
    let mut b = fx.bytes.clone();
    b[sec + 12..sec + 16].copy_from_slice(&0xFFFFu32.to_le_bytes());
    assert!(Ap2Doc::parse(&b).is_none());
}

#[test]
fn malformed_label_name_offset_past_table() {
    let fx = full_fixture();
    let mut b = fx.bytes.clone();
    let sec = section_offset(&b);
    let nr_off = u32::from_le_bytes([b[sec + 12], b[sec + 13], b[sec + 14], b[sec + 15]]) as usize;
    // Label entry = <HH> frame, string_offset — poison the string offset.
    let so = sec + nr_off + 2;
    b[so..so + 2].copy_from_slice(&0xFFF0u16.to_le_bytes());
    assert!(Ap2Doc::parse(&b).is_none());
}

#[test]
fn malformed_sprite_subtag_pointer() {
    let fx = full_fixture();
    let sec = section_offset(&fx.bytes);
    // Root tags at +32: tag0 spans 8 bytes (4 hdr + 4 payload); the sprite
    // tag header follows, its payload at +4, relative pointer at payload +4.
    let ptr = sec + 32 + 8 + 4 + 4;

    for bad in [2u32, 0xFFFF] {
        let mut b = fx.bytes.clone();
        b[ptr..ptr + 4].copy_from_slice(&bad.to_le_bytes());
        assert!(Ap2Doc::parse(&b).is_none(), "pointer {bad}");
    }
}

#[test]
fn malformed_overlapping_regions() {
    // Hand-rolled section: 1 frame at +24 overlapping a 1-entry label array
    // at +26.
    let mut section = Vec::new();
    section.extend_from_slice(&0u16.to_le_bytes()); // nr flags
    section.extend_from_slice(&1u16.to_le_bytes()); // nr count
    section.extend_from_slice(&1u32.to_le_bytes()); // frame count
    section.extend_from_slice(&0u32.to_le_bytes()); // tag count
    section.extend_from_slice(&26u32.to_le_bytes()); // nr offset (overlaps frames)
    section.extend_from_slice(&24u32.to_le_bytes()); // frame offset
    section.extend_from_slice(&32u32.to_le_bytes()); // tags offset (empty)
    section.extend_from_slice(&frame_word(0, 0));
    section.extend_from_slice(&[0, 0, 0, 0]); // label entry space
    let (strings, offs) = plain_string_table(&["m"]);
    let bytes = raw_file(&RawFileSpec {
        exported_name_offset: offs[0],
        strings,
        section,
        ..Default::default()
    });
    assert!(Ap2Doc::parse(&bytes).is_none());
}

#[test]
fn malformed_truncated_final_tag_padding() {
    // Strings-first file whose section is the LAST region and whose final tag
    // (size 2) is missing its 2 padding bytes.
    let mut section = Vec::new();
    section.extend_from_slice(&0u16.to_le_bytes()); // nr flags
    section.extend_from_slice(&0u16.to_le_bytes()); // nr count
    section.extend_from_slice(&0u32.to_le_bytes()); // frame count
    section.extend_from_slice(&1u32.to_le_bytes()); // tag count
    section.extend_from_slice(&24u32.to_le_bytes()); // nr offset (empty)
    section.extend_from_slice(&24u32.to_le_bytes()); // frame offset (empty)
    section.extend_from_slice(&24u32.to_le_bytes()); // tags offset
    section.extend_from_slice(&tag_header(0x10, 2));
    section.extend_from_slice(&[1, 2]); // payload only — pad truncated by EOF
    let (strings, offs) = plain_string_table(&["m"]);
    let bytes = raw_file(&RawFileSpec {
        exported_name_offset: offs[0],
        strings,
        section,
        strings_first: true,
        ..Default::default()
    });
    assert!(Ap2Doc::parse(&bytes).is_none());
}

// ---------------------------------------------------------------------------
// Task-01: PlaceObject view decode (read order transcribed from bemaniutils
// swf.py ~1281).
// ---------------------------------------------------------------------------

fn build_view_payload() -> Vec<u8> {
    let flags: u32 = 0x2 | 0x10 | 0x20 | 0x40 | 0x20000 | 0x100 | 0x400;
    let mut d = Vec::new();
    d.extend_from_slice(&flags.to_le_bytes());
    d.extend_from_slice(&3u16.to_le_bytes()); // depth
    d.extend_from_slice(&77u16.to_le_bytes()); // object id
    d.extend_from_slice(&0x0505u16.to_le_bytes()); // 0x2 source tag
    d.extend_from_slice(&0x0606u16.to_le_bytes()); // 0x10 label
    d.extend_from_slice(&0x0707u16.to_le_bytes()); // 0x20 movie name
    d.extend_from_slice(&0x0808u16.to_le_bytes()); // 0x40 unk3
    d.push(0x09); // 0x20000 blend
    d.extend_from_slice(&[0xEE, 0xEE, 0xEE]); // realign catchup (content-free)
    d.extend_from_slice(&2048i32.to_le_bytes()); // 0x100 scale a
    d.extend_from_slice(&(-1024i32).to_le_bytes()); // 0x100 scale d
    d.extend_from_slice(&(-40i32).to_le_bytes()); // 0x400 tx
    d.extend_from_slice(&100i32.to_le_bytes()); // 0x400 ty
    d.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // opaque tail
    d
}

#[test]
fn place_object_view_decodes_interleaved_fields() {
    let po = PlaceObject {
        data: build_view_payload(),
        pad: Vec::new(),
    };
    let v = po.view().expect("decodes");
    assert_eq!(
        v.flags,
        (0x2 | 0x10 | 0x20 | 0x40 | 0x20000 | 0x100 | 0x400) as u64
    );
    assert_eq!(v.depth, 3);
    assert_eq!(v.object_id, 77);
    assert_eq!(v.source_tag_id, Some(0x0505));
    assert_eq!(v.label_id, Some(0x0606));
    assert_eq!(v.movie_name_offset, Some(0x0707));
    assert_eq!(v.unk3, Some(0x0808));
    assert_eq!(v.blend, Some(9));
    assert_eq!(v.scale, Some((2048, -1024)));
    assert_eq!(v.rotate, None);
    assert_eq!(v.translate, Some((-40, 100)));
}

#[test]
fn place_object_view_second_flag_word() {
    let mut d = Vec::new();
    d.extend_from_slice(&0x8000_0002u32.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&2u16.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes()); // more_flags
    d.extend_from_slice(&42u16.to_le_bytes()); // 0x2 source tag
    let v = PlaceObject {
        data: d,
        pad: Vec::new(),
    }
    .view()
    .expect("decodes");
    assert_eq!(v.flags, 0x1_8000_0002);
    assert_eq!(v.source_tag_id, Some(42));
}

#[test]
fn place_object_view_short_payload_rejected() {
    // Flags claim 0x2 but the field is missing.
    let mut d = Vec::new();
    d.extend_from_slice(&0x2u32.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&2u16.to_le_bytes());
    assert!(PlaceObject {
        data: d,
        pad: Vec::new()
    }
    .view()
    .is_none());
    assert!(PlaceObject {
        data: vec![0; 7],
        pad: Vec::new()
    }
    .view()
    .is_none());
}

// ---------------------------------------------------------------------------
// Task-02: byte-identity round trips.
// ---------------------------------------------------------------------------

fn assert_bytes_roundtrip(bytes: &[u8], what: &str) {
    let doc = Ap2Doc::parse(bytes).unwrap_or_else(|| panic!("{what}: parses"));
    let out = doc
        .serialize()
        .unwrap_or_else(|| panic!("{what}: serializes"));
    assert_eq!(out, bytes, "{what}: byte identity");
}

/// Compare two sections' SEMANTICS (frames, labels, tag content), ignoring
/// the parse-time layout carriage (`SectionLayout.orig_offset`, captured
/// zero pads) that built models legitimately lack.
fn assert_section_semantics_eq(a: &TagSection, b: &TagSection, what: &str) {
    assert_eq!(a.frames, b.frames, "{what}: frames");
    assert_eq!(a.labels, b.labels, "{what}: labels");
    assert_eq!(a.tags.len(), b.tags.len(), "{what}: tag count");
    for (i, (ta, tb)) in a.tags.iter().zip(&b.tags).enumerate() {
        match (ta, tb) {
            (Tag::DefineSprite(sa), Tag::DefineSprite(sb)) => {
                assert_eq!((sa.flags, sa.id), (sb.flags, sb.id), "{what}: sprite {i}");
                assert_eq!(sa.pre_section, sb.pre_section, "{what}: sprite {i} pre");
                assert_eq!(sa.post_section, sb.post_section, "{what}: sprite {i} post");
                assert_section_semantics_eq(&sa.section, &sb.section, what);
            }
            (Tag::Opaque(oa), Tag::Opaque(ob)) => {
                assert_eq!(
                    (oa.tag_id, &oa.data),
                    (ob.tag_id, &ob.data),
                    "{what}: opaque {i}"
                );
            }
            (Tag::PlaceObject(pa), Tag::PlaceObject(pb)) => {
                assert_eq!(pa.data, pb.data, "{what}: place {i}");
            }
            (Tag::Shape(sa), Tag::Shape(sb)) => assert_eq!(sa, sb, "{what}: shape {i}"),
            (ta, tb) => panic!("{what}: tag {i} kind mismatch: {ta:?} vs {tb:?}"),
        }
    }
}

fn builder_fixture_matrix() -> Vec<(String, Ap2Doc)> {
    let mut out = Vec::new();

    out.push(("minimal".to_string(), FixtureBuilder::new("mini").finish()));

    let mut b = FixtureBuilder::new("labels");
    b.push_frame(&[], 0, 0);
    b.push_frame(&[], 0, 0);
    b.add_label(&[], "intro", 0);
    b.add_label(&[], "loop_start", 1);
    out.push(("root labels".to_string(), b.finish()));

    let mut b = FixtureBuilder::new("sprites");
    b.push_opaque(&[], 0x11, &[1, 2, 3]);
    let s0 = b.push_sprite(&[], 10);
    b.push_frame(&[s0], 0, 1);
    b.push_opaque(&[s0], 0x50, &[5; 6]);
    b.add_label(&[s0], "seg_a", 0);
    let s1 = b.push_sprite(&[], 11);
    b.push_frame(&[s1], 0, 0);
    b.add_label(&[s1], "seg_b", 0);
    // Nested-in-nested.
    let s1_0 = b.push_sprite(&[s1], 12);
    b.push_opaque(&[s1, s1_0], 0x51, &[7]);
    b.push_frame(&[], 0, 3);
    out.push(("nested sprites".to_string(), b.finish()));

    let mut b = FixtureBuilder::new("pads");
    for n in 0..9usize {
        b.push_opaque(&[], 0x20 + n as u16, &vec![n as u8; n]);
    }
    b.push_frame(&[], 0, 9);
    out.push(("pad coverage".to_string(), b.finish()));

    let mut b = FixtureBuilder::new("places");
    b.push_shape(&[], 0, 1);
    let flag_sets: Vec<PlaceObjectParams> = vec![
        PlaceObjectParams {
            depth: 1,
            object_id: 300,
            source_tag_id: Some(1),
            ..Default::default()
        },
        PlaceObjectParams {
            depth: 2,
            object_id: 301,
            ..Default::default()
        },
        PlaceObjectParams {
            depth: 3,
            object_id: 302,
            scale: Some((512, 2048)),
            ..Default::default()
        },
        PlaceObjectParams {
            depth: 4,
            object_id: 303,
            rotate: Some((-3, 3)),
            ..Default::default()
        },
        PlaceObjectParams {
            depth: 5,
            object_id: 304,
            translate: Some((-800, 640)),
            ..Default::default()
        },
        PlaceObjectParams {
            depth: 6,
            object_id: 305,
            source_tag_id: Some(1),
            scale: Some((1024, 1024)),
            rotate: Some((0, 0)),
            translate: Some((20, -20)),
            ..Default::default()
        },
    ];
    let n_places = flag_sets.len();
    for (i, params) in flag_sets.into_iter().enumerate() {
        let name = if i % 2 == 0 {
            Some(format!("inst_{i}"))
        } else {
            None
        };
        b.push_place(&[], params, name.as_deref());
    }
    b.push_frame(&[], 0, (1 + n_places) as u32);
    out.push(("place objects".to_string(), b.finish()));

    let mut b = FixtureBuilder::new("frames");
    b.push_opaque(&[], 0x30, &[0; 4]);
    b.push_opaque(&[], 0x31, &[1; 4]);
    b.push_frame(&[], 0, 1);
    b.push_frame(&[], 1, 1);
    b.push_frame(&[], 2, 0);
    b.push_frame(&[], 0, 2);
    out.push(("multi frame spans".to_string(), b.finish()));

    out
}

#[test]
fn roundtrip_builder_fixture_matrix() {
    for (what, doc) in builder_fixture_matrix() {
        let bytes = doc
            .serialize()
            .unwrap_or_else(|| panic!("{what}: serializes"));
        assert_bytes_roundtrip(&bytes, &what);
        // And structural: the parsed doc matches the built model where it
        // must (root semantics + strings + name; the parsed side additionally
        // carries layout metadata built models lack).
        let parsed = Ap2Doc::parse(&bytes).unwrap();
        assert_eq!(parsed.exported_name(), doc.exported_name(), "{what}");
        assert_eq!(parsed.strings, doc.strings, "{what}");
        assert_section_semantics_eq(&parsed.root, &doc.root, &what);
    }
}

#[test]
fn roundtrip_hand_assembled_full_fixture() {
    let fx = full_fixture();
    assert_bytes_roundtrip(&fx.bytes, "full fixture");
}

#[test]
fn roundtrip_gaps_and_nonzero_padding() {
    let (strings, offs) = plain_string_table(&["gappy", "lab"]);
    let section = raw_section(&RawSectionSpec {
        frames: vec![(0, 2)],
        gap_before_tags: vec![0xEE; 4],
        tags: vec![
            raw_tag(0x33, &[9, 9, 9], &[0xAA]),
            raw_tag_zero_pad(0x34, &[8; 4]),
        ],
        gap_before_labels: vec![0xDD; 8],
        labels: vec![(0, offs[1])],
        ..Default::default()
    });
    let bytes = raw_file(&RawFileSpec {
        exported_name_offset: offs[0],
        strings,
        section,
        gap_middle: vec![0xCC; 4],
        suffix: vec![0x77; 12],
        ..Default::default()
    });
    assert_bytes_roundtrip(&bytes, "gaps and padding");
}

#[test]
fn roundtrip_strings_first_order() {
    let (strings, offs) = plain_string_table(&["backwards"]);
    let section = raw_section(&RawSectionSpec {
        frames: vec![(0, 1)],
        tags: vec![raw_tag_zero_pad(0x40, &[1, 2])],
        ..Default::default()
    });
    let bytes = raw_file(&RawFileSpec {
        exported_name_offset: offs[0],
        strings,
        section,
        strings_first: true,
        gap_middle: vec![0xBB; 4],
        ..Default::default()
    });
    assert_bytes_roundtrip(&bytes, "strings-first order");
}

#[test]
fn roundtrip_sprite_with_pre_and_post_section_bytes() {
    let (strings, offs) = plain_string_table(&["slack"]);
    let nested = raw_section(&RawSectionSpec {
        frames: vec![(0, 0)],
        ..Default::default()
    });
    let sprite_payload = raw_sprite_payload_newstyle(
        3,
        &[0xF1, 0xF2, 0xF3, 0xF4],
        &nested,
        &[0xE1, 0xE2, 0xE3, 0xE4],
    );
    let section = raw_section(&RawSectionSpec {
        frames: vec![(0, 1)],
        tags: vec![raw_tag_zero_pad(TAG_DEFINE_SPRITE, &sprite_payload)],
        ..Default::default()
    });
    let bytes = raw_file(&RawFileSpec {
        exported_name_offset: offs[0],
        strings,
        section,
        ..Default::default()
    });
    let doc = Ap2Doc::parse(&bytes).expect("parses");
    match &doc.root.tags[0] {
        Tag::DefineSprite(s) => {
            assert_eq!(s.pre_section, vec![0xF1, 0xF2, 0xF3, 0xF4]);
            assert_eq!(s.post_section, vec![0xE1, 0xE2, 0xE3, 0xE4]);
        }
        other => panic!("expected sprite, got {other:?}"),
    }
    assert_bytes_roundtrip(&bytes, "sprite slack bytes");
}

// ---------------------------------------------------------------------------
// Task-02: mutation stability.
// ---------------------------------------------------------------------------

#[test]
fn mutation_stability() {
    let fx = full_fixture();
    let mut doc = Ap2Doc::parse(&fx.bytes).unwrap();

    // Mutate: new string, appended opaque tag, new root label, new sprite.
    let new_off = doc.strings.intern("in_smarvelous").expect("intern");
    doc.root.tags.push(Tag::Opaque(OpaqueTag {
        tag_id: 0x21,
        data: vec![0xCD; 6],
        pad: Vec::new(),
    }));
    doc.root.labels.push(Label {
        frame: 1,
        name_offset: new_off,
        name: "in_smarvelous".to_string(),
    });
    doc.root.tags.push(Tag::DefineSprite(DefineSprite {
        flags: 1,
        id: 6,
        pre_section: Vec::new(),
        section: TagSection::new(),
        post_section: Vec::new(),
        pad: Vec::new(),
    }));

    let out1 = doc.serialize().expect("mutated doc serializes");
    assert_ne!(out1, fx.bytes);

    let reparsed = Ap2Doc::parse(&out1).expect("mutated output parses");
    assert_eq!(reparsed.root.tags.len(), 5);
    assert_eq!(reparsed.root.label_frame("in_smarvelous"), Some(1));
    assert_eq!(reparsed.strings.get(new_off), Some("in_smarvelous"));
    assert_eq!(reparsed.max_character_id(), 6);
    // Original content untouched.
    assert_eq!(reparsed.root.label_frame("seg_label"), Some(1));
    match &reparsed.root.tags[3] {
        Tag::Opaque(o) => assert_eq!((o.tag_id, o.data.as_slice()), (0x21, &[0xCD; 6][..])),
        other => panic!("expected appended opaque, got {other:?}"),
    }

    // Fixed point after one emission.
    let out2 = reparsed.serialize().expect("re-serializes");
    assert_eq!(out2, out1);
}

#[test]
fn header_pointer_fixup_shifts_with_growth() {
    // A file with an opaque suffix region referenced from header @40.
    let (strings, offs) = plain_string_table(&["fixup"]);
    let section = raw_section(&RawSectionSpec::default());
    let mut spec = RawFileSpec {
        exported_name_offset: offs[0],
        strings,
        section,
        suffix: vec![0x77; 8],
        ..Default::default()
    };
    let probe = raw_file(&spec);
    let suffix_start = (probe.len() - 8) as u32;
    spec.asset_offset = Some(suffix_start);
    let bytes = raw_file(&spec);

    // Unmodified: byte identity (pointer untouched).
    assert_bytes_roundtrip(&bytes, "fixup fixture");

    // Grown root section shifts the suffix; @40 must follow exactly.
    let mut doc = Ap2Doc::parse(&bytes).unwrap();
    doc.root.tags.push(Tag::Opaque(OpaqueTag {
        tag_id: 0x11,
        data: vec![0; 8],
        pad: Vec::new(),
    }));
    let out = doc.serialize().expect("serializes");
    let growth = (out.len() - bytes.len()) as u32;
    assert_eq!(growth, 12); // 4-byte header + 8 payload
    let new_ptr = u32::from_le_bytes([out[40], out[41], out[42], out[43]]);
    assert_eq!(new_ptr, suffix_start + growth);
    // The suffix bytes themselves moved intact.
    assert_eq!(&out[new_ptr as usize..new_ptr as usize + 8], &[0x77; 8]);
}

// ---------------------------------------------------------------------------
// Task-02: limits fail closed.
// ---------------------------------------------------------------------------

#[test]
fn limit_frame_start_over_20_bits() {
    let mut doc = FixtureBuilder::new("lim").finish();
    doc.root.frames.push(FrameSpan {
        start_tag: FRAME_START_MAX + 1,
        tag_count: 0,
    });
    assert!(doc.serialize().is_none());
    doc.root.frames[0].start_tag = FRAME_START_MAX;
    assert!(doc.serialize().is_some());
}

#[test]
fn limit_frame_count_over_12_bits() {
    let mut doc = FixtureBuilder::new("lim").finish();
    doc.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: FRAME_COUNT_MAX + 1,
    });
    assert!(doc.serialize().is_none());
    doc.root.frames[0].tag_count = FRAME_COUNT_MAX;
    assert!(doc.serialize().is_some());
}

#[test]
fn limit_tag_id_over_10_bits() {
    let mut doc = FixtureBuilder::new("lim").finish();
    doc.root.tags.push(Tag::Opaque(OpaqueTag {
        tag_id: TAG_ID_MAX + 1,
        data: Vec::new(),
        pad: Vec::new(),
    }));
    assert!(doc.serialize().is_none());
}

#[test]
fn limit_tag_size_over_22_bits() {
    let mut doc = FixtureBuilder::new("lim").finish();
    doc.root.tags.push(Tag::Opaque(OpaqueTag {
        tag_id: 0x11,
        data: vec![0; TAG_SIZE_MAX as usize + 1],
        pad: Vec::new(),
    }));
    assert!(doc.serialize().is_none());
}

#[test]
fn limit_label_count_over_u16() {
    let mut doc = FixtureBuilder::new("lim").finish();
    let off = doc.strings.intern("x").unwrap();
    for _ in 0..0x10000usize {
        doc.root.labels.push(Label {
            frame: 0,
            name_offset: off,
            name: "x".to_string(),
        });
    }
    assert!(doc.serialize().is_none());
}

#[test]
fn limit_string_table_intern_past_64k() {
    let mut table = StringTable::new_minimal();
    let mut hit_limit = false;
    for i in 0..2000usize {
        let s = format!("padding_string_{i:04}_{}", "y".repeat(48));
        if table.intern(&s).is_none() {
            hit_limit = true;
            break;
        }
    }
    assert!(hit_limit, "intern must refuse past 64 KiB");
    assert!(table.len() <= STRING_TABLE_MAX);
    assert_eq!(table.len() % 4, 0);
}

#[test]
fn string_table_from_plain_bytes_limits() {
    assert!(StringTable::from_plain_bytes(&vec![0; 0x10004]).is_none()); // > 64 KiB
    assert!(StringTable::from_plain_bytes(&[0; 6]).is_none()); // misaligned
    assert!(StringTable::from_plain_bytes(&[0; 8]).is_some());
}

// ---------------------------------------------------------------------------
// Task-02: PlaceObject encode/decode round trip.
// ---------------------------------------------------------------------------

#[test]
fn place_object_build_view_roundtrip_matrix() {
    let cases: Vec<PlaceObjectParams> = vec![
        PlaceObjectParams {
            depth: 0,
            object_id: 0,
            ..Default::default()
        },
        PlaceObjectParams {
            depth: 9,
            object_id: 300,
            source_tag_id: Some(7),
            ..Default::default()
        },
        PlaceObjectParams {
            depth: 1,
            object_id: 1,
            movie_name_offset: Some(0x44),
            ..Default::default()
        },
        // Sign edges: negative translate, extreme scales.
        PlaceObjectParams {
            depth: 2,
            object_id: 2,
            translate: Some((i32::MIN, i32::MAX)),
            ..Default::default()
        },
        PlaceObjectParams {
            depth: 3,
            object_id: 3,
            scale: Some((i32::MIN, i32::MAX)),
            rotate: Some((-1, 1)),
            ..Default::default()
        },
        PlaceObjectParams {
            depth: u16::MAX,
            object_id: u16::MAX,
            source_tag_id: Some(u16::MAX),
            movie_name_offset: Some(u16::MAX),
            scale: Some((-2048, 2048)),
            rotate: Some((512, -512)),
            translate: Some((-20, 20)),
        },
    ];
    for params in cases {
        let po = PlaceObject::build(&params).expect("encodes");
        assert_eq!(po.data.len() % 4, 0, "encoder keeps payload 4-aligned");
        let v = po.view().expect("decodes");
        assert_eq!(v.depth, params.depth);
        assert_eq!(v.object_id, params.object_id);
        assert_eq!(v.source_tag_id, params.source_tag_id);
        assert_eq!(v.movie_name_offset, params.movie_name_offset);
        assert_eq!(v.scale, params.scale);
        assert_eq!(v.rotate, params.rotate);
        assert_eq!(v.translate, params.translate);
        assert_eq!(v.label_id, None);
        assert_eq!(v.unk3, None);
        assert_eq!(v.blend, None);
        // Flag word carries exactly the modeled bits.
        let mut expect_flags = 0u64;
        if params.source_tag_id.is_some() {
            expect_flags |= 0x2;
        }
        if params.movie_name_offset.is_some() {
            expect_flags |= 0x20;
        }
        if params.scale.is_some() {
            expect_flags |= 0x100;
        }
        if params.rotate.is_some() {
            expect_flags |= 0x200;
        }
        if params.translate.is_some() {
            expect_flags |= 0x400;
        }
        assert_eq!(v.flags, expect_flags);
    }
}

#[test]
fn place_object_minimal_build_matches_afp_rs_shape() {
    // The 0x22-flag shape core/afp.rs::make_place_object emits: 12 bytes,
    // src tag + name offset back to back, no realign padding needed.
    let po = PlaceObject::build(&PlaceObjectParams {
        depth: 4,
        object_id: 300,
        source_tag_id: Some(9),
        movie_name_offset: Some(0x10),
        ..Default::default()
    })
    .expect("encodes");
    let mut expect = Vec::new();
    expect.extend_from_slice(&0x22u32.to_le_bytes());
    expect.extend_from_slice(&4u16.to_le_bytes());
    expect.extend_from_slice(&300u16.to_le_bytes());
    expect.extend_from_slice(&9u16.to_le_bytes());
    expect.extend_from_slice(&0x10u16.to_le_bytes());
    assert_eq!(po.data, expect);
}

// ---------------------------------------------------------------------------
// Step-3 task-01: editing primitives (edit.rs).
// ---------------------------------------------------------------------------

/// Editing fixture ("in_marvelous"-shaped): root = [Shape id1 (the word art),
/// Shape id2, DefineSprite id10] + one root frame (0,3); the sprite section
/// carries labels in_marvelous→0 / loop_b→3 over 5 consecutive single-tag
/// frames (segment A = frames 0..2, segment B = frames 3..4) covering 5 tags.
fn edit_fixture() -> (Ap2Doc, SpritePath) {
    let mut b = FixtureBuilder::new("edit_fx");
    b.push_shape(&[], 0, 1);
    b.push_shape(&[], 0, 2);
    let sp = b.push_sprite(&[], 10);
    b.push_place(
        &[sp],
        PlaceObjectParams {
            depth: 1,
            object_id: 300,
            source_tag_id: Some(1),
            ..Default::default()
        },
        Some("word_inst"),
    );
    b.push_place(
        &[sp],
        PlaceObjectParams {
            depth: 1,
            object_id: 300,
            source_tag_id: Some(1),
            translate: Some((20, 40)),
            ..Default::default()
        },
        None,
    );
    b.push_opaque(&[sp], 0x50, &[0xA1; 4]);
    b.push_place(
        &[sp],
        PlaceObjectParams {
            depth: 2,
            object_id: 301,
            source_tag_id: Some(2),
            ..Default::default()
        },
        None,
    );
    b.push_opaque(&[sp], 0x51, &[0xB2; 8]);
    for (s, c) in [(0u32, 1u32), (1, 1), (2, 1), (3, 1), (4, 1)] {
        b.push_frame(&[sp], s, c);
    }
    b.add_label(&[sp], "in_marvelous", 0);
    b.add_label(&[sp], "loop_b", 3);
    b.push_frame(&[], 0, 3);
    (
        b.finish(),
        SpritePath {
            tag_indices: vec![sp],
        },
    )
}

/// AC2 fixture: root frame 0 executes a shape + placements at depths 1..3
/// (each named, translated); frame 1 executes one opaque tag after them.
fn placements_fixture() -> Ap2Doc {
    let mut b = FixtureBuilder::new("rows");
    b.push_shape(&[], 0, 1);
    for (i, d) in [1u16, 2, 3].into_iter().enumerate() {
        let name = format!("row_{d}");
        b.push_place(
            &[],
            PlaceObjectParams {
                depth: d,
                object_id: 300 + i as u16,
                source_tag_id: Some(1),
                translate: Some((d as i32 * 100, 500)),
                ..Default::default()
            },
            Some(&name),
        );
    }
    b.push_opaque(&[], 0x60, &[1, 2, 3, 4]);
    b.push_frame(&[], 0, 4);
    b.push_frame(&[], 4, 1);
    b.finish()
}

/// AC3 helper: the doc still serializes byte-identically to `baseline`.
fn assert_unchanged(doc: &Ap2Doc, baseline: &[u8], what: &str) {
    let out = doc
        .serialize()
        .unwrap_or_else(|| panic!("{what}: still serializes"));
    assert_eq!(out, baseline, "{what}: doc changed by a failed edit");
}

fn place_view(tag: &Tag) -> PlaceObjectView {
    match tag {
        Tag::PlaceObject(p) => p.view().expect("place decodes"),
        other => panic!("expected PlaceObject, got {other:?}"),
    }
}

#[test]
fn edit_add_label_success() {
    let (mut doc, sp) = edit_fixture();
    doc.add_label(&SpritePath::default(), "root_lab", 0)
        .expect("root label");
    doc.add_label(&sp, "mid", 2).expect("nested label");
    assert_eq!(doc.root.label_frame("root_lab"), Some(0));
    assert_eq!(doc.section(&sp).unwrap().label_frame("mid"), Some(2));
    // Interned + resolvable after a round trip.
    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");
    assert_eq!(re.root.label_frame("root_lab"), Some(0));
    assert_eq!(re.section(&sp).unwrap().label_frame("mid"), Some(2));
}

#[test]
fn edit_add_label_failures_leave_doc_unchanged() {
    let (mut doc, sp) = edit_fixture();
    let baseline = doc.serialize().unwrap();
    // Duplicate name within the section.
    assert!(doc.add_label(&sp, "in_marvelous", 4).is_none());
    // Frame out of range (sprite has 5 frames).
    assert!(doc.add_label(&sp, "late", 5).is_none());
    // Unknown sprite path / path addressing a non-sprite tag.
    assert!(doc
        .add_label(
            &SpritePath {
                tag_indices: vec![7]
            },
            "x",
            0
        )
        .is_none());
    assert!(doc
        .add_label(
            &SpritePath {
                tag_indices: vec![0]
            },
            "x",
            0
        )
        .is_none());
    assert_unchanged(&doc, &baseline, "add_label failures");
    // The same name in ANOTHER section is legal (label maps are per-section).
    doc.add_label(&SpritePath::default(), "in_marvelous", 0)
        .expect("cross-section duplicate");
}

#[test]
fn edit_add_shape_allocates_and_inserts() {
    let (mut doc, sp) = edit_fixture();
    assert_eq!(doc.max_character_id(), 10);
    let id = doc
        .add_shape(&SpritePath::default(), 0, 7)
        .expect("allocates");
    assert_eq!(id, 11);
    // Inserted at the END of root frame 0's span (after the sprite tag).
    assert_eq!(
        doc.root.frames[0],
        FrameSpan {
            start_tag: 0,
            tag_count: 4
        }
    );
    assert_eq!(doc.root.tags[3], Tag::Shape(Shape { unknown: 7, id: 11 }));
    assert_eq!(doc.max_character_id(), 11);
    assert!(
        doc.section(&sp).is_some(),
        "sprite path survives the insert"
    );
    // Second allocation continues from the new max.
    assert_eq!(doc.add_shape(&SpritePath::default(), 0, 0), Some(12));
    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");
    assert_eq!(re.max_character_id(), 12);
}

#[test]
fn edit_add_shape_shifts_later_frames_and_keeps_labels() {
    let (mut doc, sp) = edit_fixture();
    let id = doc.add_shape(&sp, 0, 0).expect("adds");
    assert_eq!(id, 11);
    let sec = doc.section(&sp).unwrap();
    assert_eq!(
        sec.frames[0],
        FrameSpan {
            start_tag: 0,
            tag_count: 2
        }
    );
    let shifted: Vec<FrameSpan> = (2..=5)
        .map(|s| FrameSpan {
            start_tag: s,
            tag_count: 1,
        })
        .collect();
    assert_eq!(&sec.frames[1..], shifted.as_slice());
    assert_eq!(sec.tags.len(), 6);
    assert_eq!(sec.tags[1], Tag::Shape(Shape { unknown: 0, id: 11 }));
    // Labels reference frame numbers — structurally stable under tag inserts.
    assert_eq!(sec.label_frame("in_marvelous"), Some(0));
    assert_eq!(sec.label_frame("loop_b"), Some(3));
    let bytes = doc.serialize().expect("serializes");
    assert!(Ap2Doc::parse(&bytes).is_some());
}

#[test]
fn edit_add_shape_failures() {
    let (mut doc, _sp) = edit_fixture();
    let baseline = doc.serialize().unwrap();
    assert!(doc
        .add_shape(
            &SpritePath {
                tag_indices: vec![9]
            },
            0,
            0
        )
        .is_none());
    assert!(doc.add_shape(&SpritePath::default(), 1, 0).is_none()); // root has 1 frame
    assert_unchanged(&doc, &baseline, "add_shape failures");

    // Character-id space exhausted.
    let mut full = Ap2Doc::new("full").unwrap();
    full.root.tags.push(Tag::Shape(Shape {
        unknown: 0,
        id: u16::MAX,
    }));
    full.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 1,
    });
    let b2 = full.serialize().unwrap();
    assert!(full.add_shape(&SpritePath::default(), 0, 0).is_none());
    assert_unchanged(&full, &b2, "id exhaustion");
}

#[test]
fn edit_clone_segment_ac1() {
    let (mut doc, sp) = edit_fixture();
    let remap = TagRemap::from([(1u16, 11u16)]);
    doc.clone_labeled_segment(&sp, "in_marvelous", "in_smarvelous", &remap)
        .expect("clones");

    let sec = doc.section(&sp).unwrap();
    assert_eq!(sec.frames.len(), 8);
    assert_eq!(sec.tags.len(), 8);
    let appended: Vec<FrameSpan> = (5..=7)
        .map(|s| FrameSpan {
            start_tag: s,
            tag_count: 1,
        })
        .collect();
    assert_eq!(&sec.frames[5..], appended.as_slice());
    assert_eq!(sec.label_frame("in_marvelous"), Some(0));
    assert_eq!(sec.label_frame("loop_b"), Some(3));
    assert_eq!(sec.label_frame("in_smarvelous"), Some(5));

    // Clones carry the remapped id; everything else copied.
    let orig = place_view(&sec.tags[0]);
    let cl = place_view(&sec.tags[5]);
    assert_eq!(orig.source_tag_id, Some(1), "original untouched");
    assert_eq!(cl.source_tag_id, Some(11));
    assert_eq!(cl.depth, 1);
    assert_eq!(cl.object_id, 300);
    assert_eq!(cl.movie_name_offset, orig.movie_name_offset);
    let cl1 = place_view(&sec.tags[6]);
    assert_eq!(cl1.source_tag_id, Some(11));
    assert_eq!(cl1.translate, Some((20, 40)));
    match &sec.tags[7] {
        Tag::Opaque(o) => assert_eq!((o.tag_id, o.data.as_slice()), (0x50, &[0xA1; 4][..])),
        other => panic!("expected opaque clone, got {other:?}"),
    }

    // Round trip + one-emission fixed point.
    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");
    assert_eq!(
        re.section(&sp).unwrap().label_frame("in_smarvelous"),
        Some(5)
    );
    assert_eq!(re.serialize().unwrap(), bytes);
}

#[test]
fn edit_clone_segment_at_section_end_empty_remap() {
    let (mut doc, sp) = edit_fixture();
    let baseline_tags = doc.section(&sp).unwrap().tags.clone();
    doc.clone_labeled_segment(&sp, "loop_b", "loop_b2", &TagRemap::new())
        .expect("clones");
    let sec = doc.section(&sp).unwrap();
    assert_eq!(sec.frames.len(), 7);
    assert_eq!(
        &sec.frames[5..],
        &[
            FrameSpan {
                start_tag: 5,
                tag_count: 1
            },
            FrameSpan {
                start_tag: 6,
                tag_count: 1
            }
        ]
    );
    assert_eq!(sec.label_frame("loop_b2"), Some(5));
    assert_eq!(sec.tags.len(), 7);
    assert_eq!(sec.tags[5], baseline_tags[3], "verbatim clone");
    assert_eq!(sec.tags[6], baseline_tags[4], "verbatim clone");
    let bytes = doc.serialize().expect("serializes");
    assert!(Ap2Doc::parse(&bytes).is_some());
}

#[test]
fn edit_clone_remaps_shape_and_sprite_definition_ids() {
    let mut doc = Ap2Doc::new("defs").unwrap();
    doc.root.tags.push(Tag::Shape(Shape { unknown: 3, id: 5 }));
    doc.root.tags.push(Tag::DefineSprite(DefineSprite {
        flags: 1,
        id: 6,
        pre_section: Vec::new(),
        section: TagSection::new(),
        post_section: Vec::new(),
        pad: Vec::new(),
    }));
    doc.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 2,
    });
    doc.root.frames.push(FrameSpan {
        start_tag: 2,
        tag_count: 0,
    });
    doc.add_label(&SpritePath::default(), "seg", 0).unwrap();
    doc.add_label(&SpritePath::default(), "post", 1).unwrap();
    let remap = TagRemap::from([(5u16, 9u16), (6, 12)]);
    doc.clone_labeled_segment(&SpritePath::default(), "seg", "seg2", &remap)
        .expect("clones");
    assert_eq!(doc.root.tags[2], Tag::Shape(Shape { unknown: 3, id: 9 }));
    match &doc.root.tags[3] {
        Tag::DefineSprite(s) => assert_eq!(s.id, 12),
        other => panic!("expected sprite clone, got {other:?}"),
    }
    assert_eq!(doc.root.label_frame("seg2"), Some(2));
    assert_eq!(doc.max_character_id(), 12);
    let bytes = doc.serialize().expect("serializes");
    assert!(Ap2Doc::parse(&bytes).is_some());
}

#[test]
fn edit_clone_preserves_unmodeled_place_object_bytes() {
    // Unmodeled fields (label_id/unk3/blend + junk realign bytes + opaque
    // tail): the clone must be byte-identical except the 2 spliced id bytes.
    let payload = build_view_payload(); // src 0x0505 at offset 8, depth 3
    let mut doc = Ap2Doc::new("um").unwrap();
    doc.root.tags.push(Tag::PlaceObject(PlaceObject {
        data: payload.clone(),
        pad: Vec::new(),
    }));
    doc.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 1,
    });
    doc.add_label(&SpritePath::default(), "seg", 0).unwrap();
    doc.clone_labeled_segment(
        &SpritePath::default(),
        "seg",
        "seg2",
        &TagRemap::from([(0x0505u16, 0x0666u16)]),
    )
    .expect("clones");
    let Tag::PlaceObject(cl) = &doc.root.tags[1] else {
        panic!("expected cloned place");
    };
    let mut expect = payload.clone();
    expect[8..10].copy_from_slice(&0x0666u16.to_le_bytes());
    assert_eq!(cl.data, expect);
    assert_eq!(cl.view().unwrap().source_tag_id, Some(0x0666));
    let Tag::PlaceObject(orig) = &doc.root.tags[0] else {
        panic!("expected original place");
    };
    assert_eq!(orig.data, payload, "original untouched");

    // Second-flag-word payload: the source id sits at offset 12.
    let mut d2 = Vec::new();
    d2.extend_from_slice(&0x8000_0002u32.to_le_bytes());
    d2.extend_from_slice(&1u16.to_le_bytes());
    d2.extend_from_slice(&2u16.to_le_bytes());
    d2.extend_from_slice(&0u32.to_le_bytes()); // more_flags
    d2.extend_from_slice(&0x0505u16.to_le_bytes());
    d2.extend_from_slice(&[0, 0]); // trailing alignment junk
    let mut doc2 = Ap2Doc::new("um2").unwrap();
    doc2.root.tags.push(Tag::PlaceObject(PlaceObject {
        data: d2.clone(),
        pad: Vec::new(),
    }));
    doc2.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 1,
    });
    doc2.add_label(&SpritePath::default(), "seg", 0).unwrap();
    doc2.clone_labeled_segment(
        &SpritePath::default(),
        "seg",
        "seg2",
        &TagRemap::from([(0x0505u16, 0x0666u16)]),
    )
    .expect("clones");
    let Tag::PlaceObject(cl2) = &doc2.root.tags[1] else {
        panic!("expected cloned place");
    };
    let mut e2 = d2.clone();
    e2[12..14].copy_from_slice(&0x0666u16.to_le_bytes());
    assert_eq!(cl2.data, e2);
}

#[test]
fn edit_clone_failures_leave_doc_unchanged() {
    let (mut doc, sp) = edit_fixture();
    let baseline = doc.serialize().unwrap();
    let empty = TagRemap::new();
    // Missing source label.
    assert!(doc
        .clone_labeled_segment(&sp, "absent", "x", &empty)
        .is_none());
    // Duplicate new label.
    assert!(doc
        .clone_labeled_segment(&sp, "in_marvelous", "loop_b", &empty)
        .is_none());
    // Unknown sprite path / non-sprite tag path.
    assert!(doc
        .clone_labeled_segment(
            &SpritePath {
                tag_indices: vec![9]
            },
            "in_marvelous",
            "x",
            &empty
        )
        .is_none());
    assert!(doc
        .clone_labeled_segment(
            &SpritePath {
                tag_indices: vec![0]
            },
            "in_marvelous",
            "x",
            &empty
        )
        .is_none());
    assert_unchanged(&doc, &baseline, "clone failures");

    // New-label frame index would not fit the u16 name-reference field.
    let mut big = Ap2Doc::new("big").unwrap();
    big.root.frames.resize(
        0x10000,
        FrameSpan {
            start_tag: 0,
            tag_count: 0,
        },
    );
    big.add_label(&SpritePath::default(), "seg", 0).unwrap();
    let b2 = big.serialize().unwrap();
    assert!(big
        .clone_labeled_segment(&SpritePath::default(), "seg", "seg2", &empty)
        .is_none());
    assert_unchanged(&big, &b2, "u16 frame-index exhaustion");

    // Undecodable PlaceObject in the segment: a non-empty remap cannot be
    // applied safely (fails); an empty remap clones verbatim (succeeds).
    let mut trunc = Ap2Doc::new("trunc").unwrap();
    let mut short = Vec::new();
    short.extend_from_slice(&0x2u32.to_le_bytes()); // claims a source id...
    short.extend_from_slice(&1u16.to_le_bytes());
    short.extend_from_slice(&2u16.to_le_bytes()); // ...but the field is missing
    trunc.root.tags.push(Tag::PlaceObject(PlaceObject {
        data: short,
        pad: Vec::new(),
    }));
    trunc.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 1,
    });
    trunc.add_label(&SpritePath::default(), "seg", 0).unwrap();
    let b3 = trunc.serialize().unwrap();
    assert!(trunc
        .clone_labeled_segment(
            &SpritePath::default(),
            "seg",
            "seg2",
            &TagRemap::from([(1u16, 2u16)])
        )
        .is_none());
    assert_unchanged(&trunc, &b3, "undecodable place with non-empty remap");
    assert!(trunc
        .clone_labeled_segment(&SpritePath::default(), "seg", "seg2", &empty)
        .is_some());
}

#[test]
fn edit_add_place_object_named_ac2() {
    let mut doc = placements_fixture();
    doc.add_place_object_named(
        &SpritePath::default(),
        &NamedPlacement {
            frame: 0,
            depth: 4,
            object_id: 310,
            source_tag_id: 1,
            instance_name: "smarvelous_num_usr",
            translate: Some((-100, 220)),
        },
    )
    .expect("inserts");
    // Lands inside frame 0's span; frame 1's span shifts.
    assert_eq!(
        doc.root.frames[0],
        FrameSpan {
            start_tag: 0,
            tag_count: 5
        }
    );
    assert_eq!(
        doc.root.frames[1],
        FrameSpan {
            start_tag: 5,
            tag_count: 1
        }
    );
    let v = place_view(&doc.root.tags[4]);
    assert_eq!(v.depth, 4);
    assert_eq!(v.object_id, 310);
    assert_eq!(v.source_tag_id, Some(1));
    assert_eq!(v.translate, Some((-100, 220)));
    assert_eq!(
        doc.strings.get(v.movie_name_offset.unwrap()),
        Some("smarvelous_num_usr")
    );
    // Round trip: the name still resolves through the re-parsed table.
    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");
    assert_eq!(re.root.frames, doc.root.frames);
    let rv = place_view(&re.root.tags[4]);
    assert_eq!(
        re.strings.get(rv.movie_name_offset.unwrap()),
        Some("smarvelous_num_usr")
    );
}

#[test]
fn edit_add_place_object_named_failures() {
    let mut doc = placements_fixture();
    let baseline = doc.serialize().unwrap();
    // Occupied depth within the target frame.
    assert!(doc
        .add_place_object_named(
            &SpritePath::default(),
            &NamedPlacement {
                frame: 0,
                depth: 2,
                object_id: 311,
                source_tag_id: 1,
                instance_name: "dup",
                translate: None,
            },
        )
        .is_none());
    // Frame out of range / unknown path.
    assert!(doc
        .add_place_object_named(
            &SpritePath::default(),
            &NamedPlacement {
                frame: 2,
                depth: 9,
                object_id: 311,
                source_tag_id: 1,
                instance_name: "oob",
                translate: None,
            },
        )
        .is_none());
    assert!(doc
        .add_place_object_named(
            &SpritePath {
                tag_indices: vec![3]
            },
            &NamedPlacement {
                frame: 0,
                depth: 9,
                object_id: 311,
                source_tag_id: 1,
                instance_name: "badpath",
                translate: None,
            },
        )
        .is_none());
    assert_unchanged(&doc, &baseline, "add_place_object_named failures");
    // Depth uniqueness is PER-FRAME: depth 2 is free in frame 1.
    doc.add_place_object_named(
        &SpritePath::default(),
        &NamedPlacement {
            frame: 1,
            depth: 2,
            object_id: 312,
            source_tag_id: 1,
            instance_name: "ok",
            translate: None,
        },
    )
    .expect("per-frame uniqueness");
}

#[test]
fn edit_adjust_placements_predicate_scoping() {
    let mut doc = placements_fixture();
    let before_depth1 = match &doc.root.tags[1] {
        Tag::PlaceObject(p) => p.data.clone(),
        other => panic!("expected place, got {other:?}"),
    };
    let n = doc.adjust_placements(|v| v.depth >= 2, (7, -3));
    assert_eq!(n, 2);
    assert_eq!(place_view(&doc.root.tags[1]).translate, Some((100, 500)));
    assert_eq!(place_view(&doc.root.tags[2]).translate, Some((207, 497)));
    assert_eq!(place_view(&doc.root.tags[3]).translate, Some((307, 497)));
    match &doc.root.tags[1] {
        Tag::PlaceObject(p) => assert_eq!(p.data, before_depth1, "unmatched tag byte-identical"),
        other => panic!("expected place, got {other:?}"),
    }
    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");
    assert_eq!(place_view(&re.root.tags[2]).translate, Some((207, 497)));
}

#[test]
fn edit_adjust_placements_skips_missing_translate_and_reaches_nested() {
    let (mut doc, sp) = edit_fixture();
    // src==1 placements live in the NESTED sprite: tag 0 has no translate
    // field (skipped, not counted), tag 1 has one (adjusted).
    let n = doc.adjust_placements(|v| v.source_tag_id == Some(1), (5, 5));
    assert_eq!(n, 1);
    let sec = doc.section(&sp).unwrap();
    assert_eq!(place_view(&sec.tags[0]).translate, None, "still no field");
    assert_eq!(place_view(&sec.tags[1]).translate, Some((25, 45)));
}

#[test]
fn edit_adjust_placements_no_match_is_identity() {
    let mut doc = placements_fixture();
    let baseline = doc.serialize().unwrap();
    assert_eq!(doc.adjust_placements(|v| v.depth == 99, (1, 1)), 0);
    assert_unchanged(&doc, &baseline, "no-match adjust");
}

#[test]
fn edit_adjust_placements_preserves_unmodeled_bytes() {
    // build_view_payload: depth 3, translate (-40,100) at bytes 28..36,
    // opaque tail at 36..40 — only the 8 translate bytes may change.
    let payload = build_view_payload();
    let mut doc = Ap2Doc::new("um3").unwrap();
    doc.root.tags.push(Tag::PlaceObject(PlaceObject {
        data: payload.clone(),
        pad: Vec::new(),
    }));
    doc.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 1,
    });
    assert_eq!(doc.adjust_placements(|v| v.depth == 3, (10, -10)), 1);
    let Tag::PlaceObject(p) = &doc.root.tags[0] else {
        panic!("expected place");
    };
    let mut expect = payload.clone();
    expect[28..32].copy_from_slice(&(-30i32).to_le_bytes());
    expect[32..36].copy_from_slice(&90i32.to_le_bytes());
    assert_eq!(p.data, expect);
}

/// A doc shaped like the results score tab's judgement rows: two "rows"
/// (depths 23/22 at ty 860/1240 raw) placed in ROOT and duplicated in a
/// nested sprite (the dual-timeline pattern), each with a later same-depth
/// translate UPDATE record at a different tx but the same ty (the guest
/// layout move), plus a same-depth record at a DIFFERENT ty that must never
/// match. Each row therefore matches exactly 4 records.
fn row_shift_fixture() -> Ap2Doc {
    let mut b = FixtureBuilder::new("tab");
    b.push_shape(&[], 0, 1);
    let sp = b.push_sprite(&[], 2);
    for path in [vec![], vec![sp]] {
        for (i, (depth, ty)) in [(23u16, 860i32), (22, 1240)].into_iter().enumerate() {
            let name = format!("row{depth}");
            // f0 initial placement (tx = the registered layout column).
            b.push_place(
                &path,
                PlaceObjectParams {
                    depth,
                    object_id: 300 + i as u16,
                    source_tag_id: Some(1),
                    translate: Some((2780, ty)),
                    ..Default::default()
                },
                Some(&name),
            );
        }
        // Decoy: same depth as row 23, different ty — must not match.
        b.push_place(
            &path,
            PlaceObjectParams {
                depth: 23,
                object_id: 310,
                source_tag_id: Some(1),
                translate: Some((2780, 999)),
                ..Default::default()
            },
            None,
        );
        // f1 guest-move updates: same depth + ty, different tx.
        for (depth, ty) in [(23u16, 860i32), (22, 1240)] {
            b.push_place(
                &path,
                PlaceObjectParams {
                    depth,
                    object_id: 300,
                    translate: Some((5280, ty)),
                    ..Default::default()
                },
                None,
            );
        }
        // Frame spans: the root section carries the shape + sprite tags
        // ahead of the placements; the sprite section starts at its rows.
        let lead = if path.is_empty() { 2u32 } else { 0 };
        b.push_frame(&path, 0, lead + 3);
        b.push_frame(&path, lead + 3, 2);
    }
    b.finish()
}

#[test]
fn edit_shift_row_translates_moves_all_and_only_matches() {
    let mut doc = row_shift_fixture();
    let rows = [(23u16, 860i32, 320i32), (22, 1240, 260)];
    doc.shift_row_translates(&rows, 4).expect("shifts");
    // Root: initial placements moved (ty 860+320 / 1240+260), decoy kept.
    assert_eq!(place_view(&doc.root.tags[2]).translate, Some((2780, 1180)));
    assert_eq!(place_view(&doc.root.tags[3]).translate, Some((2780, 1500)));
    assert_eq!(place_view(&doc.root.tags[4]).translate, Some((2780, 999)));
    // Root: guest updates moved too (tx untouched).
    assert_eq!(place_view(&doc.root.tags[5]).translate, Some((5280, 1180)));
    assert_eq!(place_view(&doc.root.tags[6]).translate, Some((5280, 1500)));
    // Nested sprite copy moved identically.
    let sp = SpritePath {
        tag_indices: vec![1],
    };
    let sec = doc.section(&sp).unwrap();
    assert_eq!(place_view(&sec.tags[0]).translate, Some((2780, 1180)));
    assert_eq!(place_view(&sec.tags[3]).translate, Some((5280, 1180)));
    // Still serializes and re-parses.
    let bytes = doc.serialize().expect("serializes");
    Ap2Doc::parse(&bytes).expect("re-parses");
}

#[test]
fn edit_shift_row_translates_count_mismatch_is_identity() {
    let mut doc = row_shift_fixture();
    let baseline = doc.serialize().unwrap();
    // Expected count wrong (rows match 4 each, we demand 5) — refused,
    // untouched.
    assert!(doc
        .shift_row_translates(&[(23, 860, 320), (22, 1240, 260)], 5)
        .is_none());
    assert_unchanged(&doc, &baseline, "count-mismatch shift");
    // One row valid + one row absent — refused ATOMICALLY (the valid row
    // must not move either).
    assert!(doc
        .shift_row_translates(&[(23, 860, 320), (21, 777, 100)], 4)
        .is_none());
    assert_unchanged(&doc, &baseline, "partial-match shift");
}

#[test]
fn edit_integration_smarvelous_clone() {
    let (mut doc, _) = edit_fixture();
    // Workflow order matters: an insert into an ancestor section can shift
    // sprite tag indices — allocate the shape FIRST, then resolve the path.
    let new_shape = doc
        .add_shape(&SpritePath::default(), 0, 0)
        .expect("new shape");
    assert_eq!(new_shape, 11);
    let sp = doc.find_sprite_by_label("in_marvelous").expect("sprite");
    doc.clone_labeled_segment(
        &sp,
        "in_marvelous",
        "in_smarvelous",
        &TagRemap::from([(1u16, new_shape)]),
    )
    .expect("clones");

    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");
    let sec = re.section(&sp).unwrap();
    assert_eq!(sec.label_frame("in_smarvelous"), Some(5));
    // Every cloned placement of the old art now references the new shape...
    assert_eq!(place_view(&sec.tags[5]).source_tag_id, Some(new_shape));
    assert_eq!(place_view(&sec.tags[6]).source_tag_id, Some(new_shape));
    // ...while the original segment still shares the old one.
    assert_eq!(place_view(&sec.tags[0]).source_tag_id, Some(1));
    assert_eq!(re.max_character_id(), new_shape);
    assert_eq!(re.serialize().unwrap(), bytes, "fixed point");
}

// ---------------------------------------------------------------------------
// Step-4 task-01: definition-aware cloning (edit.rs).
// ---------------------------------------------------------------------------

/// Step-4 fixture mirroring the real dance_judge template's shape (research
/// display-side-re.md §10, the task's implementation notes): frame 0 executes
/// the whole dictionary PLUS the segment's placements — [DefineSprite 3
/// (places shape 32), Shape 32, Shape 8, DefineSprite 35 (places 32),
/// PlaceObject(35, depth 2), PlaceObject(8, depth 3)]; frames 1..3 are
/// translate-only placement updates. Labels seg1@0 (frames 0..1), seg2@2
/// (frames 2..3). Ids mirror the real template (35 = word sprite, 32 = word
/// shape, 8 = flash shape).
fn template_fixture() -> Ap2Doc {
    let mut b = FixtureBuilder::new("dance_judge_fx");
    let sa = b.push_sprite(&[], 3);
    b.push_place(
        &[sa],
        PlaceObjectParams {
            depth: 1,
            object_id: 200,
            source_tag_id: Some(32),
            ..Default::default()
        },
        None,
    );
    b.push_frame(&[sa], 0, 1);
    b.push_shape(&[], 7, 32);
    b.push_shape(&[], 7, 8);
    let sw = b.push_sprite(&[], 35);
    b.push_place(
        &[sw],
        PlaceObjectParams {
            depth: 1,
            object_id: 210,
            source_tag_id: Some(32),
            translate: Some((100, 200)),
            ..Default::default()
        },
        Some("word_art"),
    );
    b.push_frame(&[sw], 0, 1);
    b.push_place(
        &[],
        PlaceObjectParams {
            depth: 2,
            object_id: 300,
            source_tag_id: Some(35),
            ..Default::default()
        },
        Some("word_usr"),
    );
    b.push_place(
        &[],
        PlaceObjectParams {
            depth: 3,
            object_id: 301,
            source_tag_id: Some(8),
            ..Default::default()
        },
        None,
    );
    // Frames 1..3: translate-only placement updates (the real segments'
    // per-frame animation shape — no source ids, no definitions).
    for ty in [10i32, 20, 30] {
        b.push_place(
            &[],
            PlaceObjectParams {
                depth: 2,
                object_id: 300,
                translate: Some((0, ty)),
                ..Default::default()
            },
            None,
        );
    }
    b.push_frame(&[], 0, 6);
    b.push_frame(&[], 6, 1);
    b.push_frame(&[], 7, 1);
    b.push_frame(&[], 8, 1);
    b.add_label(&[], "seg1", 0);
    b.add_label(&[], "seg2", 2);
    b.finish()
}

#[test]
fn edit_clone_sprite_def_ac1() {
    let mut doc = template_fixture();
    let new_shape = doc.add_shape(&SpritePath::default(), 0, 7).expect("shape");
    assert_eq!(new_shape, 36);
    let remap = TagRemap::from([(32u16, new_shape)]);
    let new_id = doc
        .clone_sprite_definition(&SpritePath::default(), 35, &remap)
        .expect("clones");
    assert_eq!(new_id, 37);

    // Directly after the original definition (post-add_shape layout put
    // Sprite35 at index 3 → the copy lands at index 4, same frame span).
    let Tag::DefineSprite(copy) = &doc.root.tags[4] else {
        panic!(
            "expected sprite copy at index 4, got {:?}",
            doc.root.tags[4]
        );
    };
    assert_eq!(copy.id, 37, "copy carries the NEW id");
    let nested = place_view(&copy.section.tags[0]);
    assert_eq!(nested.source_tag_id, Some(new_shape), "internal remap");
    assert_eq!(nested.translate, Some((100, 200)), "other fields preserved");
    // Original untouched; instance-name offset shared with the copy.
    let Tag::DefineSprite(orig) = &doc.root.tags[3] else {
        panic!("expected original sprite at index 3");
    };
    assert_eq!(orig.id, 35);
    let onested = place_view(&orig.section.tags[0]);
    assert_eq!(onested.source_tag_id, Some(32));
    assert_eq!(nested.movie_name_offset, onested.movie_name_offset);

    // Frame 0's span grew by 1; later spans shifted right by 1.
    assert_eq!(
        doc.root.frames[0],
        FrameSpan {
            start_tag: 0,
            tag_count: 8
        }
    );
    let shifted: Vec<FrameSpan> = (8..=10)
        .map(|s| FrameSpan {
            start_tag: s,
            tag_count: 1,
        })
        .collect();
    assert_eq!(&doc.root.frames[1..], shifted.as_slice());
    assert_eq!(doc.root.label_frame("seg1"), Some(0));
    assert_eq!(doc.root.label_frame("seg2"), Some(2));
    assert_eq!(doc.max_character_id(), 37);

    // Round trip + fixed point + nested check through the re-parse.
    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");
    let re_sec = re
        .section(&SpritePath {
            tag_indices: vec![4],
        })
        .expect("copy section");
    assert_eq!(place_view(&re_sec.tags[0]).source_tag_id, Some(new_shape));
    assert_eq!(re.serialize().unwrap(), bytes, "fixed point");
}

#[test]
fn edit_clone_sprite_def_recurses_nesting() {
    // Outer sprite 10 carries its own private dictionary: Shape 2, nested
    // DefineSprite 11 (placing 2), and a placement of root shape 1. The
    // remap must reach ALL nesting levels of the copy.
    let mut doc = Ap2Doc::new("nested").unwrap();
    doc.root.tags.push(Tag::Shape(Shape { unknown: 0, id: 1 }));
    let mut inner11 = TagSection::new();
    inner11.tags.push(Tag::PlaceObject(
        PlaceObject::build(&PlaceObjectParams {
            depth: 1,
            object_id: 400,
            source_tag_id: Some(2),
            ..Default::default()
        })
        .unwrap(),
    ));
    inner11.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 1,
    });
    let mut outer10 = TagSection::new();
    outer10.tags.push(Tag::Shape(Shape { unknown: 5, id: 2 }));
    outer10.tags.push(Tag::DefineSprite(DefineSprite {
        flags: 1,
        id: 11,
        pre_section: Vec::new(),
        section: inner11,
        post_section: Vec::new(),
        pad: Vec::new(),
    }));
    outer10.tags.push(Tag::PlaceObject(
        PlaceObject::build(&PlaceObjectParams {
            depth: 1,
            object_id: 401,
            source_tag_id: Some(1),
            ..Default::default()
        })
        .unwrap(),
    ));
    outer10.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 3,
    });
    doc.root.tags.push(Tag::DefineSprite(DefineSprite {
        flags: 1,
        id: 10,
        pre_section: Vec::new(),
        section: outer10,
        post_section: Vec::new(),
        pad: Vec::new(),
    }));
    doc.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 2,
    });

    let remap = TagRemap::from([(1u16, 91u16), (2, 92), (11, 93)]);
    let new_id = doc
        .clone_sprite_definition(&SpritePath::default(), 10, &remap)
        .expect("clones");
    assert_eq!(new_id, 12); // max was the inner sprite's 11

    let Tag::DefineSprite(copy) = &doc.root.tags[2] else {
        panic!("copy directly after the original");
    };
    assert_eq!(copy.id, 12, "outer id is the allocation, not the map");
    assert_eq!(
        copy.section.tags[0],
        Tag::Shape(Shape { unknown: 5, id: 92 })
    );
    let Tag::DefineSprite(inner) = &copy.section.tags[1] else {
        panic!("inner sprite clone");
    };
    assert_eq!(inner.id, 93);
    assert_eq!(
        place_view(&inner.section.tags[0]).source_tag_id,
        Some(92),
        "two levels deep"
    );
    assert_eq!(place_view(&copy.section.tags[2]).source_tag_id, Some(91));

    // The original tree is untouched at every level.
    let Tag::DefineSprite(orig) = &doc.root.tags[1] else {
        panic!("original sprite");
    };
    assert_eq!(orig.id, 10);
    assert_eq!(
        orig.section.tags[0],
        Tag::Shape(Shape { unknown: 5, id: 2 })
    );
    let Tag::DefineSprite(oinner) = &orig.section.tags[1] else {
        panic!("original inner sprite");
    };
    assert_eq!(oinner.id, 11);
    assert_eq!(place_view(&oinner.section.tags[0]).source_tag_id, Some(2));

    let bytes = doc.serialize().expect("serializes");
    assert!(Ap2Doc::parse(&bytes).is_some());
}

#[test]
fn edit_clone_sprite_def_failures() {
    let mut doc = template_fixture();
    let baseline = doc.serialize().unwrap();
    let empty = TagRemap::new();
    let root = SpritePath::default();
    // Unknown / non-sprite path.
    assert!(doc
        .clone_sprite_definition(
            &SpritePath {
                tag_indices: vec![9]
            },
            35,
            &empty
        )
        .is_none());
    assert!(doc
        .clone_sprite_definition(
            &SpritePath {
                tag_indices: vec![1]
            },
            35,
            &empty
        )
        .is_none());
    // src_id absent; src_id belonging to a Shape, not a DefineSprite.
    assert!(doc.clone_sprite_definition(&root, 99, &empty).is_none());
    assert!(doc.clone_sprite_definition(&root, 32, &empty).is_none());
    assert_unchanged(&doc, &baseline, "clone_sprite_definition failures");

    // Definition not covered by any executed frame span: no "same frame
    // span" exists to insert into — fail closed.
    let mut off = Ap2Doc::new("off").unwrap();
    off.root.tags.push(Tag::DefineSprite(DefineSprite {
        flags: 1,
        id: 4,
        pre_section: Vec::new(),
        section: TagSection::new(),
        post_section: Vec::new(),
        pad: Vec::new(),
    }));
    off.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 0,
    });
    let b2 = off.serialize().unwrap();
    assert!(off
        .clone_sprite_definition(&SpritePath::default(), 4, &empty)
        .is_none());
    assert_unchanged(&off, &b2, "definition outside every span");

    // Character-id space exhausted.
    let mut doc2 = template_fixture();
    doc2.root.tags.push(Tag::Shape(Shape {
        unknown: 0,
        id: u16::MAX,
    }));
    doc2.root.frames.push(FrameSpan {
        start_tag: 9,
        tag_count: 1,
    });
    let b3 = doc2.serialize().unwrap();
    assert!(doc2
        .clone_sprite_definition(&SpritePath::default(), 35, &empty)
        .is_none());
    assert_unchanged(&doc2, &b3, "id exhaustion");

    // Undecodable nested PlaceObject: the non-empty remap fails closed, the
    // empty remap clones verbatim (same rule as clone_labeled_segment).
    let mut trunc = Ap2Doc::new("trunc").unwrap();
    let mut short = Vec::new();
    short.extend_from_slice(&0x2u32.to_le_bytes()); // claims a source id...
    short.extend_from_slice(&1u16.to_le_bytes());
    short.extend_from_slice(&2u16.to_le_bytes()); // ...but the field is missing
    let mut inner = TagSection::new();
    inner.tags.push(Tag::PlaceObject(PlaceObject {
        data: short,
        pad: Vec::new(),
    }));
    inner.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 1,
    });
    trunc.root.tags.push(Tag::DefineSprite(DefineSprite {
        flags: 1,
        id: 7,
        pre_section: Vec::new(),
        section: inner,
        post_section: Vec::new(),
        pad: Vec::new(),
    }));
    trunc.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 1,
    });
    let b4 = trunc.serialize().unwrap();
    assert!(trunc
        .clone_sprite_definition(&SpritePath::default(), 7, &TagRemap::from([(1u16, 2u16)]))
        .is_none());
    assert_unchanged(&trunc, &b4, "undecodable nested place with non-empty remap");
    assert_eq!(
        trunc.clone_sprite_definition(&SpritePath::default(), 7, &empty),
        Some(8),
        "empty remap clones verbatim"
    );
}

#[test]
fn edit_clone_placements_only_ac2() {
    let mut doc = template_fixture();
    let before_tags = doc.root.tags.clone();
    let remap = TagRemap::from([(35u16, 55u16)]);
    doc.clone_labeled_segment_placements_only(&SpritePath::default(), "seg1", "seg1_s", &remap)
        .expect("clones");

    // Exactly the 3 non-definition tags appended (frame 0's two placements +
    // frame 1's update) — no definition duplicated.
    assert_eq!(doc.root.tags.len(), 12);
    assert_eq!(
        &doc.root.tags[..9],
        before_tags.as_slice(),
        "originals untouched"
    );
    let defs = doc
        .root
        .tags
        .iter()
        .filter(|t| matches!(t, Tag::DefineSprite(_) | Tag::Shape(_)))
        .count();
    assert_eq!(defs, 4, "no new definitions");

    let cl0 = place_view(&doc.root.tags[9]);
    assert_eq!(cl0.source_tag_id, Some(55), "remapped");
    assert_eq!(cl0.depth, 2);
    assert_eq!(
        cl0.movie_name_offset,
        place_view(&before_tags[4]).movie_name_offset
    );
    let cl1 = place_view(&doc.root.tags[10]);
    assert_eq!(cl1.source_tag_id, Some(8), "not in map: shared definition");
    assert_eq!(cl1.depth, 3);
    let cl2 = place_view(&doc.root.tags[11]);
    assert_eq!(cl2.translate, Some((0, 10)), "frame-1 update cloned");

    // Spans shrink to the copied tags; label at the first cloned frame.
    assert_eq!(doc.root.frames.len(), 6);
    assert_eq!(
        doc.root.frames[4],
        FrameSpan {
            start_tag: 9,
            tag_count: 2
        }
    );
    assert_eq!(
        doc.root.frames[5],
        FrameSpan {
            start_tag: 11,
            tag_count: 1
        }
    );
    assert_eq!(doc.root.label_frame("seg1_s"), Some(4));
    assert_eq!(doc.root.label_frame("seg1"), Some(0));
    assert_eq!(doc.root.label_frame("seg2"), Some(2));

    // Round trip + fixed point.
    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");
    assert_eq!(re.root.label_frame("seg1_s"), Some(4));
    assert_eq!(re.serialize().unwrap(), bytes, "fixed point");
}

#[test]
fn edit_clone_placements_only_zero_tag_frames() {
    // A span holding ONLY definitions clones to count 0 (legal, task
    // requirement); a source count-0 frame stays count 0. The serializer
    // must accept count-0 spans, including one pointing AT the tag-list end.
    let mut b = FixtureBuilder::new("zt");
    b.push_shape(&[], 0, 1);
    b.push_place(
        &[],
        PlaceObjectParams {
            depth: 1,
            object_id: 100,
            source_tag_id: Some(1),
            ..Default::default()
        },
        None,
    );
    b.push_frame(&[], 0, 1); // definitions only
    b.push_frame(&[], 1, 1); // the placement
    b.push_frame(&[], 2, 0); // already empty
    b.add_label(&[], "seg", 0);
    let mut doc = b.finish();

    doc.clone_labeled_segment_placements_only(
        &SpritePath::default(),
        "seg",
        "seg2",
        &TagRemap::new(),
    )
    .expect("clones");
    assert_eq!(doc.root.tags.len(), 3, "one placement copied");
    assert_eq!(doc.root.frames.len(), 6);
    assert_eq!(
        doc.root.frames[3],
        FrameSpan {
            start_tag: 2,
            tag_count: 0
        }
    );
    assert_eq!(
        doc.root.frames[4],
        FrameSpan {
            start_tag: 2,
            tag_count: 1
        }
    );
    // Count-0 span pointing AT the tag-list end: semantics-free, legal.
    assert_eq!(
        doc.root.frames[5],
        FrameSpan {
            start_tag: 3,
            tag_count: 0
        }
    );
    assert_eq!(doc.root.label_frame("seg2"), Some(3));

    let bytes = doc.serialize().expect("count-0 spans serialize");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");
    assert_eq!(re.root.frames, doc.root.frames);
    assert_eq!(re.serialize().unwrap(), bytes, "fixed point");
}

#[test]
fn edit_clone_placements_only_skips_opaque_definition_ids() {
    // Definition-class tags carried as Opaque must be classified by TAG ID:
    // 0x78 font, 0x7D text, 0x7E edit text, 0x82 morph, 0x83 image (the
    // modeled 0x79/0x84 are covered by the other tests). 0x80 (RemoveObject)
    // and an unknown 0x50 are NOT definitions and must clone.
    let mut b = FixtureBuilder::new("opq");
    for (i, id) in [0x78u16, 0x7D, 0x7E, 0x82, 0x83, 0x80, 0x50]
        .into_iter()
        .enumerate()
    {
        b.push_opaque(&[], id, &[i as u8; 4]);
    }
    b.push_frame(&[], 0, 7);
    b.add_label(&[], "seg", 0);
    let mut doc = b.finish();

    doc.clone_labeled_segment_placements_only(
        &SpritePath::default(),
        "seg",
        "seg2",
        &TagRemap::new(),
    )
    .expect("clones");
    assert_eq!(
        doc.root.tags.len(),
        9,
        "only the two non-definition opaques"
    );
    assert_eq!(
        doc.root.frames[1],
        FrameSpan {
            start_tag: 7,
            tag_count: 2
        }
    );
    match (&doc.root.tags[7], &doc.root.tags[8]) {
        (Tag::Opaque(a), Tag::Opaque(b2)) => {
            assert_eq!((a.tag_id, a.data.as_slice()), (0x80, &[5u8; 4][..]));
            assert_eq!((b2.tag_id, b2.data.as_slice()), (0x50, &[6u8; 4][..]));
        }
        other => panic!("expected two opaque clones, got {other:?}"),
    }
    let bytes = doc.serialize().expect("serializes");
    assert!(Ap2Doc::parse(&bytes).is_some());
}

#[test]
fn edit_clone_placements_only_failures() {
    let mut doc = template_fixture();
    let baseline = doc.serialize().unwrap();
    let empty = TagRemap::new();
    let root = SpritePath::default();
    // Missing source label / duplicate new label / bad paths.
    assert!(doc
        .clone_labeled_segment_placements_only(&root, "absent", "x", &empty)
        .is_none());
    assert!(doc
        .clone_labeled_segment_placements_only(&root, "seg1", "seg2", &empty)
        .is_none());
    assert!(doc
        .clone_labeled_segment_placements_only(
            &SpritePath {
                tag_indices: vec![9]
            },
            "seg1",
            "x",
            &empty
        )
        .is_none());
    assert!(doc
        .clone_labeled_segment_placements_only(
            &SpritePath {
                tag_indices: vec![1]
            },
            "seg1",
            "x",
            &empty
        )
        .is_none());
    assert_unchanged(&doc, &baseline, "placements-only clone failures");

    // Span pointing past the tag list.
    let mut broken = Ap2Doc::new("broken").unwrap();
    broken.root.frames.push(FrameSpan {
        start_tag: 5,
        tag_count: 9,
    });
    broken.add_label(&SpritePath::default(), "seg", 0).unwrap();
    let b2 = broken.serialize().unwrap();
    assert!(broken
        .clone_labeled_segment_placements_only(&root, "seg", "seg2", &empty)
        .is_none());
    assert_unchanged(&broken, &b2, "span past the tag list");

    // Undecodable PlaceObject (a NON-definition tag, so it IS cloned): the
    // non-empty remap fails closed, the empty remap clones verbatim.
    let mut trunc = Ap2Doc::new("trunc2").unwrap();
    let mut short = Vec::new();
    short.extend_from_slice(&0x2u32.to_le_bytes());
    short.extend_from_slice(&1u16.to_le_bytes());
    short.extend_from_slice(&2u16.to_le_bytes());
    trunc.root.tags.push(Tag::PlaceObject(PlaceObject {
        data: short,
        pad: Vec::new(),
    }));
    trunc.root.frames.push(FrameSpan {
        start_tag: 0,
        tag_count: 1,
    });
    trunc.add_label(&SpritePath::default(), "seg", 0).unwrap();
    let b3 = trunc.serialize().unwrap();
    assert!(trunc
        .clone_labeled_segment_placements_only(
            &root,
            "seg",
            "seg2",
            &TagRemap::from([(1u16, 2u16)])
        )
        .is_none());
    assert_unchanged(&trunc, &b3, "undecodable place with non-empty remap");
    assert!(trunc
        .clone_labeled_segment_placements_only(&root, "seg", "seg2", &empty)
        .is_some());
}

#[test]
fn edit_integration_definition_aware_patch_shape() {
    // The real Step-4 dance_judge patch sequence (research §10): add the
    // S-MARVELOUS shape, clone the word sprite's DEFINITION re-pointed at
    // it, then a placements-only clone of the segment re-pointed at the new
    // sprite. The dictionary stays singular; only placements duplicate.
    let mut doc = template_fixture();
    let root = SpritePath::default();
    let new_shape = doc.add_shape(&root, 0, 7).expect("new shape"); // "54"
    let new_sprite = doc
        .clone_sprite_definition(&root, 35, &TagRemap::from([(32u16, new_shape)]))
        .expect("new sprite"); // "55"
    doc.clone_labeled_segment_placements_only(
        &root,
        "seg1",
        "seg1_s",
        &TagRemap::from([(35u16, new_sprite)]),
    )
    .expect("clones");

    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");

    // Dictionary: every id defined exactly once.
    let mut def_ids = Vec::new();
    for tag in &re.root.tags {
        match tag {
            Tag::DefineSprite(s) => def_ids.push(s.id),
            Tag::Shape(s) => def_ids.push(s.id),
            _ => {}
        }
    }
    let mut sorted = def_ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        def_ids.len(),
        "duplicate definitions: {def_ids:?}"
    );
    assert!(def_ids.contains(&new_shape) && def_ids.contains(&new_sprite));

    // The cloned segment's first frame places the new sprite + the shared
    // flash shape; the new sprite's own placement references the new shape.
    let label = re.root.label_frame("seg1_s").expect("label") as usize;
    let span = re.root.frames[label];
    let s = span.start_tag as usize;
    assert_eq!(span.tag_count, 2);
    assert_eq!(place_view(&re.root.tags[s]).source_tag_id, Some(new_sprite));
    assert_eq!(place_view(&re.root.tags[s + 1]).source_tag_id, Some(8));
    let copy_idx = re
        .root
        .tags
        .iter()
        .position(|t| matches!(t, Tag::DefineSprite(sp) if sp.id == new_sprite))
        .expect("sprite copy");
    let re_sec = re
        .section(&SpritePath {
            tag_indices: vec![copy_idx],
        })
        .unwrap();
    assert_eq!(place_view(&re_sec.tags[0]).source_tag_id, Some(new_shape));
    // The ORIGINAL segment still references the stock chain.
    let orig_span = re.root.frames[0];
    let stock_places: Vec<u16> = (orig_span.start_tag as usize
        ..(orig_span.start_tag + orig_span.tag_count) as usize)
        .filter_map(|i| match &re.root.tags[i] {
            Tag::PlaceObject(p) => p.view().and_then(|v| v.source_tag_id),
            _ => None,
        })
        .collect();
    assert_eq!(stock_places, vec![35, 8]);

    assert_eq!(re.serialize().unwrap(), bytes, "fixed point");
}

// ---------------------------------------------------------------------------
// Step-4 task-02: the definition-aware word-segment recipe
// (`clone_word_segment_with_new_shape`) — the dance_judge patch fn's pure
// core, driven end-to-end on the §10-shaped template fixture.
// ---------------------------------------------------------------------------

#[test]
fn edit_word_segment_recipe_happy_path() {
    let mut doc = template_fixture();
    let clone = doc
        .clone_word_segment_with_new_shape("seg1", "seg1_s", 32)
        .expect("recipe succeeds");

    // Ids are dynamic: the word sprite resolved by structure walk, the two
    // new ids allocated sequentially past the fixture's max (35).
    assert_eq!(clone.word_sprite_id, 35);
    assert_eq!(clone.new_shape_id, 36);
    assert_eq!(clone.new_sprite_id, 37);

    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");

    // Dictionary singular: every id defined exactly once doc-wide.
    let mut def_ids = Vec::new();
    for tag in &re.root.tags {
        match tag {
            Tag::DefineSprite(s) => def_ids.push(s.id),
            Tag::Shape(s) => def_ids.push(s.id),
            _ => {}
        }
    }
    let mut sorted = def_ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), def_ids.len(), "dup definitions: {def_ids:?}");
    assert!(def_ids.contains(&clone.new_shape_id) && def_ids.contains(&clone.new_sprite_id));

    // Cloned segment: first frame places the new sprite + the shared flash
    // shape (8), NOT the stock word sprite.
    let label = re.root.label_frame("seg1_s").expect("new label") as usize;
    let span = re.root.frames[label];
    let s = span.start_tag as usize;
    assert_eq!(span.tag_count, 2);
    assert_eq!(
        place_view(&re.root.tags[s]).source_tag_id,
        Some(clone.new_sprite_id)
    );
    assert_eq!(place_view(&re.root.tags[s + 1]).source_tag_id, Some(8));

    // The sprite copy internally places the NEW shape, translate preserved.
    let copy_idx = re
        .root
        .tags
        .iter()
        .position(|t| matches!(t, Tag::DefineSprite(sp) if sp.id == clone.new_sprite_id))
        .expect("sprite copy");
    let re_sec = re
        .section(&SpritePath {
            tag_indices: vec![copy_idx],
        })
        .unwrap();
    let v = place_view(&re_sec.tags[0]);
    assert_eq!(v.source_tag_id, Some(clone.new_shape_id));
    assert_eq!(v.translate, Some((100, 200)));

    // Original segment untouched: still places {35, 8}.
    let orig_span = re.root.frames[0];
    let stock_places: Vec<u16> = (orig_span.start_tag as usize
        ..(orig_span.start_tag + orig_span.tag_count) as usize)
        .filter_map(|i| match &re.root.tags[i] {
            Tag::PlaceObject(p) => p.view().and_then(|v| v.source_tag_id),
            _ => None,
        })
        .collect();
    assert_eq!(stock_places, vec![35, 8]);

    // Round-trip fixed point.
    assert_eq!(re.serialize().unwrap(), bytes, "fixed point");
}

#[test]
fn edit_word_segment_recipe_matches_manual_primitive_sequence() {
    // The recipe must produce byte-identical output to the hand-driven
    // primitive sequence the task-01 integration test validated.
    let mut by_recipe = template_fixture();
    by_recipe
        .clone_word_segment_with_new_shape("seg1", "seg1_s", 32)
        .expect("recipe");

    let mut by_hand = template_fixture();
    let root = SpritePath::default();
    let new_shape = by_hand.add_shape(&root, 0, 7).unwrap();
    let new_sprite = by_hand
        .clone_sprite_definition(&root, 35, &TagRemap::from([(32u16, new_shape)]))
        .unwrap();
    by_hand
        .clone_labeled_segment_placements_only(
            &root,
            "seg1",
            "seg1_s",
            &TagRemap::from([(35u16, new_sprite)]),
        )
        .unwrap();

    assert_eq!(by_recipe.serialize().unwrap(), by_hand.serialize().unwrap());
}

#[test]
fn edit_word_segment_recipe_failures() {
    // Pre-mutation failures leave the document byte-identical.
    let pristine = template_fixture().serialize().unwrap();

    // Unknown word shape id.
    let mut doc = template_fixture();
    assert!(doc
        .clone_word_segment_with_new_shape("seg1", "seg1_s", 99)
        .is_none());
    assert_eq!(doc.serialize().unwrap(), pristine);

    // word_shape_id names a sprite, not a shape.
    let mut doc = template_fixture();
    assert!(doc
        .clone_word_segment_with_new_shape("seg1", "seg1_s", 3)
        .is_none());
    assert_eq!(doc.serialize().unwrap(), pristine);

    // Missing source label.
    let mut doc = template_fixture();
    assert!(doc
        .clone_word_segment_with_new_shape("nope", "seg1_s", 32)
        .is_none());
    assert_eq!(doc.serialize().unwrap(), pristine);

    // New label already present (pre-checked before any mutation).
    let mut doc = template_fixture();
    assert!(doc
        .clone_word_segment_with_new_shape("seg1", "seg2", 32)
        .is_none());
    assert_eq!(doc.serialize().unwrap(), pristine);

    // Segment places nothing that resolves to a sprite placing the shape:
    // seg2 (frames 2..) carries only translate-only placement updates.
    let mut doc = template_fixture();
    assert!(doc
        .clone_word_segment_with_new_shape("seg2", "seg2_s", 32)
        .is_none());
    assert_eq!(doc.serialize().unwrap(), pristine);
}

#[test]
fn edit_word_segment_recipe_ambiguous_word_sprite_fails_closed() {
    // TWO segment-placed sprites both internally place the word shape —
    // the recipe cannot know which is the word and must refuse.
    let mut b = FixtureBuilder::new("ambig_fx");
    b.push_shape(&[], 7, 32);
    let s1 = b.push_sprite(&[], 40);
    b.push_place(
        &[s1],
        PlaceObjectParams {
            depth: 1,
            object_id: 1,
            source_tag_id: Some(32),
            ..Default::default()
        },
        None,
    );
    b.push_frame(&[s1], 0, 1);
    let s2 = b.push_sprite(&[], 41);
    b.push_place(
        &[s2],
        PlaceObjectParams {
            depth: 1,
            object_id: 2,
            source_tag_id: Some(32),
            ..Default::default()
        },
        None,
    );
    b.push_frame(&[s2], 0, 1);
    b.push_place(
        &[],
        PlaceObjectParams {
            depth: 2,
            object_id: 3,
            source_tag_id: Some(40),
            ..Default::default()
        },
        None,
    );
    b.push_place(
        &[],
        PlaceObjectParams {
            depth: 3,
            object_id: 4,
            source_tag_id: Some(41),
            ..Default::default()
        },
        None,
    );
    b.push_frame(&[], 0, 5);
    b.add_label(&[], "seg1", 0);
    let mut doc = b.finish();
    let pristine = doc.serialize().unwrap();

    assert!(doc
        .clone_word_segment_with_new_shape("seg1", "seg1_s", 32)
        .is_none());
    assert_eq!(doc.serialize().unwrap(), pristine);
}

/// v3-shaped fixture: the word chain is THREE sprites deep
/// (46 → 43 → 42 → shape 41) — the live `dance_judge_v3` template's shape
/// (cabinet deploy #2 finding, research §10 amendment). Labels `seg1@0`,
/// `seg2@2`; the segment places the TOP chain sprite (46) + a flash shape
/// (8) that must stay shared.
fn nested_template_fixture() -> Ap2Doc {
    let mut b = FixtureBuilder::new("dance_judge_v3_fx");
    b.push_shape(&[], 7, 41); // word art shape
    b.push_shape(&[], 7, 8); // flash shape (no word art)
    let s42 = b.push_sprite(&[], 42);
    b.push_place(
        &[s42],
        PlaceObjectParams {
            depth: 1,
            object_id: 200,
            source_tag_id: Some(41),
            translate: Some((10, 20)),
            ..Default::default()
        },
        Some("word_art"),
    );
    b.push_frame(&[s42], 0, 1);
    let s43 = b.push_sprite(&[], 43);
    b.push_place(
        &[s43],
        PlaceObjectParams {
            depth: 1,
            object_id: 201,
            source_tag_id: Some(42),
            ..Default::default()
        },
        None,
    );
    b.push_frame(&[s43], 0, 1);
    let s46 = b.push_sprite(&[], 46);
    b.push_place(
        &[s46],
        PlaceObjectParams {
            depth: 3,
            object_id: 202,
            source_tag_id: Some(43),
            ..Default::default()
        },
        None,
    );
    b.push_frame(&[s46], 0, 1);
    b.push_place(
        &[],
        PlaceObjectParams {
            depth: 2,
            object_id: 300,
            source_tag_id: Some(46),
            ..Default::default()
        },
        Some("word_usr"),
    );
    b.push_place(
        &[],
        PlaceObjectParams {
            depth: 3,
            object_id: 301,
            source_tag_id: Some(8),
            ..Default::default()
        },
        None,
    );
    // Frame 0 executes the whole dictionary + the two placements
    // (root tags: shape41, shape8, sprite42, sprite43, sprite46, place,
    // place = 7).
    b.push_frame(&[], 0, 7);
    // Frames 1..2: animation updates (opaque non-definition filler).
    b.push_opaque(&[], 0x05, &[0, 0, 0, 0]);
    b.push_frame(&[], 7, 1);
    b.push_opaque(&[], 0x05, &[1, 0, 0, 0]);
    b.push_frame(&[], 8, 1);
    b.add_label(&[], "seg1", 0);
    b.add_label(&[], "seg2", 2);
    b.finish()
}

#[test]
fn edit_word_segment_recipe_nested_chain() {
    let mut doc = nested_template_fixture();
    let clone = doc
        .clone_word_segment_with_new_shape("seg1", "seg1_s", 41)
        .expect("nested chain resolves and clones");

    // Top of the chain is the segment-placed sprite.
    assert_eq!(clone.word_sprite_id, 46);
    // New shape allocated first (max id 46 → 47), then the chain clones
    // bottom-up: 42→48, 43→49, 46→50 (the reported new_sprite_id).
    assert_eq!(clone.new_shape_id, 47);
    assert_eq!(clone.new_sprite_id, 50);

    let bytes = doc.serialize().expect("serializes");
    let re = Ap2Doc::parse(&bytes).expect("re-parses");
    let path = re.find_sprite_by_label("seg1_s").expect("label exists");
    let sec = re.section(&path).unwrap();

    // The cloned segment's word placement references the TOP clone; the
    // flash shape placement stays shared (id 8).
    let start = sec.label_frame("seg1_s").unwrap() as usize;
    let mut placed = Vec::new();
    for f in &sec.frames[start..] {
        let s = f.start_tag as usize;
        for t in &sec.tags[s..s + f.tag_count as usize] {
            if let Tag::PlaceObject(po) = t {
                if let Some(id) = po.view().and_then(|v| v.source_tag_id) {
                    placed.push(id);
                }
            }
        }
    }
    assert!(
        placed.contains(&50),
        "segment places the top clone: {placed:?}"
    );
    assert!(placed.contains(&8), "flash shape stays shared: {placed:?}");
    assert!(
        !placed.contains(&46),
        "stock word sprite not re-placed: {placed:?}"
    );

    // Each chain level's clone exists and references the next level's clone.
    let places = |sprite: u16, target: u16| -> bool {
        sec.tags.iter().any(|t| {
            matches!(t, Tag::DefineSprite(sp) if sp.id == sprite
                && sp.section.tags.iter().any(|nt| matches!(
                    nt,
                    Tag::PlaceObject(po)
                        if po.view().and_then(|v| v.source_tag_id) == Some(target))))
        })
    };
    assert!(places(48, 47), "42-clone places the new shape");
    assert!(places(49, 48), "43-clone places the 42-clone");
    assert!(places(50, 49), "46-clone places the 43-clone");
    // Stock chain untouched.
    assert!(places(42, 41) && places(43, 42) && places(46, 43));
}

#[test]
fn edit_find_word_shape_by_geo() {
    // Geo-first resolution on the nested (v3-shaped) fixture: shape 41's
    // geo carries the `*_marvelous` region; shape 8's geo has no word art.
    let doc = nested_template_fixture();
    let lookup = |name: &str| -> Option<Vec<String>> {
        match name {
            "dance_judge_v3_fx_shape41" => Some(vec!["daju_marvelous".into()]),
            "dance_judge_v3_fx_shape8" => Some(vec!["daju_flash".into()]),
            _ => None,
        }
    };
    assert_eq!(
        doc.find_word_shape_by_geo("seg1", "marvelous", lookup),
        Some((41, "daju_marvelous".to_string()))
    );

    // Ambiguity fails closed: two geos matching the suffix.
    let ambiguous = |_: &str| -> Option<Vec<String>> { Some(vec!["x_marvelous".into()]) };
    assert_eq!(
        doc.find_word_shape_by_geo("seg1", "marvelous", ambiguous),
        None
    );

    // No geo matches ⇒ None; unknown label ⇒ None.
    let none = |_: &str| -> Option<Vec<String>> { None };
    assert_eq!(doc.find_word_shape_by_geo("seg1", "marvelous", none), None);
    assert_eq!(
        doc.find_word_shape_by_geo("nope", "marvelous", lookup),
        None
    );
}

/// Shared-chain fixture shaped like the dance_fullcombo splash (Step 6):
/// shapes 10/11/12 = art; sprite 20 places {10,11}, sprite 21 places
/// {11,12} (shape 11 SHARED), sprite 30 places both sprites; the labeled
/// segment places sprite 30 + a non-art shape 5 that must stay shared.
fn shared_chain_fixture() -> Ap2Doc {
    let mut b = FixtureBuilder::new("dance_fc_fx");
    b.push_shape(&[], 7, 10);
    b.push_shape(&[], 7, 11);
    b.push_shape(&[], 7, 12);
    b.push_shape(&[], 7, 5); // non-art, stays shared
    let s20 = b.push_sprite(&[], 20);
    for (d, id) in [(1u16, 10u16), (2, 11)] {
        b.push_place(
            &[s20],
            PlaceObjectParams {
                depth: d,
                object_id: 100 + d,
                source_tag_id: Some(id),
                ..Default::default()
            },
            None,
        );
    }
    b.push_frame(&[s20], 0, 2);
    let s21 = b.push_sprite(&[], 21);
    for (d, id) in [(1u16, 11u16), (2, 12)] {
        b.push_place(
            &[s21],
            PlaceObjectParams {
                depth: d,
                object_id: 110 + d,
                source_tag_id: Some(id),
                ..Default::default()
            },
            None,
        );
    }
    b.push_frame(&[s21], 0, 2);
    let s30 = b.push_sprite(&[], 30);
    for (d, id) in [(1u16, 20u16), (2, 21)] {
        b.push_place(
            &[s30],
            PlaceObjectParams {
                depth: d,
                object_id: 120 + d,
                source_tag_id: Some(id),
                ..Default::default()
            },
            None,
        );
    }
    b.push_frame(&[s30], 0, 2);
    // Root segment: places sprite 30 + the shared non-art shape 5.
    b.push_place(
        &[],
        PlaceObjectParams {
            depth: 1,
            object_id: 130,
            source_tag_id: Some(30),
            ..Default::default()
        },
        None,
    );
    b.push_place(
        &[],
        PlaceObjectParams {
            depth: 2,
            object_id: 131,
            source_tag_id: Some(5),
            ..Default::default()
        },
        None,
    );
    b.push_frame(&[], 4, 2); // 4 shapes + 3 sprites... start index set below
    b.add_label(&[], "marbelous_in", 0);
    b.finish()
}

#[test]
fn edit_clone_segment_with_new_shapes_shared_chain() {
    let mut doc = shared_chain_fixture();
    // Fix the root frame span to cover the placements (builder emits tags
    // in push order: 4 shapes, 3 sprites, 2 places = indices 7,8).
    doc.root.frames[0] = FrameSpan {
        start_tag: 0,
        tag_count: doc.root.tags.len() as u32,
    };
    let res = doc
        .clone_segment_with_new_shapes("marbelous_in", "s_marbelous_in", &[10, 11, 12])
        .expect("clone");
    assert_eq!(res.shapes.len(), 3);
    // Exactly the 3 reaching sprites cloned, ONCE each (dedup despite the
    // shared shape 11).
    assert_eq!(res.sprites.len(), 3);
    let cloned_old: Vec<u16> = res.sprites.iter().map(|(o, _)| *o).collect();
    assert!(cloned_old.contains(&20) && cloned_old.contains(&21) && cloned_old.contains(&30));

    let out = doc.serialize().expect("serialize");
    let re = Ap2Doc::parse(&out).expect("re-parse");
    let sec = &re.root;
    assert!(sec.labels.iter().any(|l| l.name == "s_marbelous_in"));

    // The cloned top sprite's tree must reference ONLY new shape ids.
    let (_, new30) = res.sprites.iter().find(|(o, _)| *o == 30).unwrap();
    let new_ids: Vec<u16> = res.shapes.iter().map(|(_, n)| *n).collect();
    fn placed_ids(sec: &TagSection, sprite: u16, out: &mut Vec<u16>) {
        let sp = sec
            .tags
            .iter()
            .find_map(|t| match t {
                Tag::DefineSprite(s) if s.id == sprite => Some(s),
                _ => None,
            })
            .expect("sprite");
        for t in &sp.section.tags {
            if let Tag::PlaceObject(po) = t {
                if let Some(id) = po.view().and_then(|v| v.source_tag_id) {
                    out.push(id);
                }
            }
        }
    }
    let mut top = Vec::new();
    placed_ids(sec, *new30, &mut top);
    for id in &top {
        // top places cloned sprites only
        assert!(
            res.sprites.iter().any(|(_, n)| n == id),
            "top places {}",
            id
        );
    }
    for (_, new_sprite) in res.sprites.iter().filter(|(o, _)| *o != 30) {
        let mut leaf = Vec::new();
        placed_ids(sec, *new_sprite, &mut leaf);
        for id in &leaf {
            assert!(new_ids.contains(id), "leaf sprite places old id {}", id);
        }
    }

    // The new segment's own placements: sprite 30 remapped, shape 5 kept.
    let label_frame = sec.label_frame("s_marbelous_in").unwrap() as usize;
    let span = &sec.frames[label_frame];
    let mut seg_ids = Vec::new();
    for t in &sec.tags[span.start_tag as usize..(span.start_tag + span.tag_count) as usize] {
        if let Tag::PlaceObject(po) = t {
            if let Some(id) = po.view().and_then(|v| v.source_tag_id) {
                seg_ids.push(id);
            }
        }
    }
    assert!(
        seg_ids.contains(new30),
        "segment places the cloned top sprite"
    );
    assert!(seg_ids.contains(&5), "shared non-art shape stays");

    // Unknown shape id fails closed, doc untouched semantics (fresh doc).
    let mut doc2 = shared_chain_fixture();
    doc2.root.frames[0] = FrameSpan {
        start_tag: 0,
        tag_count: doc2.root.tags.len() as u32,
    };
    assert!(doc2
        .clone_segment_with_new_shapes("marbelous_in", "x_in", &[99])
        .is_none());
}
