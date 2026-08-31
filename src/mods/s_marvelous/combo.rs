//! Combo digit override (design §4.5): while a side's combo is ALL
//! S-Marvelous (every step in the chain within the window; O.K. freeze ends
//! carry no delta — state.rs mirrors the stock 6→0 grade fold), the combo
//! counter shows the S-Marvelous digit set + tint instead of Marvelous.
//!
//! Mechanism: one post-original `GenericDetour` on the ComboActor
//! digit-refresh (`combo_digit_refresh` signature — event-driven, called at
//! init and on combo-changed messages with combo ≥ 4, never per-frame).
//! Post-original, when the stock worst-judgement index says Marvelous tier
//! (`this+0x6C == 0`) AND the side's all-S-Marv bit holds:
//!
//! 1. layer root1 (`this+0x70`): re-load places {10, 100, 1000} with
//!    `daco_combo_smarvelous_%d`, replicating the stock traversal-6 walk
//!    (`afp_layer_mc_refer` on `combo_usr/number_usr/%d_usr`, then every
//!    same-name sibling via `afp_mc_traversal(id, 6)`). The ONES place
//!    stays stock-unsuffixed (stock quirk — layer 0's ones place always
//!    uses `daco_combo_%d`).
//! 2. layers root2/root3 (`this+0x78/+0x80`): apply the S-Marvelous tint
//!    pair through the wrapper's SetColor vfunc (+0x98,
//!    `float[4]{r,g,b,1.0}`), overriding the just-applied Marvelous pair.
//!
//! Self-healing (no cleanup path): when the bit drops, the very judgement
//! that dropped it triggers a stock refresh repainting Marvelous art; this
//! detour then declines (bit false) and the stock visuals stand.
//!
//! Fail-open: unresolved signature ⇒ no detour (stock combo); un-staged
//! textures ⇒ the load_bitmap calls fail silently per place (libafp WARNs
//! once in its own log) — gated behind `assets_ready` so that can't happen
//! in practice.

use std::sync::atomic::{AtomicBool, Ordering};

use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::services::bm2d_api;
use crate::{log_info, log_warn};

use super::state;

type ComboRefreshFn = unsafe extern "C" fn(*mut u8);

static DETOUR: once_cell::sync::OnceCell<GenericDetour<ComboRefreshFn>> =
    once_cell::sync::OnceCell::new();

/// Whether the digit textures staged successfully (set at enable). The
/// override declines without it so we never paint half a digit set.
static ASSETS_READY: AtomicBool = AtomicBool::new(false);

/// One-shot INFO for the first successful override (cabinet-log
/// confirmation the whole path is live).
static FIRST_OVERRIDE_LOGGED: AtomicBool = AtomicBool::new(false);

// ── ComboActor field offsets (display-side RE §6) ────────────────────
const ACTOR_SIDE_INFO: usize = 0x58; // ptr → ptr → side dword
const ACTOR_WORST_INDEX: usize = 0x6C; // i32: 0..3 grade tier, 0xFF reset
const ACTOR_ROOT1: usize = 0x70; // wrapper: digit art layer
const ACTOR_ROOT2: usize = 0x78; // wrapper: tinted underlay
const ACTOR_ROOT3: usize = 0x80; // wrapper: tinted underlay

/// S-Marvelous tint pair for root2/root3 (deep violet, matched to the
/// placeholder word art's palette; stock marvelous pair = 0xA9FEEC /
/// 0xDFA6EF in the same encoding). Byte order follows the stock immediates
/// (0xRRGGBB read as u32; converted to float RGB at apply).
const TINT_ROOT2: u32 = 0xE9C8F8; // pale violet (root2 = the lighter pair)
const TINT_ROOT3: u32 = 0xB05CE0; // saturated violet (root3 = the deeper)

/// Digit places re-loaded on root1. The ones place (1) is intentionally
/// absent — stock leaves layer 0's ones place unsuffixed (RE §6 quirk).
const PLACES: [u32; 3] = [10, 100, 1000];

pub fn set_assets_ready(ready: bool) {
    ASSETS_READY.store(ready, Ordering::Release);
}

pub fn install(signatures: &SignatureStore) -> bool {
    let Some(target) = signatures.get_address("combo_digit_refresh") else {
        log_warn!("SMarvelous: combo_digit_refresh unresolved — combo stays stock");
        return false;
    };
    let target: ComboRefreshFn = unsafe { std::mem::transmute(target) };
    match unsafe { GenericDetour::new(target, combo_refresh_hook) } {
        Ok(detour) => {
            if unsafe { detour.enable() }.is_err() {
                log_warn!("SMarvelous: combo refresh detour enable failed — combo stays stock");
                return false;
            }
            let _ = DETOUR.set(detour);
            log_info!("SMarvelous: combo digit refresh detour installed");
            true
        }
        Err(e) => {
            log_warn!(
                "SMarvelous: combo refresh detour failed: {:?} — combo stays stock",
                e
            );
            false
        }
    }
}

unsafe extern "C" fn combo_refresh_hook(actor: *mut u8) {
    // Original FIRST — the stock repaint (art + tint) must precede the
    // override so declining leaves pure stock visuals.
    if let Some(detour) = DETOUR.get() {
        detour.call(actor);
    }
    if let Err(e) = std::panic::catch_unwind(|| override_if_all_smarv(actor)) {
        let _ = e;
    }
}

fn override_if_all_smarv(actor: *mut u8) {
    if actor.is_null() || !ASSETS_READY.load(Ordering::Acquire) {
        return;
    }
    unsafe {
        // Stock worst tier must be Marvelous (0) — anything else means the
        // combo already carries a worse grade and stock art is correct.
        let worst = (actor.add(ACTOR_WORST_INDEX) as *const i32).read_unaligned();
        if worst != 0 {
            return;
        }
        // Side: `**(this+0x58)` (design §4.5 — ComboActor+0x58 → side-info
        // object whose first dword is the play side).
        let side_info = (actor.add(ACTOR_SIDE_INFO) as *const *const u8).read_unaligned();
        if side_info.is_null() {
            return;
        }
        let side = (side_info as *const i32).read_unaligned();
        if !(0..=1).contains(&side) || !state::combo_is_all_smarv(side as usize) {
            return;
        }

        // 1. Digit art on root1, places {10,100,1000} — traversal-6 walk
        // (the stock refresh's own shape: `afp_layer_mc_refer` then every
        // same-name sibling; digit value per place from the display-clamped
        // combo, exactly like the stock `min(combo, 9999)` loop).
        let combo_raw = (actor.add(0x68) as *const i32).read_unaligned();
        let combo = combo_raw.clamp(0, 9999) as u32;
        let root1 = (actor.add(ACTOR_ROOT1) as *const *const u8).read_unaligned();
        if let Some(layer_id) = wrapper_layer(root1) {
            for place in PLACES {
                let digit = (combo / place) % 10;
                let name = format!("daco_combo_smarvelous_{}", digit);
                let path = format!("combo_usr/number_usr/{}_usr", place);
                let mut mc = bm2d_api::layer_find_child(layer_id, &path);
                while let Some(id) = mc {
                    let _ = bm2d_api::mc_load_bitmap(id, &name);
                    mc = bm2d_api::mc_traversal(id, 6);
                }
            }
        }

        // 2. Tint pairs on root2/root3 via the wrapper SetColor vfunc.
        let root2 = (actor.add(ACTOR_ROOT2) as *const *const u8).read_unaligned();
        let root3 = (actor.add(ACTOR_ROOT3) as *const *const u8).read_unaligned();
        apply_tint(root2, TINT_ROOT2);
        apply_tint(root3, TINT_ROOT3);

        if !FIRST_OVERRIDE_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SMarvelous: combo override live — first all-S repaint (side {})",
                side
            );
        }
    }
}

/// Read a pool wrapper's type-1 layer id (+0x08), null-safe.
unsafe fn wrapper_layer(wrapper: *const u8) -> Option<u32> {
    if wrapper.is_null() {
        return None;
    }
    let id = (wrapper.add(0x08) as *const u32).read_unaligned();
    if id == 0 {
        None
    } else {
        Some(id)
    }
}

/// Apply an 0xRRGGBB tint through the wrapper's SetColor vfunc (+0x98,
/// `(wrapper, float[4]{r,g,b,1.0})` — the stock refresh's own call shape).
unsafe fn apply_tint(wrapper: *const u8, rgb: u32) {
    if wrapper.is_null() {
        return;
    }
    let vtable = (wrapper as *const *const u8).read_unaligned();
    if vtable.is_null() {
        return;
    }
    // `vtable` is a BYTE pointer (one deref already happened) — the slot
    // offset is applied in BYTES. (`.add(0x98/8)` here was the deploy-#13
    // crash: 19 BYTES into the vtable, a garbage "function pointer"
    // straddling two slots → wild call with a float-data IP.)
    let func = (vtable.add(0x98) as *const *const u8).read_unaligned();
    if func.is_null() {
        return;
    }
    let set_color: unsafe extern "C" fn(*const u8, *const f32) = std::mem::transmute(func);
    let color: [f32; 4] = [
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
        1.0,
    ];
    set_color(wrapper, color.as_ptr());
}
