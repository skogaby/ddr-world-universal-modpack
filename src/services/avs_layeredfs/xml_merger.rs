//! XML Merging — `.merged.xml` support for appending content to game XML files.
//!
//! Multiple mods can append nodes to the same game XML (e.g., musicdb.xml)
//! without replacing the entire file. Merged output is cached with hash-based
//! invalidation.

use std::io::Write;

use crate::{log_info, log_warn};

use super::avs_resolver::*;
use super::cache_hasher::{CacheHasher, CACHE_FOLDER};
use super::mod_paths;

// ── Public API ───────────────────────────────────────────────────────

/// Check for .merged.xml files and merge them into the original XML.
/// Returns Some(cached_path) if merging occurred, None otherwise.
pub fn merge_xmls(norm_path: &str, original_path: &str) -> Option<String> {
    // Build the merged.xml search path
    let merge_search = norm_path.replace(".xml", ".merged.xml");
    let mut to_merge = mod_paths::find_all_modfile(&merge_search);

    // Also try with IFS expansion
    if to_merge.is_empty() {
        let ifs_merge = merge_search.replace(".ifs", "_ifs");
        to_merge = mod_paths::find_all_modfile(&ifs_merge);
    }

    if to_merge.is_empty() {
        return None;
    }

    let out = format!("{}/{}", CACHE_FOLDER, norm_path);

    // Cache check
    let hash_file = format!("{}.hashed", out);
    let mut hasher = CacheHasher::new(&hash_file);
    hasher.add(original_path);
    for path in &to_merge {
        hasher.add(path);
    }
    hasher.finish();

    if hasher.matches() {
        log_info!("LayeredFS: merged XML cache up to date for {}", norm_path);
        return Some(out);
    }

    log_info!("LayeredFS: merging XML for {}", norm_path);
    for path in &to_merge {
        log_info!("LayeredFS:   + {}", path);
    }

    // Load original XML
    let original_xml = match load_xml_from_avs_path(original_path) {
        Some(xml) => xml,
        None => {
            log_warn!("LayeredFS: can't load original XML {}", original_path);
            return None;
        }
    };

    // Merge: append child nodes from each merged file into original's last root node
    let mut merged = original_xml;
    for path in &to_merge {
        let merge_xml = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => {
                log_warn!("LayeredFS: can't read merge file {}", path);
                continue;
            }
        };
        merged = append_xml_children(&merged, &merge_xml);
    }

    // Write to cache
    let out_folder = out.rsplit_once('/').map(|(f, _)| f).unwrap_or(".");
    mod_paths::mkdir_p(out_folder);

    match std::fs::File::create(&out) {
        Ok(mut f) => {
            let _ = f.write_all(merged.as_bytes());
        }
        Err(e) => {
            log_warn!("LayeredFS: can't write merged XML cache: {}", e);
            return None;
        }
    }

    hasher.commit();
    Some(out)
}

// ── XML manipulation ─────────────────────────────────────────────────

/// Append all child nodes from `merge_xml` into the last root element of `base_xml`.
/// Uses string-level manipulation to avoid full XML DOM overhead.
fn append_xml_children(base_xml: &str, merge_xml: &str) -> String {
    // Find the last closing tag in the base XML (e.g., </mdb>)
    // We insert the merge content just before it
    let trimmed = base_xml.trim_end();

    // Find the last closing tag
    if let Some(last_close_pos) = trimmed.rfind("</") {
        // Extract the merge content: everything between the root open and close tags
        let merge_content = extract_root_children(merge_xml);

        let mut result = String::with_capacity(base_xml.len() + merge_content.len());
        result.push_str(&trimmed[..last_close_pos]);
        result.push_str(&merge_content);
        result.push('\n');
        result.push_str(&trimmed[last_close_pos..]);
        result
    } else {
        // Can't find closing tag, return base unchanged
        base_xml.to_string()
    }
}

/// Extract the children of the last root element from an XML string.
fn extract_root_children(xml: &str) -> String {
    let trimmed = xml.trim();

    // Skip XML declaration if present
    let body = if trimmed.starts_with("<?") {
        trimmed
            .find("?>")
            .map(|i| &trimmed[i + 2..])
            .unwrap_or(trimmed)
    } else {
        trimmed
    };
    let body = body.trim();

    // Find the first opening tag's close (end of <root ...>)
    let first_close = match body.find('>') {
        Some(i) => i + 1,
        None => return String::new(),
    };

    // Find the last closing tag
    let last_open = match body.rfind("</") {
        Some(i) => i,
        None => return String::new(),
    };

    if last_open <= first_close {
        return String::new();
    }

    body[first_close..last_open].to_string()
}

// ── AVS XML loading ──────────────────────────────────────────────────

/// Load an arbitrary file through AVS into a byte vector. Uses trampolines
/// to bypass our own hooks and avoid recursion.
pub fn load_bytes_from_avs_path(path: &str) -> Option<Vec<u8>> {
    use super::file_hooks::{orig_fs_close, orig_fs_fstat, orig_fs_open, orig_fs_read};

    let cpath = std::ffi::CString::new(path).ok()?;
    let mode = avs_open_mode_read(super::avs_version());

    let handle = unsafe { orig_fs_open(cpath.as_ptr(), mode, 420) };
    if handle < 0 {
        return None;
    }

    let mut stat = AvsStat::default();
    unsafe { orig_fs_fstat(handle, &mut stat) };
    let size = stat.filesize as usize;
    if size == 0 {
        unsafe { orig_fs_close(handle) };
        return None;
    }

    let mut buf = vec![0u8; size];
    unsafe { orig_fs_read(handle, buf.as_mut_ptr(), size) };
    unsafe { orig_fs_close(handle) };

    Some(buf)
}

/// Load an XML file through AVS, handling binary property format.
/// Uses trampoline functions to bypass our hooks and avoid recursion.
/// Binary kbin format is decoded natively (no AVS property API needed).
pub fn load_xml_from_avs_path(path: &str) -> Option<String> {
    let buf = match load_bytes_from_avs_path(path) {
        Some(b) => b,
        None => {
            log_warn!("LayeredFS: load_xml: read failed for {}", path);
            return None;
        }
    };

    if buf.first() == Some(&0xA0) {
        // Binary kbin format — decode natively
        match super::kbin::reader::decode_to_string(&buf) {
            Ok(xml) => Some(xml),
            Err(e) => {
                log_warn!("LayeredFS: kbin decode failed for {}: {}", path, e);
                None
            }
        }
    } else {
        // Plain text XML
        String::from_utf8(buf).ok()
    }
}
