//! The seqlocked per-instance pose channel between the game thread (the
//! Background Dancers director) and the engine's scene-graph update job
//! (our node's `visit(2)`), design §3.3 / §4.4 (NFR-5).
//!
//! One statically allocated board of [`MAX_INSTANCES`] slots. Every payload
//! word is an `AtomicU32` so the lock-free read is defined behaviour; the
//! `seq` word is the seqlock (odd = a write is in progress, 0 = never
//! published). Writers: game thread only, one `publish` per instance per
//! frame. Readers: the job thread, one bounded `read_slot_into` per node per
//! frame — no locks, no allocation, no logging, never spins unbounded.
//!
//! The board is deliberately std-only (no `crate::`): the host harness
//! mounts it. Copying the payload INTO a render item happens in `node.rs`
//! (the raw item setters live there); this module only moves numbers.

use std::sync::atomic::{fence, AtomicU32, Ordering};

/// Slots on the board (a full Step 8 scene is ≤ 8 stage parts + 2 dancers
/// + 10 parts + 2 shadows = 22).
pub const MAX_INSTANCES: usize = 32;
/// Bone matrices per slot (stock maximum: 33 dancers, 43 stage props).
pub const MAX_BONES: usize = 64;
/// `SceneNode.instance` value meaning "no board slot" (static item).
pub const NO_SLOT: u32 = u32::MAX;
/// Bounded seqlock retries before a reader gives up for this frame.
const MAX_READ_RETRIES: usize = 8;

pub type Mat4 = [f32; 16];

struct Slot {
    seq: AtomicU32,
    hidden: AtomicU32,
    bone_count: AtomicU32,
    world: [AtomicU32; 16],
    tint: [AtomicU32; 4],
    bones: [AtomicU32; MAX_BONES * 16],
}

const ZERO: AtomicU32 = AtomicU32::new(0);
const EMPTY_SLOT: Slot = Slot {
    seq: ZERO,
    hidden: ZERO,
    bone_count: ZERO,
    world: [ZERO; 16],
    tint: [ZERO; 4],
    bones: [ZERO; MAX_BONES * 16],
};

static BOARD: [Slot; MAX_INSTANCES] = [EMPTY_SLOT; MAX_INSTANCES];

/// A consistent snapshot of one slot (stack-sized: ~4 KB).
pub struct SlotRead {
    pub world: Mat4,
    pub tint: [f32; 4],
    pub hidden: bool,
    pub bone_count: usize,
    pub bones: [Mat4; MAX_BONES],
}

impl SlotRead {
    pub const fn zeroed() -> SlotRead {
        SlotRead {
            world: [0.0; 16],
            tint: [0.0; 4],
            hidden: true,
            bone_count: 0,
            bones: [[0.0; 16]; MAX_BONES],
        }
    }
}

#[inline]
fn slot(i: u32) -> Option<&'static Slot> {
    BOARD.get(i as usize)
}

/// Game thread: publish an instance's frame. `bones` beyond [`MAX_BONES`]
/// are truncated. Out-of-range slots are ignored.
pub fn publish(slot_idx: u32, world: &Mat4, tint: [f32; 4], hidden: bool, bones: &[Mat4]) {
    let Some(s) = slot(slot_idx) else { return };
    let n = bones.len().min(MAX_BONES);
    // Enter the write: seq becomes odd.
    let start = s.seq.load(Ordering::Relaxed);
    let odd = if start & 1 == 1 {
        start
    } else {
        start.wrapping_add(1)
    };
    s.seq.store(odd, Ordering::Release);
    fence(Ordering::Release);
    s.hidden.store(hidden as u32, Ordering::Relaxed);
    s.bone_count.store(n as u32, Ordering::Relaxed);
    for (dst, v) in s.world.iter().zip(world.iter()) {
        dst.store(v.to_bits(), Ordering::Relaxed);
    }
    for (dst, v) in s.tint.iter().zip(tint.iter()) {
        dst.store(v.to_bits(), Ordering::Relaxed);
    }
    for (b, m) in bones.iter().take(n).enumerate() {
        let base = b * 16;
        for (k, v) in m.iter().enumerate() {
            s.bones[base + k].store(v.to_bits(), Ordering::Relaxed);
        }
    }
    // Leave the write: seq becomes even and ≥ 2 (never 0 again).
    let mut even = odd.wrapping_add(1);
    if even == 0 {
        even = 2;
    }
    s.seq.store(even, Ordering::Release);
}

/// Game thread: mark a slot "never published" (readers leave the item as
/// built). Used at teardown so a recycled slot cannot serve stale poses.
pub fn clear(slot_idx: u32) {
    if let Some(s) = slot(slot_idx) {
        s.seq.store(0, Ordering::Release);
    }
}

pub fn clear_all() {
    for i in 0..MAX_INSTANCES as u32 {
        clear(i);
    }
}

/// Whether a slot has ever been published (and is not mid-write).
pub fn is_published(slot_idx: u32) -> bool {
    slot(slot_idx).map_or(false, |s| {
        let v = s.seq.load(Ordering::Acquire);
        v != 0 && v & 1 == 0
    })
}

/// Job thread, zero stack footprint: copy a consistent snapshot STRAIGHT into
/// the caller's destinations — `world` (16 f32), `tint` (4 f32) and up to
/// `bones_cap` bone matrices at `bones` (16 f32 each, contiguous). Returns
/// `Some((hidden, bone_count_written))` when the copy was consistent; `None`
/// when nothing was published / the slot is out of range (destinations
/// untouched) or when every retry was torn (destinations then hold a MIX of
/// two frames' floats — harmless for a pose, never a pointer).
///
/// This is the `visit(2)` path: the 4 KB [`SlotRead`] buffer stays off the
/// engine worker's stack (whose size is not ours to know).
///
/// # Safety
/// The destinations are writable for the stated lengths.
pub unsafe fn read_slot_into_raw(
    slot_idx: u32,
    world: *mut f32,
    tint: *mut f32,
    bones: *mut f32,
    bones_cap: usize,
) -> Option<(bool, usize)> {
    let s = slot(slot_idx)?;
    let cap = bones_cap.min(MAX_BONES);
    for _ in 0..MAX_READ_RETRIES {
        let s1 = s.seq.load(Ordering::Acquire);
        if s1 == 0 {
            return None;
        }
        if s1 & 1 == 1 {
            std::hint::spin_loop();
            continue;
        }
        let hidden = s.hidden.load(Ordering::Relaxed) != 0;
        let n = (s.bone_count.load(Ordering::Relaxed) as usize).min(cap);
        for (k, src) in s.world.iter().enumerate() {
            *world.add(k) = f32::from_bits(src.load(Ordering::Relaxed));
        }
        for (k, src) in s.tint.iter().enumerate() {
            *tint.add(k) = f32::from_bits(src.load(Ordering::Relaxed));
        }
        for b in 0..n {
            let base = b * 16;
            for k in 0..16 {
                *bones.add(base + k) = f32::from_bits(s.bones[base + k].load(Ordering::Relaxed));
            }
        }
        fence(Ordering::Acquire);
        let s2 = s.seq.load(Ordering::Acquire);
        if s1 == s2 {
            return Some((hidden, n));
        }
    }
    None
}

/// Copy a consistent snapshot into `out` (tests / non-hot paths). `false` =
/// nothing published, a write was in progress for all retries, or the slot
/// is out of range — `out` is then unspecified and must not be applied.
pub fn read_slot_into(slot_idx: u32, out: &mut SlotRead) -> bool {
    let Some(s) = slot(slot_idx) else {
        return false;
    };
    for _ in 0..MAX_READ_RETRIES {
        let s1 = s.seq.load(Ordering::Acquire);
        if s1 == 0 {
            return false;
        }
        if s1 & 1 == 1 {
            std::hint::spin_loop();
            continue;
        }
        let hidden = s.hidden.load(Ordering::Relaxed) != 0;
        let n = (s.bone_count.load(Ordering::Relaxed) as usize).min(MAX_BONES);
        for (dst, src) in out.world.iter_mut().zip(s.world.iter()) {
            *dst = f32::from_bits(src.load(Ordering::Relaxed));
        }
        for (dst, src) in out.tint.iter_mut().zip(s.tint.iter()) {
            *dst = f32::from_bits(src.load(Ordering::Relaxed));
        }
        for b in 0..n {
            let base = b * 16;
            for k in 0..16 {
                out.bones[b][k] = f32::from_bits(s.bones[base + k].load(Ordering::Relaxed));
            }
        }
        fence(Ordering::Acquire);
        let s2 = s.seq.load(Ordering::Acquire);
        if s1 == s2 {
            out.hidden = hidden;
            out.bone_count = n;
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mat(seed: f32) -> Mat4 {
        let mut m = [0.0f32; 16];
        for (i, v) in m.iter_mut().enumerate() {
            *v = seed + i as f32 * 0.5;
        }
        m
    }

    #[test]
    fn publish_read_round_trip_and_unpublished() {
        let mut out = SlotRead::zeroed();
        assert!(!read_slot_into(5, &mut out), "never published");
        assert!(!is_published(5));
        let bones: Vec<Mat4> = (0..33).map(|i| mat(i as f32 * 100.0)).collect();
        publish(5, &mat(1.0), [0.1, 0.2, 0.3, 0.4], false, &bones);
        assert!(is_published(5));
        assert!(read_slot_into(5, &mut out));
        assert_eq!(out.world, mat(1.0));
        assert_eq!(out.tint, [0.1, 0.2, 0.3, 0.4]);
        assert!(!out.hidden);
        assert_eq!(out.bone_count, 33);
        for (i, b) in bones.iter().enumerate() {
            assert_eq!(&out.bones[i], b);
        }
        // republish with hidden + fewer bones
        publish(5, &mat(2.0), [1.0; 4], true, &bones[..3]);
        assert!(read_slot_into(5, &mut out));
        assert!(out.hidden);
        assert_eq!(out.bone_count, 3);
        assert_eq!(out.world, mat(2.0));
        clear(5);
        assert!(!read_slot_into(5, &mut out));
        assert!(!is_published(5));
    }

    #[test]
    fn bone_truncation_and_bounds() {
        let bones: Vec<Mat4> = (0..MAX_BONES + 9).map(|i| mat(i as f32)).collect();
        publish(7, &mat(0.0), [1.0; 4], false, &bones);
        let mut out = SlotRead::zeroed();
        assert!(read_slot_into(7, &mut out));
        assert_eq!(out.bone_count, MAX_BONES);
        assert_eq!(out.bones[MAX_BONES - 1], mat((MAX_BONES - 1) as f32));
        // out of range: no-ops
        publish(MAX_INSTANCES as u32, &mat(0.0), [1.0; 4], false, &bones);
        assert!(!read_slot_into(MAX_INSTANCES as u32, &mut out));
        assert!(!read_slot_into(NO_SLOT, &mut out));
        clear(NO_SLOT);
        // (tests share the static board and run in parallel — never
        // `clear_all` here; each test owns its own slot index)
        clear(7);
        assert!(!is_published(7));
    }

    #[test]
    fn writer_in_progress_is_never_returned() {
        // Simulate a torn write: odd seq.
        let s = &BOARD[9];
        publish(9, &mat(3.0), [1.0; 4], false, &[mat(0.0)]);
        let even = s.seq.load(Ordering::Relaxed);
        s.seq.store(even | 1, Ordering::Release);
        let mut out = SlotRead::zeroed();
        assert!(!read_slot_into(9, &mut out));
        assert!(!is_published(9));
        s.seq.store(even, Ordering::Release);
        assert!(read_slot_into(9, &mut out));
        assert_eq!(out.world, mat(3.0));
    }

    #[test]
    fn raw_reader_matches_buffered_reader_and_clamps() {
        let bones: Vec<Mat4> = (0..40).map(|i| mat(i as f32 * 3.0)).collect();
        publish(13, &mat(7.0), [0.5, 0.6, 0.7, 0.8], false, &bones);
        let mut world = [0.0f32; 16];
        let mut tint = [0.0f32; 4];
        let mut dst = [[0.0f32; 16]; 33];
        // SAFETY: stack destinations of the stated lengths.
        let r = unsafe {
            read_slot_into_raw(
                13,
                world.as_mut_ptr(),
                tint.as_mut_ptr(),
                dst.as_mut_ptr() as *mut f32,
                33,
            )
        };
        assert_eq!(r, Some((false, 33)), "clamped to the destination capacity");
        assert_eq!(world, mat(7.0));
        assert_eq!(tint, [0.5, 0.6, 0.7, 0.8]);
        for (i, b) in bones.iter().take(33).enumerate() {
            assert_eq!(&dst[i], b);
        }
        let mut out = SlotRead::zeroed();
        assert!(read_slot_into(13, &mut out));
        assert_eq!(out.bone_count, 40);
        assert_eq!(out.bones[..33], dst[..]);
        // unpublished / out of range: None, destinations untouched
        let mut w2 = [9.0f32; 16];
        let r = unsafe {
            read_slot_into_raw(
                14,
                w2.as_mut_ptr(),
                tint.as_mut_ptr(),
                dst.as_mut_ptr() as *mut f32,
                33,
            )
        };
        assert_eq!(r, None);
        assert_eq!(w2, [9.0; 16]);
        let r = unsafe {
            read_slot_into_raw(
                NO_SLOT,
                w2.as_mut_ptr(),
                tint.as_mut_ptr(),
                dst.as_mut_ptr() as *mut f32,
                33,
            )
        };
        assert_eq!(r, None);
        // zero capacity: world/tint still copied, no bones
        let r = unsafe {
            read_slot_into_raw(
                13,
                w2.as_mut_ptr(),
                tint.as_mut_ptr(),
                dst.as_mut_ptr() as *mut f32,
                0,
            )
        };
        assert_eq!(r, Some((false, 0)));
        assert_eq!(w2, mat(7.0));
    }

    #[test]
    fn seq_never_returns_to_zero() {
        let s = &BOARD[11];
        s.seq.store(u32::MAX - 1, Ordering::Release);
        publish(11, &mat(0.0), [1.0; 4], false, &[]);
        let v = s.seq.load(Ordering::Relaxed);
        assert!(v != 0 && v & 1 == 0, "{v}");
        assert!(is_published(11));
        let mut out = SlotRead::zeroed();
        assert!(read_slot_into(11, &mut out));
        assert_eq!(out.bone_count, 0);
    }
}
