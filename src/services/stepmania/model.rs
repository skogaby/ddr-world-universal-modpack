//! Index records and on-disk tables for the StepMania pack loader — pure.
//!
//! [`SongEntry`] is one indexed song (design §5.1): identity, display text,
//! declared levels, the audio facts the bank builder needs without
//! re-probing, the art source and the fingerprint that decides whether a
//! warm boot may reuse the song's artefacts. [`IndexFile`] (`index.bin`) holds
//! every entry plus the fingerprinted rejections so a warm boot neither
//! re-parses unchanged songs nor re-converts songs it already knows it must
//! skip. [`LruTable`] (`lru.bin`) is the byte-budgeted touch-order ledger
//! over the on-demand artefacts (`xwb/`, `jacket/`).
//!
//! Binary formats are hand-rolled little-endian, length-prefixed and fully
//! bounds-checked; any malformation parses to `None` and the caller treats
//! the file as absent (a cold rebuild — the artefacts are regenerable).
//! [`FORMAT_VERSION`] covers every gamemdx-independent conversion parameter
//! (writer rules, lead-in rule, bank layout, art pipeline): bump it when any
//! of those changes and every song re-converts on the next boot.
//!
//! Purity contract: `std` only, no logging, no `crate::` imports outside
//! `crate::core`. Mounted by `scripts/validate_stepmania.sh` and
//! `tools/sm_pack_check`.

use std::collections::HashMap;

use crate::core::sm::scroll::{ScrollGimmicks, ScrollTable, SpeedTable};

/// `index.bin` magic.
pub const INDEX_MAGIC: [u8; 8] = *b"DDRSMIX1";
/// Bump on ANY change to the conversion pipeline or these layouts.
pub const FORMAT_VERSION: u32 = 5; // 5: native display titles + title_sort (4: split-timing files + split_levels)
/// Largest scroll/speed table stored per song (corrupt-count guard).
pub const MAX_SCROLL_SEGMENTS: u32 = 65_536;
/// `lru.bin` magic.
pub const LRU_MAGIC: [u8; 8] = *b"DDRSMLRU";
/// Sanity caps for corrupt counts.
pub const MAX_ENTRIES: u32 = 100_000;
pub const MAX_STR: u32 = 4096;

/// Slot count of the musicdb level/limited arrays.
pub const SLOTS: usize = 10;
/// Split-file suffixes `N` = 1..=5 (`<b>_N.ssq`), index `N − 1`.
pub const SPLIT_FILES: usize = 5;

/// What the bank builder needs to re-create the audio plan without parsing
/// the simfile again. Frames are at the normalised (bank) rate.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudioMeta {
    /// Audio file path relative to the song folder.
    pub rel_path: String,
    /// Bank sample rate (44 100 or 48 000).
    pub rate: u32,
    /// Normalised song frames (before lead-in).
    pub frames: u64,
    /// Lead-in silence in frames (whole ADPCM blocks) and the matching ms the
    /// SSQ was shifted by.
    pub lead_in_frames: u64,
    pub lead_in_ms: u32,
    /// Preview window in normalised frames.
    pub preview_start: u64,
    pub preview_len: u64,
}

/// Where the jacket comes from; `None` = placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArtMeta {
    /// Path relative to the songs root (`<pack>/<song>/x.png` or `<pack>/x.png`).
    pub source: Option<String>,
    /// `jacket::SourceKind::tag()`; 0 when `source` is `None`.
    pub kind: u8,
}

/// One indexed song.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SongEntry {
    pub pack: String,
    pub folder: String,
    /// 6 ASCII chars (`z` + 5 base36).
    pub basename: String,
    pub mcode: u32,
    /// Wheel/stage title: native when the title fonts can draw it, else the
    /// transliteration (`convert::display_title`).
    pub title: String,
    /// `title_yomi` source: the transliterated title when present, else the
    /// native one (`convert::sort_title`); empty = same as `title`.
    pub title_sort: String,
    pub artist: String,
    pub bpm_min: u16,
    pub bpm_max: u16,
    /// Level per musicdb slot (0 = absent); slot = difficulty + 5·double.
    pub levels: [u8; SLOTS],
    pub audio: AudioMeta,
    pub art: ArtMeta,
    /// fnv1a64 over the (path, size, mtime) of simfile + audio + art and
    /// [`FORMAT_VERSION`].
    pub fingerprint: u64,
    /// A bank build failed for this song last boot; it is rejected until its
    /// fingerprint changes.
    pub audio_failed: bool,
    /// Display-only `#SCROLLS`/`#SPEEDS` of the committed charts, resolved
    /// through the chart's tempo map; `None` = identity (nothing to remap).
    pub scroll: Option<ScrollGimmicks>,
    /// Music-count (SSQ tempo ms) of beat 0 in the committed `<b>.ssq` —
    /// `tempo_data[0]` = `−#OFFSET·1000 + lead_in_ms`. Converts the
    /// renderer's clock into the speed table's elapsed-seconds domain.
    pub beat0_ms: i32,
    /// Per-chart-timing split files: `split_levels[N − 1]` is the bitmask of
    /// levels (bit `d`, 0..4 = Beginner..Challenge, either style) whose
    /// charts `ssq/<b>_N.ssq` holds and passed the self-check; all zero when
    /// the song is one `ssq/<b>.ssq`. The writer emits `N = level + 1`, so
    /// each non-zero entry is normally `1 << (N − 1)`.
    pub split_levels: [u8; SPLIT_FILES],
}

impl SongEntry {
    /// `<pack>/<song>` identity key (casefolded by `identity::key`).
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}/{}", self.pack, self.folder)
    }

    #[must_use]
    pub fn declared_slots(&self) -> Vec<u8> {
        (0..SLOTS as u8)
            .filter(|&s| self.levels[s as usize] != 0)
            .collect()
    }

    /// Whether any `<b>_N.ssq` exists for this song.
    #[must_use]
    pub fn has_split_files(&self) -> bool {
        self.split_levels.iter().any(|&m| m != 0)
    }

    /// The `N` of `<b>_N.ssq` that answers `build_ssq_path(b, d)`, or `None`
    /// for the unsplit `<b>.ssq`. Rule A (`split_ssq_auto_discovery::resolver`):
    /// the highest `N ≤ d + 1` whose file holds a level-`d` chart.
    #[must_use]
    pub fn split_file_for(&self, d: usize) -> Option<u8> {
        split_file_for(&self.split_levels, d)
    }
}

/// Rule-A lookup over a split table (see [`SongEntry::split_file_for`]).
/// `d` outside `0..5` is `None`.
#[must_use]
pub fn split_file_for(split_levels: &[u8; SPLIT_FILES], d: usize) -> Option<u8> {
    if d >= SPLIT_FILES {
        return None;
    }
    (1..=d + 1)
        .rev()
        .find(|&n| split_levels[n - 1] & (1 << d) != 0)
        .map(|n| n as u8)
}

/// A song the index knows it must skip, remembered so a warm boot does not
/// redo the work of discovering that.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RejectEntry {
    pub pack: String,
    pub folder: String,
    pub fingerprint: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexFile {
    pub songs: Vec<SongEntry>,
    pub rejects: Vec<RejectEntry>,
}

// ---------------------------------------------------------------------
// fnv1a64 — shared by the fingerprint and the digest
// ---------------------------------------------------------------------

#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_continue(0xcbf2_9ce4_8422_2325, bytes)
}

#[must_use]
pub fn fnv1a64_continue(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// One `(path, size, mtime)` fact folded into a fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp<'a> {
    pub path: &'a str,
    pub size: u64,
    pub mtime_secs: u64,
}

/// Fingerprint of a song's inputs: every stamp in order plus the format
/// version, so a writer change re-converts every song.
#[must_use]
pub fn fingerprint(stamps: &[FileStamp<'_>]) -> u64 {
    let mut h = fnv1a64(&FORMAT_VERSION.to_le_bytes());
    for s in stamps {
        h = fnv1a64_continue(h, s.path.as_bytes());
        h = fnv1a64_continue(h, &[0]);
        h = fnv1a64_continue(h, &s.size.to_le_bytes());
        h = fnv1a64_continue(h, &s.mtime_secs.to_le_bytes());
    }
    h
}

/// Digest of what the musicdb transform would inject: fnv over the sorted
/// `(mcode, basename, levels, title)` tuples plus the format version. The
/// `startup.arc` cache key and the thumbnail arc sidecar carry it.
#[must_use]
pub fn index_digest(songs: &[SongEntry]) -> u64 {
    let mut keys: Vec<(u32, &str, [u8; SLOTS], &str)> = songs
        .iter()
        .map(|s| (s.mcode, s.basename.as_str(), s.levels, s.title.as_str()))
        .collect();
    keys.sort();
    let mut h = fnv1a64(&FORMAT_VERSION.to_le_bytes());
    for (mcode, basename, levels, title) in keys {
        h = fnv1a64_continue(h, &mcode.to_le_bytes());
        h = fnv1a64_continue(h, basename.as_bytes());
        h = fnv1a64_continue(h, &levels);
        h = fnv1a64_continue(h, title.as_bytes());
        h = fnv1a64_continue(h, &[0]);
    }
    h
}

// ---------------------------------------------------------------------
// Binary cursor
// ---------------------------------------------------------------------

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8)
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .map(u64::from_le_bytes)
    }
    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }
    fn f64(&mut self) -> Option<f64> {
        self.u64().map(f64::from_bits)
    }
    fn str(&mut self) -> Option<String> {
        let len = self.u32()?;
        if len > MAX_STR {
            return None;
        }
        let bytes = self.take(len as usize)?;
        String::from_utf8(bytes.to_vec()).ok()
    }
    fn done(&self) -> bool {
        self.pos == self.data.len()
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(MAX_STR as usize);
    out.extend_from_slice(&(len as u32).to_le_bytes());
    out.extend_from_slice(&bytes[..len]);
}

fn put_f64(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_bits().to_le_bytes());
}

/// `scroll` record: `u8` present flag, then the scroll `(beat, factor)`
/// pairs and the resolved speed `(beat, ratio, delay, start_s, end_s)`
/// segments, each as a `u32` count + f64 fields.
fn put_scroll(out: &mut Vec<u8>, scroll: Option<&ScrollGimmicks>) {
    let Some(g) = scroll else {
        out.push(0);
        return;
    };
    out.push(1);
    let scrolls: Vec<(f64, f64)> = g.scroll.entries().collect();
    out.extend_from_slice(&(scrolls.len() as u32).to_le_bytes());
    for (b, f) in scrolls {
        put_f64(out, b);
        put_f64(out, f);
    }
    let speeds: Vec<(f64, f64, f64, f64, f64)> = g.speed.entries().collect();
    out.extend_from_slice(&(speeds.len() as u32).to_le_bytes());
    for (b, r, d, s, e) in speeds {
        put_f64(out, b);
        put_f64(out, r);
        put_f64(out, d);
        put_f64(out, s);
        put_f64(out, e);
    }
}

fn get_scroll(c: &mut Cursor<'_>) -> Option<Option<ScrollGimmicks>> {
    match c.u8()? {
        0 => Some(None),
        1 => {
            let n = c.u32()?;
            if n > MAX_SCROLL_SEGMENTS {
                return None;
            }
            let mut pairs = Vec::with_capacity(n as usize);
            for _ in 0..n {
                pairs.push((c.f64()?, c.f64()?));
            }
            let m = c.u32()?;
            if m > MAX_SCROLL_SEGMENTS {
                return None;
            }
            let mut segs = Vec::with_capacity(m as usize);
            for _ in 0..m {
                segs.push((c.f64()?, c.f64()?, c.f64()?, c.f64()?, c.f64()?));
            }
            Some(Some(ScrollGimmicks {
                scroll: ScrollTable::from_pairs(&pairs),
                speed: SpeedTable::from_segments(&segs),
            }))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------
// index.bin
// ---------------------------------------------------------------------

/// Serialize an index file.
#[must_use]
pub fn serialize_index(file: &IndexFile) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + file.songs.len() * 256);
    out.extend_from_slice(&INDEX_MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(file.songs.len() as u32).to_le_bytes());
    for s in &file.songs {
        put_str(&mut out, &s.pack);
        put_str(&mut out, &s.folder);
        put_str(&mut out, &s.basename);
        out.extend_from_slice(&s.mcode.to_le_bytes());
        put_str(&mut out, &s.title);
        put_str(&mut out, &s.title_sort);
        put_str(&mut out, &s.artist);
        out.extend_from_slice(&s.bpm_min.to_le_bytes());
        out.extend_from_slice(&s.bpm_max.to_le_bytes());
        out.extend_from_slice(&s.levels);
        put_str(&mut out, &s.audio.rel_path);
        out.extend_from_slice(&s.audio.rate.to_le_bytes());
        out.extend_from_slice(&s.audio.frames.to_le_bytes());
        out.extend_from_slice(&s.audio.lead_in_frames.to_le_bytes());
        out.extend_from_slice(&s.audio.lead_in_ms.to_le_bytes());
        out.extend_from_slice(&s.audio.preview_start.to_le_bytes());
        out.extend_from_slice(&s.audio.preview_len.to_le_bytes());
        put_str(&mut out, s.art.source.as_deref().unwrap_or(""));
        out.push(s.art.kind);
        out.extend_from_slice(&s.fingerprint.to_le_bytes());
        out.push(u8::from(s.audio_failed));
        put_scroll(&mut out, s.scroll.as_ref());
        out.extend_from_slice(&s.beat0_ms.to_le_bytes());
        out.extend_from_slice(&s.split_levels);
    }
    out.extend_from_slice(&(file.rejects.len() as u32).to_le_bytes());
    for r in &file.rejects {
        put_str(&mut out, &r.pack);
        put_str(&mut out, &r.folder);
        out.extend_from_slice(&r.fingerprint.to_le_bytes());
        put_str(&mut out, &r.reason);
    }
    out
}

/// Parse an index file; `None` on any malformation or a version mismatch.
#[must_use]
pub fn parse_index(bytes: &[u8]) -> Option<IndexFile> {
    let mut c = Cursor::new(bytes);
    if c.take(8)? != INDEX_MAGIC {
        return None;
    }
    if c.u32()? != FORMAT_VERSION {
        return None;
    }
    let n = c.u32()?;
    if n > MAX_ENTRIES {
        return None;
    }
    let mut songs = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let pack = c.str()?;
        let folder = c.str()?;
        let basename = c.str()?;
        let mcode = c.u32()?;
        let title = c.str()?;
        let title_sort = c.str()?;
        let artist = c.str()?;
        let bpm_min = c.u16()?;
        let bpm_max = c.u16()?;
        let mut levels = [0u8; SLOTS];
        levels.copy_from_slice(c.take(SLOTS)?);
        let rel_path = c.str()?;
        let rate = c.u32()?;
        let frames = c.u64()?;
        let lead_in_frames = c.u64()?;
        let lead_in_ms = c.u32()?;
        let preview_start = c.u64()?;
        let preview_len = c.u64()?;
        let source = c.str()?;
        let kind = c.u8()?;
        let fingerprint = c.u64()?;
        let audio_failed = c.u8()? != 0;
        let scroll = get_scroll(&mut c)?;
        let beat0_ms = c.i32()?;
        let mut split_levels = [0u8; SPLIT_FILES];
        split_levels.copy_from_slice(c.take(SPLIT_FILES)?);
        songs.push(SongEntry {
            pack,
            folder,
            basename,
            mcode,
            title,
            title_sort,
            artist,
            bpm_min,
            bpm_max,
            levels,
            audio: AudioMeta {
                rel_path,
                rate,
                frames,
                lead_in_frames,
                lead_in_ms,
                preview_start,
                preview_len,
            },
            art: ArtMeta {
                source: if source.is_empty() {
                    None
                } else {
                    Some(source)
                },
                kind,
            },
            fingerprint,
            audio_failed,
            scroll,
            beat0_ms,
            split_levels,
        });
    }
    let m = c.u32()?;
    if m > MAX_ENTRIES {
        return None;
    }
    let mut rejects = Vec::with_capacity(m as usize);
    for _ in 0..m {
        rejects.push(RejectEntry {
            pack: c.str()?,
            folder: c.str()?,
            fingerprint: c.u64()?,
            reason: c.str()?,
        });
    }
    if !c.done() {
        return None;
    }
    Some(IndexFile { songs, rejects })
}

// ---------------------------------------------------------------------
// LRU ledger (lru.bin)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LruEntry {
    /// Cache-relative path (`xwb/<b>.xwb`, `xwb/<b>.p.xwb`, `jacket/<b>_jk.arc`).
    pub key: String,
    pub bytes: u64,
    /// Logical clock of the last touch (monotonic within the table).
    pub touch: u64,
}

/// Byte-budgeted least-recently-served table. Pure accounting: the caller
/// deletes the files named by [`LruTable::insert`]'s eviction list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LruTable {
    entries: Vec<LruEntry>,
    clock: u64,
}

impl LruTable {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            clock: 0,
        }
    }

    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.bytes).sum()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.entries.iter().any(|e| e.key == key)
    }

    #[must_use]
    pub fn entries(&self) -> &[LruEntry] {
        &self.entries
    }

    fn tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    /// Mark `key` as just served. Returns `true` when the order changed
    /// (the key exists and was not already the most recent) — the caller's
    /// cue to persist. Unknown keys are ignored.
    pub fn touch(&mut self, key: &str) -> bool {
        let newest = self.entries.iter().map(|e| e.touch).max().unwrap_or(0);
        let Some(idx) = self.entries.iter().position(|e| e.key == key) else {
            return false;
        };
        if newest != 0 && self.entries[idx].touch == newest {
            return false;
        }
        let t = self.tick();
        self.entries[idx].touch = t;
        true
    }

    /// Record a freshly written artefact (replacing any entry of the same
    /// key) and evict least-recently-touched OTHER entries until the total
    /// fits `budget_bytes`. Returns the evicted keys in eviction order. The
    /// new entry is never evicted, even when it alone exceeds the budget.
    pub fn insert(&mut self, key: &str, bytes: u64, budget_bytes: u64) -> Vec<String> {
        self.entries.retain(|e| e.key != key);
        let t = self.tick();
        self.entries.push(LruEntry {
            key: key.to_string(),
            bytes,
            touch: t,
        });
        let mut evicted = Vec::new();
        while self.total_bytes() > budget_bytes {
            let victim = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.key != key)
                .min_by_key(|(_, e)| (e.touch, e.key.clone()))
                .map(|(i, _)| i);
            match victim {
                Some(i) => evicted.push(self.entries.remove(i).key),
                None => break,
            }
        }
        evicted
    }

    /// Forget `key` (the caller deleted the file, or it vanished).
    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.key != key);
        before != self.entries.len()
    }

    /// Reconcile with what is actually on disk: entries whose file is gone
    /// are dropped; files the table does not know are adopted with the
    /// oldest possible touch (so they are evicted first) and their real size.
    pub fn reconcile(&mut self, on_disk: &HashMap<String, u64>) {
        self.entries.retain(|e| on_disk.contains_key(&e.key));
        for e in &mut self.entries {
            if let Some(&size) = on_disk.get(&e.key) {
                e.bytes = size;
            }
        }
        let mut adopted: Vec<(&String, &u64)> = on_disk
            .iter()
            .filter(|(k, _)| !self.entries.iter().any(|e| &e.key == *k))
            .collect();
        adopted.sort();
        for (k, &size) in adopted {
            self.entries.push(LruEntry {
                key: k.clone(),
                bytes: size,
                touch: 0,
            });
        }
    }

    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.entries.len() * 48);
        out.extend_from_slice(&LRU_MAGIC);
        out.extend_from_slice(&self.clock.to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            put_str(&mut out, &e.key);
            out.extend_from_slice(&e.bytes.to_le_bytes());
            out.extend_from_slice(&e.touch.to_le_bytes());
        }
        out
    }

    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let mut c = Cursor::new(bytes);
        if c.take(8)? != LRU_MAGIC {
            return None;
        }
        let clock = c.u64()?;
        let n = c.u32()?;
        if n > MAX_ENTRIES {
            return None;
        }
        let mut entries = Vec::with_capacity(n as usize);
        for _ in 0..n {
            entries.push(LruEntry {
                key: c.str()?,
                bytes: c.u64()?,
                touch: c.u64()?,
            });
        }
        if !c.done() {
            return None;
        }
        Some(Self { entries, clock })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(n: u32) -> SongEntry {
        SongEntry {
            pack: "Pack".into(),
            folder: format!("Song {n}"),
            basename: format!("z{n:05}"),
            mcode: 9_000_000 + n,
            title: format!("Title {n}"),
            title_sort: String::new(),
            artist: "Artist".into(),
            bpm_min: 120,
            bpm_max: 180,
            levels: [0, 4, 8, 12, 0, 0, 0, 0, 0, 0],
            audio: AudioMeta {
                rel_path: "song.ogg".into(),
                rate: 44_100,
                frames: 4_000_000,
                lead_in_frames: 128 * 606,
                lead_in_ms: 1759,
                preview_start: 1_000_000,
                preview_len: 661_504,
            },
            art: ArtMeta {
                source: Some("Pack/Song/bn.png".into()),
                kind: 2,
            },
            fingerprint: 0xDEAD_BEEF_0000_0000 | u64::from(n),
            audio_failed: n % 2 == 1,
            scroll: (n % 2 == 0).then(|| ScrollGimmicks {
                scroll: ScrollTable::from_pairs(&[(0.0, 1.0), (116.0, 0.0), (116.5, 1.0)]),
                speed: SpeedTable::from_segments(&[
                    (0.0, 1.0, 0.0, 0.0, 0.0),
                    (160.0, 0.0, 0.0, 80.0, 80.0),
                    (160.021, 1.0, 6.0, 80.0105, 83.0105),
                ]),
            }),
            beat0_ms: 1759 + 241,
            split_levels: if n % 2 == 0 {
                [0, 0, 0, 1 << 3, 1 << 4]
            } else {
                [0; SPLIT_FILES]
            },
        }
    }

    #[test]
    fn split_file_for_follows_rule_a() {
        // Unsplit song: every slot is the base file.
        let plain = song(1);
        assert!(!plain.has_split_files());
        assert_eq!(
            (0..5).map(|d| plain.split_file_for(d)).collect::<Vec<_>>(),
            vec![None; 5]
        );
        // Writer shape: `_4` holds Expert, `_5` holds Challenge.
        let split = song(2);
        assert!(split.has_split_files());
        assert_eq!(
            (0..5).map(|d| split.split_file_for(d)).collect::<Vec<_>>(),
            vec![None, None, None, Some(4), Some(5)]
        );
        assert_eq!(split.split_file_for(5), None);
        // Rule A: the highest N ≤ d+1 holding level d; a file past d+1 is ignored.
        let table = [0, 0, (1 << 2) | (1 << 3), 0, 1 << 4];
        assert_eq!(split_file_for(&table, 2), Some(3));
        assert_eq!(split_file_for(&table, 3), Some(3));
        assert_eq!(split_file_for(&table, 4), Some(5));
        assert_eq!(split_file_for(&table, 1), None);
        let late = [0, 0, 1 << 1, 0, 0]; // Basic copy in _3: N=3 > d+1=2
        assert_eq!(split_file_for(&late, 1), None);
    }

    #[test]
    fn index_round_trips_and_rejects_garbage() {
        let file = IndexFile {
            songs: vec![song(1), song(2)],
            rejects: vec![RejectEntry {
                pack: "Pack".into(),
                folder: "Broken".into(),
                fingerprint: 7,
                reason: "no playable chart".into(),
            }],
        };
        let bytes = serialize_index(&file);
        assert_eq!(parse_index(&bytes), Some(file.clone()));
        let back = parse_index(&bytes).unwrap();
        assert!(back.songs[0].scroll.is_none() && back.songs[1].scroll.is_some());
        let g = back.songs[1].scroll.as_ref().unwrap();
        assert!((g.scroll.display_beat(116.25) - 116.0).abs() < 1e-9);
        assert!((g.speed.speed_at(163.021, 81.5105) - 0.5).abs() < 1e-6);
        assert_eq!(back.songs[1].beat0_ms, 2000);
        assert_eq!(back.songs[1].split_levels, [0, 0, 0, 8, 16]);
        assert!(!back.songs[0].has_split_files());
        assert!(parse_index(&bytes[..bytes.len() - 1]).is_none());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(parse_index(&extra).is_none());
        let mut wrong_version = bytes.clone();
        wrong_version[8] ^= 1;
        assert!(parse_index(&wrong_version).is_none());
        assert!(parse_index(b"").is_none());
    }

    #[test]
    fn digest_is_order_independent_and_content_sensitive() {
        let a = index_digest(&[song(1), song(2)]);
        let b = index_digest(&[song(2), song(1)]);
        assert_eq!(a, b);
        let mut changed = song(2);
        changed.levels[3] = 13;
        assert_ne!(a, index_digest(&[song(1), changed]));
        assert_ne!(a, index_digest(&[song(1)]));
    }

    #[test]
    fn fingerprint_changes_with_any_stamp() {
        let base = [FileStamp {
            path: "a.ssc",
            size: 10,
            mtime_secs: 100,
        }];
        let f0 = fingerprint(&base);
        assert_eq!(f0, fingerprint(&base));
        assert_ne!(
            f0,
            fingerprint(&[FileStamp {
                path: "a.ssc",
                size: 11,
                mtime_secs: 100,
            }])
        );
        assert_ne!(
            f0,
            fingerprint(&[FileStamp {
                path: "a.ssc",
                size: 10,
                mtime_secs: 101,
            }])
        );
    }

    #[test]
    fn lru_evicts_least_recent_other_entries_only() {
        let mut lru = LruTable::new();
        assert!(lru.insert("xwb/a.xwb", 40, 100).is_empty());
        assert!(lru.insert("xwb/b.xwb", 40, 100).is_empty());
        lru.touch("xwb/a.xwb");
        // c pushes total to 120 > 100: evict b (least recent), not a.
        assert_eq!(
            lru.insert("xwb/c.xwb", 40, 100),
            vec!["xwb/b.xwb".to_string()]
        );
        assert_eq!(lru.total_bytes(), 80);
        // An oversize insert evicts everything else but keeps itself.
        assert_eq!(
            lru.insert("xwb/big.xwb", 500, 100),
            vec!["xwb/a.xwb".to_string(), "xwb/c.xwb".to_string()]
        );
        assert_eq!(lru.len(), 1);
        assert!(lru.contains("xwb/big.xwb"));
        // Re-insert replaces.
        lru.insert("xwb/big.xwb", 10, 100);
        assert_eq!(lru.total_bytes(), 10);
        assert!(!lru.touch("nope"));
        // Touching the most recent entry again changes nothing.
        assert!(!lru.touch("xwb/big.xwb"));
        assert!(lru.remove("xwb/big.xwb"));
        assert!(lru.is_empty());
    }

    #[test]
    fn lru_round_trips_and_reconciles() {
        let mut lru = LruTable::new();
        lru.insert("jacket/a_jk.arc", 5, 1000);
        lru.insert("xwb/a.xwb", 50, 1000);
        let bytes = lru.serialize();
        assert_eq!(LruTable::parse(&bytes), Some(lru.clone()));
        assert!(LruTable::parse(&bytes[..5]).is_none());
        let mut on_disk = HashMap::new();
        on_disk.insert("xwb/a.xwb".to_string(), 60u64);
        on_disk.insert("xwb/orphan.xwb".to_string(), 7u64);
        lru.reconcile(&on_disk);
        assert!(!lru.contains("jacket/a_jk.arc"));
        assert_eq!(lru.total_bytes(), 67);
        // The orphan has touch 0 → first out.
        assert_eq!(
            lru.insert("xwb/new.xwb", 40, 100),
            vec!["xwb/orphan.xwb".to_string()]
        );
    }
}
