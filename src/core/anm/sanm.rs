//! `.sanm` material-parameter clips — `docs/3d_model_format_research.md` §7
//! (layout VERIFIED on the stock `*_play_loop.sanm`), `scripts/anm_dump.py::
//! {parse_anm (chunk types 14 / 15), evaluate_materials}`.
//!
//! ```text
//! header as anm.rs; +0x08 u32 fps (60 in every stock file), +0x0C u32 1
//! type 14: u32 rel_offsets[] @+8 (relative to the CHUNK), 0-terminated → kind-8
//!          float tracks: target byte +6 = slot into the type-15 list, byte +7 =
//!          float index into the material's 32-float `parameters` block (§3.7):
//!          [0..3] m_vTexAnime (scaleU, scaleV, offU, offV), [4..7] vConstatntColor,
//!          [8..11] vOffsetColor
//! type 15: u16 count @+4, then 32-byte entries { u64 material identity (the .model
//!          material's +0x00, §2 packing), u64 shading-group identity, u32 shader
//!          FNV-1, u32 flags (0x2000), u64 0 }
//! ```
//!
//! The engine registers these files but the modpack never binds them to the
//! engine's animation player: the Background Dancers director evaluates a
//! stage part's `_play_loop.sanm` itself on the same clock as its `_play_loop.anm`
//! and writes the sampled floats into the render item's PRIVATE material copies
//! (`services::scene3d::render_item::set_material_params_raw`). World's own
//! `gm_boom00_bg_play_loop.sanm` scrolls a sky (`offV`), `gm_dawnstreet00_glo`
//! pulses its glow (`vConstatntColor.rgb`); the SuperNova stage port
//! (`tools/blender_ddr_addon/examples/port_stage_supernova.py`) writes the same
//! two shapes for its `_uvani` conveyors / water and `glo` layers.

use super::anm::{parse_track, AnmError, Track, CHUNK_BASE, DEFAULT_FPS, HEADER_LOOP_BIT, MAGIC};
use super::sample::{sample, Sample};
use super::Le;

/// Floats in the per-material `parameters` block (`+0x28..+0xA8` of the
/// runtime record; `+0x20..+0xA0` in the file).
pub const PARAM_FLOATS: usize = 32;
/// `m_vTexAnime.z` / `.w` — the UV offset the `mdl_*` vertex shaders add.
pub const PARAM_OFF_U: u8 = 2;
pub const PARAM_OFF_V: u8 = 3;
/// `vConstatntColor.rgb` — multiplied in by the `_c` pixel shaders only.
pub const PARAM_CONST_R: u8 = 4;

/// One type-15 entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialTarget {
    /// The `.model` material's packed name identity (`ktmdl::material_identities`).
    pub identity: u64,
    /// FNV-1 of the material's shader name (informational).
    pub shader_hash: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sanm {
    pub frame_count: u16,
    pub fps: f32,
    pub loops: bool,
    /// Kind-8 float tracks: `target` = slot into `targets`, `sub` = float index.
    pub tracks: Vec<Track>,
    pub targets: Vec<MaterialTarget>,
}

impl Sanm {
    #[inline]
    pub fn duration_s(&self) -> f32 {
        self.frame_count as f32 / self.fps
    }
}

/// One sampled parameter write: material index `material` (into the item's
/// material array), float `index` (0..31), `value`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamWrite {
    pub material: u16,
    pub index: u8,
    pub value: f32,
}

/// Upper bound on list lengths we accept (chunks, tracks, targets).
const MAX_LIST: usize = 65_536;
const TARGET_ENTRY: usize = 32;

/// Parse a `.sanm`. Chunk types other than 14 / 15 are skipped; a file with
/// neither is `Malformed` (it is not a material clip).
pub fn parse(bytes: &[u8]) -> Result<Sanm, AnmError> {
    let r = Le(bytes);
    let magic = r.u32(0).ok_or(AnmError::Truncated)?;
    if magic != MAGIC {
        return Err(AnmError::BadMagic(magic));
    }
    let frame_count = r.u16(4).ok_or(AnmError::Truncated)?;
    let flags = r.u16(6).ok_or(AnmError::Truncated)?;
    let fps_field = r.u32(8).ok_or(AnmError::Truncated)?;

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

    let mut tracks = Vec::new();
    let mut targets = Vec::new();
    let mut seen = false;
    for co in chunk_offs {
        let tag = r.u32(co).ok_or(AnmError::Truncated)?;
        match tag.wrapping_sub(CHUNK_BASE) {
            14 => {
                seen = true;
                let mut p = co + 8;
                loop {
                    let rel = r.u32(p).ok_or(AnmError::Truncated)? as usize;
                    if rel == 0 {
                        break;
                    }
                    let to = co.checked_add(rel).ok_or(AnmError::Truncated)?;
                    let t = parse_track(&r, to)?;
                    if !matches!(t.channel, super::anm::Channel::CamScalar) {
                        return Err(AnmError::UnknownKind(t.kind));
                    }
                    tracks.push(t);
                    p += 4;
                    if tracks.len() > MAX_LIST {
                        return Err(AnmError::Malformed);
                    }
                }
            }
            15 => {
                seen = true;
                let n = r.u16(co + 4).ok_or(AnmError::Truncated)? as usize;
                for i in 0..n {
                    let e = co
                        .checked_add(8)
                        .and_then(|b| b.checked_add(i.checked_mul(TARGET_ENTRY)?))
                        .ok_or(AnmError::Truncated)?;
                    let lo = r.u32(e).ok_or(AnmError::Truncated)? as u64;
                    let hi = r.u32(e + 4).ok_or(AnmError::Truncated)? as u64;
                    let shader_hash = r.u32(e + 16).ok_or(AnmError::Truncated)?;
                    // +20 flags (0x2000), +24 zero: the whole entry must be present
                    r.u32(e + TARGET_ENTRY - 4).ok_or(AnmError::Truncated)?;
                    targets.push(MaterialTarget {
                        identity: lo | (hi << 32),
                        shader_hash,
                    });
                }
            }
            _ => {}
        }
    }
    if !seen {
        return Err(AnmError::Malformed);
    }
    // Stock files store the fps at +8 like the camera shape; guard the
    // skeletal-style `1` and garbage.
    let fps = if (10..=1000).contains(&fps_field) {
        fps_field as f32
    } else {
        DEFAULT_FPS
    };
    Ok(Sanm {
        frame_count,
        fps,
        loops: flags & HEADER_LOOP_BIT != 0,
        tracks,
        targets,
    })
}

/// Slot → material index: each target's identity looked up in the model's
/// material identities (`ktmdl::material_identities`, file order = the GPU
/// resource's / the item's private copies' order). `None` = not in the model.
pub fn bind_targets(sanm: &Sanm, model_identities: &[u64]) -> Vec<Option<u16>> {
    sanm.targets
        .iter()
        .map(|t| {
            model_identities
                .iter()
                .position(|&id| id == t.identity)
                .and_then(|i| u16::try_from(i).ok())
        })
        .collect()
}

/// Sample every track at `frame`: `emit` is called once per track whose slot
/// is bound and whose float index is inside the parameters block, in track
/// order. Tracks on unbound slots are skipped. No allocation.
pub fn sample_writes(
    sanm: &Sanm,
    bytes: &[u8],
    frame: f32,
    binding: &[Option<u16>],
    mut emit: impl FnMut(ParamWrite),
) {
    for t in &sanm.tracks {
        let Some(Some(material)) = binding.get(t.target as usize) else {
            continue;
        };
        if t.sub as usize >= PARAM_FLOATS {
            continue;
        }
        let Some(Sample::Scalar(value)) = sample(bytes, t, frame) else {
            continue;
        };
        emit(ParamWrite {
            material: *material,
            index: t.sub,
            value,
        });
    }
}

/// [`sample_writes`] collected into `out` (cleared first).
pub fn evaluate_into(
    sanm: &Sanm,
    bytes: &[u8],
    frame: f32,
    binding: &[Option<u16>],
    out: &mut Vec<ParamWrite>,
) {
    out.clear();
    sample_writes(sanm, bytes, frame, binding, |w| out.push(w));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-material clip in the writer's layout: offU ramp 0 → −2 over 240
    /// frames on slot 0 (identity 0x11), constant offV; rgb pulse 1 → 0 → 1
    /// on slot 1 (identity 0x22).
    fn sample_file() -> Vec<u8> {
        let mut f: Vec<u8> = Vec::new();
        let push_u32 = |f: &mut Vec<u8>, v: u32| f.extend_from_slice(&v.to_le_bytes());
        let push_u16 = |f: &mut Vec<u8>, v: u16| f.extend_from_slice(&v.to_le_bytes());
        // header
        push_u32(&mut f, MAGIC);
        push_u16(&mut f, 240);
        push_u16(&mut f, 1);
        push_u32(&mut f, 60);
        push_u32(&mut f, 1);
        // chunk offsets: type 14 @0x1c, type 15 later (patched)
        push_u32(&mut f, 0x1c);
        push_u32(&mut f, 0); // patched
        push_u32(&mut f, 0);
        assert_eq!(f.len(), 0x1c);
        // type 14 chunk: tag, h4, h6, 5 track offsets + terminator, padded to 16
        let chunk14 = f.len();
        push_u32(&mut f, CHUNK_BASE + 14);
        push_u16(&mut f, 0);
        push_u16(&mut f, 0);
        let list_at = f.len();
        for _ in 0..6 {
            push_u32(&mut f, 0);
        }
        while f.len() % 16 != 0 {
            f.push(0);
        }
        let specs: [(u8, u8, &[(u16, f32)]); 5] = [
            (0, 2, &[(0, 0.0), (240, -2.0)]),
            (0, 3, &[(0, 0.0)]),
            (1, 4, &[(0, 1.0), (120, 0.0), (240, 1.0)]),
            (1, 5, &[(0, 1.0), (120, 0.0), (240, 1.0)]),
            (1, 6, &[(0, 1.0), (120, 0.0), (240, 1.0)]),
        ];
        let mut offs = Vec::new();
        for (target, sub, keys) in specs.iter() {
            let start = f.len();
            offs.push((start - chunk14) as u32);
            let n = keys.len();
            let times_len = 2 * n;
            let values_rel = (16 + times_len + 15) & !15;
            push_u16(&mut f, 8);
            push_u16(&mut f, 0);
            push_u16(&mut f, n as u16);
            f.push(*target);
            f.push(*sub);
            push_u32(&mut f, 16);
            push_u32(&mut f, values_rel as u32);
            for (t, _) in keys.iter() {
                push_u16(&mut f, *t);
            }
            while f.len() < start + values_rel {
                f.push(0);
            }
            for (_, v) in keys.iter() {
                push_u32(&mut f, v.to_bits());
            }
            while f.len() % 16 != 0 {
                f.push(0);
            }
        }
        for (i, o) in offs.iter().enumerate() {
            f[list_at + 4 * i..list_at + 4 * i + 4].copy_from_slice(&o.to_le_bytes());
        }
        // type 15 chunk
        let chunk15 = f.len() as u32;
        f[0x14..0x18].copy_from_slice(&chunk15.to_le_bytes());
        push_u32(&mut f, CHUNK_BASE + 15);
        push_u16(&mut f, 2);
        push_u16(&mut f, 0);
        for (identity, hash) in [(0x11u64, 0xAAu32), (0x22u64, 0xBBu32)] {
            f.extend_from_slice(&identity.to_le_bytes());
            f.extend_from_slice(&0u64.to_le_bytes());
            push_u32(&mut f, hash);
            push_u32(&mut f, 0x2000);
            f.extend_from_slice(&0u64.to_le_bytes());
        }
        f
    }

    #[test]
    fn parses_tracks_and_targets() {
        let bytes = sample_file();
        let s = parse(&bytes).unwrap();
        assert_eq!((s.frame_count, s.fps, s.loops), (240, 60.0, true));
        assert_eq!(s.tracks.len(), 5);
        assert_eq!(
            s.tracks
                .iter()
                .map(|t| (t.kind, t.target, t.sub, t.key_count))
                .collect::<Vec<_>>(),
            vec![
                (8, 0, 2, 2),
                (8, 0, 3, 1),
                (8, 1, 4, 3),
                (8, 1, 5, 3),
                (8, 1, 6, 3)
            ]
        );
        assert_eq!(s.tracks[0].times.as_deref(), Some(&[0u16, 240][..]));
        assert_eq!(
            s.targets,
            vec![
                MaterialTarget {
                    identity: 0x11,
                    shader_hash: 0xAA
                },
                MaterialTarget {
                    identity: 0x22,
                    shader_hash: 0xBB
                }
            ]
        );
        assert!((s.duration_s() - 4.0).abs() < 1e-6);
    }

    #[test]
    fn binds_by_identity_in_model_order_and_evaluates() {
        let bytes = sample_file();
        let s = parse(&bytes).unwrap();
        // model materials: [other, slot 1's, slot 0's] → slot 0 → 2, slot 1 → 1
        let binding = bind_targets(&s, &[0x99, 0x22, 0x11]);
        assert_eq!(binding, vec![Some(2), Some(1)]);
        let mut out = Vec::new();
        evaluate_into(&s, &bytes, 0.0, &binding, &mut out);
        assert_eq!(out.len(), 5);
        assert_eq!(
            out[0],
            ParamWrite {
                material: 2,
                index: 2,
                value: 0.0
            }
        );
        evaluate_into(&s, &bytes, 60.0, &binding, &mut out);
        assert!((out[0].value + 0.5).abs() < 1e-6, "{:?}", out[0]);
        assert!((out[2].value - 0.5).abs() < 1e-6, "{:?}", out[2]);
        assert_eq!((out[2].material, out[2].index), (1, 4));
        evaluate_into(&s, &bytes, 180.0, &binding, &mut out);
        assert!((out[0].value + 1.5).abs() < 1e-6);
        assert!((out[3].value - 0.5).abs() < 1e-6);
        // past the last key: held
        evaluate_into(&s, &bytes, 999.0, &binding, &mut out);
        assert!((out[0].value + 2.0).abs() < 1e-6);
        // an unbound slot drops its tracks
        let partial = bind_targets(&s, &[0x22]);
        assert_eq!(partial, vec![None, Some(0)]);
        evaluate_into(&s, &bytes, 0.0, &partial, &mut out);
        assert_eq!(out.len(), 3);
        assert!(out
            .iter()
            .all(|w| w.material == 0 && (4..=6).contains(&w.index)));
    }

    #[test]
    fn rejects_non_material_files_and_bad_magic() {
        assert_eq!(parse(&[0u8; 16]), Err(AnmError::BadMagic(0)));
        // a camera-shaped header with no 14/15 chunk
        let mut f = Vec::new();
        f.extend_from_slice(&MAGIC.to_le_bytes());
        f.extend_from_slice(&[0u8; 12]);
        f.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(parse(&f), Err(AnmError::Malformed));
        // truncated inside the target table
        let bytes = sample_file();
        assert_eq!(parse(&bytes[..bytes.len() - 8]), Err(AnmError::Truncated));
    }
}
