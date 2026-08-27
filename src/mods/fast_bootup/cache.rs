//! Pure bin-format layer for the ultrafast-boot step-data cache
//! (`data_mods/_cache/step_data/v1.bin` — design §Data Models → Cache file).
//!
//! Stores the boot-time analyzer outputs per file × difficulty × mode so
//! later boots can replay them without reading or parsing the SSQ. The
//! format is hand-rolled little-endian, fully bounds-checked, and versioned.
//!
//! Fail-open contract (design §Error Handling): ANY whole-file malformation
//! — bad magic, unknown version, gamemdx build mismatch, truncation, counts
//! that exceed the remaining bytes or the sanity caps, trailing garbage —
//! yields [`CacheLoad::Empty`] with a reason string. Never a panic, never a
//! partial `Loaded`. Callers translate `Empty` into one WARN + full rebuild.
//!
//! Dependency-free on purpose: no I/O, no game types, no statics, no
//! logging — host-tested via `cargo test`. File I/O belongs to the identity
//! layer and the completion-time writer.

/// File magic (8 bytes).
pub const MAGIC: [u8; 8] = *b"DDRSSQC1";
/// Bump on ANY layout change; readers treat other versions as `Empty`.
pub const FORMAT_VERSION: u32 = 1;
/// Sanity caps: corrupt counts must not drive huge allocations.
pub const MAX_ENTRIES: u32 = 65536;
/// Per-file payload cap: 5 difficulties × 2 modes.
pub const MAX_PAYLOADS: u8 = 10;
/// Path-string length cap.
pub const MAX_STR: u16 = 1024;

/// One (difficulty, mode) analyzer outcome — the exact blocks captured at
/// the Analyze boundary. `result` carries the game's 14-int block verbatim
/// (f64 BPMs live at [8..9], [10..11], [12..13] as raw bit patterns).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotPayload {
    pub difficulty: u8,
    pub mode: u8,
    pub ret: u8,
    pub result: [i32; 14],
    pub radar: [i32; 5],
}

/// What backed a cached file when it was captured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// A real file: the LayeredFS-resolved host path that was analyzed plus
    /// its size/mtime at capture time.
    File {
        resolved_path: String,
        size: u64,
        mtime_secs: u64,
    },
    /// No backing file existed (chartless customs): the cached outcome is
    /// the zeroed/failed analysis. A file appearing later is an identity
    /// mismatch and the entry goes stale.
    Absent,
}

/// One SSQ file's cached outcomes, keyed by its registered game path
/// (e.g. `data/mdb_apx/ssq/puty.ssq`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub game_path: String,
    pub identity: Identity,
    pub payloads: Vec<SlotPayload>,
}

/// Parsed cache document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheFile {
    pub entries: Vec<FileEntry>,
}

/// Parse outcome. `Empty` means "behave as if no cache exists".
#[derive(Debug)]
pub enum CacheLoad {
    Loaded(CacheFile),
    Empty { reason: &'static str },
}

/// Bounds-checked little-endian cursor. Every read returns `Option`; `None`
/// anywhere aborts the parse into `Empty`.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        if end > self.buf.len() {
            return None;
        }
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Option<u16> {
        let b = self.bytes(2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        let b = self.bytes(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Option<u64> {
        let b = self.bytes(8)?;
        Some(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(self.u32()? as i32)
    }

    /// u16-length-prefixed UTF-8 string, capped at [`MAX_STR`].
    fn string(&mut self) -> Option<String> {
        let len = self.u16()?;
        if len > MAX_STR {
            return None;
        }
        let bytes = self.bytes(len as usize)?;
        String::from_utf8(bytes.to_vec()).ok()
    }

    fn exhausted(&self) -> bool {
        self.pos == self.buf.len()
    }
}

/// Parse a cache blob. `expected_stamp`/`expected_size` are the loaded
/// gamemdx module's PE TimeDateStamp and SizeOfImage — a mismatch means the
/// analyzer may have changed, so the whole cache is invalid.
pub fn parse(bytes: &[u8], expected_stamp: u32, expected_size: u32) -> CacheLoad {
    match parse_inner(bytes, expected_stamp, expected_size) {
        Ok(file) => CacheLoad::Loaded(file),
        Err(reason) => CacheLoad::Empty { reason },
    }
}

fn parse_inner(
    bytes: &[u8],
    expected_stamp: u32,
    expected_size: u32,
) -> Result<CacheFile, &'static str> {
    let mut r = Reader::new(bytes);

    let magic = r.bytes(8).ok_or("truncated header")?;
    if magic != MAGIC {
        return Err("bad magic");
    }
    let version = r.u32().ok_or("truncated header")?;
    if version != FORMAT_VERSION {
        return Err("unknown format version");
    }
    let stamp = r.u32().ok_or("truncated header")?;
    let size = r.u32().ok_or("truncated header")?;
    if stamp != expected_stamp || size != expected_size {
        return Err("gamemdx build mismatch");
    }
    let entry_count = r.u32().ok_or("truncated header")?;
    if entry_count > MAX_ENTRIES {
        return Err("entry count over cap");
    }

    let mut entries = Vec::new();
    for _ in 0..entry_count {
        let game_path = r.string().ok_or("bad entry path")?;
        let identity = match r.u8().ok_or("truncated entry")? {
            0 => {
                let resolved_path = r.string().ok_or("bad resolved path")?;
                let size = r.u64().ok_or("truncated identity")?;
                let mtime_secs = r.u64().ok_or("truncated identity")?;
                Identity::File {
                    resolved_path,
                    size,
                    mtime_secs,
                }
            }
            1 => Identity::Absent,
            _ => return Err("unknown identity kind"),
        };
        let payload_count = r.u8().ok_or("truncated entry")?;
        if payload_count > MAX_PAYLOADS {
            return Err("payload count over cap");
        }
        let mut payloads = Vec::with_capacity(payload_count as usize);
        for _ in 0..payload_count {
            let difficulty = r.u8().ok_or("truncated payload")?;
            let mode = r.u8().ok_or("truncated payload")?;
            let ret = r.u8().ok_or("truncated payload")?;
            let mut result = [0i32; 14];
            for slot in result.iter_mut() {
                *slot = r.i32().ok_or("truncated payload")?;
            }
            let mut radar = [0i32; 5];
            for slot in radar.iter_mut() {
                *slot = r.i32().ok_or("truncated payload")?;
            }
            payloads.push(SlotPayload {
                difficulty,
                mode,
                ret,
                result,
                radar,
            });
        }
        entries.push(FileEntry {
            game_path,
            identity,
            payloads,
        });
    }

    if !r.exhausted() {
        return Err("trailing garbage");
    }
    Ok(CacheFile { entries })
}

/// Serialize a cache document (exact inverse of [`parse`]). The caller
/// supplies the gamemdx stamp/size recorded in the header.
pub fn serialize(file: &CacheFile, gamemdx_stamp: u32, gamemdx_size: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + file.entries.len() * 512);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&gamemdx_stamp.to_le_bytes());
    out.extend_from_slice(&gamemdx_size.to_le_bytes());
    out.extend_from_slice(&(file.entries.len() as u32).to_le_bytes());
    for entry in &file.entries {
        push_string(&mut out, &entry.game_path);
        match &entry.identity {
            Identity::File {
                resolved_path,
                size,
                mtime_secs,
            } => {
                out.push(0);
                push_string(&mut out, resolved_path);
                out.extend_from_slice(&size.to_le_bytes());
                out.extend_from_slice(&mtime_secs.to_le_bytes());
            }
            Identity::Absent => out.push(1),
        }
        out.push(entry.payloads.len() as u8);
        for p in &entry.payloads {
            out.push(p.difficulty);
            out.push(p.mode);
            out.push(p.ret);
            for v in &p.result {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for v in &p.radar {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    out
}

fn push_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    debug_assert!(bytes.len() <= MAX_STR as usize);
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
}

// ── Pure identity / merge helpers (host-tested) ─────────────────────────
//
// These are the dependency-free parts of the identity + write layers. The
// game-facing glue (`identity.rs` stat + verifier thread, `capture.rs` store
// + writer) calls into them so the filesystem-free logic stays in a module
// the offline harness already mounts.

/// A stat outcome for one candidate backing path: `Some((path, size,
/// mtime_secs))` if the file exists, `None` if it doesn't. Injected here so
/// [`resolve_identity`] is testable without touching disk.
pub type StatHit = Option<(String, u64, u64)>;

/// Strip a registered game path (`"data/mdb_apx/ssq/puty.ssq"`) down to the
/// LayeredFS-normalized relative path (`"mdb_apx/ssq/puty.ssq"`) that
/// `mod_paths` lookups use. Mirrors `mod_paths::normalise_path`'s "find the
/// first `data/`" rule (case-insensitive), and normalizes backslashes.
/// `None` when the path carries no `data/` segment.
pub fn normalize_ssq_rel(game_path: &str) -> Option<String> {
    let s = game_path.replace('\\', "/");
    let pos = s.to_lowercase().find("data/")?;
    Some(s[pos + 5..].to_string())
}

/// Resolve a file's cache identity from its two candidate stats. The
/// LayeredFS mod-folder override wins over the stock path; if neither exists
/// the file is [`Identity::Absent`]. This is the pure core of
/// `identity::resolve` (the actual `std::fs` stats are done by the caller).
pub fn resolve_identity(mod_hit: StatHit, stock_hit: StatHit) -> Identity {
    if let Some((resolved_path, size, mtime_secs)) = mod_hit.or(stock_hit) {
        Identity::File {
            resolved_path,
            size,
            mtime_secs,
        }
    } else {
        Identity::Absent
    }
}

/// True iff a cached identity still matches the current one — a cache HIT.
/// Files match on resolved path + size + mtime (a LayeredFS override that
/// changed the backing path, an edit that changed size, or a touch that
/// changed mtime all miss). `Absent` matches only `Absent`: a chart whose
/// file appeared since capture is a mismatch → stock path (D13).
pub fn identity_matches(cached: &Identity, current: &Identity) -> bool {
    match (cached, current) {
        (Identity::Absent, Identity::Absent) => true,
        (
            Identity::File {
                resolved_path: a,
                size: sa,
                mtime_secs: ma,
            },
            Identity::File {
                resolved_path: b,
                size: sb,
                mtime_secs: mb,
            },
        ) => a == b && sa == sb && ma == mb,
        _ => false,
    }
}

/// Merge freshly-captured entries over a loaded cache: fresh wins, keyed by
/// `game_path`; entries only in the loaded cache (unchanged charts that were
/// replayed, so never re-captured) are preserved. This is the completion
/// writer's document step (FR-5 partial-miss rewrite).
/// Merge freshly-captured entries over a loaded cache, keyed by `game_path`:
///
/// * new file (not in the loaded cache) ⇒ inserted;
/// * same file, **identity changed** ⇒ replaced wholesale (the old payloads
///   described a different file version and are dropped);
/// * same file, **identity unchanged** ⇒ payloads are UNIONed per
///   (difficulty, mode) with fresh winning, so a partial re-capture (the
///   final song processes only its final difficulty stock while the rest
///   replay) never truncates the entry's other cached difficulties.
///
/// Entries only in the loaded cache (unchanged charts that were fully
/// replayed, so never re-captured) are preserved untouched.
pub fn merge(mut loaded: CacheFile, fresh: Vec<FileEntry>) -> CacheFile {
    use std::collections::HashMap;
    // Owned keys so the map never borrows `loaded.entries` while we mutate it.
    let mut index: HashMap<String, usize> = HashMap::with_capacity(loaded.entries.len());
    for (i, e) in loaded.entries.iter().enumerate() {
        index.insert(e.game_path.clone(), i);
    }
    for f in fresh {
        match index.get(&f.game_path) {
            Some(&i) => {
                if identity_matches(&loaded.entries[i].identity, &f.identity) {
                    let payloads = union_payloads(&loaded.entries[i].payloads, f.payloads);
                    loaded.entries[i] = FileEntry {
                        game_path: f.game_path,
                        identity: f.identity,
                        payloads,
                    };
                } else {
                    loaded.entries[i] = f;
                }
            }
            None => {
                index.insert(f.game_path.clone(), loaded.entries.len());
                loaded.entries.push(f);
            }
        }
    }
    loaded
}

/// Union two payload sets keyed by (difficulty, mode); `fresh` wins on
/// collision. Result sorted (difficulty, mode) for deterministic output.
fn union_payloads(cached: &[SlotPayload], fresh: Vec<SlotPayload>) -> Vec<SlotPayload> {
    use std::collections::BTreeMap;
    let mut by_key: BTreeMap<(u8, u8), SlotPayload> = BTreeMap::new();
    for p in cached {
        by_key.insert((p.difficulty, p.mode), *p);
    }
    for p in fresh {
        by_key.insert((p.difficulty, p.mode), p);
    }
    by_key.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMP: u32 = 0x1234_5678;
    const SIZE: u32 = 0x0070_0000;

    fn payload(difficulty: u8, mode: u8, min_bpm: f64, max_bpm: f64) -> SlotPayload {
        let mut result = [0i32; 14];
        result[0] = 250; // steps
        result[1] = 12; // freezes
        result[2] = 3; // shocks
        let min_bits = min_bpm.to_bits();
        result[8] = (min_bits & 0xFFFF_FFFF) as u32 as i32;
        result[9] = (min_bits >> 32) as u32 as i32;
        let max_bits = max_bpm.to_bits();
        result[12] = (max_bits & 0xFFFF_FFFF) as u32 as i32;
        result[13] = (max_bits >> 32) as u32 as i32;
        SlotPayload {
            difficulty,
            mode,
            ret: 1,
            result,
            radar: [10, 20, 30, 40, 50],
        }
    }

    fn fixture() -> CacheFile {
        let mut full = Vec::new();
        for diff in 0..5u8 {
            for mode in 0..2u8 {
                full.push(payload(diff, mode, 65.0, 400.5));
            }
        }
        CacheFile {
            entries: vec![
                FileEntry {
                    game_path: "data/mdb_apx/ssq/puty.ssq".into(),
                    identity: Identity::File {
                        resolved_path: "data/mdb_apx/ssq/puty.ssq".into(),
                        size: 123_456,
                        mtime_secs: 1_755_000_000,
                    },
                    payloads: full,
                },
                FileEntry {
                    game_path: "data/mdb_apx/ssq/gone.ssq".into(),
                    identity: Identity::Absent,
                    payloads: vec![SlotPayload {
                        difficulty: 0,
                        mode: 0,
                        ret: 0,
                        result: [0; 14],
                        radar: [0; 5],
                    }],
                },
                FileEntry {
                    game_path: "data_mods/customs/ssq/mod.ssq".into(),
                    identity: Identity::File {
                        resolved_path: "data_mods/customs/ssq/mod.ssq".into(),
                        size: 999,
                        mtime_secs: 1,
                    },
                    payloads: vec![],
                },
            ],
        }
    }

    fn expect_empty(bytes: &[u8]) -> &'static str {
        match parse(bytes, STAMP, SIZE) {
            CacheLoad::Empty { reason } => reason,
            CacheLoad::Loaded(_) => panic!("expected Empty, got Loaded"),
        }
    }

    #[test]
    fn round_trip() {
        let original = fixture();
        let bytes = serialize(&original, STAMP, SIZE);
        match parse(&bytes, STAMP, SIZE) {
            CacheLoad::Loaded(parsed) => assert_eq!(parsed, original),
            CacheLoad::Empty { reason } => panic!("round trip failed: {reason}"),
        }
    }

    #[test]
    fn truncation_sweep_never_panics_never_loads() {
        let bytes = serialize(&fixture(), STAMP, SIZE);
        for len in 0..bytes.len() {
            match parse(&bytes[..len], STAMP, SIZE) {
                CacheLoad::Empty { .. } => {}
                CacheLoad::Loaded(_) => panic!("prefix {len} parsed as Loaded"),
            }
        }
    }

    #[test]
    fn invalidators() {
        let good = serialize(&fixture(), STAMP, SIZE);

        let mut bad_magic = good.clone();
        bad_magic[0] ^= 0xFF;
        assert_eq!(expect_empty(&bad_magic), "bad magic");

        let mut bad_version = good.clone();
        bad_version[8] = 0xEE; // format_version LSB
        assert_eq!(expect_empty(&bad_version), "unknown format version");

        // Stamp / size mismatch: parse with different expectations.
        match parse(&good, STAMP ^ 1, SIZE) {
            CacheLoad::Empty { reason } => assert_eq!(reason, "gamemdx build mismatch"),
            CacheLoad::Loaded(_) => panic!("stamp mismatch accepted"),
        }
        match parse(&good, STAMP, SIZE + 1) {
            CacheLoad::Empty { reason } => assert_eq!(reason, "gamemdx build mismatch"),
            CacheLoad::Loaded(_) => panic!("size mismatch accepted"),
        }
    }

    #[test]
    fn absurd_counts_rejected() {
        // entry_count over cap (header is 8+4+4+4 = 20 bytes in).
        let mut bytes = serialize(&CacheFile::default(), STAMP, SIZE);
        bytes[20..24].copy_from_slice(&(MAX_ENTRIES + 1).to_le_bytes());
        assert_eq!(expect_empty(&bytes), "entry count over cap");

        // payload_count over cap.
        let one = CacheFile {
            entries: vec![FileEntry {
                game_path: "x".into(),
                identity: Identity::Absent,
                payloads: vec![],
            }],
        };
        let mut bytes = serialize(&one, STAMP, SIZE);
        let last = bytes.len() - 1; // trailing byte is payload_count
        bytes[last] = MAX_PAYLOADS + 1;
        assert_eq!(expect_empty(&bytes), "payload count over cap");

        // String length over cap.
        let mut bytes = serialize(&one, STAMP, SIZE);
        bytes[24..26].copy_from_slice(&(MAX_STR + 1).to_le_bytes());
        assert_eq!(expect_empty(&bytes), "bad entry path");
    }

    #[test]
    fn trailing_garbage_rejected() {
        let mut bytes = serialize(&fixture(), STAMP, SIZE);
        bytes.push(0);
        assert_eq!(expect_empty(&bytes), "trailing garbage");
    }

    #[test]
    fn empty_cache_round_trips() {
        let bytes = serialize(&CacheFile::default(), STAMP, SIZE);
        match parse(&bytes, STAMP, SIZE) {
            CacheLoad::Loaded(parsed) => assert!(parsed.entries.is_empty()),
            CacheLoad::Empty { reason } => panic!("empty doc failed: {reason}"),
        }
    }

    #[test]
    fn normalize_ssq_rel_strips_data_prefix() {
        assert_eq!(
            normalize_ssq_rel("data/mdb_apx/ssq/puty.ssq").as_deref(),
            Some("mdb_apx/ssq/puty.ssq")
        );
        // Backslashes normalize; the first `data/` wins.
        assert_eq!(
            normalize_ssq_rel("data\\mdb_apx\\ssq\\x.ssq").as_deref(),
            Some("mdb_apx/ssq/x.ssq")
        );
        // Absolute / prefixed forms still find the segment.
        assert_eq!(
            normalize_ssq_rel("/dev/nvme/data/mdb_apx/ssq/y.ssq").as_deref(),
            Some("mdb_apx/ssq/y.ssq")
        );
        assert_eq!(normalize_ssq_rel("nonsense/path.ssq"), None);
    }

    #[test]
    fn resolve_identity_prefers_mod_override() {
        let modh = Some(("data_mods/x/mdb_apx/ssq/m.ssq".to_string(), 10, 100));
        let stockh = Some(("data/mdb_apx/ssq/m.ssq".to_string(), 20, 200));
        // Mod override wins over the stock path.
        assert_eq!(
            resolve_identity(modh.clone(), stockh.clone()),
            Identity::File {
                resolved_path: "data_mods/x/mdb_apx/ssq/m.ssq".into(),
                size: 10,
                mtime_secs: 100,
            }
        );
        // Only stock present → stock backs it.
        assert_eq!(
            resolve_identity(None, stockh),
            Identity::File {
                resolved_path: "data/mdb_apx/ssq/m.ssq".into(),
                size: 20,
                mtime_secs: 200,
            }
        );
        // Neither present → Absent.
        assert_eq!(resolve_identity(None, None), Identity::Absent);
    }

    #[test]
    fn identity_matches_rules() {
        let a = Identity::File {
            resolved_path: "p".into(),
            size: 1,
            mtime_secs: 2,
        };
        let a2 = a.clone();
        let diff_path = Identity::File {
            resolved_path: "q".into(),
            size: 1,
            mtime_secs: 2,
        };
        let diff_size = Identity::File {
            resolved_path: "p".into(),
            size: 9,
            mtime_secs: 2,
        };
        let diff_mtime = Identity::File {
            resolved_path: "p".into(),
            size: 1,
            mtime_secs: 9,
        };
        assert!(identity_matches(&a, &a2));
        assert!(!identity_matches(&a, &diff_path));
        assert!(!identity_matches(&a, &diff_size));
        assert!(!identity_matches(&a, &diff_mtime));
        assert!(identity_matches(&Identity::Absent, &Identity::Absent));
        // present↔absent never matches (either direction).
        assert!(!identity_matches(&a, &Identity::Absent));
        assert!(!identity_matches(&Identity::Absent, &a));
    }

    #[test]
    fn merge_fresh_wins_and_appends() {
        let loaded = fixture(); // puty (File), gone (Absent), mod (File, empty payloads)
                                // Fresh: override puty with new identity+payloads, flip gone to present,
                                // add a brand-new file.
        let fresh = vec![
            FileEntry {
                game_path: "data/mdb_apx/ssq/puty.ssq".into(),
                identity: Identity::File {
                    resolved_path: "data/mdb_apx/ssq/puty.ssq".into(),
                    size: 777, // changed
                    mtime_secs: 999,
                },
                payloads: vec![payload(0, 0, 100.0, 200.0)],
            },
            FileEntry {
                game_path: "data/mdb_apx/ssq/gone.ssq".into(),
                identity: Identity::File {
                    resolved_path: "data/mdb_apx/ssq/gone.ssq".into(),
                    size: 5,
                    mtime_secs: 5,
                },
                payloads: vec![],
            },
            FileEntry {
                game_path: "data/mdb_apx/ssq/new.ssq".into(),
                identity: Identity::Absent,
                payloads: vec![],
            },
        ];
        let merged = merge(loaded, fresh);
        // Original three entries preserved in place + one appended.
        assert_eq!(merged.entries.len(), 4);
        let by_path = |p: &str| merged.entries.iter().find(|e| e.game_path == p).unwrap();
        // puty overridden (fresh identity wins).
        assert_eq!(
            by_path("data/mdb_apx/ssq/puty.ssq").identity,
            Identity::File {
                resolved_path: "data/mdb_apx/ssq/puty.ssq".into(),
                size: 777,
                mtime_secs: 999,
            }
        );
        // gone flipped Absent→present.
        assert!(matches!(
            by_path("data/mdb_apx/ssq/gone.ssq").identity,
            Identity::File { .. }
        ));
        // The untouched mod.ssq entry survives.
        assert!(merged
            .entries
            .iter()
            .any(|e| e.game_path == "data_mods/customs/ssq/mod.ssq"));
        // new.ssq appended.
        assert_eq!(
            by_path("data/mdb_apx/ssq/new.ssq").identity,
            Identity::Absent
        );
    }

    #[test]
    fn merge_unions_payloads_when_identity_unchanged() {
        // A file cached with all 4 (diff,mode) payloads; the replay boot only
        // re-captures one of them stock (the final-song case). With identity
        // unchanged the other three cached payloads must survive.
        let ident = Identity::File {
            resolved_path: "data/mdb_apx/ssq/a.ssq".into(),
            size: 100,
            mtime_secs: 200,
        };
        let full = vec![
            payload(0, 0, 60.0, 120.0),
            payload(0, 1, 60.0, 120.0),
            payload(1, 0, 60.0, 120.0),
            payload(1, 1, 60.0, 120.0),
        ];
        let loaded = CacheFile {
            entries: vec![FileEntry {
                game_path: "data/mdb_apx/ssq/a.ssq".into(),
                identity: ident.clone(),
                payloads: full,
            }],
        };
        // Fresh: same identity, only (0,0) re-captured with a new max BPM.
        let fresh_slot = payload(0, 0, 60.0, 300.0);
        let fresh = vec![FileEntry {
            game_path: "data/mdb_apx/ssq/a.ssq".into(),
            identity: ident.clone(),
            payloads: vec![fresh_slot.clone()],
        }];
        let merged = merge(loaded, fresh);
        assert_eq!(merged.entries.len(), 1);
        let e = &merged.entries[0];
        // All four difficulties/modes preserved (union), sorted.
        assert_eq!(e.payloads.len(), 4);
        assert_eq!(
            e.payloads
                .iter()
                .map(|p| (p.difficulty, p.mode))
                .collect::<Vec<_>>(),
            vec![(0, 0), (0, 1), (1, 0), (1, 1)]
        );
        // The (0,0) slot is the FRESH one (fresh wins on collision).
        assert_eq!(e.payloads[0], fresh_slot);
    }

    #[test]
    fn merge_replaces_wholesale_when_identity_changed() {
        let loaded = CacheFile {
            entries: vec![FileEntry {
                game_path: "data/mdb_apx/ssq/a.ssq".into(),
                identity: Identity::File {
                    resolved_path: "data/mdb_apx/ssq/a.ssq".into(),
                    size: 100,
                    mtime_secs: 200,
                },
                payloads: vec![payload(0, 0, 60.0, 120.0), payload(1, 0, 60.0, 120.0)],
            }],
        };
        // Same path, DIFFERENT identity (file changed) → drop stale payloads.
        let fresh = vec![FileEntry {
            game_path: "data/mdb_apx/ssq/a.ssq".into(),
            identity: Identity::File {
                resolved_path: "data/mdb_apx/ssq/a.ssq".into(),
                size: 999, // changed
                mtime_secs: 200,
            },
            payloads: vec![payload(0, 0, 80.0, 160.0)],
        }];
        let merged = merge(loaded, fresh);
        assert_eq!(merged.entries.len(), 1);
        assert_eq!(merged.entries[0].payloads.len(), 1); // wholesale replace
        assert_eq!(
            merged.entries[0].identity,
            Identity::File {
                resolved_path: "data/mdb_apx/ssq/a.ssq".into(),
                size: 999,
                mtime_secs: 200,
            }
        );
    }
}
