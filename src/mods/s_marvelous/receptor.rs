//! Violet receptor hit flash for S-Marvelous — hue AND size tier.
//!
//! The white flash a Marvelous paints on the receptor is the BM2D
//! `dance_effect` clip (one per panel, `NoteResultActor+0xE8..+0xF0`): the
//! stock grade handler (`FUN_18007B300` case 0x1028, RE §3.2) plays it at
//! `in_marvelous` — a GREYSCALE `dance_effect_bomb` texture drawn
//! ADDITIVELY, 0.80→1.15 over 8 frames. Perfect plays the same bomb at
//! `in_perfect`, 0.40→0.80. It is the ONLY receptor feedback a Marvelous
//! gets: `GamePlayActor::judgeNotes` pushes a `screen::JudgeEffectRenderer`
//! burst record (the per-type coloured sprite — yellow/green/blue) for
//! grades 1..=3 ONLY (20260825 `0x18005F560`, byte-identical on 20250805).
//!
//! Two mechanisms, both post-original in the judge tap:
//!
//! * **Hue** — a multiplicative CXFORM on the hit panel's clip LAYER
//!   (`afp_layer_set_color`, libafp Ordinal 49 — composed down the display
//!   hierarchy) turns the greyscale additive bomb into a single-hue violet
//!   glow. The layer colour block PERSISTS, and the same clip also plays
//!   Perfect's bomb and the YELLOW freeze bomb (`in_marvelous_freeze`, msg
//!   0x1032 at hold completion — dispatched in the same `judgeNotes` call,
//!   BEFORE the grade-6 O.K. submit for the same lanes). So every judgement
//!   event RE-ASSERTS the tint of every lane it touched: violet iff THIS
//!   event classified S-Marvelous, else identity. The O.K. event's identity
//!   write lands before the frame renders, so the yellow freeze bomb is
//!   never tinted. Writes are elided when the lane's tracked tint already
//!   matches (one relaxed load per lane per event).
//! * **Size tier** — the `dance_effect` template is patched at load
//!   (`receptor_patch`): stock `in_marvelous` is cloned to `in_smarvelous`,
//!   then `in_marvelous` shrinks to Perfect's old ramp and `in_perfect`
//!   halves. The stock handler already played + seeked the hit lanes' clips
//!   to (the now-smaller) `in_marvelous`; an S-Marv event RE-SEEKS them to
//!   `in_smarvelous` with the stock handler's own shape — `0x1012` label
//!   lookup + `0xF08` SetFrame at `label_frame + info.frame_offset`. Loose
//!   Marvelous / Perfect need nothing: the patch already resized their
//!   segments.
//!
//! Fail-open: `afp_layer_set_color` unresolved, no NoteResultActor, a bad
//! vector, or a null clip ⇒ stock flash, one WARN per class; an unpatched
//! template ⇒ no re-seek (S-Marv shows the stock-size Marvelous flash in
//! violet). Game-thread-only (inside the judge_submit dispatch).

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::log_warn;
use crate::services::bm2d_api;

use super::receptor_patch;

/// NoteResultActor field offsets (`playfield_styling::lane_hook` — from the
/// setup decompile; the handler indexes the vector by lane bit).
const OFF_NRA_FLASH_BEGIN: usize = 0xE8;
const OFF_NRA_FLASH_END: usize = 0xF0;
/// Pool-wrapper slots: the clip's type-1 AFP layer id / type-4 MC id (the
/// stock handler drives `Ordinal_115/114` on `+0x110` — RE §3.2).
const OFF_WRAPPER_LAYER_ID: usize = 0x08;
const OFF_WRAPPER_MC_ID: usize = 0x110;
/// `judge_submit` info struct: `+0x00 side, +0x04 delta ms, +0x08 lane
/// bitset (bit i = panel i, doubles 0..7), +0x10 note*, +0x18 frame offset`
/// (`judgeNotes` builds it; the 0x1028 handler reads `param_3[2]` as the
/// per-panel mask and adds `param_3[6]` to the label frame — RE §1.2/§3.2).
const INFO_LANES_OFFSET: usize = 0x08;
const INFO_FRAME_OFFSET: usize = 0x18;
/// Maximum panels per side (doubles).
const MAX_LANES: usize = 8;

/// The synthesized top-tier segment (must match `receptor_patch::NEW_LABEL`).
const LABEL: &std::ffi::CStr = c"in_smarvelous";

/// The S-Marvelous violet as a CXFORM multiplier on the greyscale bomb.
///
/// NOT the pastel art-language hue (`0xB05CE0`, `combo::TINT_ROOT3`): the
/// clip is drawn ADDITIVELY, so the flash ADDS `texture × mult` onto
/// whatever is already on screen and the pastel's green channel (92/255)
/// pushed bright lane backgrounds straight to white (cabinet, 2026-09-10 —
/// "washed out"). A saturated electric violet — green near zero, blue at
/// full, red ~63 % — keeps the added light on the red/blue axis so the
/// glow reads violet instead of white. Darkening the multiplier would only
/// make the glow fainter; saturation is the lever under additive blend.
const VIOLET_RGB: u32 = 0xA030FF;

/// Tracked tint per (side, lane). 0 = unknown (fresh clips this song —
/// the first event writes unconditionally), 1 = identity, 2 = violet.
const TINT_UNKNOWN: u8 = 0;
const TINT_IDENTITY: u8 = 1;
const TINT_VIOLET: u8 = 2;

static TINT: [[AtomicU8; MAX_LANES]; 2] = [
    [const { AtomicU8::new(TINT_UNKNOWN) }; MAX_LANES],
    [const { AtomicU8::new(TINT_UNKNOWN) }; MAX_LANES],
];

/// One-shot WARN latches per failure class.
static WARNED_NO_COLOR_API: AtomicBool = AtomicBool::new(false);
static WARNED_NO_ACTOR: AtomicBool = AtomicBool::new(false);
static WARNED_BAD_VECTOR: AtomicBool = AtomicBool::new(false);
static WARNED_SET_FAILED: AtomicBool = AtomicBool::new(false);
static WARNED_SEEK_FAILED: AtomicBool = AtomicBool::new(false);
/// One-shot INFO on the first violet write / first re-seek of the session.
static FIRST_TINT_LOGGED: AtomicBool = AtomicBool::new(false);
static FIRST_SEEK_LOGGED: AtomicBool = AtomicBool::new(false);

/// Forget every tracked tint (play-scene entry: the NoteResultActor and its
/// clips are rebuilt per song, so the layers start at identity again and
/// the tracker must not elide the first violet write). Also clears the
/// WARN latches so a transient class can re-report on a later song.
pub fn reset_for_song() {
    for side in &TINT {
        for lane in side {
            lane.store(TINT_UNKNOWN, Ordering::Relaxed);
        }
    }
    WARNED_NO_COLOR_API.store(false, Ordering::Relaxed);
    WARNED_NO_ACTOR.store(false, Ordering::Relaxed);
    WARNED_BAD_VECTOR.store(false, Ordering::Relaxed);
    WARNED_SET_FAILED.store(false, Ordering::Relaxed);
    WARNED_SEEK_FAILED.store(false, Ordering::Relaxed);
}

/// Pure: the lanes the event touched, as a bitmask restricted to
/// `count` panels (bits at or above the clip count are dropped — the
/// vector is the authority on how many panels exist).
pub fn lane_mask(info_lanes: u32, count: usize) -> u32 {
    let count = count.min(MAX_LANES);
    if count == 0 {
        return 0;
    }
    info_lanes & ((1u32 << count) - 1)
}

/// Pure: the CXFORM multiplier for a tint choice.
pub fn tint_rgba(smarv: bool) -> [f32; 4] {
    if smarv {
        [
            ((VIOLET_RGB >> 16) & 0xFF) as f32 / 255.0,
            ((VIOLET_RGB >> 8) & 0xFF) as f32 / 255.0,
            (VIOLET_RGB & 0xFF) as f32 / 255.0,
            1.0,
        ]
    } else {
        [1.0, 1.0, 1.0, 1.0]
    }
}

/// Pure: the frame the S-Marv re-seek lands on — the label frame plus the
/// stock handler's per-event frame offset (`info+0x18`, `param_3[6]`), the
/// same sum `CMovieClip::SetFrame` receives for `in_marvelous`. `None` on
/// overflow or a negative result (a bad offset read).
pub fn seek_frame(label_frame: u32, frame_offset: i32) -> Option<i32> {
    let f = i32::try_from(label_frame).ok()?.checked_add(frame_offset)?;
    (f >= 0).then_some(f)
}

/// Post-original, for EVERY grade event (0..=6) of an armed side: re-assert
/// the tint of the lanes this event touched — violet iff `smarv` — and, on
/// an S-Marvelous event with the template patched, re-seek those lanes'
/// clips to the top-tier `in_smarvelous` segment.
///
/// * `nra` — the side's NoteResultActor (resolved by the flash re-drive);
///   `None` ⇒ one WARN, stock flash.
/// * `info` — the judge_submit info struct (lane bitset at +0x08, frame
///   offset at +0x18); null ⇒ nothing to do (the stock handler played
///   nothing either).
pub fn on_judge_event(side: usize, nra: Option<*mut u8>, info: *const u8, smarv: bool) {
    if info.is_null() {
        return;
    }
    if !bm2d_api::layer_color_available() {
        if smarv && !WARNED_NO_COLOR_API.swap(true, Ordering::Relaxed) {
            log_warn!(
                "SMarvelous: receptor flash -- afp_layer_set_color unresolved; stock white flash"
            );
        }
        return;
    }
    let Some(nra) = nra else {
        if smarv && !WARNED_NO_ACTOR.swap(true, Ordering::Relaxed) {
            log_warn!(
                "SMarvelous: receptor flash -- no NoteResultActor (side {}); stock white flash",
                side
            );
        }
        return;
    };

    // SAFETY: game thread, inside the judge_submit dispatch where the actor
    // tree and its clips are live; every pointer is null/alignment-checked
    // before the dereference (the same walk `lane_hook` runs per song).
    unsafe {
        let begin = (nra.add(OFF_NRA_FLASH_BEGIN) as *const *const *const u8).read_unaligned();
        let end = (nra.add(OFF_NRA_FLASH_END) as *const *const *const u8).read_unaligned();
        let (b, e) = (begin as usize, end as usize);
        if begin.is_null() || end.is_null() || e <= b || b % 8 != 0 || e % 8 != 0 {
            if smarv && !WARNED_BAD_VECTOR.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "SMarvelous: receptor flash -- flash clip vector unreadable (side {}); stock white flash",
                    side
                );
            }
            return;
        }
        let count = (e - b) / 8;
        if count == 0 || count > MAX_LANES {
            if smarv && !WARNED_BAD_VECTOR.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "SMarvelous: receptor flash -- flash clip count {} out of range (side {}); stock white flash",
                    count,
                    side
                );
            }
            return;
        }

        let lanes = lane_mask(
            (info.add(INFO_LANES_OFFSET) as *const u32).read_unaligned(),
            count,
        );
        if lanes == 0 {
            return;
        }
        let want = if smarv { TINT_VIOLET } else { TINT_IDENTITY };
        let [r, g, b_, a] = tint_rgba(smarv);
        let tracker = &TINT[side & 1];
        // Size tier: only an S-Marv event on a patched template re-seeks.
        let reseek = smarv && receptor_patch::patch_applied();
        let frame_offset = (info.add(INFO_FRAME_OFFSET) as *const i32).read_unaligned();

        for lane in 0..count {
            if lanes & (1u32 << lane) == 0 {
                continue;
            }
            let need_tint = tracker[lane].load(Ordering::Relaxed) != want;
            if !need_tint && !reseek {
                continue;
            }
            let clip = begin.add(lane).read();
            if clip.is_null() || (clip as usize) % 8 != 0 {
                continue;
            }
            if need_tint {
                let layer_id = (clip.add(OFF_WRAPPER_LAYER_ID) as *const u32).read_unaligned();
                if layer_id != 0 {
                    if bm2d_api::layer_set_color_raw(layer_id, r, g, b_, a) {
                        tracker[lane].store(want, Ordering::Relaxed);
                        if smarv && !FIRST_TINT_LOGGED.swap(true, Ordering::Relaxed) {
                            crate::log_info!(
                                "SMarvelous: receptor flash live -- first violet tint (side {}, lane {}, layer {})",
                                side,
                                lane,
                                layer_id
                            );
                        }
                    } else if smarv && !WARNED_SET_FAILED.swap(true, Ordering::Relaxed) {
                        // Tracker keeps its old value so the next event
                        // retries; the clip shows stock this event.
                        log_warn!(
                            "SMarvelous: receptor flash -- afp_layer_set_color refused (layer {}); stock white flash",
                            layer_id
                        );
                    }
                }
            }
            if reseek {
                reseek_lane(side, lane, clip, frame_offset);
            }
        }
    }
}

/// Re-seek one lane's clip to `in_smarvelous` — the stock handler's own
/// shape (`Ordinal_115(mc, 0x1012, label, &frame)` then
/// `CMovieClip::SetFrame(frame + info.frame_offset)` → `afp_mc_op(mc,
/// 0xF08, frame)`), issued AFTER the stock seek to the (now-smaller)
/// `in_marvelous` so ours is the last write this event. Play + visibility
/// were already set by the stock handler. Failures fail open to the
/// Marvelous-size flash with one WARN.
unsafe fn reseek_lane(side: usize, lane: usize, clip: *const u8, frame_offset: i32) {
    let mc_id = (clip.add(OFF_WRAPPER_MC_ID) as *const u32).read_unaligned();
    if mc_id < 1 {
        return;
    }
    let Some(label_frame) = bm2d_api::mc_frame_by_label(mc_id, LABEL) else {
        if !WARNED_SEEK_FAILED.swap(true, Ordering::Relaxed) {
            log_warn!(
                "SMarvelous: receptor flash -- in_smarvelous label missing on clip mc {} (side {}); Marvelous-size flash",
                mc_id,
                side
            );
        }
        return;
    };
    let Some(frame) = seek_frame(label_frame, frame_offset) else {
        return;
    };
    if bm2d_api::mc_op(mc_id, 0xF08, frame) {
        if !FIRST_SEEK_LOGGED.swap(true, Ordering::Relaxed) {
            crate::log_info!(
                "SMarvelous: receptor flash -- first in_smarvelous re-seek (side {}, lane {}, mc {}, frame {} = label {} + offset {})",
                side,
                lane,
                mc_id,
                frame,
                label_frame,
                frame_offset
            );
        }
    } else if !WARNED_SEEK_FAILED.swap(true, Ordering::Relaxed) {
        log_warn!(
            "SMarvelous: receptor flash -- mc_op(0xF08) refused on mc {} (side {}); Marvelous-size flash",
            mc_id,
            side
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_mask_clips_to_panel_count() {
        assert_eq!(lane_mask(0xFF, 4), 0x0F);
        assert_eq!(lane_mask(0xFF, 8), 0xFF);
        assert_eq!(lane_mask(0x1_00, 8), 0); // bit 8 never maps to a panel
        assert_eq!(lane_mask(0x05, 4), 0x05);
        assert_eq!(lane_mask(0x05, 0), 0);
        assert_eq!(lane_mask(0xFFFF_FFFF, 16), 0xFF); // capped at MAX_LANES
    }

    #[test]
    fn tint_values() {
        assert_eq!(tint_rgba(false), [1.0, 1.0, 1.0, 1.0]);
        let v = tint_rgba(true);
        let ch = |shift: u32| ((VIOLET_RGB >> shift) & 0xFF) as f32 / 255.0;
        assert!((v[0] - ch(16)).abs() < 1e-6);
        assert!((v[1] - ch(8)).abs() < 1e-6);
        assert!((v[2] - ch(0)).abs() < 1e-6);
        assert_eq!(v[3], 1.0); // alpha untouched — the additive glow keeps its own ramp
                               // Additive-blend saturation contract: the added light must stay on
                               // the red/blue axis (low green) with blue dominant, or the glow
                               // washes to white on bright backgrounds.
        assert!(v[1] < 0.25, "green must stay low under additive blend");
        assert!(v[2] > v[0] && v[0] > v[1], "violet = blue > red > green");
    }

    #[test]
    fn seek_frame_mirrors_stock_sum() {
        assert_eq!(seek_frame(300, 0), Some(300));
        assert_eq!(seek_frame(300, 2), Some(302)); // late-detected hit
        assert_eq!(seek_frame(300, -1), Some(299));
        assert_eq!(seek_frame(0, -1), None); // negative frame = bad read
        assert_eq!(seek_frame(u32::MAX, 0), None); // overflow
        assert_eq!(seek_frame(i32::MAX as u32, 1), None);
    }
}
