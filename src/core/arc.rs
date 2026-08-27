//! ARC archive parser/writer — reads and writes Konami's ARC container format.
//!
//! ARC files contain one or more files (typically IFS archives), optionally
//! compressed with Konami LZ77. Format:
//!   - Magic: 0x19751120
//!   - Header: version (u32), file_count (u32), compression (u32)
//!   - Cue table: file_count × 16-byte entries (str_offset, file_offset, unpacked, packed)
//!   - String table + file data
//!
//! The `parse`/`extract` API is the read-only fast path used by atlas_cloner
//! and folder_expansion to pull a single inner IFS out of a stock arc without
//! materializing the whole archive. The `ArcArchive` API is the read+modify+write
//! path used by the LayeredFS arc handler when overlaying mod files onto a
//! game arc — it owns every entry's data, sorts by name (to mirror upstream's
//! std::map iteration order), and writes back uncompressed with 64-byte alignment.

use std::collections::BTreeMap;

use crate::log_warn;
use crate::services::avs_layeredfs::avslz;

const ARC_MAGIC: u32 = 0x19751120;
const ARC_VERSION: u32 = 1;
const ARC_COMPRESSION_NONE: u32 = 0;
const ARC_COMPRESSION_AVSLZ: u32 = 2;
const ARC_ALIGN: u32 = 64;

pub struct ArcEntry {
    pub path: String,
    pub data_offset: u32,
    pub decompressed_size: u32,
    pub compressed_size: u32,
}

/// Parse an ARC file and return its entries.
pub fn parse(data: &[u8]) -> Option<Vec<ArcEntry>> {
    if data.len() < 16 {
        return None;
    }
    let magic = u32::from_le_bytes(data[0..4].try_into().ok()?);
    if magic != ARC_MAGIC {
        log_warn!("ARC: bad magic 0x{:08X}", magic);
        return None;
    }
    let file_count = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
    let cue_start = 16usize;
    let cue_end = cue_start + file_count * 16;
    if data.len() < cue_end {
        return None;
    }

    let mut entries = Vec::with_capacity(file_count);
    for i in 0..file_count {
        let off = cue_start + i * 16;
        let path_offset = u32::from_le_bytes(data[off..off + 4].try_into().ok()?) as usize;
        let data_offset = u32::from_le_bytes(data[off + 4..off + 8].try_into().ok()?);
        let decompressed_size = u32::from_le_bytes(data[off + 8..off + 12].try_into().ok()?);
        let compressed_size = u32::from_le_bytes(data[off + 12..off + 16].try_into().ok()?);

        // A truncated/corrupt arc can carry a cue whose string offset points
        // past the file; slicing below would panic (even for the empty range,
        // start > len is out of bounds). This parser is fed raw disk bytes on
        // paths that reach extern "C" frames, so it must never panic.
        if path_offset > data.len() {
            log_warn!(
                "ARC: cue entry {} path offset {} out of range ({} bytes) — corrupt arc",
                i,
                path_offset,
                data.len()
            );
            return None;
        }

        // Read null-terminated path string
        let mut end = path_offset;
        while end < data.len() && data[end] != 0 {
            end += 1;
        }
        let path = String::from_utf8_lossy(&data[path_offset..end]).into_owned();

        entries.push(ArcEntry {
            path,
            data_offset,
            decompressed_size,
            compressed_size,
        });
    }
    Some(entries)
}

/// Extract a single entry's data, decompressing if needed.
pub fn extract(data: &[u8], entry: &ArcEntry) -> Option<Vec<u8>> {
    let start = entry.data_offset as usize;
    let end = start + entry.compressed_size as usize;
    if end > data.len() {
        return None;
    }
    let raw = &data[start..end];

    if entry.compressed_size == entry.decompressed_size {
        Some(raw.to_vec())
    } else {
        avslz::decompress(raw)
    }
}

/// Read+modify+write ARC archive. Owns all entry data after `from_bytes` (decompressed).
/// Iteration order is stable (BTreeMap → name-sorted), matching upstream's std::map.
pub struct ArcArchive {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl ArcArchive {
    /// Build an empty archive — used when overlaying mods onto a `.arc` that
    /// doesn't exist in the base game.
    pub fn empty() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }

    /// Parse an arc file and load all entries (decompressing as needed) into memory.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            log_warn!("arc: header too short");
            return None;
        }
        let magic = u32::from_le_bytes(data[0..4].try_into().ok()?);
        if magic != ARC_MAGIC {
            log_warn!("arc: bad magic 0x{:08X}", magic);
            return None;
        }
        let compression = u32::from_le_bytes(data[12..16].try_into().ok()?);
        if compression != ARC_COMPRESSION_NONE && compression != ARC_COMPRESSION_AVSLZ {
            log_warn!("arc: unknown compression {}", compression);
            return None;
        }

        let entries = parse(data)?;
        let mut files = BTreeMap::new();
        for entry in &entries {
            let bytes = extract(data, entry).or_else(|| {
                log_warn!("arc: couldn't extract '{}'", entry.path);
                None
            })?;
            files.insert(entry.path.clone(), bytes);
        }

        Some(Self { files })
    }

    /// Insert or replace an entry by name.
    pub fn add_or_replace(&mut self, name: impl Into<String>, data: Vec<u8>) {
        self.files.insert(name.into(), data);
    }

    /// Serialize to bytes. Writes uncompressed (compression=0), with the
    /// game-required 64-byte alignment between sections and entries.
    pub fn to_bytes(&self) -> Vec<u8> {
        let file_count = self.files.len() as u32;
        let cue_start = 16u32;
        let str_table_offset = cue_start + file_count * 16;

        let mut str_offsets: Vec<u32> = Vec::with_capacity(self.files.len());
        let mut str_cursor = str_table_offset;
        for name in self.files.keys() {
            str_offsets.push(str_cursor);
            str_cursor += name.len() as u32 + 1;
        }

        let data_start = align_up(str_cursor, ARC_ALIGN);
        let mut file_offsets: Vec<u32> = Vec::with_capacity(self.files.len());
        let mut data_cursor = data_start;
        for data in self.files.values() {
            file_offsets.push(data_cursor);
            data_cursor = align_up(data_cursor + data.len() as u32, ARC_ALIGN);
        }

        let mut out = Vec::with_capacity(data_cursor as usize);
        out.extend_from_slice(&ARC_MAGIC.to_le_bytes());
        out.extend_from_slice(&ARC_VERSION.to_le_bytes());
        out.extend_from_slice(&file_count.to_le_bytes());
        out.extend_from_slice(&ARC_COMPRESSION_NONE.to_le_bytes());

        for (i, data) in self.files.values().enumerate() {
            let size = data.len() as u32;
            out.extend_from_slice(&str_offsets[i].to_le_bytes());
            out.extend_from_slice(&file_offsets[i].to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes()); // unpacked
            out.extend_from_slice(&size.to_le_bytes()); // packed (== unpacked since uncompressed)
        }

        for name in self.files.keys() {
            out.extend_from_slice(name.as_bytes());
            out.push(0);
        }

        // Pad to data_start
        while out.len() < data_start as usize {
            out.push(0);
        }

        for data in self.files.values() {
            out.extend_from_slice(data);
            let pad = align_up(data.len() as u32, ARC_ALIGN) as usize - data.len();
            for _ in 0..pad {
                out.push(0);
            }
        }

        out
    }

    /// Save to disk.
    pub fn save(&self, path: &str) -> bool {
        let bytes = self.to_bytes();
        match std::fs::write(path, &bytes) {
            Ok(()) => true,
            Err(e) => {
                log_warn!("arc: couldn't write '{}': {}", path, e);
                false
            }
        }
    }
}

/// Rewrite entry PATHS without touching payload bytes. The payload ranges are
/// copied raw — compressed entries stay compressed (no decompress/re-encode),
/// and the header's compression flag + per-entry packed/unpacked sizes are
/// preserved. Only the cue/string tables are rebuilt (the string table can
/// grow/shrink, so data offsets are re-laid-out with the same 64-byte
/// alignment the game uses).
///
/// `rename` is called with each entry's path; `Some(new)` renames it, `None`
/// keeps it. Returns `None` on a corrupt arc (bad magic / out-of-range cue /
/// non-UTF-8 path / total size overflowing [`REWRITE_MAX_OUT`]).
///
/// Used by the background-preview alias cache: the BM2D data manager locates
/// an arc's inner IFS by the FNV-1a hash of `<arc_name>.ifs`, so an arc copied
/// under an alias name MUST have its inner IFS renamed to match — a stale
/// original name makes the manager's per-IFS object lookup return null, and
/// `Manager::Update` writes through it unchecked (cabinet crash, 2026-07-09).
pub fn rewrite_paths(data: &[u8], rename: impl Fn(&str) -> Option<String>) -> Option<Vec<u8>> {
    /// Output-size cap: per-entry payload ranges are bounds-checked against
    /// the input, but a corrupt cue table with many overlapping ranges could
    /// still sum far past the input size — cap the re-laid-out total instead
    /// of letting the u32 cursor wrap (debug panic / release garbage).
    const REWRITE_MAX_OUT: u64 = 256 * 1024 * 1024;

    if data.len() < 16 {
        return None;
    }
    let version = u32::from_le_bytes(data[4..8].try_into().ok()?);
    let compression = u32::from_le_bytes(data[12..16].try_into().ok()?);
    let entries = parse(data)?; // validates magic + cue bounds

    // Validate every payload range up front (parse checks only path offsets),
    // and reject paths `parse` lossy-decoded (a U+FFFD in the re-emitted
    // string table would silently alter a "kept" name and any name-derived
    // hash — the game's paths are ASCII, so this only fires on corruption).
    for e in &entries {
        let start = e.data_offset as usize;
        let end = start.checked_add(e.compressed_size as usize)?;
        if end > data.len() {
            log_warn!("arc: entry '{}' payload out of range — corrupt arc", e.path);
            return None;
        }
        if e.path.contains('\u{FFFD}') {
            log_warn!("arc: entry path is not valid UTF-8 — refusing to rewrite");
            return None;
        }
    }

    let new_paths: Vec<String> = entries
        .iter()
        .map(|e| rename(&e.path).unwrap_or_else(|| e.path.clone()))
        .collect();

    // Lay out the new tables with u64 cursors, then cap — no wrap possible.
    let file_count = entries.len() as u32;
    let cue_start = 16u64;
    let str_table_offset = cue_start + file_count as u64 * 16;

    let mut str_offsets: Vec<u64> = Vec::with_capacity(new_paths.len());
    let mut str_cursor = str_table_offset;
    for path in &new_paths {
        str_offsets.push(str_cursor);
        str_cursor += path.len() as u64 + 1;
    }

    let data_start = align_up_u64(str_cursor, ARC_ALIGN as u64);
    let mut file_offsets: Vec<u64> = Vec::with_capacity(entries.len());
    let mut data_cursor = data_start;
    for e in &entries {
        file_offsets.push(data_cursor);
        data_cursor = align_up_u64(data_cursor + e.compressed_size as u64, ARC_ALIGN as u64);
    }
    if data_cursor > REWRITE_MAX_OUT {
        log_warn!(
            "arc: rewritten size {} exceeds cap — corrupt cue table? refusing",
            data_cursor
        );
        return None;
    }

    let mut out = Vec::with_capacity(data_cursor as usize);
    out.extend_from_slice(&ARC_MAGIC.to_le_bytes());
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&file_count.to_le_bytes());
    out.extend_from_slice(&compression.to_le_bytes());

    for (i, e) in entries.iter().enumerate() {
        out.extend_from_slice(&(str_offsets[i] as u32).to_le_bytes());
        out.extend_from_slice(&(file_offsets[i] as u32).to_le_bytes());
        out.extend_from_slice(&e.decompressed_size.to_le_bytes());
        out.extend_from_slice(&e.compressed_size.to_le_bytes());
    }

    for path in &new_paths {
        out.extend_from_slice(path.as_bytes());
        out.push(0);
    }
    while out.len() < data_start as usize {
        out.push(0);
    }

    for e in &entries {
        let start = e.data_offset as usize;
        let end = start + e.compressed_size as usize; // bounds-checked above
        out.extend_from_slice(&data[start..end]);
        let pad = align_up_u64(e.compressed_size as u64, ARC_ALIGN as u64) as usize
            - e.compressed_size as usize;
        out.resize(out.len() + pad, 0);
    }

    Some(out)
}

fn align_up_u64(v: u64, align: u64) -> u64 {
    (v + align - 1) & !(align - 1)
}

fn align_up(v: u32, align: u32) -> u32 {
    (v + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_two_files() {
        let mut arc = ArcArchive::empty();
        arc.add_or_replace("hello.txt", b"hello world".to_vec());
        arc.add_or_replace("nested/path/data.bin", vec![1, 2, 3, 4, 5]);

        let bytes = arc.to_bytes();
        let parsed = ArcArchive::from_bytes(&bytes).expect("roundtrip parse");

        assert_eq!(parsed.files.len(), 2);
        assert_eq!(
            parsed.files.get("hello.txt").map(|v| v.as_slice()),
            Some(&b"hello world"[..])
        );
        assert_eq!(
            parsed
                .files
                .get("nested/path/data.bin")
                .map(|v| v.as_slice()),
            Some(&[1u8, 2, 3, 4, 5][..])
        );
    }

    #[test]
    fn roundtrip_empty() {
        let arc = ArcArchive::empty();
        let bytes = arc.to_bytes();
        let parsed = ArcArchive::from_bytes(&bytes).expect("roundtrip parse");
        assert!(parsed.files.is_empty());
    }

    #[test]
    fn replaces_existing_entry() {
        let mut arc = ArcArchive::empty();
        arc.add_or_replace("a.bin", vec![1, 2, 3]);
        arc.add_or_replace("a.bin", vec![9, 9]);
        assert_eq!(
            arc.files.get("a.bin").map(|v| v.as_slice()),
            Some(&[9u8, 9][..])
        );
    }
}
