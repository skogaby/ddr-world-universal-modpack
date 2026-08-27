//! AOB Pattern Scanner — Scans memory for byte patterns with wildcard support.
//!
//! Pattern format: "48 89 5C 24 ? 48 89 74 24 ? 57"
//! '?' matches any single byte.
//!
//! ## Multi-pattern fast path
//!
//! For workloads that resolve many patterns at once (notably
//! `signatures::resolve_all`), use [`scan_patterns_batch`]. It runs a single
//! pass over the module bytes using an Aho-Corasick prefilter on each
//! pattern's longest contiguous wildcard-free literal run, then verifies
//! candidate hits against the full pattern (including wildcards). For ~50
//! patterns over a 50MB module, this is ~10x faster than calling
//! [`scan_pattern`] in a loop.

use std::collections::HashMap;

use crate::log_warn;

pub struct ScanResult {
    pub address: *const u8,
    pub offset: usize,
}

/// Decode a RIP-relative 32-bit displacement. `disp_addr` must point to the
/// 4 displacement bytes within a RIP-relative instruction (e.g., the byte
/// right after the 0xE8 of `CALL rel32`, or the 4th byte of `48 8D 05 xx xx xx xx`).
/// Returns the absolute target: `disp_addr + 4 + (i32)*disp_addr`.
///
/// # Safety
/// `disp_addr..disp_addr+4` must be readable.
pub unsafe fn decode_rip_relative(disp_addr: *const u8) -> *const u8 {
    let disp = (disp_addr as *const i32).read_unaligned();
    disp_addr.add(4).offset(disp as isize)
}

/// Decode a `CALL rel32` instruction at `call_addr` (expects opcode 0xE8) and
/// return the absolute target address.
///
/// # Safety
/// `call_addr` must point to at least 5 readable bytes forming a valid
/// `E8 xx xx xx xx` instruction encoding.
pub unsafe fn decode_call_rel32(call_addr: *const u8) -> *const u8 {
    decode_rip_relative(call_addr.add(1))
}

/// Scan `len` bytes from `start` for the first `CALL rel32` (opcode 0xE8) and
/// return its target. Useful for finding "the call" in a small wrapper function.
///
/// # Safety
/// `start..start+len` must be readable.
pub unsafe fn scan_first_call_rel32(start: *const u8, len: usize) -> Option<*const u8> {
    for i in 0..len {
        let p = start.add(i);
        if *p == 0xE8 {
            return Some(decode_call_rel32(p));
        }
    }
    None
}

/// Scan a memory region for every `CALL rel32` (opcode 0xE8) whose target
/// equals `target`. Returns the addresses of the `E8` opcode bytes.
/// Use for finding all xrefs to a specific function within a module.
///
/// # Safety
/// `base..base+size` must be readable.
pub unsafe fn scan_xrefs_to(base: *const u8, size: usize, target: *const u8) -> Vec<*const u8> {
    let mut out = scan_xrefs_to_batch(base, size, &[target]);
    out.pop().unwrap_or_default()
}

/// Walk a memory region once, collecting `CALL rel32` sites that target
/// any of the given addresses. Returns a parallel vec: result `i` holds
/// the call sites targeting `targets[i]`.
///
/// Replaces N independent `scan_xrefs_to` calls (which each walk the
/// module) with a single pass; for `signatures::resolve_derived` this
/// converts ~3×O(M) work into 1×O(M).
///
/// # Safety
/// `base..base+size` must be readable.
pub unsafe fn scan_xrefs_to_batch(
    base: *const u8,
    size: usize,
    targets: &[*const u8],
) -> Vec<Vec<*const u8>> {
    let mut results: Vec<Vec<*const u8>> = vec![Vec::new(); targets.len()];
    if targets.is_empty() {
        return results;
    }

    // HashMap<*const u8, usize>: fast target → index lookup. The pointer
    // values are stable for the process lifetime (game module bytes are
    // mapped once), so using them as HashMap keys is safe.
    let target_index: HashMap<*const u8, usize> =
        targets.iter().enumerate().map(|(i, t)| (*t, i)).collect();

    for i in 0..size.saturating_sub(5) {
        let p = base.add(i);
        if *p != 0xE8 {
            continue;
        }
        let resolved = decode_call_rel32(p);
        if let Some(&idx) = target_index.get(&resolved) {
            results[idx].push(p);
        }
    }
    results
}

/// Parse a pattern string into a vec of Option<u8> (None = wildcard).
fn parse_pattern(pattern: &str) -> Vec<Option<u8>> {
    pattern
        .split_whitespace()
        .map(|tok| {
            if tok == "?" || tok == "??" {
                None
            } else {
                Some(u8::from_str_radix(tok, 16).unwrap_or(0))
            }
        })
        .collect()
}

/// Scan a memory region for the given AOB pattern. Returns the first match.
pub fn scan_pattern(base: *const u8, size: usize, pattern: &str) -> Option<ScanResult> {
    let _t = std::time::Instant::now();
    let result = scan_pattern_inner(base, size, pattern);
    crate::core::profiling::record_scan_pattern(pattern, _t.elapsed());
    result
}

fn scan_pattern_inner(base: *const u8, size: usize, pattern: &str) -> Option<ScanResult> {
    let parsed = parse_pattern(pattern);
    if parsed.is_empty() || parsed.len() > size {
        return None;
    }

    let (run_offset, run_bytes) = longest_literal_run(&parsed);
    if run_bytes.len() < 2 {
        // Slow fallback: linear walk with first-byte prefilter.
        return scan_pattern_scalar(base, size, &parsed);
    }

    let bytes = unsafe { std::slice::from_raw_parts(base, size) };
    let ac = match aho_corasick::AhoCorasickBuilder::new()
        .match_kind(aho_corasick::MatchKind::Standard)
        .build([&run_bytes[..]])
    {
        Ok(ac) => ac,
        Err(e) => {
            log_warn!(
                "[scanner] aho-corasick build failed: {}; falling back to scalar",
                e
            );
            return scan_pattern_scalar(base, size, &parsed);
        }
    };

    for m in ac.find_iter(bytes) {
        let hit_start = m.start();
        if hit_start < run_offset {
            continue;
        }
        let pattern_start = hit_start - run_offset;
        if verify_at(bytes, pattern_start, &parsed) {
            return Some(ScanResult {
                address: unsafe { base.add(pattern_start) },
                offset: pattern_start,
            });
        }
    }
    None
}

/// Linear walk fallback for patterns that lack a usable literal run for
/// Aho-Corasick. Identical semantics to the original byte-by-byte
/// scanner.
fn scan_pattern_scalar(base: *const u8, size: usize, parsed: &[Option<u8>]) -> Option<ScanResult> {
    if parsed.is_empty() || parsed.len() > size {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(base, size) };
    let end = size - parsed.len();

    let first_fixed = parsed.iter().enumerate().find(|(_, b)| b.is_some());

    for i in 0..=end {
        if let Some((idx, Some(val))) = first_fixed {
            if bytes[i + idx] != *val {
                continue;
            }
        }
        if verify_at(bytes, i, parsed) {
            return Some(ScanResult {
                address: unsafe { base.add(i) },
                offset: i,
            });
        }
    }
    None
}

/// Scan for all occurrences of a pattern.
pub fn scan_pattern_all(base: *const u8, size: usize, pattern: &str) -> Vec<ScanResult> {
    let _t = std::time::Instant::now();
    let result = scan_pattern_all_inner(base, size, pattern);
    crate::core::profiling::record_scan_pattern_all(pattern, _t.elapsed());
    result
}

fn scan_pattern_all_inner(base: *const u8, size: usize, pattern: &str) -> Vec<ScanResult> {
    let parsed = parse_pattern(pattern);
    if parsed.is_empty() || parsed.len() > size {
        return Vec::new();
    }

    let (run_offset, run_bytes) = longest_literal_run(&parsed);
    if run_bytes.len() < 2 {
        return scan_pattern_all_scalar(base, size, &parsed);
    }

    let bytes = unsafe { std::slice::from_raw_parts(base, size) };
    let ac = match aho_corasick::AhoCorasickBuilder::new()
        .match_kind(aho_corasick::MatchKind::Standard)
        .build([&run_bytes[..]])
    {
        Ok(ac) => ac,
        Err(e) => {
            log_warn!(
                "[scanner] aho-corasick build failed: {}; falling back to scalar",
                e
            );
            return scan_pattern_all_scalar(base, size, &parsed);
        }
    };

    let mut results = Vec::new();
    for m in ac.find_iter(bytes) {
        let hit_start = m.start();
        if hit_start < run_offset {
            continue;
        }
        let pattern_start = hit_start - run_offset;
        if verify_at(bytes, pattern_start, &parsed) {
            results.push(ScanResult {
                address: unsafe { base.add(pattern_start) },
                offset: pattern_start,
            });
        }
    }
    results
}

/// Linear walk fallback collecting every match.
fn scan_pattern_all_scalar(base: *const u8, size: usize, parsed: &[Option<u8>]) -> Vec<ScanResult> {
    if parsed.is_empty() || parsed.len() > size {
        return Vec::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(base, size) };
    let end = size - parsed.len();
    let mut results = Vec::new();

    for i in 0..=end {
        if verify_at(bytes, i, parsed) {
            results.push(ScanResult {
                address: unsafe { base.add(i) },
                offset: i,
            });
        }
    }
    results
}

/// Scan for RIP-relative LEA instructions (`48/4C 8D xx [disp32]`) that
/// resolve to `target`. Returns the address of each matching LEA opcode.
/// Covers all ModRM encodings where mod=00 and r/m=101 (RIP-relative).
pub unsafe fn scan_lea_xrefs_to(base: *const u8, size: usize, target: *const u8) -> Vec<*const u8> {
    let mut results = Vec::new();
    let slice = std::slice::from_raw_parts(base, size);
    let end = size.saturating_sub(7);

    for i in 0..end {
        let rex = slice[i];
        if (rex != 0x48 && rex != 0x4C) || slice[i + 1] != 0x8D {
            continue;
        }
        // ModRM byte: mod=00, r/m=101 means RIP-relative
        if (slice[i + 2] & 0xC7) != 0x05 {
            continue;
        }
        let disp = i32::from_le_bytes([slice[i + 3], slice[i + 4], slice[i + 5], slice[i + 6]]);
        let rip = base.add(i + 7);
        let resolved = rip.offset(disp as isize);
        if resolved == target {
            results.push(base.add(i));
        }
    }
    results
}

/// Walk backwards from `addr` to find the likely function entry point.
/// Looks for `CC` (int3 padding) or `C3` (ret) in the preceding byte,
/// which is the standard MSVC inter-function boundary pattern.
/// Falls back to 16-byte alignment if no boundary marker is found within
/// 256 bytes.
pub unsafe fn find_function_entry(addr: *const u8, module_base: *const u8) -> *const u8 {
    let offset = addr as usize - module_base as usize;
    let slice = std::slice::from_raw_parts(module_base, offset);
    let start = offset.saturating_sub(256);

    for i in (start..offset).rev() {
        if i == 0 {
            return module_base;
        }
        let prev = slice[i - 1];
        if prev == 0xCC || prev == 0xC3 {
            return module_base.add(i);
        }
    }
    // Fallback: 16-byte aligned
    module_base.add(offset & !0xF)
}

// ── Multi-pattern single-pass scanner ───────────────────────────────
//
// Strategy: extract the longest contiguous wildcard-free literal run from
// each pattern, build an Aho-Corasick matcher over those runs, walk the
// module once, and verify every AC hit against the full pattern (which
// includes wildcards).
//
// Patterns whose longest run is < 2 bytes don't benefit from AC and are
// scanned via the legacy scalar path. This is rare and a warning is
// logged; callers should phrase patterns to have a usable literal run.

/// Per-pattern metadata for the multi-pattern engine. One of these per
/// (name, pattern) pair the caller passed in.
struct PatternMeta {
    /// Caller-provided name; passed through into the result map.
    name: String,
    /// Parsed pattern as `Vec<Option<u8>>` (None = wildcard).
    parsed: Vec<Option<u8>>,
    /// Offset within `parsed` where the longest literal run begins. The
    /// AC needle starts at `parsed[run_offset]` and runs for `run_len`
    /// bytes.
    run_offset: usize,
}

/// Find the longest run of `Some(u8)` in `parsed`. Returns
/// `(run_offset, run_bytes)`.
fn longest_literal_run(parsed: &[Option<u8>]) -> (usize, Vec<u8>) {
    let mut best_offset = 0;
    let mut best_bytes: Vec<u8> = Vec::new();

    let mut current_offset = 0;
    let mut current_bytes: Vec<u8> = Vec::new();

    for (i, b) in parsed.iter().enumerate() {
        match b {
            Some(byte) => {
                if current_bytes.is_empty() {
                    current_offset = i;
                }
                current_bytes.push(*byte);
            }
            None => {
                if current_bytes.len() > best_bytes.len() {
                    best_offset = current_offset;
                    best_bytes = std::mem::take(&mut current_bytes);
                } else {
                    current_bytes.clear();
                }
            }
        }
    }
    if current_bytes.len() > best_bytes.len() {
        best_offset = current_offset;
        best_bytes = current_bytes;
    }
    (best_offset, best_bytes)
}

/// Verify that `pattern` (with wildcards) fully matches the bytes at
/// `bytes[start..start + pattern.len()]`. Caller must ensure the slice
/// is long enough; this only checks the byte values.
fn verify_at(bytes: &[u8], start: usize, pattern: &[Option<u8>]) -> bool {
    if start + pattern.len() > bytes.len() {
        return false;
    }
    for (j, expected) in pattern.iter().enumerate() {
        if let Some(val) = expected {
            if bytes[start + j] != *val {
                return false;
            }
        }
    }
    true
}

/// Multi-pattern single-pass scanner. Returns a HashMap mapping each
/// pattern's name to its first match address; patterns with no match in
/// the module are absent from the map.
///
/// # Safety
/// `base..base+size` must be readable.
pub fn scan_patterns_batch(
    base: *const u8,
    size: usize,
    patterns: &[(&str, &str)],
) -> HashMap<String, ScanResult> {
    let _t = std::time::Instant::now();
    let result = scan_patterns_batch_inner(base, size, patterns);
    crate::core::profiling::record_scan_batch(patterns.len(), result.len(), _t.elapsed());
    result
}

fn scan_patterns_batch_inner(
    base: *const u8,
    size: usize,
    patterns: &[(&str, &str)],
) -> HashMap<String, ScanResult> {
    if patterns.is_empty() || size == 0 {
        return HashMap::new();
    }

    // Partition patterns by whether they have a usable (≥ 2 byte) literal
    // run. AC-eligible patterns go into `metas` with their needle stored
    // in parallel `needles`. Slow-fallback patterns are recorded in
    // `slow_idx` and scanned individually at the end.
    let mut metas: Vec<PatternMeta> = Vec::with_capacity(patterns.len());
    let mut needles: Vec<Vec<u8>> = Vec::with_capacity(patterns.len());
    let mut slow_idx: Vec<usize> = Vec::new(); // indexes into `patterns`

    for (i, (name, pat_str)) in patterns.iter().enumerate() {
        let parsed = parse_pattern(pat_str);
        if parsed.is_empty() {
            continue;
        }
        let (run_offset, run_bytes) = longest_literal_run(&parsed);
        if run_bytes.len() < 2 {
            log_warn!(
                "[scanner] pattern {:?} has no >=2-byte literal run; using slow fallback",
                name
            );
            slow_idx.push(i);
            continue;
        }
        metas.push(PatternMeta {
            name: name.to_string(),
            parsed,
            run_offset,
        });
        needles.push(run_bytes);
    }

    let mut results: HashMap<String, ScanResult> = HashMap::new();

    if !metas.is_empty() {
        let bytes = unsafe { std::slice::from_raw_parts(base, size) };

        match aho_corasick::AhoCorasickBuilder::new()
            .match_kind(aho_corasick::MatchKind::Standard)
            .build(&needles)
        {
            Ok(ac) => {
                for m in ac.find_iter(bytes) {
                    let needle_id = m.pattern().as_usize();
                    let meta = &metas[needle_id];
                    if results.contains_key(&meta.name) {
                        continue; // already resolved; first-match-per-name semantics
                    }
                    let hit_start = m.start();
                    if hit_start < meta.run_offset {
                        continue; // pattern would start before the module
                    }
                    let pattern_start = hit_start - meta.run_offset;
                    if verify_at(bytes, pattern_start, &meta.parsed) {
                        results.insert(
                            meta.name.clone(),
                            ScanResult {
                                address: unsafe { base.add(pattern_start) },
                                offset: pattern_start,
                            },
                        );
                    }
                }
            }
            Err(e) => {
                log_warn!(
                    "[scanner] aho-corasick build failed: {}; falling back to scalar for all patterns",
                    e
                );
                // Treat every still-eligible pattern as slow-fallback. We
                // need the original `patterns` index; since the AC group
                // matches the original input order minus already-recorded
                // slow_idx entries, walk metas back to source.
                for meta in &metas {
                    // Find by name in the original patterns slice — small
                    // O(N) scan, only on the rare build-failure path.
                    if let Some(orig_i) = patterns.iter().position(|(n, _)| *n == meta.name) {
                        if !slow_idx.contains(&orig_i) {
                            slow_idx.push(orig_i);
                        }
                    }
                }
            }
        }
    }

    // Slow-fallback path for patterns without a usable literal run.
    for &i in &slow_idx {
        let (name, pat_str) = patterns[i];
        if let Some(r) = scan_pattern_inner(base, size, pat_str) {
            results.insert(name.to_string(), r);
        }
    }

    results
}
