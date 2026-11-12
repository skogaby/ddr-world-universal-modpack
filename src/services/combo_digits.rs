//! Five-digit combo display.
//!
//! World's combo counter is an uncapped `i32`, but what the player sees
//! stops at 9,999: `ComboActor`'s digit refresh clamps the value and the
//! digit count to four, and the `number` sprite inside each of the three
//! `dance_combo_root%d` clips has four digit instances (`1_usr` … `1000_usr`)
//! and four layouts (`loop_1` … `loop_1000`). No stock chart gets there; a
//! StepMania marathon does. One chart holds at most 65,535 steps, so one
//! more digit covers every combo the game can count.
//!
//! Two halves, both needed:
//!
//! 1. **The clips grow a fifth digit** — an `afp_patcher` patch on the three
//!    root templates clones the `loop_1000` layout as `loop_10000`, moves
//!    the cloned row over and adds a `10000_usr` instance (a copy of the
//!    thousands digit). The recipe is `Ap2Doc::
//!    clone_segment_with_extra_instance`; the constants and the layout
//!    arithmetic are in `combo_digits_math.rs`. A template the recipe
//!    refuses streams stock.
//! 2. **The refresh shows it** — an EXTEND subscriber on the shared combo
//!    refresh detour (`services::combo_hooks`), which runs after World's
//!    refresh and before the POST subscribers. For a combo past 9,999 it
//!    moves each root's `combo_usr/number_usr` to `loop_10000` (only when
//!    it is not already there — the stock refresh sees four digits before
//!    and after, so it never moves it back) and loads all five digit
//!    textures with the stock naming and the stock traversal-6 walk over
//!    same-name instances. Below 10,000 it does nothing: the stock refresh
//!    picks a stock layout as soon as the digit count changes.
//!
//! Fail-open at every step: the label is looked up on the live clip
//! (`mc_frame_by_label`), so an unpatched clip — a refused template, a
//! package variant, a failed patch — keeps the stock 9,999 display; one WARN
//! says so. A DDR SELECTION legacy combo never reaches World's refresh (its
//! own override replaces it), so the legacy skins keep their stock cap.
//!
//! POST subscribers that repaint digits ask [`five_digits_shown`] for the
//! row World's refresh and this module left behind (s_marvelous' violet
//! digits).
//!
//! Game thread only (the refresh is an actor callback). The patch function
//! runs on whichever thread streams the template.
//!
//! RE: `docs/stepmania_runtime_packs_feasibility.md` §20. Host tests:
//! `scripts/validate_combo_digits.sh` (the recipe on the real templates).

use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;

use crate::core::ap2::{Ap2Doc, ExtraInstance};
use crate::core::memory;
use crate::services::combo_digits_math as math;
use crate::services::{afp_patcher, bm2d_api, combo_hooks};
use crate::{log_info, log_warn};

/// `afp_mc_op` goto-frame-label + play (the stock refresh's own layout op).
const OP_GOTO_LABEL: i32 = 0xF09;
/// `afp_mc_traversal` mode the stock refresh walks same-name instances with.
const TRAVERSE_SAME_NAME: i32 = 6;
/// [`math::NEW_LABEL`] for the libafp calls.
const NEW_LABEL_C: &CStr = c"loop_10000";

/// The actor fields the refresh step reads (all derived, `combo_hooks`).
#[derive(Clone, Copy)]
struct Layout {
    combo_off: usize,
    worst_off: usize,
    root1_off: usize,
}

static LAYOUT: OnceLock<Layout> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
/// The actor of the refresh in progress when it ended on the five-digit row
/// (0 otherwise) — written by every EXTEND call, read by that call's POST
/// subscribers.
static FIVE_SHOWN_FOR: AtomicUsize = AtomicUsize::new(0);

static FIRST_SHOWN_LOGGED: AtomicBool = AtomicBool::new(false);
static WARNED_UNPATCHED: AtomicBool = AtomicBool::new(false);
static WARNED_PATCH: AtomicBool = AtomicBool::new(false);

/// Turn the fifth digit on (idempotent): register the clip patches and
/// subscribe the refresh step. `false` — and nothing registered — when the
/// combo refresh or the actor layout did not resolve; the display then keeps
/// its stock cap.
pub fn enable() -> bool {
    if ENABLED.load(Ordering::Acquire) {
        return true;
    }
    let (Some(actor), Some(root1_off)) = (combo_hooks::actor_sites(), combo_hooks::root1_off())
    else {
        log_warn!(
            "ComboDigits: ComboActor layout unresolved -- the combo display stays capped at 9999"
        );
        return false;
    };
    let _ = LAYOUT.set(Layout {
        combo_off: actor.combo_off,
        worst_off: actor.worst_off,
        root1_off,
    });
    combo_hooks::subscribe_refresh_extend(extend_refresh);
    if !combo_hooks::acquire_refresh() {
        log_warn!(
            "ComboDigits: combo_digit_refresh unresolved or detour failed -- the combo display stays capped at 9999"
        );
        return false;
    }
    for template in math::TEMPLATES {
        afp_patcher::register_patch(template, Box::new(patch_template));
    }
    ENABLED.store(true, Ordering::Release);
    log_info!(
        "ComboDigits: five-digit combo display enabled (cap {})",
        math::MAX_SHOWN
    );
    true
}

/// Whether the refresh in progress on `actor` ended on the five-digit row —
/// for POST subscribers that repaint digits: paint [`math::PLACES`] from
/// [`math::shown`] when `true`, the stock four places from the 9,999 clamp
/// otherwise. Only meaningful inside a refresh POST callback.
pub fn five_digits_shown(actor: *const u8) -> bool {
    !actor.is_null() && FIVE_SHOWN_FOR.load(Ordering::Acquire) == actor as usize
}

// ── The clip patch ───────────────────────────────────────────────────

/// Add the `loop_10000` layout to one root template. `None` = stream stock.
fn patch_template(afp: &[u8], _bsi: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let run = || -> Option<Vec<u8>> {
        let mut doc = Ap2Doc::parse(afp)?;
        let paths = doc.sections_with_label(math::SRC_LABEL);
        if paths.is_empty() {
            return None;
        }
        for path in &paths {
            doc.clone_segment_with_extra_instance(
                path,
                math::SRC_LABEL,
                math::NEW_LABEL,
                math::ROW_SHIFT,
                &ExtraInstance {
                    donor_name: math::DONOR_INSTANCE,
                    name: math::NEW_INSTANCE,
                    depth: math::NEW_DEPTH,
                    translate: math::NEW_TRANSLATE,
                },
            )?;
        }
        doc.serialize()
    };
    // The recipe is panic-free by contract; the guard keeps a surprise from
    // unwinding into libafp's caller.
    match std::panic::catch_unwind(run) {
        Ok(Some(out)) => Some((out, vec![0u8; 2])),
        _ => {
            if !WARNED_PATCH.swap(true, Ordering::AcqRel) {
                log_warn!(
                    "ComboDigits: a dance_combo_root template did not take the five-digit patch -- streaming stock (combo display capped at 9999)"
                );
            }
            None
        }
    }
}

// ── The refresh step ─────────────────────────────────────────────────

/// A pool wrapper's type-1 layer id (`+0x08`), null-safe.
unsafe fn wrapper_layer(wrapper: *const u8) -> Option<u32> {
    if wrapper.is_null() || !memory::is_readable(wrapper, 0x10) {
        return None;
    }
    let id = (wrapper.add(0x08) as *const u32).read_unaligned();
    (id != 0).then_some(id)
}

/// EXTEND subscriber: after World's refresh, show the fifth digit. The
/// outcome is recorded for the POST subscribers of this same refresh call
/// (they run next, on this thread, for this actor).
fn extend_refresh(actor: *mut u8) {
    let shown = !actor.is_null() && show_five_digits(actor);
    FIVE_SHOWN_FOR.store(if shown { actor as usize } else { 0 }, Ordering::Release);
}

/// Move the three roots to the five-digit layout and load the five digit
/// textures. `false` when the combo fits four digits or the clips do not
/// carry the layout (stock display stands).
fn show_five_digits(actor: *mut u8) -> bool {
    let Some(layout) = LAYOUT.get() else {
        return false;
    };
    // SAFETY: `actor` is the live ComboActor the refresh was called on; the
    // offsets are the ones its own code uses (derived, `combo_hooks`).
    let (combo, worst, roots) = unsafe {
        let combo = (actor.add(layout.combo_off) as *const i32).read_unaligned();
        let worst = (actor.add(layout.worst_off) as *const i32).read_unaligned();
        let root =
            |i: usize| (actor.add(layout.root1_off + i * 8) as *const *const u8).read_unaligned();
        (combo, worst, [root(0), root(1), root(2)])
    };
    if !math::needs_five(combo) {
        return false;
    }

    // Resolve all three rows first: five digits on one layer and four on
    // another would be worse than the stock cap.
    let mut rows = [(0u32, 0u32, 0u32); 3];
    for (row, &wrapper) in rows.iter_mut().zip(roots.iter()) {
        // SAFETY: a root wrapper pointer read from the live actor.
        let resolved = unsafe { wrapper_layer(wrapper) }.and_then(|layer| {
            let number = bm2d_api::layer_find_child(layer, math::NUMBER_PATH)?;
            let frame = bm2d_api::mc_frame_by_label(number, NEW_LABEL_C)?;
            Some((layer, number, frame))
        });
        let Some(resolved) = resolved else {
            if !WARNED_UNPATCHED.swap(true, Ordering::AcqRel) {
                log_warn!(
                    "ComboDigits: combo {} but the combo clips carry no {} layout -- display stays at 9999",
                    combo,
                    math::NEW_LABEL
                );
            }
            return false;
        };
        *row = resolved;
    }

    let shown = math::shown(combo);
    for (index, &(layer, number, frame)) in rows.iter().enumerate() {
        // The five-digit layout is the last segment of the sprite: a frame
        // at or past its label is already inside it.
        if bm2d_api::mc_current_frame(number).is_none_or(|current| current < frame) {
            bm2d_api::mc_op_str(number, OP_GOTO_LABEL, NEW_LABEL_C);
        }
        for place in math::PLACES {
            let Some(name) = math::texture_name(index, place, worst, math::digit(shown, place))
            else {
                continue;
            };
            let mut instance = bm2d_api::layer_find_child(layer, &math::instance_path(place));
            while let Some(id) = instance {
                let _ = bm2d_api::mc_load_bitmap(id, &name);
                instance = bm2d_api::mc_traversal(id, TRAVERSE_SAME_NAME);
            }
        }
    }

    if !FIRST_SHOWN_LOGGED.swap(true, Ordering::Relaxed) {
        log_info!(
            "ComboDigits: five-digit combo display live (combo {})",
            combo
        );
    }
    true
}
