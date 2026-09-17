//! ANM-family container parser (`.anm` skeletal clips and `.camanm`
//! cameras) — `docs/3d_model_format_research.md` §5/§5.1/§6, a port of
//! `scripts/anm_dump.py::parse_anm` + `_parse_track` restricted to the two
//! chunk types the runtime consumes.
//!
//! ```text
//! 0x00 u32 0xFF010001
//! 0x04 u16 frame_count          duration = frame_count / fps
//! 0x06 u16 flags                bit 0 = loop (`*_loop` clips 1, `*_exec` 0)
//! 0x08 u32 fps                  read ONLY when a type-4 (camera) chunk exists, else 60.0
//! 0x10 u32 chunk_offsets[]      absolute, 0-terminated
//! chunk: u32 tag = 0xFF010002 + type, u16 h4, u16 h6
//!   type 0: u32 rel_offsets[] @+8 (relative to the CHUNK start), 0-terminated → bone tracks
//!   type 4: six u32 rel offsets @+8..+0x1C (0 = absent)        → camera slots
//!   anything else: ignored (type 1 = exporter-side hierarchy validation data)
//! track (16 B): u16 kind, u16 tag, u16 key_count, u8 target, u8 sub,
//!               u32 rel → u16 times[key_count] (0 = uniform), u32 rel → values
//!               (both relative to the TRACK start)
//! ```

use super::Le;
use std::ops::Range;

pub const MAGIC: u32 = 0xFF01_0001;
pub const CHUNK_BASE: u32 = 0xFF01_0002;
pub const DEFAULT_FPS: f32 = 60.0;
pub const HEADER_LOOP_BIT: u16 = 1;

/// Which pose/camera channel a track drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Rotation,
    Translation,
    Scale,
    /// Camera slot 0 (kind 1, f32×4).
    CamQuat,
    /// Camera slot 1 (kind 4, f32×3 + pad).
    CamPos,
    /// Camera slots 2..5 (kind 8, f32) — also the STEP scalar kind `0x1B`.
    CamScalar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub kind: u16,
    pub channel: Channel,
    /// Bone index (type 0) or camera slot (type 4).
    pub target: u8,
    pub key_count: u16,
    /// Explicit key times in frames, or `None` for uniform keys (key `k` at
    /// frame `k`).
    pub times: Option<Vec<u16>>,
    /// Byte range of the value block inside the file (exactly `key_count`
    /// keys; kind `0x1F` = 12-byte base + 6 bytes per key).
    pub values: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Anm {
    pub frame_count: u16,
    pub fps: f32,
    pub loops: bool,
    pub bone_tracks: Vec<Track>,
    pub camera_slots: [Option<Track>; 6],
}

impl Anm {
    #[inline]
    pub fn duration_s(&self) -> f32 {
        self.frame_count as f32 / self.fps
    }
    /// The `.camanm` shape (a type-4 chunk was present).
    pub fn has_camera(&self) -> bool {
        self.camera_slots.iter().any(|s| s.is_some())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnmError {
    BadMagic(u32),
    /// A header field, chunk, track or value block runs past the buffer.
    Truncated,
    /// A track kind the runtime has no decoder for.
    UnknownKind(u16),
    /// A chunk/track table is longer than any real file (guard against a
    /// runaway 0-terminated list on garbage input).
    Malformed,
}

impl std::fmt::Display for AnmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnmError::BadMagic(m) => write!(f, "not an ANM-family file (magic {m:#010x})"),
            AnmError::Truncated => f.write_str("truncated"),
            AnmError::UnknownKind(k) => write!(f, "unknown track kind {k:#x}"),
            AnmError::Malformed => f.write_str("malformed chunk/track table"),
        }
    }
}

/// Upper bound on list lengths we accept (chunks, tracks, keys). Stock files:
/// ≤ 3 chunks, ≤ 100 tracks, ≤ ~6300 keys.
const MAX_LIST: usize = 65_536;

/// `(channel, bytes per key)` for every kind the decoders support —
/// `anm_dump.py::KINDS` restricted to the runtime set. `0x1F` is special:
/// 6 bytes per key AFTER a 12-byte base.
pub fn kind_info(kind: u16) -> Option<(Channel, usize)> {
    Some(match kind {
        1 => (Channel::CamQuat, 16),
        4 => (Channel::CamPos, 16),
        8 | 0x1B => (Channel::CamScalar, 4),
        10 => (Channel::Scale, 16),
        0x1C => (Channel::Rotation, 6),
        0x1D => (Channel::Translation, 12),
        0x1E => (Channel::Translation, 6),
        0x1F => (Channel::Translation, 6),
        _ => return None,
    })
}

fn parse_track(r: &Le<'_>, o: usize) -> Result<Track, AnmError> {
    let kind = r.u16(o).ok_or(AnmError::Truncated)?;
    let (channel, stride) = kind_info(kind).ok_or(AnmError::UnknownKind(kind))?;
    let key_count = r.u16(o + 4).ok_or(AnmError::Truncated)?;
    let target = r.u8(o + 6).ok_or(AnmError::Truncated)?;
    let times_rel = r.u32(o + 8).ok_or(AnmError::Truncated)? as usize;
    let values_rel = r.u32(o + 0xC).ok_or(AnmError::Truncated)? as usize;
    let n = key_count as usize;

    let times = if times_rel != 0 {
        let base = o.checked_add(times_rel).ok_or(AnmError::Truncated)?;
        let mut v = Vec::with_capacity(n);
        for i in 0..n {
            v.push(r.u16(base + 2 * i).ok_or(AnmError::Truncated)?);
        }
        Some(v)
    } else {
        None
    };

    let values_start = o.checked_add(values_rel).ok_or(AnmError::Truncated)?;
    let values_len = if kind == 0x1F {
        12usize.checked_add(stride.checked_mul(n).ok_or(AnmError::Malformed)?)
    } else {
        stride.checked_mul(n)
    }
    .ok_or(AnmError::Malformed)?;
    let values_end = values_start
        .checked_add(values_len)
        .ok_or(AnmError::Truncated)?;
    if values_end > r.len() {
        return Err(AnmError::Truncated);
    }

    Ok(Track {
        kind,
        channel,
        target,
        key_count,
        times,
        values: values_start..values_end,
    })
}

/// Parse an ANM-family file. Chunk types other than 0 (bone tracks) and 4
/// (camera slots) are skipped; a type-0 track with an unsupported kind is an
/// error (the runtime could not evaluate the clip anyway).
pub fn parse(bytes: &[u8]) -> Result<Anm, AnmError> {
    let r = Le(bytes);
    let magic = r.u32(0).ok_or(AnmError::Truncated)?;
    if magic != MAGIC {
        return Err(AnmError::BadMagic(magic));
    }
    let frame_count = r.u16(4).ok_or(AnmError::Truncated)?;
    let flags = r.u16(6).ok_or(AnmError::Truncated)?;
    let fps_field = r.u32(8).ok_or(AnmError::Truncated)?;

    // Absolute chunk offsets, 0-terminated.
    let mut chunk_offs = Vec::new();
    let mut o = 0x10;
    loop {
        let v = r.u32(o).ok_or(AnmError::Truncated)?;
        if v == 0 {
            break;
        }
        chunk_offs.push(v as usize);
        o += 4;
        if chunk_offs.len() > MAX_LIST {
            return Err(AnmError::Malformed);
        }
    }

    let mut bone_tracks = Vec::new();
    let mut camera_slots: [Option<Track>; 6] = Default::default();
    let mut has_camera = false;

    for co in chunk_offs {
        let tag = r.u32(co).ok_or(AnmError::Truncated)?;
        let typ = tag.wrapping_sub(CHUNK_BASE);
        match typ {
            0 => {
                let mut p = co + 8;
                loop {
                    let rel = r.u32(p).ok_or(AnmError::Truncated)? as usize;
                    if rel == 0 {
                        break;
                    }
                    let to = co.checked_add(rel).ok_or(AnmError::Truncated)?;
                    bone_tracks.push(parse_track(&r, to)?);
                    p += 4;
                    if bone_tracks.len() > MAX_LIST {
                        return Err(AnmError::Malformed);
                    }
                }
            }
            4 => {
                has_camera = true;
                for (slot, out) in camera_slots.iter_mut().enumerate() {
                    let rel = r.u32(co + 8 + 4 * slot).ok_or(AnmError::Truncated)? as usize;
                    if rel != 0 {
                        let to = co.checked_add(rel).ok_or(AnmError::Truncated)?;
                        *out = Some(parse_track(&r, to)?);
                    }
                }
            }
            _ => {}
        }
    }

    // `+8` is the fps only in the camera shape; skeletal clips store 1 there
    // and run at the engine default (`DAT_1802dc26c` = 60.0).
    let fps = if has_camera && fps_field != 0 {
        fps_field as f32
    } else {
        DEFAULT_FPS
    };

    Ok(Anm {
        frame_count,
        fps,
        loops: flags & HEADER_LOOP_BIT != 0,
        bone_tracks,
        camera_slots,
    })
}
