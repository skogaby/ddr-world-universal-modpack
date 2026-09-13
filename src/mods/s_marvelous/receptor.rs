//! Violet receptor BURST for S-Marvelous.
//!
//! Two separate systems paint the receptor on a judgement (RE addendum
//! 2026-09-10 in `docs/s_marvelous_judgement_research.md`):
//!
//! * the BM2D `dance_effect` clip (per panel, `NoteResultActor+0xE8..`) —
//!   the greyscale additive `ef_bomb` star, played at `in_marvelous` for
//!   Marvelous and `in_perfect` (half size) for Perfect. **Left stock**:
//!   S-Marvelous shows the exact white Marvelous bomb (maintainer
//!   directive 2026-09-12 — the earlier violet CXFORM tint and the size
//!   retier are retired).
//! * the `screen::JudgeEffectRenderer` burst (`GamePlayActor+0x150`) — the
//!   arrow-shaped flash that turns the receptor YELLOW on Perfect, GREEN on
//!   Great, BLUE on Good. A record is `{t0, lane_bits, type}` pushed by the
//!   game's own `JudgeEffectRenderer::push(this, u8 lanes, int type)`
//!   (`judge_effect_push`); `judgeNotes` pushes types 1/2/3 only, the
//!   freeze-hold tick pushes type 4. Marvelous gets no burst. The draw
//!   routine classifies the type twice: `0 / 5 / 6` = the 150 ms "flash"
//!   class (2.0 grow, colour doubled + saturated), `1..=6` = the per-type
//!   colour switch (yellow / green / blue). **A type ≥ 7 falls through
//!   BOTH**: 200 ms lifetime, 1.25 grow — Perfect's exact geometry and
//!   timing — with the colour left at its base `(f, f, f)`, a linear white
//!   fade. No stock code pushes or reads a type ≥ 7 on any supported build
//!   (the records are write-only from game logic; only the renderer's
//!   prune + emit read them, and both use the same classification).
//!
//! This module gives S-Marvelous the burst Marvelous never had, in violet,
//! and shaped exactly like Perfect's: on an S-Marv event (post-original in
//! the judge tap) it calls the game's pusher with the event's lane bits and
//! [`BURST_TYPE`] (7); those quads render WHITE `(f,f,f,0xFF)`, and the
//! shared `render_sprite_final` fill hook (`playfield_styling::fill_hook`,
//! refcounted consumer) hands every `JudgeEffectRenderer` quad's COLOR4B to
//! [`recolor_burst`], which maps white → `f × violet`. White is the
//! discriminator: no stock type yields `R == G == B` except the fully-faded
//! `(0,0,0)`, which maps to itself. Everything else the burst does — clock,
//! lifetime, expansion, rotation per lane, the additive draw under the
//! JUDGE shader — is the game's, identical to the Perfect burst.
//!
//! Fail-open: pusher / offset / vtable / fill hook unresolved ⇒ no burst
//! (stock Marvelous look), one WARN at enable; an unreadable renderer or a
//! vtable mismatch at event time ⇒ skip, one WARN per song. Game-thread-
//! only (inside the judge_submit dispatch — the same thread the stock
//! pushers run on).
//!
//! Operator choice: the "Receptor Flash Color" row ([`ReceptorFlash`]) —
//! PURPLE pushes the violet burst (above), WHITE pushes nothing so the
//! receptor is the stock Marvelous one (the white bomb alone). The choice
//! is a live flag read per event ([`set_flash_mode`]); the fill hook stays
//! acquired in both modes (its recolour only ever touches greyscale quads,
//! which nothing pushes in WHITE mode — a toggle never installs or removes
//! a detour).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

use super::receptor_color::{lane_bits, recolor, ReceptorFlash};

/// `judge_submit` info struct: `+0x08` lane bitset (bit i = panel i,
/// doubles 0..7) — `judgeNotes` builds it; the stock 0x1028 handler reads
/// `param_3[2]` as the per-panel mask (RE §1.2/§3.2).
const INFO_LANES_OFFSET: usize = 0x08;

/// The record type we push. Stock pushers use 1/2/3 (judgeNotes) and 4
/// (freeze hold). 7 is outside both of the draw routine's classifications
/// (flash class `0/5/6`, colour switch `1..=6`), so it renders with the
/// NON-flash geometry Perfect uses (200 ms, 1.25 grow) and the un-overridden
/// base colour `(f,f,f)` — the greyscale the fill recolours. Type 0 was the
/// first cut (2026-09-12): its 150 ms / 2.0-grow flash class read as too
/// large next to the stock bursts.
pub const BURST_TYPE: i32 = 7;

/// `JudgeEffectRenderer` bytes that must be readable before we hand the
/// pointer to the pusher: vtable @+0, clock @+0x94, records vector
/// @+0xA0..+0xB8 (begin/end/cap).
const RENDERER_MIN_READABLE: usize = 0xB8;

/// `void JudgeEffectRenderer::push(this, u8 lane_bits, int type)`.
type PushFn = unsafe extern "C" fn(this: *mut u8, lanes: u8, kind: i32);

/// Resolved at init (null / 0 = unavailable).
static PUSH_FN: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static FILL_FN: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static JUDGE_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static RENDERER_OFF: AtomicUsize = AtomicUsize::new(0);

/// Mod enabled AND the fill hook acquired — the push + recolour gate.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// The LIVE "Receptor Flash Color" choice as a `ReceptorFlash::index()`.
/// Written by [`set_flash_mode`] (mod enable + the overlay row), read by
/// [`on_smarvelous`] per event — an edit applies to the very next hit.
static FLASH_MODE: AtomicI32 = AtomicI32::new(0);

/// One-shot WARN latches per failure class (reset per song).
static WARNED_RENDERER: AtomicBool = AtomicBool::new(false);
static WARNED_VTABLE: AtomicBool = AtomicBool::new(false);
/// One-shot INFO on the first burst pushed this session.
static FIRST_PUSH_LOGGED: AtomicBool = AtomicBool::new(false);

/// Resolve the burst's four inputs. Returns whether the burst CAN work
/// (`activate` gates on it). Any miss ⇒ one WARN naming it.
pub fn init(sig: &SignatureStore) -> bool {
    let push = sig.get_address("judge_effect_push");
    let fill = sig.get_address("render_sprite_final");
    let vt = sig.get_address("judge_effect_renderer_vtable");
    let off = sig.gpa_judge_effect_off();
    let (Some(push), Some(fill), Some(vt), Some(off)) = (push, fill, vt, off) else {
        log_warn!(
            "SMarvelous: receptor burst unavailable (push={} fill={} vtable={} offset={}) -- S-Marv shows the stock Marvelous receptor",
            push.is_some(),
            fill.is_some(),
            vt.is_some(),
            off.is_some()
        );
        return false;
    };
    PUSH_FN.store(push as *mut u8, Ordering::Release);
    FILL_FN.store(fill as *mut u8, Ordering::Release);
    JUDGE_VTABLE.store(vt as *mut u8, Ordering::Release);
    RENDERER_OFF.store(off, Ordering::Release);
    log_info!(
        "SMarvelous: receptor burst ready (push @ {:p}, GamePlayActor renderer offset 0x{:X}, type {})",
        push,
        off,
        BURST_TYPE
    );
    true
}

fn available() -> bool {
    !PUSH_FN.load(Ordering::Acquire).is_null() && RENDERER_OFF.load(Ordering::Acquire) != 0
}

/// Mod enable: acquire the shared fill detour (the recolour half) and arm
/// the push. Returns false (nothing armed, one WARN) when the burst is
/// unavailable or the fill hook could not be installed — a white burst
/// without the recolour is not a degradation we want, so both halves arm
/// together or not at all.
pub fn activate() -> bool {
    if !available() {
        return false;
    }
    let fill = FILL_FN.load(Ordering::Acquire) as *const u8;
    let vt = JUDGE_VTABLE.load(Ordering::Acquire) as *const u8;
    if !crate::mods::playfield_styling::fill_acquire_smarvelous(fill, vt) {
        log_warn!(
            "SMarvelous: receptor burst -- shared fill hook unavailable; S-Marv shows the stock Marvelous receptor"
        );
        return false;
    }
    ACTIVE.store(true, Ordering::Release);
    true
}

/// Mod disable: disarm and drop the fill-hook interest.
pub fn deactivate() {
    if ACTIVE.swap(false, Ordering::AcqRel) {
        crate::mods::playfield_styling::fill_release_smarvelous();
    }
}

/// Per-song reset (play-scene arm): clear the WARN latches so a transient
/// class can re-report on a later song.
pub fn reset_for_song() {
    WARNED_RENDERER.store(false, Ordering::Relaxed);
    WARNED_VTABLE.store(false, Ordering::Relaxed);
}

/// Live "Receptor Flash Color" apply: PURPLE ⇒ the next S-Marv hit pushes
/// the violet burst, WHITE ⇒ it pushes nothing (stock Marvelous receptor).
/// A pure flag — nothing is installed or torn down.
pub fn set_flash_mode(mode: ReceptorFlash) {
    FLASH_MODE.store(mode.index(), Ordering::Release);
}

/// The live choice (unknown index ⇒ default).
pub fn flash_mode() -> ReceptorFlash {
    ReceptorFlash::from_index(FLASH_MODE.load(Ordering::Acquire)).unwrap_or(ReceptorFlash::DEFAULT)
}

/// Fill-hook entry (render thread, per `JudgeEffectRenderer` quad while the
/// s_marvelous consumer holds the hook): recolour a white (ours) quad
/// violet; `None` = pass the game's colour through. `color` points at the
/// game's COLOR4B `(R,G,B,A)`; never written through. The decision and the
/// hue live in the pure `receptor_color` layer.
pub fn recolor_burst(color: *const u8) -> Option<[u8; 4]> {
    if color.is_null() {
        return None;
    }
    // SAFETY: the fill hook validated the pointer is non-null and it is the
    // 4-byte COLOR4B the game's own colour routine just built on its stack.
    let c = unsafe { [*color, *color.add(1), *color.add(2), *color.add(3)] };
    recolor(c)
}

/// Post-original, for an S-Marvelous event of an armed side: push one
/// [`BURST_TYPE`] record for the event's lanes through the game's own pusher
/// — unless the operator chose the WHITE receptor flash, in which case the
/// hit leaves the stock Marvelous receptor alone.
///
/// * `gpa` — the side's GamePlayActor (`judge_submit`'s `this`; the same
///   object `judgeNotes` reads the renderer from).
/// * `info` — the judge_submit info struct (lane bitset at +0x08); null ⇒
///   nothing to do.
pub fn on_smarvelous(side: usize, gpa: *mut u8, info: *const u8) {
    if !ACTIVE.load(Ordering::Acquire) || gpa.is_null() || info.is_null() {
        return;
    }
    if !flash_mode().pushes_burst() {
        return;
    }
    let push = PUSH_FN.load(Ordering::Acquire);
    let off = RENDERER_OFF.load(Ordering::Acquire);
    if push.is_null() || off == 0 {
        return;
    }
    // SAFETY: game thread, inside the judge_submit dispatch where the
    // GamePlayActor and its renderer are live; the renderer pointer is
    // probed (VirtualQuery) and identity-checked before it is passed on.
    unsafe {
        let lanes = lane_bits((info.add(INFO_LANES_OFFSET) as *const u32).read_unaligned());
        if lanes == 0 {
            return;
        }
        let renderer = (gpa.add(off) as *const *mut u8).read_unaligned();
        if renderer.is_null()
            || (renderer as usize) % 8 != 0
            || !memory::is_readable(renderer, RENDERER_MIN_READABLE)
        {
            if !WARNED_RENDERER.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "SMarvelous: receptor burst -- JudgeEffectRenderer unreadable (side {}, gpa+0x{:X} = {:p}); no burst this song",
                    side,
                    off,
                    renderer
                );
            }
            return;
        }
        let want_vt = JUDGE_VTABLE.load(Ordering::Acquire);
        let vt = (renderer as *const *mut u8).read_unaligned();
        if vt != want_vt {
            if !WARNED_VTABLE.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "SMarvelous: receptor burst -- object at gpa+0x{:X} is not a JudgeEffectRenderer (vtable {:p}, want {:p}); no burst this song",
                    off,
                    vt,
                    want_vt
                );
            }
            return;
        }
        let push: PushFn = std::mem::transmute(push);
        push(renderer, lanes, BURST_TYPE);
        if !FIRST_PUSH_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SMarvelous: receptor burst live -- first violet burst pushed (side {}, lanes 0x{:02X}, renderer {:p})",
                side,
                lanes,
                renderer
            );
        }
    }
}
