//! DDR SELECTION era-sound bank builder (pure, host-tested).
//!
//! Builds ONE in-memory XACT bank pair (`dsel`) holding exactly the cues a
//! legacy skin plays, copied out of the A3-generation banks World ships but
//! never loads (`voice_n.xsb`/`voice_n.xwb`, `se_normal_n.xsb`/`se_normal_n.xwb`).
//! Nothing is re-encoded: sound entries, complex cues and variation tables are
//! copied byte for byte with only their cross-references (sound offsets, wave
//! indices, the wave-bank index) remapped, and every wave's metadata and
//! sample bytes are copied verbatim into one merged in-memory wave bank.
//!
//! Layout rules are the engine's (`xactengine2_10.dll` `FUN_0040e970` /
//! `FUN_0040d810` XSB validator, `FUN_0040f120` XWB validator — RE records
//! `.agents/planning/20260725-assist-tick/research/xact-bank-format.md` §3/§5
//! and `.agents/planning/2026-09-22-ddr-selection/progress.md`):
//!
//! ```text
//! XSB: header 0x4A | soundbank name 64 | wave-bank names 64×n | sounds
//!      | simple cues (5 B) | complex cues (15 B) | variation tables, in
//!      complex-cue order, contiguous (0x32 = the first one) | hash table
//!      (max(16, cues) × u16) | name index (6 B/cue) | names to EOF
//! XWB: header 52 | bank data 96 @0x34 | entry metadata 24×n @0x94 | seek
//!      tables (empty) | entry names 64×n | wave data to EOF
//! ```
//!
//! Supported shapes (the whole A3 era-cue set uses only these): single-wave
//! simple sounds (bare flags 0, or flags 0x04 with World's own RPC block),
//! simple cues, complex cues
//! over a type-1 variation table (sound entries + weights), PCM/ADPCM waves.
//! A cue outside that envelope is reported and left out.
//!
//! Dependency-free (mounted by `scripts/validate_ddr_selection.sh`).

use std::collections::HashMap;
use std::fmt;

// ── Errors / report ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// A source bank is malformed or has an unsupported layout.
    BadSource(String),
    /// A wave's bytes could not be read.
    Read(String),
    /// None of the requested cues could be built.
    Empty,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::BadSource(s) => write!(f, "bad source bank: {s}"),
            BuildError::Read(s) => write!(f, "wave read failed: {s}"),
            BuildError::Empty => write!(f, "no requested cue could be built"),
        }
    }
}

type Result<T> = std::result::Result<T, BuildError>;

fn bad(msg: impl Into<String>) -> BuildError {
    BuildError::BadSource(msg.into())
}

// ── Little-endian helpers (bounds-checked) ───────────────────────────

fn rd_u8(b: &[u8], o: usize) -> Result<u8> {
    b.get(o)
        .copied()
        .ok_or_else(|| bad(format!("read u8 past end @0x{o:X}")))
}
fn rd_u16(b: &[u8], o: usize) -> Result<u16> {
    let s = b
        .get(o..o + 2)
        .ok_or_else(|| bad(format!("read u16 past end @0x{o:X}")))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
fn rd_u32(b: &[u8], o: usize) -> Result<u32> {
    let s = b
        .get(o..o + 4)
        .ok_or_else(|| bad(format!("read u32 past end @0x{o:X}")))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn rd_i32(b: &[u8], o: usize) -> Result<i32> {
    Ok(rd_u32(b, o)? as i32)
}
fn cstr(b: &[u8], o: usize, max: usize) -> Result<String> {
    let end = (o + max).min(b.len());
    let s = b
        .get(o..end)
        .ok_or_else(|| bad(format!("string past end @0x{o:X}")))?;
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    Ok(String::from_utf8_lossy(&s[..n]).into_owned())
}
fn put_u16(buf: &mut [u8], o: usize, v: u16) {
    buf[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn put_u32(buf: &mut [u8], o: usize, v: u32) {
    buf[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn put_i32(buf: &mut [u8], o: usize, v: i32) {
    put_u32(buf, o, v as u32);
}

// ── XSB model ────────────────────────────────────────────────────────

const XSB_MAGIC: &[u8; 4] = b"SDBK";
const XSB_VERSION: u16 = 0x2B;
const XSB_HEADER: usize = 0x4A;
const NAME_FIELD: usize = 0x40;
const SOUND_BARE_LEN: usize = 12;
/// A simple sound with a runtime-parameter block (flags 0x04).
const SOUND_RPC_LEN: usize = 19;
/// The RPC block (size 7, one curve) on every flags-0x04 sound of World's own
/// loaded `se_normal.xsb` and of the A3 `se_normal_n.xsb` era SEs.
const RPC_TAIL_WORLD: [u8; 7] = [0x07, 0x00, 0x01, 0xF8, 0x00, 0x00, 0x00];
const SIMPLE_CUE_LEN: usize = 5;
const COMPLEX_CUE_LEN: usize = 15;
const VAR_HEADER_LEN: usize = 8;
const VAR1_ENTRY_LEN: usize = 6;
const NAME_INDEX_LEN: usize = 6;

/// One cue of a source sound bank, reduced to what the copy needs.
#[derive(Debug, Clone)]
pub enum CueShape {
    /// Simple cue → one sound entry (file offset of the sound).
    Simple { flags: u8, sound: usize },
    /// Complex cue → type-1 variation table over sound entries.
    Variation {
        cue: [u8; COMPLEX_CUE_LEN],
        header: [u8; VAR_HEADER_LEN],
        /// `(sound file offset, weight min, weight max)`.
        entries: Vec<(usize, u8, u8)>,
    },
    /// Anything else (interactive/transition cues, other variation types).
    Unsupported(String),
}

/// A parsed source sound bank.
#[derive(Debug, Clone)]
pub struct Xsb {
    pub name: String,
    pub wavebank_names: Vec<String>,
    /// `(cue name, shape)` in cue-index order.
    pub cues: Vec<(String, CueShape)>,
    /// Every sound entry by file offset → raw bytes (copied as-is).
    sounds: HashMap<usize, Vec<u8>>,
}

impl Xsb {
    pub fn find(&self, cue: &str) -> Option<&CueShape> {
        self.cues.iter().find(|(n, _)| n == cue).map(|(_, s)| s)
    }
    pub fn cue_names(&self) -> impl Iterator<Item = &str> {
        self.cues.iter().map(|(n, _)| n.as_str())
    }
}

/// Parse a v43 sound bank (the layout every DDR World / A3 `.xsb` uses).
pub fn parse_xsb(b: &[u8]) -> Result<Xsb> {
    if b.len() < 0x8A || &b[0..4] != XSB_MAGIC {
        return Err(bad("not an XSB (magic)"));
    }
    if rd_u16(b, 0x06)? != XSB_VERSION {
        return Err(bad("XSB tool version is not 43"));
    }
    if rd_u8(b, 0x12)? & 1 == 0 {
        return Err(bad("XSB has no cue-name table"));
    }
    let simple = rd_u16(b, 0x13)? as usize;
    let complex = rd_u16(b, 0x15)? as usize;
    let nwb = rd_u8(b, 0x1B)? as usize;
    let nsounds = rd_u16(b, 0x1C)? as usize;
    let simple_off = rd_i32(b, 0x22)?;
    let complex_off = rd_i32(b, 0x26)?;
    let wb_off = rd_i32(b, 0x3A)?;
    let nameidx_off = rd_i32(b, 0x42)?;
    let sound_off = rd_i32(b, 0x46)?;
    let name = cstr(b, XSB_HEADER, NAME_FIELD)?;
    let mut wavebank_names = Vec::with_capacity(nwb);
    for i in 0..nwb {
        wavebank_names.push(cstr(b, wb_off as usize + i * NAME_FIELD, NAME_FIELD)?);
    }

    // Walk the sound section (entry length at +7).
    let mut sounds = HashMap::with_capacity(nsounds);
    let mut o = sound_off as usize;
    for _ in 0..nsounds {
        let len = rd_u16(b, o + 7)? as usize;
        if len < 9 {
            return Err(bad(format!("sound entry @0x{o:X} has length {len}")));
        }
        let bytes = b
            .get(o..o + len)
            .ok_or_else(|| bad(format!("sound entry @0x{o:X} past end")))?;
        sounds.insert(o, bytes.to_vec());
        o += len;
    }

    let total = simple + complex;
    let mut cues = Vec::with_capacity(total);
    for i in 0..total {
        let name_off = rd_u32(b, nameidx_off as usize + i * NAME_INDEX_LEN)? as usize;
        let cue_name = cstr(b, name_off, 256)?;
        let shape = if i < simple {
            let c = simple_off as usize + i * SIMPLE_CUE_LEN;
            CueShape::Simple {
                flags: rd_u8(b, c)?,
                sound: rd_u32(b, c + 1)? as usize,
            }
        } else {
            let c = complex_off as usize + (i - simple) * COMPLEX_CUE_LEN;
            let cue: [u8; COMPLEX_CUE_LEN] = b
                .get(c..c + COMPLEX_CUE_LEN)
                .ok_or_else(|| bad("complex cue past end"))?
                .try_into()
                .map_err(|_| bad("complex cue"))?;
            let flags = cue[0];
            if flags & 0x01 == 0 || flags & 0x06 != 0 {
                CueShape::Unsupported(format!("complex cue flags 0x{flags:02X}"))
            } else {
                let var = u32::from_le_bytes([cue[1], cue[2], cue[3], cue[4]]) as usize;
                parse_variation(b, var, cue)?
            }
        };
        cues.push((cue_name, shape));
    }
    Ok(Xsb {
        name,
        wavebank_names,
        cues,
        sounds,
    })
}

fn parse_variation(b: &[u8], off: usize, cue: [u8; COMPLEX_CUE_LEN]) -> Result<CueShape> {
    let header: [u8; VAR_HEADER_LEN] = b
        .get(off..off + VAR_HEADER_LEN)
        .ok_or_else(|| bad("variation table past end"))?
        .try_into()
        .map_err(|_| bad("variation header"))?;
    let word = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let count = (word & 0xFFFF) as usize;
    let kind = (word >> 19) & 7;
    if kind != 1 {
        return Ok(CueShape::Unsupported(format!("variation type {kind}")));
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let e = off + VAR_HEADER_LEN + i * VAR1_ENTRY_LEN;
        entries.push((rd_u32(b, e)? as usize, rd_u8(b, e + 4)?, rd_u8(b, e + 5)?));
    }
    Ok(CueShape::Variation {
        cue,
        header,
        entries,
    })
}

// ── XWB model ────────────────────────────────────────────────────────

const XWB_MAGIC: &[u8; 4] = b"WBND";
const XWB_HEADER: usize = 52;
const XWB_BANKDATA: usize = 96;
const XWB_ENTRY: usize = 24;
const XWB_ENTRY_NAME: usize = 64;
const XWB_ALIGN: usize = 4;
/// `TYPE_BUFFER` + `ENTRYNAMES` + bit 19 — the stock in-memory flags.
const XWB_FLAGS: u32 = 0x0009_0000;

/// One wave's metadata (the 24-byte entry, offsets relative to segment 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveMeta {
    pub flags_duration: u32,
    pub format: u32,
    pub offset: u32,
    pub length: u32,
    pub loop_start: u32,
    pub loop_length: u32,
}

impl WaveMeta {
    /// Codec: 0 PCM, 1 XMA, 2 ADPCM, 3 WMA.
    pub fn codec(&self) -> u32 {
        self.format & 3
    }
}

/// Parsed wave-bank header + entry table.
#[derive(Debug, Clone)]
pub struct XwbMeta {
    pub name: String,
    pub flags: u32,
    pub entries: Vec<WaveMeta>,
    pub entry_names: Vec<String>,
    /// Absolute file offset of segment 4 (wave data).
    pub data_offset: u64,
}

/// Bytes of the file needed to parse the header + entry table (+ names):
/// call with the first 52 bytes.
pub fn xwb_meta_len(header: &[u8]) -> Result<usize> {
    if header.len() < XWB_HEADER || &header[0..4] != XWB_MAGIC {
        return Err(bad("not an XWB (magic)"));
    }
    let seg = |i: usize| -> Result<(usize, usize)> {
        Ok((
            rd_u32(header, 12 + 8 * i)? as usize,
            rd_u32(header, 16 + 8 * i)? as usize,
        ))
    };
    let (s1o, s1l) = seg(1)?;
    let (s3o, s3l) = seg(3)?;
    Ok((s1o + s1l).max(s3o + s3l).max(XWB_HEADER + XWB_BANKDATA))
}

/// Parse the header, bank data and entry table (and entry names, if present).
pub fn parse_xwb_meta(b: &[u8]) -> Result<XwbMeta> {
    if b.len() < XWB_HEADER + XWB_BANKDATA || &b[0..4] != XWB_MAGIC {
        return Err(bad("not an XWB (magic)"));
    }
    if rd_u32(b, 8)? != 42 {
        return Err(bad("XWB header version is not 42"));
    }
    let seg = |i: usize| -> Result<(usize, usize)> {
        Ok((
            rd_u32(b, 12 + 8 * i)? as usize,
            rd_u32(b, 16 + 8 * i)? as usize,
        ))
    };
    let (s0, _) = seg(0)?;
    let (s1, _) = seg(1)?;
    let (s3, s3l) = seg(3)?;
    let (s4, _) = seg(4)?;
    let flags = rd_u32(b, s0)?;
    let count = rd_u32(b, s0 + 4)? as usize;
    let name = cstr(b, s0 + 8, 64)?;
    let meta_size = rd_u32(b, s0 + 72)? as usize;
    if meta_size != XWB_ENTRY {
        return Err(bad(format!(
            "XWB entry size {meta_size} (compact banks unsupported)"
        )));
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let e = s1 + i * XWB_ENTRY;
        entries.push(WaveMeta {
            flags_duration: rd_u32(b, e)?,
            format: rd_u32(b, e + 4)?,
            offset: rd_u32(b, e + 8)?,
            length: rd_u32(b, e + 12)?,
            loop_start: rd_u32(b, e + 16)?,
            loop_length: rd_u32(b, e + 20)?,
        });
    }
    let mut entry_names = Vec::new();
    if s3l >= count * XWB_ENTRY_NAME && count > 0 {
        for i in 0..count {
            entry_names.push(cstr(b, s3 + i * XWB_ENTRY_NAME, XWB_ENTRY_NAME)?);
        }
    }
    Ok(XwbMeta {
        name,
        flags,
        entries,
        entry_names,
        data_offset: s4 as u64,
    })
}

/// Random access to a wave bank's bytes (absolute file offsets).
pub trait WaveBytes {
    fn read(&mut self, offset: u64, len: usize) -> std::result::Result<Vec<u8>, String>;
}

/// In-memory implementation (tests, and banks already read whole).
impl WaveBytes for &[u8] {
    fn read(&mut self, offset: u64, len: usize) -> std::result::Result<Vec<u8>, String> {
        let o = offset as usize;
        self.get(o..o + len)
            .map(|s| s.to_vec())
            .ok_or_else(|| format!("{len} bytes @0x{o:X} past end ({})", self.len()))
    }
}

/// One wave bank a source XSB references: its metadata + byte access.
pub struct WaveSource<'a> {
    pub meta: XwbMeta,
    pub bytes: &'a mut dyn WaveBytes,
}

/// A source sound bank with the wave banks it references (matched by the
/// XSB's wave-bank name table, in order).
pub struct Source<'a> {
    pub xsb: Xsb,
    pub waves: Vec<WaveSource<'a>>,
}

// ── Build ────────────────────────────────────────────────────────────

/// The built bank pair plus what went into it.
#[derive(Debug, Clone)]
pub struct Built {
    pub xsb: Vec<u8>,
    pub xwb: Vec<u8>,
    /// Cues included, in cue-index order.
    pub cues: Vec<String>,
    /// Requested cues not found in any source.
    pub missing: Vec<String>,
    /// Requested cues found but skipped (unsupported shape/codec), with why.
    pub skipped: Vec<(String, String)>,
    pub wave_count: usize,
    pub wave_bytes: usize,
}

/// A sound the new bank carries: source identity + its remapped wave.
struct NewSound {
    bytes: Vec<u8>,
}

/// Build the `name` bank pair holding `wanted` cues, searching `sources` in
/// order (first source that has a cue wins).
pub fn build(sources: &mut [Source<'_>], wanted: &[&str], name: &str) -> Result<Built> {
    if name.is_empty() || name.len() >= 64 || !name.is_ascii() {
        return Err(bad("bank name must be 1..63 ASCII bytes"));
    }
    for s in sources.iter() {
        if s.waves.len() != s.xsb.wavebank_names.len() {
            return Err(bad(format!(
                "{}: {} wave bank(s) supplied for {} referenced",
                s.xsb.name,
                s.waves.len(),
                s.xsb.wavebank_names.len()
            )));
        }
        for (i, w) in s.waves.iter().enumerate() {
            if w.meta.name != s.xsb.wavebank_names[i] {
                return Err(bad(format!(
                    "{}: wave bank {} is '{}', XSB expects '{}'",
                    s.xsb.name, i, w.meta.name, s.xsb.wavebank_names[i]
                )));
            }
        }
    }

    // Per selected cue: where it lives.
    struct Pick {
        name: String,
        src: usize,
        shape: CueShape,
    }
    let mut picks: Vec<Pick> = Vec::new();
    let mut missing = Vec::new();
    let mut skipped = Vec::new();
    for &want in wanted {
        if picks.iter().any(|p| p.name == want) {
            continue;
        }
        let found = sources
            .iter()
            .enumerate()
            .find_map(|(i, s)| s.xsb.find(want).map(|shape| (i, shape.clone())));
        match found {
            None => missing.push(want.to_string()),
            Some((_, CueShape::Unsupported(why))) => skipped.push((want.to_string(), why)),
            Some((src, shape)) => {
                // Every referenced sound must be a bare sound on a supported wave.
                let refs: Vec<usize> = match &shape {
                    CueShape::Simple { sound, .. } => vec![*sound],
                    CueShape::Variation { entries, .. } => entries.iter().map(|e| e.0).collect(),
                    CueShape::Unsupported(_) => Vec::new(),
                };
                match check_sounds(&sources[src], &refs) {
                    Ok(()) => picks.push(Pick {
                        name: want.to_string(),
                        src,
                        shape,
                    }),
                    Err(why) => skipped.push((want.to_string(), why)),
                }
            }
        }
    }
    if picks.is_empty() {
        return Err(BuildError::Empty);
    }
    // Simple cues first (cue indices 0..S), then complex — the engine's order.
    picks.sort_by_key(|p| !matches!(p.shape, CueShape::Simple { .. }));

    // Remap sounds and waves.
    let mut sound_index: HashMap<(usize, usize), usize> = HashMap::new();
    let mut new_sounds: Vec<NewSound> = Vec::new();
    let mut wave_index: HashMap<(usize, u8, u16), u16> = HashMap::new();
    let mut new_waves: Vec<(WaveMeta, Vec<u8>, String)> = Vec::new();
    let mut remap_sound = |src: usize, off: usize, sources: &mut [Source<'_>]| -> Result<usize> {
        if let Some(&i) = sound_index.get(&(src, off)) {
            return Ok(i);
        }
        let mut bytes = sources[src]
            .xsb
            .sounds
            .get(&off)
            .cloned()
            .ok_or_else(|| bad(format!("sound @0x{off:X} missing")))?;
        let wave = u16::from_le_bytes([bytes[9], bytes[10]]);
        let wb = bytes[11];
        let key = (src, wb, wave);
        let new_wave = match wave_index.get(&key) {
            Some(&w) => w,
            None => {
                let ws = &mut sources[src].waves[wb as usize];
                let meta = *ws
                    .meta
                    .entries
                    .get(wave as usize)
                    .ok_or_else(|| bad(format!("wave {wave} out of range")))?;
                let data = ws
                    .bytes
                    .read(
                        ws.meta.data_offset + meta.offset as u64,
                        meta.length as usize,
                    )
                    .map_err(BuildError::Read)?;
                let wname = ws
                    .meta
                    .entry_names
                    .get(wave as usize)
                    .cloned()
                    .unwrap_or_default();
                if new_waves.len() >= 0xFFFF {
                    return Err(bad("too many waves"));
                }
                let w = new_waves.len() as u16;
                new_waves.push((meta, data, wname));
                wave_index.insert(key, w);
                w
            }
        };
        bytes[9..11].copy_from_slice(&new_wave.to_le_bytes());
        bytes[11] = 0;
        let i = new_sounds.len();
        new_sounds.push(NewSound { bytes });
        sound_index.insert((src, off), i);
        Ok(i)
    };

    // Resolve every pick to new sound indices.
    enum Out {
        Simple {
            flags: u8,
            sound: usize,
        },
        Variation {
            cue: [u8; COMPLEX_CUE_LEN],
            header: [u8; VAR_HEADER_LEN],
            entries: Vec<(usize, u8, u8)>,
        },
    }
    let mut outs: Vec<(String, Out)> = Vec::with_capacity(picks.len());
    for p in &picks {
        let out = match &p.shape {
            CueShape::Simple { flags, sound } => Out::Simple {
                flags: *flags,
                sound: remap_sound(p.src, *sound, sources)?,
            },
            CueShape::Variation {
                cue,
                header,
                entries,
            } => {
                let mut es = Vec::with_capacity(entries.len());
                for &(off, lo, hi) in entries {
                    es.push((remap_sound(p.src, off, sources)?, lo, hi));
                }
                Out::Variation {
                    cue: *cue,
                    header: *header,
                    entries: es,
                }
            }
            CueShape::Unsupported(why) => {
                // Filtered out above; never reached.
                return Err(bad(format!(
                    "unsupported cue {} reached the writer: {why}",
                    p.name
                )));
            }
        };
        outs.push((p.name.clone(), out));
    }

    // ── XSB layout ──
    let simple_n = outs
        .iter()
        .filter(|(_, o)| matches!(o, Out::Simple { .. }))
        .count();
    let complex_n = outs.len() - simple_n;
    let total = outs.len();
    let buckets = total.max(16);
    if total >= 0xFFFF || new_sounds.len() > 0xFFFF {
        return Err(bad("too many cues/sounds"));
    }
    let wb_names_off = XSB_HEADER + NAME_FIELD;
    let sounds_off = wb_names_off + NAME_FIELD;
    let mut sound_offs = Vec::with_capacity(new_sounds.len());
    let mut cur = sounds_off;
    for s in &new_sounds {
        sound_offs.push(cur);
        cur += s.bytes.len();
    }
    let simple_off = cur;
    cur += simple_n * SIMPLE_CUE_LEN;
    let complex_off = cur;
    cur += complex_n * COMPLEX_CUE_LEN;
    let var_off = cur;
    let mut var_offs = Vec::with_capacity(complex_n);
    for (_, o) in &outs {
        if let Out::Variation { entries, .. } = o {
            var_offs.push(cur);
            cur += VAR_HEADER_LEN + entries.len() * VAR1_ENTRY_LEN;
        }
    }
    let hash_off = cur;
    cur += buckets * 2;
    let nameidx_off = cur;
    cur += total * NAME_INDEX_LEN;
    let names_off = cur;
    let names_len: usize = outs.iter().map(|(n, _)| n.len() + 1).sum();
    if names_len > 0xFFFF {
        return Err(bad("cue-name table too large"));
    }
    let total_len = names_off + names_len;

    let mut x = vec![0u8; total_len];
    x[0..4].copy_from_slice(XSB_MAGIC);
    put_u16(&mut x, 0x04, XSB_VERSION);
    put_u16(&mut x, 0x06, XSB_VERSION);
    x[0x12] = 0x01; // cue-name table present
    put_u16(&mut x, 0x13, simple_n as u16);
    put_u16(&mut x, 0x15, complex_n as u16);
    put_u16(&mut x, 0x19, buckets as u16);
    x[0x1B] = 1;
    put_u16(&mut x, 0x1C, new_sounds.len() as u16);
    put_u16(&mut x, 0x1E, names_len as u16);
    put_i32(
        &mut x,
        0x22,
        if simple_n > 0 { simple_off as i32 } else { -1 },
    );
    put_i32(
        &mut x,
        0x26,
        if complex_n > 0 {
            complex_off as i32
        } else {
            -1
        },
    );
    put_i32(&mut x, 0x2A, names_off as i32);
    put_i32(&mut x, 0x2E, -1);
    put_i32(
        &mut x,
        0x32,
        if complex_n > 0 { var_off as i32 } else { -1 },
    );
    put_i32(&mut x, 0x36, -1);
    put_i32(&mut x, 0x3A, wb_names_off as i32);
    put_i32(&mut x, 0x3E, hash_off as i32);
    put_i32(&mut x, 0x42, nameidx_off as i32);
    put_i32(&mut x, 0x46, sounds_off as i32);
    x[XSB_HEADER..XSB_HEADER + name.len()].copy_from_slice(name.as_bytes());
    x[wb_names_off..wb_names_off + name.len()].copy_from_slice(name.as_bytes());
    for (s, &o) in new_sounds.iter().zip(&sound_offs) {
        x[o..o + s.bytes.len()].copy_from_slice(&s.bytes);
    }
    let (mut si, mut ci, mut vi) = (0usize, 0usize, 0usize);
    for (_, o) in &outs {
        match o {
            Out::Simple { flags, sound } => {
                let c = simple_off + si * SIMPLE_CUE_LEN;
                x[c] = *flags;
                put_u32(&mut x, c + 1, sound_offs[*sound] as u32);
                si += 1;
            }
            Out::Variation {
                cue,
                header,
                entries,
            } => {
                let c = complex_off + ci * COMPLEX_CUE_LEN;
                x[c..c + COMPLEX_CUE_LEN].copy_from_slice(cue);
                put_u32(&mut x, c + 1, var_offs[vi] as u32);
                let v = var_offs[vi];
                x[v..v + VAR_HEADER_LEN].copy_from_slice(header);
                for (k, &(snd, lo, hi)) in entries.iter().enumerate() {
                    let e = v + VAR_HEADER_LEN + k * VAR1_ENTRY_LEN;
                    put_u32(&mut x, e, sound_offs[snd] as u32);
                    x[e + 4] = lo;
                    x[e + 5] = hi;
                }
                ci += 1;
                vi += 1;
            }
        }
    }
    // Hash table + name index (chains: newest-first per bucket).
    for k in 0..buckets {
        put_u16(&mut x, hash_off + k * 2, 0xFFFF);
    }
    let mut name_cur = names_off;
    for (i, (cue_name, _)) in outs.iter().enumerate() {
        let bucket = cue_hash(cue_name.as_bytes()) as usize % buckets;
        let head = rd_u16(&x, hash_off + bucket * 2)?;
        let e = nameidx_off + i * NAME_INDEX_LEN;
        put_u32(&mut x, e, name_cur as u32);
        put_u16(&mut x, e + 4, head);
        put_u16(&mut x, hash_off + bucket * 2, i as u16);
        x[name_cur..name_cur + cue_name.len()].copy_from_slice(cue_name.as_bytes());
        name_cur += cue_name.len() + 1;
    }
    let crc = xact_crc16(&x[0x12..]);
    put_u16(&mut x, 0x08, crc);

    // ── XWB layout ──
    let n = new_waves.len();
    let meta_off = XWB_HEADER + XWB_BANKDATA;
    let names_seg = meta_off + n * XWB_ENTRY;
    let data_seg = round_up(names_seg + n * XWB_ENTRY_NAME, XWB_ALIGN);
    let mut data_len = 0usize;
    let mut data_offs = Vec::with_capacity(n);
    for (_, d, _) in &new_waves {
        data_len = round_up(data_len, XWB_ALIGN);
        data_offs.push(data_len);
        data_len += d.len();
    }
    let mut w = vec![0u8; data_seg + data_len];
    w[0..4].copy_from_slice(XWB_MAGIC);
    put_u32(&mut w, 4, 43);
    put_u32(&mut w, 8, 42);
    for (i, (o, l)) in [
        (XWB_HEADER, XWB_BANKDATA),
        (meta_off, n * XWB_ENTRY),
        (names_seg, 0),
        (names_seg, n * XWB_ENTRY_NAME),
        (data_seg, data_len),
    ]
    .into_iter()
    .enumerate()
    {
        put_u32(&mut w, 12 + 8 * i, o as u32);
        put_u32(&mut w, 16 + 8 * i, l as u32);
    }
    let bd = XWB_HEADER;
    put_u32(&mut w, bd, XWB_FLAGS);
    put_u32(&mut w, bd + 4, n as u32);
    w[bd + 8..bd + 8 + name.len()].copy_from_slice(name.as_bytes());
    put_u32(&mut w, bd + 72, XWB_ENTRY as u32);
    put_u32(&mut w, bd + 76, XWB_ENTRY_NAME as u32);
    put_u32(&mut w, bd + 80, XWB_ALIGN as u32);
    let mut wave_bytes = 0usize;
    for (i, (m, d, wname)) in new_waves.iter().enumerate() {
        let e = meta_off + i * XWB_ENTRY;
        // Entry-flag bits 0..2 must be clear in an in-memory bank.
        put_u32(&mut w, e, m.flags_duration & !7);
        put_u32(&mut w, e + 4, m.format);
        put_u32(&mut w, e + 8, data_offs[i] as u32);
        put_u32(&mut w, e + 12, d.len() as u32);
        put_u32(&mut w, e + 16, m.loop_start);
        put_u32(&mut w, e + 20, m.loop_length);
        let nm = if wname.is_empty() {
            format!("{name}_{i:03}")
        } else {
            wname.clone()
        };
        let nb = &nm.as_bytes()[..nm.len().min(XWB_ENTRY_NAME - 1)];
        let no = names_seg + i * XWB_ENTRY_NAME;
        w[no..no + nb.len()].copy_from_slice(nb);
        let dd = data_seg + data_offs[i];
        w[dd..dd + d.len()].copy_from_slice(d);
        wave_bytes += d.len();
    }

    Ok(Built {
        xsb: x,
        xwb: w,
        cues: outs.into_iter().map(|(n, _)| n).collect(),
        missing,
        skipped,
        wave_count: n,
        wave_bytes,
    })
}

/// Every referenced sound must be a simple single-wave sound on a PCM or
/// ADPCM wave that exists: either bare (flags 0, 12 bytes) or carrying the
/// one runtime-parameter block World's OWN `se_normal.xsb` uses on its
/// shutter / event SEs (flags 0x04, 19 bytes, tail [`RPC_TAIL_WORLD`]) — so
/// the curve it names is guaranteed to exist in World's global settings.
fn check_sounds(src: &Source<'_>, refs: &[usize]) -> std::result::Result<(), String> {
    for &off in refs {
        let s = src
            .xsb
            .sounds
            .get(&off)
            .ok_or_else(|| format!("sound @0x{off:X} is not a sound entry"))?;
        let bare = s.len() == SOUND_BARE_LEN && s[0] == 0;
        let rpc = s.len() == SOUND_RPC_LEN && s[0] == 0x04 && s[SOUND_BARE_LEN..] == RPC_TAIL_WORLD;
        if !(bare || rpc) {
            return Err(format!(
                "sound @0x{off:X} flags 0x{:02X} len {} (only bare / World-RPC sounds are copied)",
                s[0],
                s.len()
            ));
        }
        let wave = u16::from_le_bytes([s[9], s[10]]) as usize;
        let wb = s[11] as usize;
        let meta = src
            .waves
            .get(wb)
            .and_then(|w| w.meta.entries.get(wave))
            .ok_or_else(|| format!("wave {wb}/{wave} out of range"))?;
        if !matches!(meta.codec(), 0 | 2) {
            return Err(format!("wave {wb}/{wave} codec {}", meta.codec()));
        }
    }
    Ok(())
}

fn round_up(v: usize, a: usize) -> usize {
    v.div_ceil(a) * a
}

/// XACT2 cue-name hash (`xactengine2_10.dll` `FUN_0040fad0`):
/// `h = 3h + (h >> 1) + c` per byte, wrapping u16 (chars sign-extended).
pub fn cue_hash(name: &[u8]) -> u16 {
    let mut h: u16 = 0;
    for &c in name {
        h = h
            .wrapping_mul(3)
            .wrapping_add(h >> 1)
            .wrapping_add(c as i8 as i16 as u16);
    }
    h
}

/// XACT2 XSB CRC-16 (reflected poly 0x8408, init 0xFFFF, final NOT) over
/// `[0x12..]`, stored at 0x08 (`FUN_00424200`).
pub fn xact_crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc = CRC_TABLE[((b as u16 ^ crc) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

const CRC_TABLE: [u16; 256] = {
    let mut t = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u16;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0x8408
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

// ── Offline validation (mirrors the engine's structural rules) ───────

/// Re-parse a built pair and check the engine's layout rules (the subset the
/// writer is responsible for). Used by the host tests and by the DLL before
/// handing the pair to the engine.
pub fn validate(xsb: &[u8], xwb: &[u8]) -> std::result::Result<(), String> {
    let e = |m: &str| m.to_string();
    // XSB
    if xsb.len() < 0x8A || &xsb[0..4] != XSB_MAGIC {
        return Err(e("xsb magic"));
    }
    let stored = rd_u16(xsb, 0x08).map_err(|x| x.to_string())?;
    if stored != xact_crc16(&xsb[0x12..]) {
        return Err(e("xsb crc"));
    }
    let g16 = |o| rd_u16(xsb, o).map_err(|x| x.to_string());
    let g32 = |o| rd_i32(xsb, o).map_err(|x| x.to_string());
    let simple = g16(0x13)? as usize;
    let complex = g16(0x15)? as usize;
    let total = simple + complex;
    if g16(0x17)? != 0 || xsb[0x12] & 2 != 0 || xsb[0x89] != 0 {
        return Err(e("xsb header fixed fields"));
    }
    if g16(0x19)? as usize != total.max(16) {
        return Err(e("xsb bucket count"));
    }
    if g32(0x3A)? != 0x8A {
        return Err(e("xsb wavebank names offset"));
    }
    let nwb = xsb[0x1B] as usize;
    if g32(0x46)? as usize != 0x8A + nwb * 64 {
        return Err(e("xsb sounds offset"));
    }
    let names_off = g32(0x2A)? as usize;
    if g32(0x42)? as usize + total * 6 != names_off {
        return Err(e("xsb name index / names offsets"));
    }
    if g16(0x1E)? as usize != xsb.len() - names_off || *xsb.last().unwrap_or(&1) != 0 {
        return Err(e("xsb name table must run to EOF"));
    }
    let parsed = parse_xsb(xsb).map_err(|x| x.to_string())?;
    if parsed.cues.len() != total {
        return Err(e("xsb cue count"));
    }
    // Every cue name resolves through the hash chains to its own index.
    let buckets = g16(0x19)? as usize;
    let hash_off = g32(0x3E)? as usize;
    let nameidx = g32(0x42)? as usize;
    for (i, (name, _)) in parsed.cues.iter().enumerate() {
        let mut idx = g16(hash_off + (cue_hash(name.as_bytes()) as usize % buckets) * 2)?;
        let mut hops = 0;
        loop {
            if idx == 0xFFFF || hops > total {
                return Err(format!("cue {name} not reachable by hash"));
            }
            let ent = nameidx + idx as usize * 6;
            let n = cstr(
                xsb,
                rd_u32(xsb, ent).map_err(|x| x.to_string())? as usize,
                256,
            )
            .map_err(|x| x.to_string())?;
            if n == *name {
                if idx as usize != i {
                    return Err(format!("cue {name} hashes to index {idx}, expected {i}"));
                }
                break;
            }
            idx = g16(ent + 4)?;
            hops += 1;
        }
    }
    // Complex cues: variation tables contiguous from 0x32, then the hash table.
    if complex > 0 {
        let mut cur = g32(0x32)? as usize;
        let coff = g32(0x26)? as usize;
        for i in 0..complex {
            if rd_u32(xsb, coff + i * 15 + 1).map_err(|x| x.to_string())? as usize != cur {
                return Err(format!("variation table {i} not contiguous"));
            }
            let count = g16(cur)? as usize;
            cur += 8 + count * 6;
        }
        if cur != hash_off {
            return Err(e("hash table must follow the last variation table"));
        }
    }
    // XWB
    let m = parse_xwb_meta(xwb).map_err(|x| x.to_string())?;
    let w32 = |o| rd_u32(xwb, o).map_err(|x| x.to_string());
    if w32(12)? != 0x34 || w32(16)? != 0x60 || w32(20)? != 0x94 {
        return Err(e("xwb fixed segment offsets"));
    }
    if m.flags & 0xFFF0_FFFE != 0 || m.flags & 1 != 0 {
        return Err(e("xwb flags"));
    }
    let n = m.entries.len();
    if w32(20 + 4)? as usize != n * 24 {
        return Err(e("xwb meta seg length"));
    }
    if w32(36)? as usize != 0x94 + n * 24 || w32(40)? as usize != n * 64 {
        return Err(e("xwb entry names segment"));
    }
    let (d_off, d_len) = (w32(44)? as usize, w32(48)? as usize);
    if xwb.len() - d_off != d_len {
        return Err(e("xwb data must run exactly to EOF"));
    }
    for (i, en) in m.entries.iter().enumerate() {
        if en.flags_duration & 7 != 0
            || (en.offset as usize + en.length as usize) > d_len
            || en.codec() >= 3
            || !(1..=6).contains(&((en.format >> 2) & 7))
            || (en.flags_duration >> 4) < en.loop_start.saturating_add(en.loop_length)
        {
            return Err(format!("xwb entry {i} invalid"));
        }
    }
    if parsed.wavebank_names.first().map(String::as_str) != Some(m.name.as_str()) {
        return Err(e("xsb/xwb internal names differ"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny synthetic source pair: 3 simple sounds + one type-1 variation
    /// cue over two of them.
    fn synth_source() -> (Vec<u8>, Vec<u8>) {
        // Waves: 3 ADPCM mono entries of 70 bytes each, streaming-bank style
        // (entry flag bit 0 set on one to check it is cleared).
        let fmt: u32 = 2 | (1 << 2) | (44_100 << 5) | (48 << 23);
        let n = 3;
        let meta_off = 52 + 96;
        let names = meta_off + n * 24;
        let data = names + n * 64;
        let mut w = vec![0u8; data + 3 * 70];
        w[0..4].copy_from_slice(b"WBND");
        put_u32(&mut w, 4, 43);
        put_u32(&mut w, 8, 42);
        for (i, (o, l)) in [
            (52, 96),
            (meta_off, n * 24),
            (names, 0),
            (names, n * 64),
            (data, 210),
        ]
        .into_iter()
        .enumerate()
        {
            put_u32(&mut w, 12 + 8 * i, o as u32);
            put_u32(&mut w, 16 + 8 * i, l as u32);
        }
        put_u32(&mut w, 52, 0x0009_0000);
        put_u32(&mut w, 56, n as u32);
        w[60..64].copy_from_slice(b"SRCW");
        put_u32(&mut w, 52 + 72, 24);
        put_u32(&mut w, 52 + 76, 64);
        put_u32(&mut w, 52 + 80, 4);
        for i in 0..n {
            let e = meta_off + i * 24;
            put_u32(&mut w, e, (128 << 4) | if i == 1 { 1 } else { 0 });
            put_u32(&mut w, e + 4, fmt);
            put_u32(&mut w, e + 8, (i * 70) as u32);
            put_u32(&mut w, e + 12, 70);
            let nm = format!("wave{i}");
            w[names + i * 64..names + i * 64 + nm.len()].copy_from_slice(nm.as_bytes());
            for k in 0..70 {
                w[data + i * 70 + k] = (i * 16 + k) as u8;
            }
        }

        // XSB: sounds s0,s1,s2 (waves 0,1,2); simple cues "alpha"->s0,
        // "beta"->s2; complex "gamma" -> variation {s1, s2}.
        let sounds_off = 0x8A + 64;
        let snd = |wave: u16| -> Vec<u8> {
            let mut s = vec![0x00, 0x07, 0x00, 0xB4, 0, 0, 0, 12, 0];
            s.extend_from_slice(&wave.to_le_bytes());
            s.push(0);
            s
        };
        let simple_off = sounds_off + 36;
        let complex_off = simple_off + 10;
        let var_off = complex_off + 15;
        let hash_off = var_off + 8 + 12;
        let total = 3usize;
        let nameidx = hash_off + 16 * 2;
        let names_off = nameidx + total * 6;
        let cue_names = ["alpha", "beta", "gamma"];
        let names_len: usize = cue_names.iter().map(|n| n.len() + 1).sum();
        let mut x = vec![0u8; names_off + names_len];
        x[0..4].copy_from_slice(b"SDBK");
        put_u16(&mut x, 4, 43);
        put_u16(&mut x, 6, 43);
        x[0x12] = 1;
        put_u16(&mut x, 0x13, 2);
        put_u16(&mut x, 0x15, 1);
        put_u16(&mut x, 0x19, 16);
        x[0x1B] = 1;
        put_u16(&mut x, 0x1C, 3);
        put_u16(&mut x, 0x1E, names_len as u16);
        put_i32(&mut x, 0x22, simple_off as i32);
        put_i32(&mut x, 0x26, complex_off as i32);
        put_i32(&mut x, 0x2A, names_off as i32);
        put_i32(&mut x, 0x2E, -1);
        put_i32(&mut x, 0x32, var_off as i32);
        put_i32(&mut x, 0x36, -1);
        put_i32(&mut x, 0x3A, 0x8A);
        put_i32(&mut x, 0x3E, hash_off as i32);
        put_i32(&mut x, 0x42, nameidx as i32);
        put_i32(&mut x, 0x46, sounds_off as i32);
        x[0x4A..0x4E].copy_from_slice(b"SRCS");
        x[0x8A..0x8E].copy_from_slice(b"SRCW");
        for i in 0..3 {
            x[sounds_off + i * 12..sounds_off + i * 12 + 12].copy_from_slice(&snd(i as u16));
        }
        x[simple_off] = 4;
        put_u32(&mut x, simple_off + 1, sounds_off as u32);
        x[simple_off + 5] = 4;
        put_u32(&mut x, simple_off + 6, (sounds_off + 24) as u32);
        let mut cue = [0u8; 15];
        cue[0] = 1;
        cue[1..5].copy_from_slice(&(var_off as u32).to_le_bytes());
        cue[5..10].copy_from_slice(&[0xFF; 5]);
        x[complex_off..complex_off + 15].copy_from_slice(&cue);
        x[var_off..var_off + 8].copy_from_slice(&[2, 0, 0x0B, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
        put_u32(&mut x, var_off + 8, (sounds_off + 12) as u32);
        x[var_off + 13] = 0xFF;
        put_u32(&mut x, var_off + 14, (sounds_off + 24) as u32);
        x[var_off + 19] = 0xFF;
        for k in 0..16 {
            put_u16(&mut x, hash_off + k * 2, 0xFFFF);
        }
        let mut nc = names_off;
        for (i, n) in cue_names.iter().enumerate() {
            let bkt = cue_hash(n.as_bytes()) as usize % 16;
            let head = u16::from_le_bytes([x[hash_off + bkt * 2], x[hash_off + bkt * 2 + 1]]);
            put_u32(&mut x, nameidx + i * 6, nc as u32);
            put_u16(&mut x, nameidx + i * 6 + 4, head);
            put_u16(&mut x, hash_off + bkt * 2, i as u16);
            x[nc..nc + n.len()].copy_from_slice(n.as_bytes());
            nc += n.len() + 1;
        }
        let crc = xact_crc16(&x[0x12..]);
        put_u16(&mut x, 8, crc);
        (x, w)
    }

    fn build_from(xsb: &[u8], xwb: &[u8], wanted: &[&str]) -> Result<Built> {
        let parsed = parse_xsb(xsb)?;
        let meta = parse_xwb_meta(xwb)?;
        let mut bytes: &[u8] = xwb;
        let mut srcs = vec![Source {
            xsb: parsed,
            waves: vec![WaveSource {
                meta,
                bytes: &mut bytes,
            }],
        }];
        build(&mut srcs, wanted, "dsel")
    }

    #[test]
    fn subset_build_is_valid_and_remaps() {
        let (x, w) = synth_source();
        let b = build_from(&x, &w, &["gamma", "alpha", "nope"]).unwrap();
        validate(&b.xsb, &b.xwb).unwrap();
        assert_eq!(b.cues, vec!["alpha", "gamma"]); // simple first
        assert_eq!(b.missing, vec!["nope"]);
        assert_eq!(b.wave_count, 3);
        let p = parse_xsb(&b.xsb).unwrap();
        assert_eq!(p.name, "dsel");
        assert_eq!(p.wavebank_names, vec!["dsel"]);
        let m = parse_xwb_meta(&b.xwb).unwrap();
        assert_eq!(m.name, "dsel");
        // Entry flag bit cleared, data copied verbatim.
        assert!(m.entries.iter().all(|e| e.flags_duration & 7 == 0));
        let orig = &w;
        let src_meta = parse_xwb_meta(orig).unwrap();
        for (i, e) in m.entries.iter().enumerate() {
            let d = &b.xwb[m.data_offset as usize + e.offset as usize..][..e.length as usize];
            // Find the same bytes in the source.
            let found = src_meta.entries.iter().any(|s| {
                &orig[src_meta.data_offset as usize + s.offset as usize..][..s.length as usize] == d
            });
            assert!(found, "wave {i} bytes not copied verbatim");
        }
    }

    #[test]
    fn shared_sounds_and_waves_are_deduplicated() {
        let (x, w) = synth_source();
        // beta -> s2, gamma -> {s1, s2}: s2 must appear once.
        let b = build_from(&x, &w, &["beta", "gamma"]).unwrap();
        validate(&b.xsb, &b.xwb).unwrap();
        assert_eq!(rd_u16(&b.xsb, 0x1C).unwrap(), 2);
        assert_eq!(b.wave_count, 2);
    }

    #[test]
    fn only_complex_or_only_simple() {
        let (x, w) = synth_source();
        let b = build_from(&x, &w, &["gamma"]).unwrap();
        validate(&b.xsb, &b.xwb).unwrap();
        assert_eq!(rd_i32(&b.xsb, 0x22).unwrap(), -1);
        let b = build_from(&x, &w, &["alpha"]).unwrap();
        validate(&b.xsb, &b.xwb).unwrap();
        assert_eq!(rd_i32(&b.xsb, 0x26).unwrap(), -1);
        assert_eq!(rd_i32(&b.xsb, 0x32).unwrap(), -1);
    }

    #[test]
    fn nothing_found_is_an_error() {
        let (x, w) = synth_source();
        assert_eq!(
            build_from(&x, &w, &["nope"]).unwrap_err(),
            BuildError::Empty
        );
    }

    #[test]
    fn many_cues_chain_in_the_hash_table() {
        // 40 cue names over 40 buckets must all resolve (validate walks chains).
        let (x, w) = synth_source();
        let b = build_from(&x, &w, &["alpha", "beta", "gamma"]).unwrap();
        validate(&b.xsb, &b.xwb).unwrap();
    }

    #[test]
    fn wave_bank_name_mismatch_is_rejected() {
        let (x, mut w) = synth_source();
        w[60..64].copy_from_slice(b"XXXX");
        assert!(matches!(
            build_from(&x, &w, &["alpha"]),
            Err(BuildError::BadSource(_))
        ));
    }

    #[test]
    fn crc_and_hash_match_the_engine() {
        // Values from the engine RE record (aaaa.xsb buckets).
        assert_eq!(cue_hash(b"aaaa") % 16, 1);
        assert_eq!(cue_hash(b"aaaa_s") % 16, 14);
        assert_eq!(cue_hash(b"asti") % 16, 8);
        assert_eq!(CRC_TABLE[1], 0x1189);
        assert_eq!(CRC_TABLE[255], 0x0f78);
    }

    /// Offline leg: `DDR_SEL_BANK_DIR` = a dir holding `voice_n.xsb`,
    /// `se_normal_n.xsb`, `voice_n.xwb`, `se_normal_n.xwb` (extracted by
    /// `scripts/validate_ddr_selection.sh` from `$DDR_WORLD_INSTALL`).
    #[test]
    fn real_banks_build_every_manifest_cue() {
        let Ok(dir) = std::env::var("DDR_SEL_BANK_DIR") else {
            eprintln!("DDR_SEL_BANK_DIR unset -- real-bank leg skipped");
            return;
        };
        let rd = |n: &str| std::fs::read(format!("{dir}/{n}")).unwrap();
        let (vx, sx, vw, sw) = (
            rd("voice_n.xsb"),
            rd("se_normal_n.xsb"),
            rd("voice_n.xwb"),
            rd("se_normal_n.xwb"),
        );
        let (mut vb, mut sb): (&[u8], &[u8]) = (&vw, &sw);
        let mut srcs = vec![
            Source {
                xsb: parse_xsb(&sx).unwrap(),
                waves: vec![WaveSource {
                    meta: parse_xwb_meta(&sw).unwrap(),
                    bytes: &mut sb,
                }],
            },
            Source {
                xsb: parse_xsb(&vx).unwrap(),
                waves: vec![WaveSource {
                    meta: parse_xwb_meta(&vw).unwrap(),
                    bytes: &mut vb,
                }],
            },
        ];
        let wanted = super::super::cues::all();
        let b = build(&mut srcs, &wanted, "dsel").unwrap();
        validate(&b.xsb, &b.xwb).unwrap();
        assert!(b.missing.is_empty(), "missing: {:?}", b.missing);
        assert!(b.skipped.is_empty(), "skipped: {:?}", b.skipped);
        assert_eq!(b.cues.len(), wanted.len());
        eprintln!(
            "dsel: {} cues, {} waves, {} wave bytes, xsb {} B, xwb {} B",
            b.cues.len(),
            b.wave_count,
            b.wave_bytes,
            b.xsb.len(),
            b.xwb.len()
        );
    }
}
