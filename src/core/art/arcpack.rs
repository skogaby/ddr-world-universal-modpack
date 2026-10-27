//! Uncompressed ARC containers for art — pure (`std` only), mounted by the
//! prep tool and the harness. The jacket arc (one PNG member) and the series
//! thumbnail arc (one DDS member per song) are the two containers the game
//! opens for injected songs; both are written uncompressed (an LZ77-packed
//! PNG member renders as a flat coloured square — cabinet run 2026-10-21),
//! sorted by member name with 64-byte alignment, mirroring the game's own
//! packer. `core::arc::ArcArchive::to_bytes` delegates here.

use std::collections::BTreeMap;

pub const ARC_MAGIC: u32 = 0x19751120;
pub const ARC_VERSION: u32 = 1;
pub const ARC_COMPRESSION_NONE: u32 = 0;
pub const ARC_ALIGN: u32 = 64;

/// Member path of a jacket PNG inside its arc.
#[must_use]
pub fn jacket_member(basename: &str) -> String {
    format!("data/jacket/{basename}_jk.png")
}

/// Member path of a thumbnail DDS inside the series arc.
#[must_use]
pub fn thumbnail_member(basename: &str) -> String {
    format!("data/jacket/thumbnail/{basename}_tn.dds")
}

/// `arc/jacket/<b>_jk.arc` bytes for an encoded PNG.
#[must_use]
pub fn jacket_arc(basename: &str, png: Vec<u8>) -> Vec<u8> {
    let mut files = BTreeMap::new();
    files.insert(jacket_member(basename), png);
    write_uncompressed(&files)
}

/// A series thumbnail arc from `(basename, dds bytes)` members.
#[must_use]
pub fn thumbnail_arc<I>(members: I) -> Vec<u8>
where
    I: IntoIterator<Item = (String, Vec<u8>)>,
{
    let files: BTreeMap<String, Vec<u8>> = members
        .into_iter()
        .map(|(b, dds)| (thumbnail_member(&b), dds))
        .collect();
    write_uncompressed(&files)
}

fn align_up(v: u32, align: u32) -> u32 {
    (v + align - 1) & !(align - 1)
}

/// Serialise name-sorted members uncompressed: magic, version, count,
/// compression 0; a 16-byte cue per member (string offset, data offset,
/// unpacked size, packed size); the string table; 64-byte-aligned data.
#[must_use]
pub fn write_uncompressed(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let file_count = files.len() as u32;
    let cue_start = 16u32;
    let str_table_offset = cue_start + file_count * 16;

    let mut str_offsets: Vec<u32> = Vec::with_capacity(files.len());
    let mut str_cursor = str_table_offset;
    for name in files.keys() {
        str_offsets.push(str_cursor);
        str_cursor += name.len() as u32 + 1;
    }

    let data_start = align_up(str_cursor, ARC_ALIGN);
    let mut file_offsets: Vec<u32> = Vec::with_capacity(files.len());
    let mut data_cursor = data_start;
    for data in files.values() {
        file_offsets.push(data_cursor);
        data_cursor = align_up(data_cursor + data.len() as u32, ARC_ALIGN);
    }

    let mut out = Vec::with_capacity(data_cursor as usize);
    out.extend_from_slice(&ARC_MAGIC.to_le_bytes());
    out.extend_from_slice(&ARC_VERSION.to_le_bytes());
    out.extend_from_slice(&file_count.to_le_bytes());
    out.extend_from_slice(&ARC_COMPRESSION_NONE.to_le_bytes());

    for (i, data) in files.values().enumerate() {
        let size = data.len() as u32;
        out.extend_from_slice(&str_offsets[i].to_le_bytes());
        out.extend_from_slice(&file_offsets[i].to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes()); // unpacked
        out.extend_from_slice(&size.to_le_bytes()); // packed (== unpacked)
    }

    for name in files.keys() {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
    }
    out.resize(data_start as usize, 0);

    for data in files.values() {
        out.extend_from_slice(data);
        let padded = align_up(data.len() as u32, ARC_ALIGN) as usize;
        out.resize(out.len() + (padded - data.len()), 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    #[test]
    fn layout_is_the_game_packers() {
        let arc = thumbnail_arc(vec![
            ("zb".to_string(), vec![2u8; 70]),
            ("za".to_string(), vec![1u8; 10]),
        ]);
        assert_eq!(u32_at(&arc, 0), ARC_MAGIC);
        assert_eq!(u32_at(&arc, 4), 1);
        assert_eq!(u32_at(&arc, 8), 2);
        assert_eq!(u32_at(&arc, 12), 0);
        // Members sorted by name: za first.
        let str0 = u32_at(&arc, 16) as usize;
        let name0 = &arc[str0..str0 + thumbnail_member("za").len()];
        assert_eq!(name0, thumbnail_member("za").as_bytes());
        let data0 = u32_at(&arc, 20) as usize;
        assert_eq!(data0 % 64, 0);
        assert_eq!(&arc[data0..data0 + 10], &[1u8; 10]);
        let data1 = u32_at(&arc, 36) as usize;
        assert_eq!(data1, data0 + 64);
        assert_eq!(u32_at(&arc, 40), 70);
        assert_eq!(arc.len(), data1 + 128);
    }

    #[test]
    fn jacket_arc_has_one_png_member() {
        let arc = jacket_arc("zabcde", b"\x89PNG".to_vec());
        assert_eq!(u32_at(&arc, 8), 1);
        let s = u32_at(&arc, 16) as usize;
        assert!(arc[s..].starts_with(b"data/jacket/zabcde_jk.png\0"));
    }
}
