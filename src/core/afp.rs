//! AFP format primitives — descramble, string table, tag construction.
//!
//! Pure functions for manipulating Konami's AFP animation binary format.
//! No game interaction — this is reusable format-level code.
//!
//! Reference: bemaniutils/bemani/format/afp/swf.py

//! Apply BSI byte-swap descrambling to AFP data. The operation is self-inverse:
//! applying the same BSI to descrambled data re-scrambles it.
pub fn apply_bsi(data: &mut [u8], bsi: &[u8]) {
    let swap_len: [usize; 4] = [0, 2, 4, 8];
    let mut offset: usize = 0;

    let mut i = 0;
    while i + 1 < bsi.len() {
        let swapword = u16::from_le_bytes([bsi[i], bsi[i + 1]]);
        if swapword == 0 {
            break;
        }
        i += 2;

        let jump = ((swapword & 0x7F) as usize) * 2;
        let swap_type = ((swapword >> 13) & 0x7) as usize;
        let loops = ((swapword >> 7) & 0x3F) as usize;
        offset += jump;

        if swap_type == 0 {
            offset += 256 * loops;
            continue;
        }
        if swap_type > 3 {
            continue;
        }

        let len = swap_len[swap_type];
        for _ in 0..=loops {
            if offset + len <= data.len() {
                data[offset..offset + len].reverse();
            }
            offset += len;
        }
    }
}

/// Decode the AFP string table in-place. Each byte at position i is decoded as
/// `(byte - (128 + i)) & 0xFF`. Returns a map of (offset_within_st -> string).
///
/// NOTE: `core/ap2/mod.rs` carries a DELIBERATE local duplicate of this
/// rolling cipher (`decode_string_table`/`encode_string_table`) — that module
/// must stay std-only with zero `crate::` imports so the
/// validate_s_marvelous.sh harness can mount it standalone. Keep the two in
/// sync.
pub fn decode_stringtable(
    data: &mut [u8],
    st_offset: usize,
    st_size: usize,
) -> Vec<(usize, String)> {
    let mut strings = Vec::new();
    let mut cur_bytes: Vec<u8> = Vec::new();
    let mut cur_start: usize = 0;

    for (i, addition) in (0..st_size).map(|i| (i, 128u32 + i as u32)) {
        let byte = (data[st_offset + i] as u32).wrapping_sub(addition) & 0xFF;
        data[st_offset + i] = byte as u8;

        if byte == 0 {
            if !cur_bytes.is_empty() {
                if let Ok(s) = String::from_utf8(cur_bytes.clone()) {
                    strings.push((cur_start, s));
                }
                cur_bytes.clear();
            }
            cur_start = i + 1;
        } else {
            cur_bytes.push(byte as u8);
        }
    }
    strings
}

/// Encode a plaintext string table back to AFP cipher form.
/// (Mirror-duplicated in `core/ap2/mod.rs::encode_string_table` — see the
/// note on [`decode_stringtable`].)
pub fn encode_stringtable(plain: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; plain.len()];
    for (i, byte) in plain.iter().enumerate() {
        let addition = 128u32 + i as u32;
        out[i] = (*byte as u32).wrapping_add(addition) as u8;
    }
    out
}

/// Round up to 4-byte alignment.
pub fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Patch the exported name in a scrambled AFP binary.
/// Takes raw AFP data + BSI as stored in the IFS (scrambled form).
/// Returns the patched AFP data in scrambled form (ready to serve via LayeredFS).
pub fn patch_exported_name(afp_data: &[u8], bsi_data: &[u8], new_name: &str) -> Option<Vec<u8>> {
    let mut data = afp_data.to_vec();

    // 1. Descramble: BSI byte-swapping
    apply_bsi(&mut data, bsi_data);

    // 2. Parse header to find string table
    if data.len() < 56 {
        return None;
    }
    let name_offset = u16::from_le_bytes(data[10..12].try_into().ok()?) as usize;
    let st_offset = u32::from_le_bytes(data[48..52].try_into().ok()?) as usize;
    let st_size = u32::from_le_bytes(data[52..56].try_into().ok()?) as usize;
    if st_offset + st_size > data.len() || name_offset >= st_size {
        return None;
    }

    // 3. Descramble: string table cipher
    decode_stringtable(&mut data, st_offset, st_size);

    // 4. Read the old exported name
    let name_start = st_offset + name_offset;
    let mut name_end = name_start;
    while name_end < st_offset + st_size && data[name_end] != 0 {
        name_end += 1;
    }
    let old_len = name_end - name_start;

    // 5. Write new name (must fit in old name's space)
    let new_bytes = new_name.as_bytes();
    if new_bytes.len() > old_len {
        return None;
    } // new name too long
    for i in 0..old_len {
        data[name_start + i] = if i < new_bytes.len() { new_bytes[i] } else { 0 };
    }

    // 6. Re-scramble: string table cipher (encode back)
    let plain_st = data[st_offset..st_offset + st_size].to_vec();
    let encoded_st = encode_stringtable(&plain_st);
    data[st_offset..st_offset + st_size].copy_from_slice(&encoded_st);

    // 7. Re-scramble: BSI byte-swapping (self-inverse)
    apply_bsi(&mut data, bsi_data);

    Some(data)
}

/// Read null-terminated strings from an already-decoded (plaintext) string table.
/// Returns a list of (offset_within_st, string).
fn read_plaintext_strings(data: &[u8], st_offset: usize, st_size: usize) -> Vec<(usize, String)> {
    let mut strings = Vec::new();
    let mut cur_bytes: Vec<u8> = Vec::new();
    let mut cur_start: usize = 0;
    for i in 0..st_size {
        let b = data[st_offset + i];
        if b == 0 {
            if !cur_bytes.is_empty() {
                if let Ok(s) = String::from_utf8(cur_bytes.clone()) {
                    strings.push((cur_start, s));
                }
                cur_bytes.clear();
            }
            cur_start = i + 1;
        } else {
            cur_bytes.push(b);
        }
    }
    strings
}

/// Build a 4-byte AFP tag header: `(tag_id << 22) | (data_size & 0x3FFFFF)`.
pub fn make_tag_header(tag_id: u16, data_size: u32) -> [u8; 4] {
    let val = ((tag_id as u32) << 22) | (data_size & 0x3FFFFF);
    val.to_le_bytes()
}

/// Build a minimal DefineSprite tag with 1 blank frame and the given character ID.
/// Uses new-style format (sprite_flags=1) with a relative pointer to subtags,
/// matching the format used by vanilla filter_switch_base templates.
/// A sprite needs at least 1 frame to be instantiable as a MovieClip at runtime.
pub fn make_empty_define_sprite(sprite_id: u16) -> Vec<u8> {
    // Subtags section (24-byte header + 4-byte frame entry = 28 bytes)
    // 1 frame with 0 tags = a blank, instantiable MovieClip
    let mut subtags = Vec::with_capacity(28);
    subtags.extend_from_slice(&0u16.to_le_bytes()); // name_ref_flags
    subtags.extend_from_slice(&0u16.to_le_bytes()); // name_ref_count
    subtags.extend_from_slice(&1u32.to_le_bytes()); // frame_count = 1
    subtags.extend_from_slice(&0u32.to_le_bytes()); // tags_count = 0
    subtags.extend_from_slice(&28u32.to_le_bytes()); // name_ref_offset (past header + frame)
    subtags.extend_from_slice(&24u32.to_le_bytes()); // frame_offset (at end of header)
    subtags.extend_from_slice(&28u32.to_le_bytes()); // tags_offset (past header + frame)
                                                     // Frame 0: start_tag=0, count=0 → packed as (0 << 20) | 0 = 0
    subtags.extend_from_slice(&0u32.to_le_bytes());

    // Sprite header: flags=1 (new-style), sprite_id, relative pointer to subtags
    let mut tag_data = Vec::with_capacity(8 + subtags.len());
    tag_data.extend_from_slice(&1u16.to_le_bytes()); // sprite_flags = 1 (new-style)
    tag_data.extend_from_slice(&sprite_id.to_le_bytes());
    tag_data.extend_from_slice(&8u32.to_le_bytes()); // relative offset to subtags (from tag_data start)
    tag_data.extend_from_slice(&subtags);

    let mut out = Vec::with_capacity(4 + tag_data.len());
    out.extend_from_slice(&make_tag_header(0x79, tag_data.len() as u32));
    out.extend_from_slice(&tag_data);
    out
}

/// Build a minimal AP2_PLACE_OBJECT tag that places a named child.
/// Flags: 0x22 = has source character (0x2) + has instance name (0x20).
pub fn make_place_object(
    depth: u16,
    object_id: u16,
    source_char_id: u16,
    name_st_offset: u16,
) -> Vec<u8> {
    let flags: u32 = 0x00000022;
    let mut tag_data = Vec::with_capacity(12);
    tag_data.extend_from_slice(&flags.to_le_bytes());
    tag_data.extend_from_slice(&depth.to_le_bytes());
    tag_data.extend_from_slice(&object_id.to_le_bytes());
    tag_data.extend_from_slice(&source_char_id.to_le_bytes());
    tag_data.extend_from_slice(&name_st_offset.to_le_bytes());
    // 12 bytes total, already 4-byte aligned

    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&make_tag_header(0x7F, tag_data.len() as u32));
    out.extend_from_slice(&tag_data);
    out
}

/// Describes a named empty MovieClip child to inject into an AFP template.
pub struct ChildDef {
    pub name: &'static str,
    pub depth: u16,
}

/// Patch an AFP binary to inject named empty MovieClip children.
///
/// Takes descrambled AFP data (BSI already applied, string table already decoded
/// is NOT required — this function handles the full pipeline).
///
/// Returns `Some((patched_afp, empty_bsi))` on success.
pub fn patch_inject_children(
    afp_data: &[u8],
    bsi_data: &[u8],
    children: &[ChildDef],
) -> Option<(Vec<u8>, Vec<u8>)> {
    if children.is_empty() {
        return Some((afp_data.to_vec(), bsi_data.to_vec()));
    }

    // 1. The input data is already descrambled (BSI applied, string table decoded).
    // apply_bsi with empty BSI is a no-op, but kept for correctness if non-empty BSI is passed.
    let mut data = afp_data.to_vec();
    apply_bsi(&mut data, bsi_data);

    // 2. Parse header
    let length = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    if length != data.len() {
        return None;
    }
    let st_offset = u32::from_le_bytes(data[48..52].try_into().ok()?) as usize;
    let st_size = u32::from_le_bytes(data[52..56].try_into().ok()?) as usize;
    let tags_offset = u32::from_le_bytes(data[36..40].try_into().ok()?) as usize;

    // 3. Read plaintext strings from the already-decoded string table.
    // Do NOT call decode_stringtable — the data is already plaintext.
    let _strings = read_plaintext_strings(&data, st_offset, st_size);

    // 4. Parse tags section header
    let tags_count =
        u32::from_le_bytes(data[tags_offset + 8..tags_offset + 12].try_into().ok()?) as usize;
    let tags_frame_off =
        u32::from_le_bytes(data[tags_offset + 16..tags_offset + 20].try_into().ok()?) as usize;
    let tags_tags_off =
        u32::from_le_bytes(data[tags_offset + 20..tags_offset + 24].try_into().ok()?) as usize;
    let frame_count =
        u32::from_le_bytes(data[tags_offset + 4..tags_offset + 8].try_into().ok()?) as usize;

    let abs_tags_start = tags_offset + tags_tags_off;
    let abs_frame_off = tags_offset + tags_frame_off;

    // 5. Find max character ID
    let mut max_char_id: u16 = 0;
    let mut pos = abs_tags_start;
    for _ in 0..tags_count {
        let th = u32::from_le_bytes(data[pos..pos + 4].try_into().ok()?);
        let tag_id = ((th >> 22) & 0x3FF) as u16;
        let sz = (th & 0x3FFFFF) as usize;
        if tag_id == 0x79 && sz >= 4 {
            let sid = u16::from_le_bytes(data[pos + 6..pos + 8].try_into().ok()?);
            max_char_id = max_char_id.max(sid);
        } else if tag_id == 0x84 && sz >= 4 {
            let sid = u16::from_le_bytes(data[pos + 4..pos + 6].try_into().ok()?);
            max_char_id = max_char_id.max(sid);
        }
        pos += align4(sz) + 4;
    }

    // 6. Find frame 0 info and tag data end
    let frame0_info = u32::from_le_bytes(data[abs_frame_off..abs_frame_off + 4].try_into().ok()?);
    let frame0_start = (frame0_info & 0xFFFFF) as usize;
    let frame0_count = ((frame0_info >> 20) & 0xFFF) as usize;
    let insert_tag_index = frame0_start + frame0_count;

    // Find byte offset of the insertion point and end of tag data
    pos = abs_tags_start;
    let mut insert_byte_offset = 0usize;
    for i in 0..tags_count {
        if i == insert_tag_index {
            insert_byte_offset = pos;
        }
        let th = u32::from_le_bytes(data[pos..pos + 4].try_into().ok()?);
        let sz = (th & 0x3FFFFF) as usize;
        pos += align4(sz) + 4;
    }
    let tag_data_end = pos;
    if insert_tag_index >= tags_count {
        insert_byte_offset = tag_data_end;
    }

    // 7. Build new strings (4-byte aligned)
    let st_plain = data[st_offset..st_offset + st_size].to_vec();
    let mut st_append = Vec::new();
    let mut new_string_offsets: Vec<u16> = Vec::new();

    for child in children {
        // Check if string already exists
        let existing = _strings.iter().find(|(_, s)| s == child.name);
        if let Some((off, _)) = existing {
            new_string_offsets.push(*off as u16);
        } else {
            let off = st_size + st_append.len();
            new_string_offsets.push(off as u16);
            let encoded = child.name.as_bytes();
            st_append.extend_from_slice(encoded);
            st_append.push(0); // null terminator
                               // Pad to 4-byte alignment
            let pad = (4 - (encoded.len() + 1) % 4) % 4;
            st_append.extend(std::iter::repeat_n(0u8, pad));
        }
    }

    let new_st_size = st_size + st_append.len();
    let new_st_plain: Vec<u8> = [&st_plain[..], &st_append[..]].concat();

    // 8. Build new tags
    let mut define_sprite_bytes = Vec::new();
    let mut child_char_ids = Vec::new();
    for (i, _) in children.iter().enumerate() {
        let char_id = max_char_id + 1 + i as u16;
        child_char_ids.push(char_id);
        define_sprite_bytes.extend(make_empty_define_sprite(char_id));
    }

    let mut place_object_bytes = Vec::new();
    for (i, child) in children.iter().enumerate() {
        let obj_id: u16 = 300 + i as u16;
        place_object_bytes.extend(make_place_object(
            child.depth,
            obj_id,
            child_char_ids[i],
            new_string_offsets[i],
        ));
    }

    let num_new_places = children.len();

    // 9. Splice the data
    // DefineSprite tags MUST come before PlaceObject tags that reference them.
    // Insert both at the frame 0 insertion point: defines first, then placements.
    let before_insert = &data[..insert_byte_offset];
    let between = &data[insert_byte_offset..tag_data_end];
    let between_tags_and_st = &data[tag_data_end..st_offset];
    let after_st = &data[st_offset + st_size..];

    // The data is already descrambled (plaintext string table).
    // Do NOT re-encode — afp_stream_do_create expects descrambled input.
    let new_st = &new_st_plain;

    let mut patched = Vec::with_capacity(
        data.len() + place_object_bytes.len() + define_sprite_bytes.len() + st_append.len(),
    );
    patched.extend_from_slice(before_insert);
    patched.extend_from_slice(&define_sprite_bytes);
    patched.extend_from_slice(&place_object_bytes);
    patched.extend_from_slice(between);
    patched.extend_from_slice(between_tags_and_st);
    patched.extend_from_slice(new_st);
    patched.extend_from_slice(after_st);

    // 10. Update header fields
    let num_new_defines = children.len();
    let num_new_tags = num_new_defines + num_new_places;
    let size_delta_tags = define_sprite_bytes.len() + place_object_bytes.len();
    let size_delta_st = new_st_size - st_size;
    let new_length = (length + size_delta_tags + size_delta_st) as u32;
    patched[4..8].copy_from_slice(&new_length.to_le_bytes());

    // String table offset and size
    let new_st_offset = (st_offset + size_delta_tags) as u32;
    patched[48..52].copy_from_slice(&new_st_offset.to_le_bytes());
    patched[52..56].copy_from_slice(&(new_st_size as u32).to_le_bytes());

    // Tags count
    let new_tags_count = (tags_count + num_new_tags) as u32;
    patched[tags_offset + 8..tags_offset + 12].copy_from_slice(&new_tags_count.to_le_bytes());

    // Frame 0: increase count to include both DefineSprites and PlaceObjects
    let new_frame0_info = ((frame0_count + num_new_tags) as u32) << 20 | frame0_start as u32;
    patched[abs_frame_off..abs_frame_off + 4].copy_from_slice(&new_frame0_info.to_le_bytes());

    // Shift subsequent frames
    for f in 1..frame_count {
        let foff = abs_frame_off + f * 4;
        let fi = u32::from_le_bytes(patched[foff..foff + 4].try_into().ok()?);
        let f_start = (fi & 0xFFFFF) as usize;
        let f_count = (fi >> 20) & 0xFFF;
        if f_start >= insert_tag_index {
            let new_fi = (f_count << 20) | (f_start + num_new_tags) as u32;
            patched[foff..foff + 4].copy_from_slice(&new_fi.to_le_bytes());
        }
    }

    // Verify size
    if patched.len() != new_length as usize {
        return None;
    }

    // Empty BSI (no scrambling needed — data is in LE form)
    let new_bsi = vec![0u8, 0u8];

    Some((patched, new_bsi))
}
