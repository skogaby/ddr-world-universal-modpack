//! IFS container parser — extracts files from Konami's IFS archive format.
//!
//! IFS files contain a kbin-encoded manifest describing the file tree, followed
//! by a data blob. Files within MD5-hashed folders (geo, afp, tex) are stored
//! with their MD5 hash as the filename; the manifest maps names to offsets.
//!
//! Reference: ifstools (https://github.com/mon/ifstools)

use crate::log_info;
use crate::log_warn;
use crate::services::avs_layeredfs::kbin;

const IFS_SIGNATURE: u32 = 0x6CAD8F89;

/// A parsed IFS file entry with its offset and size in the data blob.
struct IfsFileEntry {
    start: usize,
    size: usize,
}

/// Extract specific files from an IFS archive by MD5-hashed name.
///
/// `names` are the human-readable filenames (e.g. "folder_firststep_shape41").
/// `section` is the IFS section to search in (e.g. "geo", "tex", "afp").
/// Files are matched by computing MD5 of the name and finding the corresponding
/// entry in the manifest.
///
/// Returns a vec of (name, data) pairs for each file found.
pub fn extract_files(ifs_data: &[u8], section: &str, names: &[String]) -> Vec<(String, Vec<u8>)> {
    let (xml, data_offset) = match parse_manifest(ifs_data) {
        Some(v) => v,
        None => return Vec::new(),
    };

    let mut md5_to_name: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for name in names {
        let hash = format!("{:x}", md5::compute(name.as_bytes()));
        md5_to_name.insert(hash, name.clone());
    }

    let section_xml = match find_section(&xml, section) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let entries = parse_section_entries_from(&section_xml);
    log_info!(
        "IFS: section '{}' content length={}, parsed {} entries",
        section,
        section_xml.len(),
        entries.len()
    );

    let mut results = Vec::new();
    for (tag, entry) in &entries {
        if let Some(name) = md5_to_name.get(tag) {
            let start = data_offset + entry.start;
            let end = start + entry.size;
            if end <= ifs_data.len() {
                results.push((name.clone(), ifs_data[start..end].to_vec()));
                log_info!("IFS: extracted {} ({} bytes)", name, entry.size);
            }
        }
    }

    if results.len() < names.len() {
        log_warn!(
            "IFS: found {}/{} requested files in '{}'",
            results.len(),
            names.len(),
            section
        );
    }
    results
}

/// Extract a file from an IFS by its plain name (not MD5 hashed).
/// Used for files like texturelist.xml that are stored by name in the manifest.
pub fn extract_file_by_name(ifs_data: &[u8], section: &str, name: &str) -> Option<Vec<u8>> {
    let (xml, data_offset) = parse_manifest(ifs_data)?;
    let section_xml = find_section(&xml, section)?;
    let entries = parse_section_entries_from(&section_xml);

    for (tag, entry) in &entries {
        if tag == name {
            let start = data_offset + entry.start;
            let end = start + entry.size;
            if end <= ifs_data.len() {
                return Some(ifs_data[start..end].to_vec());
            }
        }
    }
    None
}

// ── Internal helpers ─────────────────────────────────────────────────

struct IfsHeader {
    manifest_start: usize,
    manifest_end: usize,
}

/// Parse IFS header and decode kbin manifest. Returns (xml_string, data_blob_offset).
fn parse_manifest(ifs_data: &[u8]) -> Option<(String, usize)> {
    let header = parse_header(ifs_data)?;
    let manifest_data = &ifs_data[header.manifest_start..header.manifest_end];
    let xml = match kbin::reader::decode_to_string(manifest_data) {
        Ok(x) => x,
        Err(e) => {
            log_warn!("IFS: kbin decode failed: {}", e);
            return None;
        }
    };
    Some((xml, header.manifest_end))
}

fn parse_header(data: &[u8]) -> Option<IfsHeader> {
    if data.len() < 36 {
        return None;
    }
    let sig = u32::from_be_bytes(data[0..4].try_into().ok()?);
    if sig != IFS_SIGNATURE {
        log_warn!("IFS: bad signature 0x{:08X}", sig);
        return None;
    }
    let version = u16::from_be_bytes(data[4..6].try_into().ok()?);
    // Offset 12 = tree_size (kbin memory size), offset 16 = manifest_end (data blob start)
    let manifest_end = u32::from_be_bytes(data[16..20].try_into().ok()?) as usize;
    let manifest_start = if version > 1 { 36 } else { 20 };

    if manifest_end > data.len() || manifest_start >= manifest_end {
        log_warn!(
            "IFS: manifest bounds invalid (start={}, end={}, len={})",
            manifest_start,
            manifest_end,
            data.len()
        );
        return None;
    }
    Some(IfsHeader {
        manifest_start,
        manifest_end,
    })
}

/// Navigate to a section in the XML, supporting slash-separated paths.
fn find_section(xml: &str, section: &str) -> Option<String> {
    let mut xml_slice = xml;
    for part in section.split('/') {
        let (start, end) = find_tag_content(xml_slice, part)?;
        xml_slice = &xml_slice[start..end];
    }
    Some(xml_slice.to_string())
}

/// Parse file entries from a pre-sliced section of the IFS manifest XML.
fn parse_section_entries_from(section_xml: &str) -> Vec<(String, IfsFileEntry)> {
    let mut results = Vec::new();
    let mut pos = 0;

    while pos < section_xml.len() {
        let tag_start = match section_xml[pos..].find('<') {
            Some(p) => pos + p,
            None => break,
        };

        if section_xml[tag_start..].starts_with("</") {
            break;
        }

        let tag_end = match section_xml[tag_start..].find('>') {
            Some(p) => tag_start + p,
            None => break,
        };
        let tag = &section_xml[tag_start + 1..tag_end];
        let is_file_entry = tag.contains("__type=\"3s32\"") || tag.contains("__type=\"2s32\"");

        if is_file_entry {
            let raw_tag = tag.split_whitespace().next().unwrap_or("");
            let tag_name = unescape_kbin_name(raw_tag);
            let content_start = tag_end + 1;
            let close_tag = format!("</{}>", raw_tag);
            if let Some(close_pos) = section_xml[content_start..].find(&close_tag) {
                let text = section_xml[content_start..content_start + close_pos].trim();
                let nums: Vec<usize> = text
                    .split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if nums.len() >= 2 {
                    results.push((
                        tag_name,
                        IfsFileEntry {
                            start: nums[0],
                            size: nums[1],
                        },
                    ));
                }
                pos = content_start + close_pos + close_tag.len();
            } else {
                pos = tag_end + 1;
            }
        } else {
            let tag_name = tag.split_whitespace().next().unwrap_or("");
            let close_tag = format!("</{}>", tag_name);
            if let Some(close_pos) = section_xml[tag_end..].find(&close_tag) {
                pos = tag_end + close_pos + close_tag.len();
            } else {
                pos = tag_end + 1;
            }
        }
    }
    results
}

fn find_tag_content(xml: &str, tag_name: &str) -> Option<(usize, usize)> {
    let open = format!("<{}", tag_name);
    let open_pos = xml.find(&open)?;
    let content_start = xml[open_pos..].find('>')? + open_pos + 1;
    let close = format!("</{}>", tag_name);
    let content_end = xml[content_start..].find(&close)? + content_start;
    Some((content_start, content_end))
}

/// Unescape kbin XML tag name escaping:
/// - Leading `_` before a hex digit is stripped (kbin prepends `_` when name starts with 0-9)
/// - `_E` → `.`, `__` → `_` (standard kbin escapes)
fn unescape_kbin_name(name: &str) -> String {
    let mut s = name.to_string();
    if s.len() >= 2 && s.starts_with('_') && s.as_bytes()[1].is_ascii_hexdigit() {
        s = s[1..].to_string();
    }
    s = s.replace("_E", ".");
    s = s.replace("__", "_");
    s
}
