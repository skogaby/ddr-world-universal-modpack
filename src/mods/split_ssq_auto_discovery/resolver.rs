//! Pure decision core for Split SSQ Auto-Discovery — no game or filesystem
//! dependencies, host-tested via `scripts/validate_split_ssq.sh`.
//!
//! The game's `build_ssq_path(out, basename, difficulty)` decides which SSQ
//! file holds a `(basename, difficulty)` chart from a hardcoded per-build song
//! table (RE: `docs/split_ssq_research.md`). This module reproduces that
//! decision from file CONTENTS instead — "Rule A":
//!
//! > For `(basename, d)`, pick the highest `N ≤ d+1` such that
//! > `<basename>_N.ssq` exists AND contains a type-3 (step) chunk of level `d`
//! > in either play mode; otherwise the unsplit `<basename>.ssq`.
//!
//! Rule A reproduces the stock table on all 39 installed split files (the one
//! divergence, `sabm` Challenge → `_5` vs stock `_3`, is chunk-identical) and
//! — because it inspects chart presence — can never name a file that lacks
//! the requested level, the one outcome that raises the game's boot-blocking
//! `ME1529 FILE CORRUPTION ERROR`.
//!
//! The resolver is basename-OPAQUE: it answers for whatever string it is
//! handed and never consults `musicdb.xml`. That is what preserves the game's
//! `toho` special case (the play sequences randomize that song's basename to
//! `toho1..toho4` before asking).

use std::collections::HashMap;

/// Difficulty index `d` (0..4 = Beginner, Basic, Difficult, Expert, Challenge)
/// → high byte of a type-3 chunk's `param2` (`docs/ssq_format.md` §5.1). The
/// low byte (`0x14` single / `0x18` double) is ignored: split files hold both
/// modes and the game's builder takes no mode argument.
pub const LEVEL_HIGH_BYTES: [u8; 5] = [0x04, 0x01, 0x02, 0x03, 0x06];

/// Number of difficulty slots the game's builder is called with.
pub const DIFFICULTIES: usize = 5;

/// Longest basename the hot path accepts (stock codes are ≤ 7 chars; `toho%d`
/// is 5). Anything longer is forwarded to the original function.
pub const MAX_BASENAME: usize = 0x20;

const PATH_PREFIX: &[u8] = b"data/mdb_apx/ssq/";
const PATH_EXT: &[u8] = b".ssq";
const CHUNK_HEADER: usize = 12;

/// One discovered `<basename>_<n>.ssq`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitFile {
    /// Raw basename bytes exactly as they appear in the filename.
    pub basename: Vec<u8>,
    /// Suffix digit, `1..=5`.
    pub n: u8,
    /// Bit `d` set ⇔ the file has a level-`d` type-3 chunk (either mode).
    pub levels: u8,
}

/// Resolver answer for one `(basename, d)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// The unsplit `<basename>.ssq`.
    Base,
    /// `<basename>_<n>.ssq`.
    Split(u8),
}

/// Precomputed Rule-A table: basename → chosen suffix per difficulty
/// (`None` = base file). Immutable after `build`; lookups are allocation-free.
#[derive(Debug, Default)]
pub struct Index {
    map: HashMap<Vec<u8>, [Option<u8>; DIFFICULTIES]>,
}

impl Index {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Apply Rule A over the listing. Duplicate `(basename, n)` entries (the
    /// same file seen in several sources) are merged by OR-ing their levels.
    pub fn build(files: &[SplitFile]) -> Self {
        // basename → [levels bitmask per n, index n-1]
        let mut per_song: HashMap<Vec<u8>, [Option<u8>; DIFFICULTIES]> = HashMap::new();
        for f in files {
            if !(1..=5).contains(&f.n) || f.basename.is_empty() {
                continue;
            }
            let slots = per_song
                .entry(f.basename.clone())
                .or_insert([None; DIFFICULTIES]);
            let slot = &mut slots[(f.n - 1) as usize];
            *slot = Some(slot.unwrap_or(0) | f.levels);
        }

        let mut map = HashMap::with_capacity(per_song.len());
        for (basename, slots) in per_song {
            let mut chosen = [None; DIFFICULTIES];
            for d in 0..DIFFICULTIES {
                // Highest N ≤ d+1 whose file exists and carries level d.
                for n in (1..=(d + 1)).rev() {
                    if let Some(levels) = slots[n - 1] {
                        if levels & (1 << d) != 0 {
                            chosen[d] = Some(n as u8);
                            break;
                        }
                    }
                }
            }
            if chosen.iter().any(Option::is_some) {
                map.insert(basename, chosen);
            }
        }
        Self { map }
    }

    /// Rule-A answer. `d` outside `0..5` and unknown basenames yield `Base`.
    pub fn resolve(&self, basename: &[u8], d: usize) -> Choice {
        if d >= DIFFICULTIES {
            return Choice::Base;
        }
        match self.map.get(basename).and_then(|c| c[d]) {
            Some(n) => Choice::Split(n),
            None => Choice::Base,
        }
    }

    /// Number of basenames with at least one split difficulty.
    pub fn song_count(&self) -> usize {
        self.map.len()
    }

    /// Sorted `(basename, chosen)` rows for the enable-time log.
    pub fn describe(&self) -> Vec<(Vec<u8>, [Option<u8>; DIFFICULTIES])> {
        let mut rows: Vec<_> = self.map.iter().map(|(k, v)| (k.clone(), *v)).collect();
        rows.sort();
        rows
    }
}

/// Level bitmask of every type-3 chunk in an SSQ blob. Walks chunk headers
/// only (12-byte header, stride = `length`), honoring the game's own walker
/// rules: `length == 0` terminates, `param2 == 0xFFFF` aborts, a malformed
/// length stops the walk. Returns whatever was read before stopping.
pub fn levels_in_blob(blob: &[u8]) -> u8 {
    let mut levels = 0u8;
    let mut offset = 0usize;
    while offset + CHUNK_HEADER <= blob.len() {
        let length = read_u32_le(blob, offset) as usize;
        if length == 0 || length < CHUNK_HEADER || offset + length > blob.len() {
            break;
        }
        let kind = read_u16_le(blob, offset + 4);
        let param2 = read_u16_le(blob, offset + 6);
        if param2 == 0xFFFF {
            break;
        }
        if kind == 3 {
            let high = (param2 >> 8) as u8;
            if let Some(d) = LEVEL_HIGH_BYTES.iter().position(|&h| h == high) {
                levels |= 1 << d;
            }
        }
        offset += length;
    }
    levels
}

/// Parse `<basename>_<n>.ssq` (case-sensitive extension, `n ∈ '1'..='5'`,
/// non-empty basename, at most `MAX_BASENAME` bytes). Anything else — the
/// unsplit `<basename>.ssq` included — is `None`.
pub fn parse_split_filename(name: &[u8]) -> Option<(Vec<u8>, u8)> {
    let stem = name.strip_suffix(PATH_EXT)?;
    // stem = "<basename>_<digit>"
    if stem.len() < 3 {
        return None;
    }
    let (basename, tail) = stem.split_at(stem.len() - 2);
    if tail[0] != b'_' || !(b'1'..=b'5').contains(&tail[1]) {
        return None;
    }
    if basename.is_empty() || basename.len() > MAX_BASENAME || basename.contains(&b'/') {
        return None;
    }
    Some((basename.to_vec(), tail[1] - b'0'))
}

/// Filter a directory listing (bare filenames, possibly from several sources)
/// down to the distinct `(basename, n)` split candidates, sorted. Non-matching
/// names are ignored; duplicates across sources collapse to one entry.
pub fn collect_split_candidates<'a, I>(names: I) -> Vec<(Vec<u8>, u8)>
where
    I: IntoIterator<Item = &'a [u8]>,
{
    let mut out: Vec<(Vec<u8>, u8)> = names.into_iter().filter_map(parse_split_filename).collect();
    out.sort();
    out.dedup();
    out
}

/// Write `data/mdb_apx/ssq/<basename>[_<n>].ssq` plus a NUL into `out`.
/// Returns `false` and writes nothing when the result (with its terminator)
/// would not fit. Allocation-free; safe for the hot path.
pub fn format_path(out: &mut [u8], basename: &[u8], choice: Choice) -> bool {
    let suffix_len = match choice {
        Choice::Base => 0,
        Choice::Split(_) => 2,
    };
    let total = PATH_PREFIX.len() + basename.len() + suffix_len + PATH_EXT.len() + 1;
    if total > out.len() {
        return false;
    }
    let mut pos = 0;
    let mut put = |bytes: &[u8], out: &mut [u8]| {
        out[pos..pos + bytes.len()].copy_from_slice(bytes);
        pos += bytes.len();
    };
    put(PATH_PREFIX, out);
    put(basename, out);
    if let Choice::Split(n) = choice {
        put(&[b'_', b'0' + n], out);
    }
    put(PATH_EXT, out);
    out[pos] = 0;
    true
}

/// NUL-aware comparison of two C strings held in fixed buffers: `true` when
/// the bytes up to (and excluding) each buffer's first NUL differ. A buffer
/// with no NUL compares over its whole length.
pub fn paths_differ(a: &[u8], b: &[u8]) -> bool {
    cstr(a) != cstr(b)
}

/// Bytes up to the first NUL (or the whole slice).
pub fn cstr(buf: &[u8]) -> &[u8] {
    match buf.iter().position(|&c| c == 0) {
        Some(end) => &buf[..end],
        None => buf,
    }
}

/// Render a `describe()` row as `name: [-,-,3,3,3]` for the log.
pub fn describe_row(basename: &[u8], chosen: &[Option<u8>; DIFFICULTIES]) -> String {
    let cells: Vec<String> = chosen
        .iter()
        .map(|c| c.map_or("-".to_string(), |n| n.to_string()))
        .collect();
    format!(
        "{}: [{}]",
        String::from_utf8_lossy(basename),
        cells.join(",")
    )
}

fn read_u32_le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn read_u16_le(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sf(name: &str, n: u8, levels: &[usize]) -> SplitFile {
        SplitFile {
            basename: name.as_bytes().to_vec(),
            n,
            levels: levels.iter().fold(0u8, |m, &d| m | (1 << d)),
        }
    }

    // Level shorthand: B=0 b=1 D=2 E=3 C=4 (Beginner Basic Difficult Expert Challenge)
    const B: usize = 0;
    const BB: usize = 1;
    const D: usize = 2;
    const E: usize = 3;
    const C: usize = 4;

    /// The 39 split files installed on the reference cabinet, with the level
    /// set of their type-3 chunks (`docs/split_ssq_research.md` §6).
    fn installed_split_files() -> Vec<SplitFile> {
        let mut v = vec![
            sf("acef", 1, &[B]),
            sf("acef", 2, &[BB]),
            sf("acef", 3, &[D]),
            sf("acef", 4, &[E]),
            sf("acef", 5, &[C]),
            sf("rabb", 4, &[E]),
            sf("stvi", 3, &[D, E]),
            sf("stvi", 5, &[C]),
            sf("dopa2", 3, &[D, E]),
            sf("dopa2", 5, &[C]),
            sf("sabm", 3, &[D, E, C]),
            sf("sabm", 5, &[C]),
            sf("hkhk", 3, &[BB, D, E, C]), // redundant Basic copy in _3
        ];
        for s in ["chao2", "kanb", "leda", "file", "shuk", "lien", "konr"] {
            v.push(sf(s, 5, &[C]));
        }
        for s in [
            "buco", "casr", "danz", "eoth", "fizz", "flor", "gogg", "kjnf2", "scre", "sipp",
            "smin", "zend",
        ] {
            v.push(sf(s, 3, &[D, E]));
        }
        for s in ["houu2", "mega", "mero", "mlwt", "mons", "suma", "yush"] {
            v.push(sf(s, 3, &[D, E, C]));
        }
        assert_eq!(v.len(), 39);
        v
    }

    fn row(idx: &Index, name: &str) -> [Choice; 5] {
        let mut r = [Choice::Base; 5];
        for d in 0..5 {
            r[d] = idx.resolve(name.as_bytes(), d);
        }
        r
    }

    use Choice::{Base as Bs, Split as Sp};

    #[test]
    fn parse_accepts_split_names() {
        assert_eq!(
            parse_split_filename(b"casr_3.ssq"),
            Some((b"casr".to_vec(), 3))
        );
        assert_eq!(
            parse_split_filename(b"dopa2_5.ssq"),
            Some((b"dopa2".to_vec(), 5))
        );
        assert_eq!(parse_split_filename(b"a_1.ssq"), Some((b"a".to_vec(), 1)));
    }

    #[test]
    fn parse_rejects_non_split_names() {
        for bad in [
            &b"casr.ssq"[..],
            b"casr_6.ssq",
            b"casr_0.ssq",
            b"casr_33.ssq",
            b"_3.ssq",
            b"casr_3.SSQ",
            b"casr_3.ssqx",
            b"casr_3",
            b"",
        ] {
            assert_eq!(parse_split_filename(bad), None, "{:?}", bad);
        }
    }

    fn chunk(kind: u16, param2: u16, body: usize) -> Vec<u8> {
        let len = (CHUNK_HEADER + body) as u32;
        let mut v = len.to_le_bytes().to_vec();
        v.extend_from_slice(&kind.to_le_bytes());
        v.extend_from_slice(&param2.to_le_bytes());
        v.extend_from_slice(&[0u8; 4]);
        v.extend(std::iter::repeat(0xAA).take(body));
        v
    }

    #[test]
    fn levels_from_type3_chunks_both_modes() {
        let mut blob = chunk(1, 0x03E8, 16);
        blob.extend(chunk(2, 0x0001, 8));
        blob.extend(chunk(3, 0x0314, 4)); // single expert
        blob.extend(chunk(3, 0x0618, 4)); // double challenge
        blob.extend(chunk(3, 0x0118, 0)); // double basic
        blob.extend([0u8; 4]); // terminator
        assert_eq!(levels_in_blob(&blob), (1 << E) | (1 << C) | (1 << BB));
    }

    #[test]
    fn levels_stop_at_sentinel_and_terminator() {
        let mut blob = chunk(3, 0x0414, 0);
        blob.extend(chunk(3, 0xFFFF, 0)); // sentinel: abort
        blob.extend(chunk(3, 0x0614, 0)); // never reached
        assert_eq!(levels_in_blob(&blob), 1 << B);

        let mut blob = chunk(3, 0x0214, 0);
        blob.extend([0u8; 12]); // zero length terminator
        blob.extend(chunk(3, 0x0614, 0));
        assert_eq!(levels_in_blob(&blob), 1 << D);
    }

    #[test]
    fn levels_tolerate_truncation_and_bad_length() {
        let mut blob = chunk(3, 0x0114, 0);
        blob.extend([0x40, 0, 0, 0, 3, 0, 0x14, 0x03]); // truncated header
        assert_eq!(levels_in_blob(&blob), 1 << BB);

        let mut blob = chunk(3, 0x0114, 0);
        let mut bad = chunk(3, 0x0314, 0);
        bad[0] = 0xFF; // length overruns the blob
        blob.extend(bad);
        assert_eq!(levels_in_blob(&blob), 1 << BB);
        assert_eq!(levels_in_blob(&[]), 0);
    }

    #[test]
    fn reproduces_stock_table() {
        let idx = Index::build(&installed_split_files());
        assert_eq!(idx.song_count(), 32);
        // Pattern A — fully split.
        assert_eq!(row(&idx, "acef"), [Sp(1), Sp(2), Sp(3), Sp(4), Sp(5)]);
        // Pattern B — Challenge only.
        for s in ["chao2", "kanb", "leda", "file", "shuk", "lien", "konr"] {
            assert_eq!(row(&idx, s), [Bs, Bs, Bs, Bs, Sp(5)], "{s}");
        }
        // Pattern C.
        assert_eq!(row(&idx, "rabb")[..4], [Bs, Bs, Bs, Sp(4)]);
        // Pattern D — hard charts in _3, Challenge in _5.
        for s in ["stvi", "dopa2"] {
            assert_eq!(row(&idx, s), [Bs, Bs, Sp(3), Sp(3), Sp(5)], "{s}");
        }
        // Pattern E — everything hard in _3 (Challenge slot only matters where a
        // Challenge chart exists; where it does, stock says _3 and so do we).
        for s in ["houu2", "mega", "mero", "mlwt", "mons", "suma", "yush"] {
            assert_eq!(row(&idx, s), [Bs, Bs, Sp(3), Sp(3), Sp(3)], "{s}");
        }
        for s in [
            "buco", "casr", "danz", "eoth", "fizz", "flor", "gogg", "kjnf2", "scre", "sipp",
            "smin", "zend",
        ] {
            assert_eq!(row(&idx, s)[..4], [Bs, Bs, Sp(3), Sp(3)], "{s}");
        }
        // hkhk: the redundant Basic copy in _3 must NOT pull Basic off the base
        // file (stock: base) — Rule A only considers N ≤ d+1 = 2.
        assert_eq!(row(&idx, "hkhk"), [Bs, Bs, Sp(3), Sp(3), Sp(3)]);
    }

    #[test]
    fn documented_sabm_divergence() {
        let idx = Index::build(&installed_split_files());
        // Stock says _3 (chunk-identical); Rule A prefers the highest N.
        assert_eq!(row(&idx, "sabm"), [Bs, Bs, Sp(3), Sp(3), Sp(5)]);
    }

    #[test]
    fn toho_and_unknown_resolve_to_base() {
        let idx = Index::build(&installed_split_files());
        for s in ["toho1", "toho2", "toho3", "toho4", "toho", "zzzz", ""] {
            assert_eq!(row(&idx, s), [Bs; 5], "{s}");
        }
        assert_eq!(idx.resolve(b"acef", 5), Bs);
        assert_eq!(Index::empty().resolve(b"acef", 0), Bs);
    }

    #[test]
    fn build_merges_duplicate_files() {
        let idx = Index::build(&[sf("x", 3, &[D]), sf("x", 3, &[E]), sf("x", 9, &[C])]);
        assert_eq!(row(&idx, "x"), [Bs, Bs, Sp(3), Sp(3), Bs]);
        assert_eq!(idx.song_count(), 1);
        // A file with no recognised level contributes nothing.
        assert_eq!(Index::build(&[sf("y", 3, &[])]).song_count(), 0);
    }

    #[test]
    fn describe_is_sorted_and_rendered() {
        let idx = Index::build(&[sf("zz", 5, &[C]), sf("aa", 3, &[D, E])]);
        let rows = idx.describe();
        assert_eq!(rows[0].0, b"aa".to_vec());
        assert_eq!(describe_row(&rows[0].0, &rows[0].1), "aa: [-,-,3,3,-]");
        assert_eq!(describe_row(&rows[1].0, &rows[1].1), "zz: [-,-,-,-,5]");
    }

    #[test]
    fn format_path_exact_bytes() {
        let mut out = [0xEEu8; 0x100];
        assert!(format_path(&mut out, b"casr", Choice::Base));
        assert_eq!(cstr(&out), b"data/mdb_apx/ssq/casr.ssq");
        assert_eq!(out[b"data/mdb_apx/ssq/casr.ssq".len()], 0);

        assert!(format_path(&mut out, b"dopa2", Choice::Split(5)));
        assert_eq!(cstr(&out), b"data/mdb_apx/ssq/dopa2_5.ssq");
    }

    #[test]
    fn format_path_refuses_overflow() {
        let mut out = [0xEEu8; 26]; // "data/mdb_apx/ssq/casr.ssq" = 25 + NUL fits exactly
        assert!(format_path(&mut out, b"casr", Choice::Base));
        let mut out = [0xEEu8; 25];
        assert!(!format_path(&mut out, b"casr", Choice::Base));
        assert!(out.iter().all(|&b| b == 0xEE), "nothing written on refusal");
        let mut out = [0xEEu8; 27];
        assert!(!format_path(&mut out, b"casr", Choice::Split(3)));
    }

    #[test]
    fn collect_candidates_dedupes_and_filters() {
        let names: [&[u8]; 6] = [
            b"casr_3.ssq",
            b"casr.ssq",
            b"acef_1.ssq",
            b"casr_3.ssq", // same file seen in a mod folder
            b"readme.txt",
            b"acef_1.SSQ",
        ];
        assert_eq!(
            collect_split_candidates(names),
            vec![(b"acef".to_vec(), 1), (b"casr".to_vec(), 3)]
        );
        assert!(collect_split_candidates(std::iter::empty()).is_empty());
    }

    #[test]
    fn paths_differ_is_nul_aware() {
        assert!(!paths_differ(b"a.ssq\0zzz", b"a.ssq\0yyy"));
        assert!(paths_differ(b"a.ssq\0", b"a_3.ssq\0"));
        assert!(!paths_differ(b"a.ssq", b"a.ssq\0x")); // no-NUL buffer compares whole
        assert!(paths_differ(b"a.ssqq", b"a.ssq\0x"));
        assert!(!paths_differ(b"a.ssq", b"a.ssq"));
    }
}
