//! Host tests for `core/anm` — run through
//! `scripts/validate_background_dancers.sh` (the DLL crate itself cannot
//! `cargo test` on non-x86 hosts).
//!
//! Two layers:
//! - synthetic: hand-built byte images pin the parser, the sampling rules,
//!   the bind-seed / pose chain and the camera recipe with no external data;
//! - fixtures (`fixtures` sub-module): the Python-generated JSON under
//!   `tests/fixtures/anm/` replayed against the REAL stock arcs from
//!   `$DDR_WORLD_INSTALL` (skipped with a note when either is absent).

use super::anm::{parse, Channel};
use super::camera::{camera_from_slots, half_tangent, CamSlots};
use super::pose::{evaluate, sampled_trs, seed_local_trs, trs_to_local, Skeleton, Trs};
use super::sample::{clip_time, decode_q48, encode_q48, half_to_float, sample, Sample};
use super::{
    b2it, ktmdl, mat_inverse, mat_mul, mat_to_quat, quat_to_rowmat, rlist, Mat4, Quat, Vec3,
    MAT4_IDENTITY,
};

mod fixtures;

// ---------------------------------------------------------------------------
// Synthetic ANM builder (the game's own layout: header, absolute chunk offset
// list, chunks; 16-byte tracks followed by optional u16 times and 16-aligned
// values — anm_dump.py's writer).
// ---------------------------------------------------------------------------

pub(super) struct TrackSpec {
    pub kind: u16,
    pub target: u8,
    pub times: Option<Vec<u16>>,
    /// Raw encoded key bytes (all keys, contiguous; for 0x1F include the
    /// 12-byte base first).
    pub values: Vec<u8>,
    pub key_count: u16,
}

fn align16(n: usize) -> usize {
    (n + 15) & !15
}

fn put_u32(buf: &mut Vec<u8>, at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// Build an ANM image: optional type-1 chunk (ignored), a type-0 chunk with
/// `bone_tracks`, optional type-4 chunk with `camera_slots` (6 entries).
pub(super) fn build_anm(
    frame_count: u16,
    flags: u16,
    fps_field: u32,
    with_hierarchy: bool,
    bone_tracks: &[TrackSpec],
    camera_slots: Option<&[Option<TrackSpec>; 6]>,
) -> Vec<u8> {
    let mut buf = vec![0u8; 0x10];
    put_u32(&mut buf, 0, 0xFF01_0001);
    buf[4..6].copy_from_slice(&frame_count.to_le_bytes());
    buf[6..8].copy_from_slice(&flags.to_le_bytes());
    put_u32(&mut buf, 8, fps_field);
    put_u32(&mut buf, 0xC, if camera_slots.is_some() { 1 } else { 0 });
    let n_chunks = 1 + with_hierarchy as usize + camera_slots.is_some() as usize;
    let list_at = buf.len();
    buf.resize(align16(list_at + 4 * (n_chunks + 1)), 0);
    let mut chunk_slot = list_at;

    if with_hierarchy {
        let co = buf.len();
        put_u32(&mut buf, chunk_slot, co as u32);
        chunk_slot += 4;
        buf.extend_from_slice(&(0xFF01_0003u32).to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes()); // h4 = bone count
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0x10u32.to_le_bytes()); // rel → pairs
        buf.extend_from_slice(&0x14u32.to_le_bytes()); // rel → trailer
        buf.extend_from_slice(&[0, 0xFF, 1, 0]); // pairs (index, parent)
        buf.extend_from_slice(&2u16.to_le_bytes()); // trailer
        buf.resize(align16(buf.len()), 0);
    }

    // type-0 chunk
    {
        let co = buf.len();
        put_u32(&mut buf, chunk_slot, co as u32);
        chunk_slot += 4;
        buf.extend_from_slice(&(0xFF01_0002u32).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        let rel_list_at = buf.len();
        buf.resize(align16(rel_list_at + 4 * (bone_tracks.len() + 1)), 0);
        for (i, t) in bone_tracks.iter().enumerate() {
            let to = write_track(&mut buf, t);
            put_u32(&mut buf, rel_list_at + 4 * i, (to - co) as u32);
        }
    }

    if let Some(slots) = camera_slots {
        let co = buf.len();
        put_u32(&mut buf, chunk_slot, co as u32);
        buf.extend_from_slice(&(0xFF01_0006u32).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        let slot_list_at = buf.len();
        buf.resize(align16(slot_list_at + 24), 0);
        for (i, s) in slots.iter().enumerate() {
            if let Some(t) = s {
                let to = write_track(&mut buf, t);
                put_u32(&mut buf, slot_list_at + 4 * i, (to - co) as u32);
            }
        }
    }
    buf
}

fn write_track(buf: &mut Vec<u8>, t: &TrackSpec) -> usize {
    let to = buf.len();
    buf.resize(to + 16, 0);
    buf[to..to + 2].copy_from_slice(&t.kind.to_le_bytes());
    buf[to + 2..to + 4].copy_from_slice(&(if t.times.is_some() { 0u16 } else { 3 }).to_le_bytes());
    buf[to + 4..to + 6].copy_from_slice(&t.key_count.to_le_bytes());
    buf[to + 6] = t.target;
    buf[to + 7] = 0;
    let mut times_rel = 0u32;
    if let Some(times) = &t.times {
        times_rel = (buf.len() - to) as u32;
        for tm in times {
            buf.extend_from_slice(&tm.to_le_bytes());
        }
    }
    buf.resize(align16(buf.len()), 0);
    let values_rel = (buf.len() - to) as u32;
    buf.extend_from_slice(&t.values);
    buf.resize(align16(buf.len()), 0);
    put_u32(buf, to + 8, times_rel);
    put_u32(buf, to + 0xC, values_rel);
    to
}

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn quat_axis(axis: Vec3, angle: f32) -> Quat {
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let (s, c) = ((angle * 0.5).sin(), (angle * 0.5).cos());
    [axis[0] / n * s, axis[1] / n * s, axis[2] / n * s, c]
}

fn quat_close(a: &Quat, b: &Quat, tol: f32) -> bool {
    let same = a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol);
    let neg = a.iter().zip(b).all(|(x, y)| (x + y).abs() <= tol);
    same || neg
}

fn mat_close(a: &Mat4, b: &Mat4, tol: f32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

fn rot_track(target: u8, keys: &[Quat], times: Option<Vec<u16>>) -> TrackSpec {
    TrackSpec {
        kind: 0x1C,
        target,
        times,
        values: keys.iter().flat_map(|q| encode_q48(q)).collect(),
        key_count: keys.len() as u16,
    }
}

fn pos_track(target: u8, keys: &[Vec3], times: Option<Vec<u16>>) -> TrackSpec {
    TrackSpec {
        kind: 0x1D,
        target,
        times,
        values: keys.iter().flat_map(|p| f32s(p)).collect(),
        key_count: keys.len() as u16,
    }
}

// ---------------------------------------------------------------------------
// q48 / half / clip_time
// ---------------------------------------------------------------------------

#[test]
fn q48_round_trip_all_branches() {
    let cases: [Quat; 8] = [
        [0.9, 0.1, 0.2, 0.3],
        [0.1, 0.9, 0.2, 0.3],
        [0.1, 0.2, 0.9, 0.3],
        [0.1, 0.2, 0.3, 0.9],
        [-0.9, 0.1, 0.2, 0.3],
        [0.1, -0.9, 0.2, 0.3],
        [0.0, 0.0, 0.0, 1.0],
        [0.5, 0.5, 0.5, 0.5],
    ];
    for q in cases {
        let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        let qn = [q[0] / n, q[1] / n, q[2] / n, q[3] / n];
        let d = decode_q48(encode_q48(&qn));
        assert!(quat_close(&d, &qn, 1e-4), "{qn:?} -> {d:?}");
        let dn = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2] + d[3] * d[3]).sqrt();
        assert!((dn - 1.0).abs() < 1e-4);
    }
    // Reference vector: identity quaternion encodes with m = 3 and the
    // three zero components at the mid code 16383.5 → 16384 (round half up
    // in Python's round → banker's rounding gives 16384 too since .5 → even).
    let e = encode_q48(&[0.0, 0.0, 0.0, 1.0]);
    let v = u64::from_le_bytes([e[0], e[1], e[2], e[3], e[4], e[5], 0, 0]);
    assert_eq!(v & 3, 3);
    assert_eq!((v >> 2) & 0x7FFF, 16384);
}

#[test]
fn half_float_known_patterns() {
    assert_eq!(half_to_float(0x3C00), 1.0);
    assert_eq!(half_to_float(0xC000), -2.0);
    assert_eq!(half_to_float(0x3800), 0.5);
    assert_eq!(half_to_float(0x0000), 0.0);
    // smallest denormal = 2^-24
    assert!((half_to_float(0x0001) - 2f32.powi(-24)).abs() < 1e-12);
    assert!(half_to_float(0x7C00).is_infinite());
    assert!(half_to_float(0xFC00).is_infinite() && half_to_float(0xFC00) < 0.0);
    // 0.333251953125 (0x3555)
    assert!((half_to_float(0x3555) - 0.333_251_953_125).abs() < 1e-7);
}

#[test]
fn clip_time_wrap_and_clamp() {
    assert_eq!(clip_time(0.5, 2.0, true), (0.5, false));
    assert_eq!(clip_time(2.5, 2.0, true), (0.5, false));
    assert_eq!(clip_time(4.0, 2.0, true), (0.0, false));
    let (t, f) = clip_time(-0.5, 2.0, true);
    assert!((t - 1.5).abs() < 1e-6 && !f);
    assert_eq!(clip_time(-1.0, 2.0, false), (0.0, false));
    assert_eq!(clip_time(1.0, 2.0, false), (1.0, false));
    assert_eq!(clip_time(2.0, 2.0, false), (2.0, true));
    assert_eq!(clip_time(9.0, 2.0, false), (2.0, true));
    assert_eq!(clip_time(1.0, 0.0, true), (0.0, true));
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

#[test]
fn parse_synthetic_skeletal_clip() {
    let q0 = quat_axis([0.0, 1.0, 0.0], 0.3);
    let q1 = quat_axis([0.0, 1.0, 0.0], 1.1);
    let tracks = [
        rot_track(1, &[q0, q1, q1], None),
        pos_track(1, &[[0.0, 1.0, 0.0], [1.0, 1.0, 0.0]], Some(vec![0, 10])),
    ];
    let img = build_anm(10, 1, 1, true, &tracks, None);
    let a = parse(&img).expect("parse");
    assert_eq!(a.frame_count, 10);
    assert!(a.loops);
    assert_eq!(a.fps, 60.0);
    assert!(!a.has_camera());
    assert!((a.duration_s() - 10.0 / 60.0).abs() < 1e-6);
    assert_eq!(a.bone_tracks.len(), 2);
    let r = &a.bone_tracks[0];
    assert_eq!(
        (r.kind, r.channel, r.target, r.key_count),
        (0x1C, Channel::Rotation, 1, 3)
    );
    assert!(r.times.is_none());
    assert_eq!(r.values.len(), 18);
    let p = &a.bone_tracks[1];
    assert_eq!(
        (p.kind, p.channel, p.target, p.key_count),
        (0x1D, Channel::Translation, 1, 2)
    );
    assert_eq!(p.times.as_deref(), Some(&[0u16, 10][..]));
    assert_eq!(p.values.len(), 24);

    // loop bit clear
    let img2 = build_anm(10, 0, 1, false, &tracks, None);
    assert!(!parse(&img2).unwrap().loops);
    // other header bits do not count as loop
    let img3 = build_anm(10, 2, 1, false, &tracks, None);
    assert!(!parse(&img3).unwrap().loops);
}

#[test]
fn parse_camera_reads_fps_and_slots() {
    let slots: [Option<TrackSpec>; 6] = [
        Some(TrackSpec {
            kind: 1,
            target: 0,
            times: Some(vec![0, 30]),
            values: f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.7071068, 0.0, 0.7071068]),
            key_count: 2,
        }),
        Some(TrackSpec {
            kind: 4,
            target: 1,
            times: None,
            values: f32s(&[100.0, 200.0, 300.0, 1.0]),
            key_count: 1,
        }),
        Some(TrackSpec {
            kind: 8,
            target: 2,
            times: None,
            values: f32s(&[41.53]),
            key_count: 1,
        }),
        None,
        Some(TrackSpec {
            kind: 8,
            target: 4,
            times: None,
            values: f32s(&[32768.0]),
            key_count: 1,
        }),
        None,
    ];
    let img = build_anm(6238, 0, 24, false, &[], Some(&slots));
    let a = parse(&img).expect("parse");
    assert!(a.has_camera());
    assert_eq!(a.fps, 24.0);
    assert!(a.camera_slots[3].is_none() && a.camera_slots[5].is_none());
    assert_eq!(
        a.camera_slots[0].as_ref().unwrap().channel,
        Channel::CamQuat
    );
    assert_eq!(a.camera_slots[1].as_ref().unwrap().channel, Channel::CamPos);
    assert_eq!(
        a.camera_slots[2].as_ref().unwrap().channel,
        Channel::CamScalar
    );

    let s = super::camera::sample_slots(&a, &img, 0.0);
    assert_eq!(s.present, [true, true, true, false, true, false]);
    assert_eq!(s.pos_cm, [100.0, 200.0, 300.0]);
    assert_eq!(s.near, super::camera::DEFAULT_NEAR);
    assert_eq!(s.far, 32768.0);
    assert_eq!(s.aspect_file, super::camera::DEFAULT_ASPECT_FILE);
    let c = camera_from_slots(&s, 1.0, 1.0);
    assert!((c.eye[0] - 1.0).abs() < 1e-6 && (c.eye[1] - 2.0).abs() < 1e-6);
    // identity orientation looks down −Z
    assert!((c.target[2] - (3.0 - 10.0)).abs() < 1e-5);
    assert_eq!(c.up, [0.0, 1.0, 0.0]);
}

#[test]
fn parse_rejects_garbage() {
    assert!(matches!(parse(&[]), Err(super::AnmError::Truncated)));
    assert!(matches!(parse(&[0u8; 3]), Err(super::AnmError::Truncated)));
    assert!(matches!(
        parse(&[0u8; 4]),
        Err(super::AnmError::BadMagic(0))
    ));
    assert!(matches!(
        parse(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]),
        Err(super::AnmError::BadMagic(_))
    ));
    // valid magic, no chunk list terminator
    assert!(matches!(
        parse(&0xFF01_0001u32.to_le_bytes()),
        Err(super::AnmError::Truncated)
    ));
    let mut img = build_anm(
        1,
        0,
        1,
        false,
        &[rot_track(0, &[[0.0, 0.0, 0.0, 1.0]], None)],
        None,
    );
    // unknown kind
    let track_off = {
        let co = u32::from_le_bytes(img[0x10..0x14].try_into().unwrap()) as usize;
        co + u32::from_le_bytes(img[co + 8..co + 12].try_into().unwrap()) as usize
    };
    img[track_off] = 0x77;
    assert!(matches!(
        parse(&img),
        Err(super::AnmError::UnknownKind(0x77))
    ));
    // truncated values (cut inside the 6-byte key)
    img[track_off] = 0x1C;
    let values_at = track_off
        + u32::from_le_bytes(img[track_off + 0xC..track_off + 0x10].try_into().unwrap()) as usize;
    let short = &img[..values_at + 3];
    assert!(matches!(parse(short), Err(super::AnmError::Truncated)));
    // intact values right up to the end still parse
    assert!(parse(&img[..values_at + 6]).is_ok());
}

// ---------------------------------------------------------------------------
// Sampling
// ---------------------------------------------------------------------------

#[test]
fn sample_uniform_rotation_slerp_and_clamp() {
    let q0 = quat_axis([0.0, 1.0, 0.0], 0.0);
    let q1 = quat_axis([0.0, 1.0, 0.0], 1.0);
    let q2 = quat_axis([0.0, 1.0, 0.0], 2.0);
    let img = build_anm(2, 0, 1, false, &[rot_track(0, &[q0, q1, q2], None)], None);
    let a = parse(&img).unwrap();
    let t = &a.bone_tracks[0];
    // exact keys
    assert!(quat_close(
        &sample(&img, t, 0.0).unwrap().quat().unwrap(),
        &q0,
        2e-4
    ));
    assert!(quat_close(
        &sample(&img, t, 1.0).unwrap().quat().unwrap(),
        &q1,
        2e-4
    ));
    // midpoint slerp of two rotations about the same axis = half angle
    let mid = sample(&img, t, 0.5).unwrap().quat().unwrap();
    assert!(
        quat_close(&mid, &quat_axis([0.0, 1.0, 0.0], 0.5), 3e-4),
        "{mid:?}"
    );
    // clamp at/after the last key
    assert!(quat_close(
        &sample(&img, t, 2.0).unwrap().quat().unwrap(),
        &q2,
        2e-4
    ));
    assert!(quat_close(
        &sample(&img, t, 7.25).unwrap().quat().unwrap(),
        &q2,
        2e-4
    ));
    // single-key track
    let img1 = build_anm(5, 0, 1, false, &[rot_track(0, &[q1], None)], None);
    let a1 = parse(&img1).unwrap();
    assert!(quat_close(
        &sample(&img1, &a1.bone_tracks[0], 3.7)
            .unwrap()
            .quat()
            .unwrap(),
        &q1,
        2e-4
    ));
}

#[test]
fn sample_slerp_takes_shortest_path() {
    let q0 = quat_axis([0.0, 1.0, 0.0], 0.2);
    let mut q1 = quat_axis([0.0, 1.0, 0.0], 0.4);
    for c in q1.iter_mut() {
        *c = -*c; // same rotation, negated representation
    }
    // Encode drops the sign anyway; test slerp directly.
    let s = super::sample::slerp(&q0, &q1, 0.5);
    assert!(
        quat_close(&s, &quat_axis([0.0, 1.0, 0.0], 0.3), 1e-5),
        "{s:?}"
    );
    let n = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2] + s[3] * s[3]).sqrt();
    assert!((n - 1.0).abs() < 1e-5);
    // nearly identical quats → lerp branch, still normalised
    let s2 = super::sample::slerp(&q0, &q0, 0.3);
    assert!(quat_close(&s2, &q0, 1e-6));
}

#[test]
fn sample_explicit_times_lerp_dup_skip_and_clamp() {
    // keys at frames 0, 10, 10 (dup), 20 — values 0, 1, 5, 2 on x
    let keys: [Vec3; 4] = [[0.0; 3], [1.0, 0.0, 0.0], [5.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
    let img = build_anm(
        30,
        0,
        1,
        false,
        &[pos_track(0, &keys, Some(vec![0, 10, 10, 20]))],
        None,
    );
    let a = parse(&img).unwrap();
    let t = &a.bone_tracks[0];
    let at = |f: f32| sample(&img, t, f).unwrap().vec3().unwrap()[0];
    assert!((at(0.0) - 0.0).abs() < 1e-6);
    assert!((at(5.0) - 0.5).abs() < 1e-6);
    assert!((at(9.5) - 0.95).abs() < 1e-6);
    // floor(frame) = 10 → i = the LAST key with time 10 (index 2, value 5),
    // i1 = 3 → lerp 5 → 2 over frames 10..20
    assert!((at(10.0) - 5.0).abs() < 1e-6);
    assert!((at(15.0) - 3.5).abs() < 1e-6);
    // fi >= last time → last key
    assert!((at(20.0) - 2.0).abs() < 1e-6);
    assert!((at(25.5) - 2.0).abs() < 1e-6);
}

#[test]
fn sample_explicit_dup_skip_forward() {
    // keys at 0, 0 (dup), 8 — i = 1 (last <= 0), i1 skips nothing since
    // times[2] != times[1]; a key list where the dup follows i:
    // times 0, 4, 4, 4, 8 → at frame 2: i = 0, i1 = 1 (no skip needed);
    // at frame 4: i = 3 (max k with t<=4), i1 = 4.
    let keys: [Vec3; 5] = [
        [0.0; 3],
        [4.0, 0.0, 0.0],
        [40.0, 0.0, 0.0],
        [8.0, 0.0, 0.0],
        [16.0, 0.0, 0.0],
    ];
    let img = build_anm(
        30,
        0,
        1,
        false,
        &[pos_track(0, &keys, Some(vec![0, 4, 4, 4, 8]))],
        None,
    );
    let a = parse(&img).unwrap();
    let t = &a.bone_tracks[0];
    let at = |f: f32| sample(&img, t, f).unwrap().vec3().unwrap()[0];
    assert!((at(2.0) - 2.0).abs() < 1e-6);
    assert!((at(4.0) - 8.0).abs() < 1e-6);
    assert!((at(6.0) - 12.0).abs() < 1e-6);
    // Python: i = max k with times[k] <= fi picks the LAST duplicate, so the
    // while-skip only matters when the dup run sits AFTER i — `i1 < n-1`
    // guard: times 0, 5, 5 with 3 keys, frame 2: i=0, i1=1, times[1]==times[0]?
    // no → lerp 0..5 keys 0,1.
    let keys3: [Vec3; 3] = [[0.0; 3], [10.0, 0.0, 0.0], [99.0, 0.0, 0.0]];
    let img3 = build_anm(
        30,
        0,
        1,
        false,
        &[pos_track(0, &keys3, Some(vec![0, 5, 5]))],
        None,
    );
    let a3 = parse(&img3).unwrap();
    let v = sample(&img3, &a3.bone_tracks[0], 2.5)
        .unwrap()
        .vec3()
        .unwrap()[0];
    assert!((v - 5.0).abs() < 1e-6);
    // frame 5: fi >= times[-1] = 5 → last key (99)
    let v = sample(&img3, &a3.bone_tracks[0], 5.0)
        .unwrap()
        .vec3()
        .unwrap()[0];
    assert!((v - 99.0).abs() < 1e-6);
}

#[test]
fn sample_half_and_base_delta_kinds() {
    // 0x1E: 3 halves per key
    let h = |f: f32| -> u16 {
        // only exact halves used below
        match f {
            1.0 => 0x3C00,
            0.5 => 0x3800,
            -2.0 => 0xC000,
            _ => 0,
        }
    };
    let mut v = Vec::new();
    for k in [[1.0f32, 0.5, -2.0], [0.5, 1.0, 0.0]] {
        for c in k {
            v.extend_from_slice(&h(c).to_le_bytes());
        }
    }
    let t1e = TrackSpec {
        kind: 0x1E,
        target: 0,
        times: None,
        values: v,
        key_count: 2,
    };
    // 0x1F: base + half deltas
    let mut v = f32s(&[10.0, 20.0, 30.0]);
    for k in [[0.0f32, 0.0, 0.0], [1.0, 0.5, -2.0]] {
        for c in k {
            v.extend_from_slice(&h(c).to_le_bytes());
        }
    }
    let t1f = TrackSpec {
        kind: 0x1F,
        target: 1,
        times: None,
        values: v,
        key_count: 2,
    };
    // scale kind 10 (16-byte keys)
    let t10 = TrackSpec {
        kind: 10,
        target: 2,
        times: None,
        values: f32s(&[2.0, 2.0, 2.0, 1.0, 4.0, 4.0, 4.0, 1.0]),
        key_count: 2,
    };
    let img = build_anm(1, 0, 1, false, &[t1e, t1f, t10], None);
    let a = parse(&img).unwrap();
    assert_eq!(a.bone_tracks[1].values.len(), 12 + 12);
    let v = sample(&img, &a.bone_tracks[0], 0.5)
        .unwrap()
        .vec3()
        .unwrap();
    assert_eq!(v, [0.75, 0.75, -1.0]);
    let v = sample(&img, &a.bone_tracks[1], 1.0)
        .unwrap()
        .vec3()
        .unwrap();
    assert_eq!(v, [11.0, 20.5, 28.0]);
    let v = sample(&img, &a.bone_tracks[2], 0.25)
        .unwrap()
        .vec3()
        .unwrap();
    assert_eq!(a.bone_tracks[2].channel, Channel::Scale);
    assert_eq!(v, [2.5, 2.5, 2.5]);
}

#[test]
fn sample_step_kind_holds() {
    let t = TrackSpec {
        kind: 0x1B,
        target: 0,
        times: None,
        values: f32s(&[1.0, 2.0]),
        key_count: 2,
    };
    let img = build_anm(1, 0, 1, false, &[t], None);
    let a = parse(&img).unwrap();
    assert_eq!(
        sample(&img, &a.bone_tracks[0], 0.99).unwrap(),
        Sample::Scalar(1.0)
    );
    assert_eq!(
        sample(&img, &a.bone_tracks[0], 1.0).unwrap(),
        Sample::Scalar(2.0)
    );
}

// ---------------------------------------------------------------------------
// Matrix / quaternion helpers
// ---------------------------------------------------------------------------

#[test]
fn mat_to_quat_inverts_quat_to_rowmat() {
    let axes: [Vec3; 5] = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
        [-0.3, 0.8, 0.5],
    ];
    for axis in axes {
        for k in 0..12 {
            let angle = k as f32 * 0.55 - 3.0; // covers all four branches
            let q = quat_axis(axis, angle);
            let r = quat_to_rowmat(&q);
            let mut m = MAT4_IDENTITY;
            for i in 0..3 {
                for j in 0..3 {
                    m[i * 4 + j] = r[i][j];
                }
            }
            let q2 = mat_to_quat(&m);
            assert!(
                quat_close(&q, &q2, 1e-5),
                "axis {axis:?} angle {angle}: {q:?} vs {q2:?}"
            );
        }
    }
}

#[test]
fn mat_inverse_round_trip() {
    let q = quat_axis([0.2, 0.9, -0.4], 1.3);
    let m = trs_to_local(&Trs {
        q,
        t: [1.0, -2.0, 3.5],
        s: [1.0, 1.0, 1.0],
    });
    let inv = mat_inverse(&m).unwrap();
    assert!(mat_close(&mat_mul(&m, &inv), &MAT4_IDENTITY, 1e-5));
    assert!(mat_close(&mat_mul(&inv, &m), &MAT4_IDENTITY, 1e-5));
}

// ---------------------------------------------------------------------------
// Pose chain / bind seed
// ---------------------------------------------------------------------------

/// A 4-bone chain 0 → 1 → 2 → 3 with rotations + translations (unit scale).
fn synthetic_rig() -> Skeleton {
    let locals = [
        Trs {
            q: quat_axis([0.0, 1.0, 0.0], 0.4),
            t: [0.0, 0.9, 0.0],
            s: [1.0; 3],
        },
        Trs {
            q: quat_axis([1.0, 0.0, 0.0], -0.7),
            t: [0.1, 0.2, 0.0],
            s: [1.0; 3],
        },
        Trs {
            q: quat_axis([0.0, 0.0, 1.0], 1.9),
            t: [0.0, 0.3, 0.05],
            s: [1.0; 3],
        },
        Trs {
            q: quat_axis([0.3, 0.3, 0.9], 2.6),
            t: [0.0, 0.25, 0.0],
            s: [1.0; 3],
        },
    ];
    let parents = vec![-1i16, 0, 1, 2];
    let mut bind_world = Vec::new();
    for (i, l) in locals.iter().enumerate() {
        let local = trs_to_local(l);
        let w = if i == 0 {
            local
        } else {
            mat_mul(&local, &bind_world[i - 1])
        };
        bind_world.push(w);
    }
    let inverse_bind = bind_world.iter().map(|m| mat_inverse(m).unwrap()).collect();
    Skeleton {
        parents,
        bind_world,
        inverse_bind,
    }
}

#[test]
fn seed_reproduces_bind_with_no_tracks() {
    let sk = synthetic_rig();
    let seed = seed_local_trs(&sk);
    assert_eq!(seed.len(), 4);
    for s in &seed {
        assert!(s.s.iter().all(|v| (v - 1.0).abs() < 1e-5), "{s:?}");
    }
    let img = build_anm(1, 0, 1, false, &[], None);
    let a = parse(&img).unwrap();
    let world = evaluate(&a, &img, 0.0, &sk, &seed);
    for i in 0..4 {
        assert!(
            mat_close(&world[i], &sk.bind_world[i], 1e-5),
            "bone {i}: {:?} vs {:?}",
            world[i],
            sk.bind_world[i]
        );
    }
    // identity seed (the Python default) does NOT reproduce the bind pose
    let ident = vec![Trs::IDENTITY; 4];
    let w2 = evaluate(&a, &img, 0.0, &sk, &ident);
    assert!(!mat_close(&w2[1], &sk.bind_world[1], 1e-3));
}

#[test]
fn partial_clip_moves_only_the_tracked_subtree() {
    let sk = synthetic_rig();
    let seed = seed_local_trs(&sk);
    // rotation track on bone 2 only: a different rotation than its bind local
    let q_new = quat_axis([1.0, 0.0, 0.0], 0.9);
    let img = build_anm(1, 0, 1, false, &[rot_track(2, &[q_new, q_new], None)], None);
    let a = parse(&img).unwrap();
    let world = evaluate(&a, &img, 0.5, &sk, &seed);
    assert!(mat_close(&world[0], &sk.bind_world[0], 1e-5));
    assert!(mat_close(&world[1], &sk.bind_world[1], 1e-5));
    assert!(!mat_close(&world[2], &sk.bind_world[2], 1e-3));
    assert!(!mat_close(&world[3], &sk.bind_world[3], 1e-3));
    // bone 2's world = new local (seed t, new q) · world[1]
    let expect2 = mat_mul(
        &trs_to_local(&Trs {
            q: decode_q48(encode_q48(&q_new)),
            t: seed[2].t,
            s: seed[2].s,
        }),
        &world[1],
    );
    assert!(mat_close(&world[2], &expect2, 1e-4));
    // bone 3 keeps its bind-relative local: world3 · inv(world2) unchanged
    let rel_bind = mat_mul(&sk.bind_world[3], &mat_inverse(&sk.bind_world[2]).unwrap());
    let rel_now = mat_mul(&world[3], &mat_inverse(&world[2]).unwrap());
    assert!(mat_close(&rel_bind, &rel_now, 1e-4));
    // the pre-chain TRS view agrees
    let trs = sampled_trs(&a, &img, 0.5, 4, &seed);
    assert!(quat_close(&trs[2].q, &q_new, 2e-4));
    assert_eq!(trs[1], seed[1]);
}

#[test]
fn scaled_bone_seed_and_compensation_a3_semantics() {
    // Stock stage props carry non-unit (often non-uniform) bind scales on
    // NON-root bones — every such bone has its own scale track and every
    // child of a scaled bone has a rotation track (checked over all 69 stock
    // `_play_loop` clips, 2026-09-16), so the game only ever relies on the
    // seed's TRANSLATION and SCALE there. A3 feeds the scaled matrix to its
    // matrix→quaternion routine unnormalised (an imperfect rotation seed that
    // the tracks overwrite) — the port keeps that, and this test pins the
    // parts the game does rely on: unit root, uniformly scaled middle bone,
    // unit child; with the tracks stock data would carry, the chain
    // reproduces the bind pose.
    let root = Trs {
        q: quat_axis([0.0, 1.0, 0.0], 0.6),
        t: [0.0, 1.0, 0.0],
        s: [1.0; 3],
    };
    let mid = Trs {
        q: quat_axis([1.0, 0.0, 0.0], 0.8),
        t: [0.0, 0.5, 0.0],
        s: [2.0, 2.0, 2.0],
    };
    let leaf = Trs {
        q: quat_axis([0.0, 0.0, 1.0], -1.1),
        t: [0.3, 0.2, 0.0],
        s: [1.0; 3],
    };
    // Bind world built with the game's own chain (column compensation).
    let w0 = trs_to_local(&root);
    let w1 = mat_mul(&trs_to_local(&mid), &w0); // root scale 1 → no compensation
    let mut l2 = trs_to_local(&leaf);
    for r in 0..3 {
        for c in 0..3 {
            l2[r * 4 + c] /= mid.s[c];
        }
    }
    let w2 = mat_mul(&l2, &w1);
    let sk = Skeleton {
        parents: vec![-1, 0, 1],
        bind_world: vec![w0, w1, w2],
        inverse_bind: vec![
            mat_inverse(&w0).unwrap(),
            mat_inverse(&w1).unwrap(),
            mat_inverse(&w2).unwrap(),
        ],
    };
    let seed = seed_local_trs(&sk);
    // translations and scales come straight out of the seed
    for (i, exp) in [root, mid, leaf].iter().enumerate() {
        assert!(
            seed[i]
                .t
                .iter()
                .zip(&exp.t)
                .all(|(a, b)| (a - b).abs() < 1e-5),
            "t{i} {:?}",
            seed[i]
        );
    }
    assert!(seed[0].s.iter().all(|v| (v - 1.0).abs() < 1e-5));
    assert!(
        seed[1].s.iter().all(|v| (v - 2.0).abs() < 1e-5),
        "{:?}",
        seed[1]
    );
    // child of a scaled parent: the bind already carries the column
    // compensation, so P = diag(ps)·M is exactly the child's own local
    // rotation — unit scale AND an exact rotation seed.
    assert!(
        seed[2].s.iter().all(|v| (v - 1.0).abs() < 1e-5),
        "{:?}",
        seed[2]
    );
    assert!(quat_close(&seed[2].q, &leaf.q, 1e-5), "{:?}", seed[2]);
    // unit-scale root: exact rotation seed too
    assert!(quat_close(&seed[0].q, &root.q, 1e-5));
    // the SCALED bone itself is the one inexact rotation seed (mat→quat of
    // an unnormalised 2·R) — stock data always tracks its rotation.
    assert!(!quat_close(&seed[1].q, &mid.q, 1e-2), "{:?}", seed[1]);

    // With just that rotation track (the scale comes from the seed) the
    // chain reproduces the bind pose, the untracked child included.
    let tracks = [rot_track(1, &[mid.q, mid.q], None)];
    let img = build_anm(1, 0, 1, false, &tracks, None);
    let a = parse(&img).unwrap();
    let world = evaluate(&a, &img, 0.5, &sk, &seed);
    assert!(
        mat_close(&world[0], &w0, 1e-4),
        "{:?} vs {:?}",
        world[0],
        w0
    );
    assert!(
        mat_close(&world[1], &w1, 2e-4),
        "{:?} vs {:?}",
        world[1],
        w1
    );
    assert!(
        mat_close(&world[2], &w2, 2e-4),
        "{:?} vs {:?}",
        world[2],
        w2
    );
}

#[test]
fn evaluate_skips_out_of_range_targets_and_reads_python_semantics() {
    let sk = synthetic_rig();
    let seed = seed_local_trs(&sk);
    let img = build_anm(
        1,
        0,
        1,
        false,
        &[rot_track(9, &[quat_axis([0.0, 1.0, 0.0], 1.0)], None)],
        None,
    );
    let a = parse(&img).unwrap();
    let world = evaluate(&a, &img, 0.0, &sk, &seed);
    for i in 0..4 {
        assert!(mat_close(&world[i], &sk.bind_world[i], 1e-5));
    }
}

// ---------------------------------------------------------------------------
// Camera recipe
// ---------------------------------------------------------------------------

fn add_on_half_tangent(fov_v_deg: f64, aspect_file: f64) -> f64 {
    // tools/blender_ddr_addon/import_anm.py::game_camera_half_tangent
    let h = (fov_v_deg.to_radians() * 0.5).tan() * aspect_file;
    let fov_prime = 2.0f64.atan2(2.0 * h);
    (0.5 * fov_prime).tan()
}

#[test]
fn camera_half_tangent_matches_add_on_and_worked_values() {
    // (fovV, aspect) → in-game hFOV (docs §6 worked values)
    let cases = [
        (41.53f32, 4.0f32 / 3.0, 63.2f32),
        (37.85, 1.5, 62.8),
        (70.4, 4.0 / 3.0, 46.8),
    ];
    for (fov, asp, hfov) in cases {
        let tp = half_tangent(fov, asp, 1.0);
        let reference = add_on_half_tangent(fov as f64, asp as f64) as f32;
        assert!(
            (tp - reference).abs() < 1e-6,
            "{fov}/{asp}: {tp} vs {reference}"
        );
        let got_hfov = 2.0 * tp.atan().to_degrees();
        assert!(
            (got_hfov - hfov).abs() < 0.1,
            "{fov}/{asp}: hFOV {got_hfov} vs {hfov}"
        );
    }
    // monotone DECREASING in the file FOV
    let mut prev = f32::INFINITY;
    let mut f = 10.0f32;
    while f <= 90.0 {
        let tp = half_tangent(f, 4.0 / 3.0, 1.0);
        assert!(tp < prev, "not decreasing at {f}");
        prev = tp;
        f += 2.5;
    }
    // aspect_mul scales exactly like the file aspect
    assert!((half_tangent(40.0, 1.5, 1.0) - half_tangent(40.0, 1.0, 1.5)).abs() < 1e-7);
}

#[test]
fn camera_from_slots_geometry() {
    let q = quat_axis([0.0, 1.0, 0.0], 0.5);
    let r = quat_to_rowmat(&q);
    let s = CamSlots {
        q,
        pos_cm: [120.0, 160.0, 500.0],
        fov_v_deg: 41.53,
        near: 0.1,
        far: 32768.0,
        aspect_file: 4.0 / 3.0,
        present: [true; 6],
    };
    let c = camera_from_slots(&s, 2.0, 1.0);
    assert!(
        (c.eye[0] - 1.2).abs() < 1e-6
            && (c.eye[1] - 1.6).abs() < 1e-6
            && (c.eye[2] - 5.0).abs() < 1e-6
    );
    let d = [
        c.target[0] - c.eye[0],
        c.target[1] - c.eye[1],
        c.target[2] - c.eye[2],
    ];
    assert!((super::vec3_len(&d) - 10.0).abs() < 1e-4);
    assert!((d[0] + 10.0 * r[2][0]).abs() < 1e-5 && (d[2] + 10.0 * r[2][2]).abs() < 1e-5);
    assert!((super::vec3_len(&c.up) - 1.0).abs() < 1e-5);
    assert_eq!(c.l, -c.r);
    assert_eq!(c.b, -c.t);
    assert!((c.r / c.t - 16.0 / 9.0).abs() < 1e-5);
    assert!((c.near - 0.2).abs() < 1e-7);
    assert_eq!(c.far, 32768.0);
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

fn build_b2it(entries: &[(&str, u32)]) -> Vec<u8> {
    let mut out = vec![0u8; 0x20];
    out[0..4].copy_from_slice(b"B2IT");
    let n = entries.len();
    let names_off = out.len();
    out.resize(out.len() + 4 * n, 0);
    for (i, (name, _)) in entries.iter().enumerate() {
        let at = out.len() as u32;
        out[names_off + 4 * i..names_off + 4 * i + 4].copy_from_slice(&at.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(0);
    }
    while out.len() % 4 != 0 {
        out.push(0);
    }
    let idx_off = out.len();
    for (_, idx) in entries {
        out.extend_from_slice(&idx.to_le_bytes());
    }
    let size = out.len() as u32;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    out[0x10..0x14].copy_from_slice(&(n as u32).to_le_bytes());
    out[0x14..0x18].copy_from_slice(&(names_off as u32).to_le_bytes());
    out[0x18..0x1C].copy_from_slice(&(idx_off as u32).to_le_bytes());
    out
}

#[test]
fn b2it_parse_and_lookup() {
    let img = build_b2it(&[
        ("Head", 16),
        ("Hips", 1),
        ("LeftForeArmRoll", 29),
        ("Spine2", 8),
    ]);
    let t = b2it::parse(&img).unwrap();
    assert_eq!(t.len(), 4);
    assert_eq!(t[0], ("Head".to_string(), 16));
    assert_eq!(b2it::index_of(&t, "Spine2"), Some(8));
    assert_eq!(b2it::index_of(&t, "Hips"), Some(1));
    assert_eq!(b2it::index_of(&t, "hips"), None);
    assert_eq!(b2it::index_of(&t, "Nope"), None);
    assert_eq!(b2it::parse(b"XXXX"), Err(super::FormatError::BadMagic));
    assert_eq!(
        b2it::parse(&img[..0x18]),
        Err(super::FormatError::Truncated)
    );
}

fn build_rlist(rows: &[(&str, &[&str])]) -> Vec<u8> {
    let mut out = vec![0u8; 0x10];
    out[0..4].copy_from_slice(b"MRL0");
    out[4..6].copy_from_slice(b"LE");
    for (key, fields) in rows {
        let rec = out.len();
        let hdr = 12 + 4 * fields.len();
        out.resize(rec + hdr, 0);
        let str_off = out.len() - rec;
        out.extend_from_slice(key.as_bytes());
        out.push(0);
        let mut offs = Vec::new();
        for f in fields.iter() {
            offs.push((out.len() - rec) as u32);
            out.extend_from_slice(f.as_bytes());
            out.push(0);
        }
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let rec_len = (out.len() - rec) as u32;
        out[rec..rec + 4].copy_from_slice(&(str_off as u32).to_le_bytes());
        out[rec + 4..rec + 8].copy_from_slice(&(fields.len() as u32).to_le_bytes());
        out[rec + 8..rec + 12].copy_from_slice(&rec_len.to_le_bytes());
        for (i, o) in offs.iter().enumerate() {
            out[rec + 12 + 4 * i..rec + 16 + 4 * i].copy_from_slice(&o.to_le_bytes());
        }
    }
    let n = rows.len() as u32;
    out[8..12].copy_from_slice(&n.to_le_bytes());
    let total = out.len() as u32;
    out[12..16].copy_from_slice(&total.to_le_bytes());
    out
}

#[test]
fn rlist_parse_preserves_duplicates_and_order() {
    let img = build_rlist(&[
        ("boom00", &["000000", "000000", "bg:-2", "stage:-1"]),
        ("dummy00", &[]),
        ("boom00", &["000000", "000000", "bg:-2", "footpanel"]),
    ]);
    let rows = rlist::parse(&img).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, "boom00");
    assert_eq!(rows[0].1, vec!["000000", "000000", "bg:-2", "stage:-1"]);
    assert!(rows[1].1.is_empty());
    assert_eq!(rows[2].1[3], "footpanel");
    assert_eq!(rlist::find(&rows, "boom00").map(|(i, _)| i), Some(0));
    assert_eq!(rlist::find(&rows, "zzz"), None);
    let mut bad = img.clone();
    bad.push(0);
    assert_eq!(rlist::parse(&bad), Err(super::FormatError::SizeMismatch));
    bad[4] = b'B';
    assert_eq!(rlist::parse(&bad), Err(super::FormatError::BadMagic));
}

fn build_ktmdl(sk: &Skeleton) -> Vec<u8> {
    let mut out = vec![0u8; 0xC0];
    out[0..8].copy_from_slice(b"KTMDL\0\0\0");
    out[8..12].copy_from_slice(&2u32.to_le_bytes());
    out[12..16].copy_from_slice(&2u32.to_le_bytes());
    out[0x10..0x12].copy_from_slice(&1u16.to_le_bytes());
    let n = sk.bone_count();
    out[0x18..0x1C].copy_from_slice(&(n as u32).to_le_bytes());
    out[0x1C..0x20].copy_from_slice(&0xC0u32.to_le_bytes());
    for i in 0..n {
        let b = out.len();
        out.resize(b + 0xB0, 0);
        out[b + 0x10..b + 0x50].copy_from_slice(&f32s(&sk.bind_world[i]));
        out[b + 0x50..b + 0x90].copy_from_slice(&f32s(&sk.inverse_bind[i]));
        out[b + 0xAC..b + 0xAE].copy_from_slice(&sk.parents[i].to_le_bytes());
        out[b + 0xAE..b + 0xB0]
            .copy_from_slice(&(if sk.parents[i] < 0 { 0xFFFFu16 } else { 0 }).to_le_bytes());
    }
    out
}

#[test]
fn ktmdl_bone_table_round_trip() {
    let sk = synthetic_rig();
    let img = build_ktmdl(&sk);
    let got = ktmdl::bone_table(&img).unwrap();
    assert_eq!(got, sk);
    assert_eq!(
        ktmdl::bone_table(&img[..0x100]),
        Err(super::FormatError::Truncated)
    );
    let mut bad = img.clone();
    bad[8] = 3;
    assert_eq!(ktmdl::bone_table(&bad), Err(super::FormatError::Malformed));
    assert_eq!(
        ktmdl::bone_table(b"KTMDX\0\0\0"),
        Err(super::FormatError::BadMagic)
    );
}
