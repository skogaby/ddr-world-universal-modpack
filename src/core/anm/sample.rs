//! Key decoding and game-equivalent track sampling —
//! `scripts/anm_dump.py::{decode_q48, encode_q48, half_to_float, decode_key,
//! sample_track, _slerp}` (format record §5.1/§5.2; evaluator `FUN_18013ab80`,
//! q48 decoder `FUN_180138b20`, slerp `FUN_180190880`).

use super::anm::{Channel, Track};
use super::{Le, Quat, Vec3};

/// `16383.5` — `DAT_180288bcc`.
pub const Q15_OFFSET: f32 = 16383.5;
/// `16383.5 · √2` — `DAT_180288bc8`.
pub const Q15_SCALE: f32 = 23169.767_578_125;

/// A decoded key / interpolated sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sample {
    Quat(Quat),
    Vec3(Vec3),
    Scalar(f32),
}

impl Sample {
    pub fn quat(self) -> Option<Quat> {
        match self {
            Sample::Quat(q) => Some(q),
            _ => None,
        }
    }
    pub fn vec3(self) -> Option<Vec3> {
        match self {
            Sample::Vec3(v) => Some(v),
            _ => None,
        }
    }
    pub fn scalar(self) -> Option<f32> {
        match self {
            Sample::Scalar(s) => Some(s),
            _ => None,
        }
    }
}

/// 48-bit smallest-three quaternion → `(x, y, z, w)` (§5.2).
pub fn decode_q48(b: [u8; 6]) -> Quat {
    let v = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], 0, 0]);
    let a = ((v >> 32) & 0x7FFF) as f32;
    let bq = ((v >> 17) & 0x7FFF) as f32;
    let c = ((v >> 2) & 0x7FFF) as f32;
    let m = (v & 3) as u8;
    let f = |x: f32| (x - Q15_OFFSET) / Q15_SCALE;
    let (aa, bb, cc) = (f(a), f(bq), f(c));
    let d = (1.0 - (aa * aa + bb * bb + cc * cc)).max(0.0).sqrt();
    match m {
        0 => [d, aa, bb, cc],
        1 => [aa, d, bb, cc],
        2 => [aa, bb, d, cc],
        _ => [aa, bb, cc, d],
    }
}

/// Inverse of [`decode_q48`] (`anm_dump.py::encode_q48`): the largest-
/// magnitude component is dropped (sign-flipped positive), the other three
/// quantised with `round(c·Q15_SCALE + Q15_OFFSET)`. Test/exporter helper.
pub fn encode_q48(q: &Quat) -> [u8; 6] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let n = if n == 0.0 { 1.0 } else { n };
    let mut qn = [q[0] / n, q[1] / n, q[2] / n, q[3] / n];
    let mut m = 0usize;
    for i in 1..4 {
        if qn[i].abs() > qn[m].abs() {
            m = i;
        }
    }
    if qn[m] < 0.0 {
        for c in qn.iter_mut() {
            *c = -*c;
        }
    }
    let mut rest = [0.0f32; 3];
    let mut k = 0;
    for (i, c) in qn.iter().enumerate() {
        if i != m {
            rest[k] = *c;
            k += 1;
        }
    }
    let quant = |c: f32| -> u64 {
        let v = (c * Q15_SCALE + Q15_OFFSET).round();
        v.clamp(0.0, 0x7FFF as f32) as u64
    };
    let v = (quant(rest[0]) << 32) | (quant(rest[1]) << 17) | (quant(rest[2]) << 2) | m as u64;
    let b = v.to_le_bytes();
    [b[0], b[1], b[2], b[3], b[4], b[5]]
}

/// IEEE binary16 → f32, the Python `half_to_float` arithmetic (denormals via
/// `m/1024·2^-14`, exponent 31 ⇒ ±inf regardless of the mantissa).
pub fn half_to_float(h: u16) -> f32 {
    let s = (h >> 15) & 1;
    let e = ((h >> 10) & 0x1F) as i32;
    let m = (h & 0x3FF) as f32;
    let v = if e == 0 {
        (m / 1024.0) * 2f32.powi(-14)
    } else if e == 31 {
        f32::INFINITY
    } else {
        (1.0 + m / 1024.0) * 2f32.powi(e - 15)
    };
    if s != 0 {
        -v
    } else {
        v
    }
}

/// Decode key `i` of a track — `anm_dump.py::decode_key` for the runtime
/// kinds. `None` when the key would fall outside the track's value range
/// (a parser bug or a corrupt file; never for a stock file).
pub fn decode_key(bytes: &[u8], t: &Track, i: usize) -> Option<Sample> {
    let r = Le(bytes);
    let v = t.values.start;
    let end = t.values.end;
    let chk = |o: usize, len: usize| -> Option<usize> {
        let e = o.checked_add(len)?;
        if e <= end {
            Some(o)
        } else {
            None
        }
    };
    Some(match t.kind {
        1 => {
            let o = chk(v + 16 * i, 16)?;
            Sample::Quat(r.f32s::<4>(o)?)
        }
        4 | 10 => {
            let o = chk(v + 16 * i, 12)?;
            Sample::Vec3(r.f32s::<3>(o)?)
        }
        8 | 0x1B => {
            let o = chk(v + 4 * i, 4)?;
            Sample::Scalar(r.f32(o)?)
        }
        0x1C => {
            let o = chk(v + 6 * i, 6)?;
            let b = bytes.get(o..o + 6)?;
            Sample::Quat(decode_q48([b[0], b[1], b[2], b[3], b[4], b[5]]))
        }
        0x1D => {
            let o = chk(v + 12 * i, 12)?;
            Sample::Vec3(r.f32s::<3>(o)?)
        }
        0x1E => {
            let o = chk(v + 6 * i, 6)?;
            Sample::Vec3([
                half_to_float(r.u16(o)?),
                half_to_float(r.u16(o + 2)?),
                half_to_float(r.u16(o + 4)?),
            ])
        }
        0x1F => {
            let base = r.f32s::<3>(chk(v, 12)?)?;
            let o = chk(v + 12 + 6 * i, 6)?;
            Sample::Vec3([
                base[0] + half_to_float(r.u16(o)?),
                base[1] + half_to_float(r.u16(o + 2)?),
                base[2] + half_to_float(r.u16(o + 4)?),
            ])
        }
        _ => return None,
    })
}

/// Kinds the game decodes by STEP (key `i` while `u < 1`, else key `i+1`).
#[inline]
pub fn is_step_kind(kind: u16) -> bool {
    matches!(kind, 0x1B | 0x20)
}

/// Shortest-path slerp (`FUN_180190880` / `anm_dump.py::_slerp`).
pub fn slerp(a: &Quat, b: &Quat, t: f32) -> Quat {
    let mut b = *b;
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        for c in b.iter_mut() {
            *c = -*c;
        }
        dot = -dot;
    }
    if 1.0 - dot <= 1e-5 {
        return [
            (1.0 - t) * a[0] + t * b[0],
            (1.0 - t) * a[1] + t * b[1],
            (1.0 - t) * a[2] + t * b[2],
            (1.0 - t) * a[3] + t * b[3],
        ];
    }
    let th = dot.min(1.0).acos();
    let s = th.sin();
    let wa = ((1.0 - t) * th).sin() / s;
    let wb = (t * th).sin() / s;
    [
        wa * a[0] + wb * b[0],
        wa * a[1] + wb * b[1],
        wa * a[2] + wb * b[2],
        wa * a[3] + wb * b[3],
    ]
}

fn lerp_sample(a: Sample, b: Sample, u: f32) -> Sample {
    match (a, b) {
        (Sample::Quat(x), Sample::Quat(y)) => Sample::Quat([
            (1.0 - u) * x[0] + u * y[0],
            (1.0 - u) * x[1] + u * y[1],
            (1.0 - u) * x[2] + u * y[2],
            (1.0 - u) * x[3] + u * y[3],
        ]),
        (Sample::Vec3(x), Sample::Vec3(y)) => Sample::Vec3([
            (1.0 - u) * x[0] + u * y[0],
            (1.0 - u) * x[1] + u * y[1],
            (1.0 - u) * x[2] + u * y[2],
        ]),
        (Sample::Scalar(x), Sample::Scalar(y)) => Sample::Scalar((1.0 - u) * x + u * y),
        (a, _) => a,
    }
}

/// Python `int(frame)` for non-negative frames (truncation), with negatives
/// clamped to 0 — callers pass `clip_time` output, which is never negative.
#[inline]
fn floor_frame(frame: f32) -> u32 {
    if frame <= 0.0 || frame.is_nan() {
        0
    } else if frame >= u32::MAX as f32 {
        u32::MAX
    } else {
        frame as u32
    }
}

/// Game-equivalent sampling of a track at a fractional frame
/// (`anm_dump.py::sample_track`): uniform keys clamp at the last key;
/// explicit times pick `i = max k with times[k] <= floor(frame)`, skip
/// duplicate times for `i1`, and clamp once `floor(frame) >= times[n-1]`.
/// Rotations slerp, step kinds hold, everything else lerps. `None` only on a
/// corrupt value block or a zero-key track.
pub fn sample(bytes: &[u8], t: &Track, frame: f32) -> Option<Sample> {
    let n = t.key_count as usize;
    if n == 0 {
        return None;
    }
    let (i, i1, u) = match &t.times {
        None => {
            let i = floor_frame(frame) as usize;
            if n == 1 || i >= n - 1 {
                return decode_key(bytes, t, n - 1);
            }
            (i, i + 1, frame - i as f32)
        }
        Some(times) => {
            let fi = floor_frame(frame);
            let last = *times.get(n - 1)?;
            if fi >= last as u32 {
                return decode_key(bytes, t, n - 1);
            }
            // Largest k with times[k] <= fi (binary search — the game's own
            // evaluator does the same; key times are ascending in every
            // stock file). A frame before the first key (never in stock
            // data — keys start at 0) uses key 0.
            let i = times
                .partition_point(|&tk| tk as u32 <= fi)
                .saturating_sub(1);
            let ti = times[i];
            let mut i1 = i + 1;
            while i1 < n - 1 && times[i1] == ti {
                i1 += 1;
            }
            if i1 >= n {
                return decode_key(bytes, t, i);
            }
            let span = times[i1] as f32 - ti as f32;
            if span == 0.0 {
                return decode_key(bytes, t, i);
            }
            (i, i1, (frame - ti as f32) / span)
        }
    };
    let a = decode_key(bytes, t, i)?;
    let b = decode_key(bytes, t, i1)?;
    if is_step_kind(t.kind) {
        return Some(if u < 1.0 { a } else { b });
    }
    if t.channel == Channel::Rotation || t.channel == Channel::CamQuat {
        if let (Sample::Quat(qa), Sample::Quat(qb)) = (a, b) {
            return Some(Sample::Quat(slerp(&qa, &qb, u)));
        }
    }
    Some(lerp_sample(a, b, u))
}

/// Map a content time onto a clip: looping clips wrap (`t mod dur`, `+dur`
/// when negative); one-shot clips clamp to `[0, dur]` and report `finished`
/// at `dur`. A zero/negative duration is `(0.0, true)`.
pub fn clip_time(t: f32, dur: f32, loops: bool) -> (f32, bool) {
    if !(dur > 0.0) {
        return (0.0, true);
    }
    if loops {
        let mut r = t % dur;
        if r < 0.0 {
            r += dur;
        }
        (r, false)
    } else if t <= 0.0 {
        (0.0, false)
    } else if t >= dur {
        (dur, true)
    } else {
        (t, false)
    }
}
