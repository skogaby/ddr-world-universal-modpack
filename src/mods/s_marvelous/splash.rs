//! Full-combo splash override (design §4.6): an all-S-Marvelous MFC plays
//! the S-MARVELOUS splash segment instead of the stock MARVELOUS one.
//!
//! Mechanism: one post-original `GenericDetour` on the FullcomboActor
//! message handler (`fullcombo_actor_on_message` — the actor's ONLY
//! handled message is 0x1034, fired once at song end when a full combo was
//! achieved; payload dword = MFC type 0..3 computed inside judge_submit).
//! Post-original, for `msg == 0x1034 && type == 0 && combo_is_all_smarv`:
//! re-drive the splash clip's label to `s_marbelous_in` via `mc_op(0xF09)`
//! — the engine's child-redirect + per-clip label lookup (the Step-4
//! flash's proven op; all four templates carry the label in BOTH timeline
//! sections via the multi-shape clone). Stock already played
//! `se_game_fullcombo` and set play/visibility — only the goto is
//! re-driven.
//!
//! Fail-open: unresolved signature ⇒ no detour; un-patched templates ⇒ the
//! goto is a benign no-op on stock labels (0xF09 swallows misses — the
//! stock MFC splash shows).
//!
//! Actor layout (display-side RE §5): `+0x88` side-info ptr (first dword =
//! side), `+0x98` splash clip wrapper.

use std::sync::atomic::{AtomicBool, Ordering};

use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::services::{afp_patcher, bm2d_api};
use crate::{log_info, log_warn};

use super::{assets, state};

type FcOnMessageFn = unsafe extern "C" fn(*mut u8, u32, *mut u8) -> u64;

static DETOUR: once_cell::sync::OnceCell<GenericDetour<FcOnMessageFn>> =
    once_cell::sync::OnceCell::new();

/// Whether the splash templates/assets staged + patches registered (set at
/// enable). The re-drive declines without it (stock splash).
static ASSETS_READY: AtomicBool = AtomicBool::new(false);
static FIRST_REDRIVE_LOGGED: AtomicBool = AtomicBool::new(false);
/// One-time patch registration latch (afp_patcher registrations persist).
static PATCHES_REGISTERED: AtomicBool = AtomicBool::new(false);

const ACTOR_SIDE_INFO: usize = 0x88;
const ACTOR_SPLASH_WRAPPER: usize = 0x98;
const MSG_FULLCOMBO: u32 = 0x1034;
const LABEL: &std::ffi::CStr = c"s_marbelous_in";

pub fn install(signatures: &SignatureStore) -> bool {
    let Some(target) = signatures.get_address("fullcombo_actor_on_message") else {
        log_warn!("SMarvelous: fullcombo_actor_on_message unresolved — splash stays stock");
        return false;
    };
    let target: FcOnMessageFn = unsafe { std::mem::transmute(target) };
    match unsafe { GenericDetour::new(target, fc_message_hook) } {
        Ok(detour) => {
            if unsafe { detour.enable() }.is_err() {
                log_warn!("SMarvelous: splash detour enable failed — splash stays stock");
                return false;
            }
            let _ = DETOUR.set(detour);
            log_info!("SMarvelous: fullcombo splash detour installed");
            true
        }
        Err(e) => {
            log_warn!(
                "SMarvelous: splash detour failed: {:?} — splash stays stock",
                e
            );
            false
        }
    }
}

/// Stage the splash assets + register the four template patches. Called
/// from enable; sets ASSETS_READY on success.
pub fn activate() {
    if PATCHES_REGISTERED.load(Ordering::Acquire) {
        // Assets staged + patches registered once per boot (inputs can't
        // change mid-session); re-enable just re-arms the re-drive.
        ASSETS_READY.store(true, Ordering::Release);
        return;
    }
    let staged = assets::stage_fullcombo();
    if staged.is_empty() {
        return; // WARNs already emitted
    }
    for patch in staged {
        let template: &'static str = patch.template;
        let stock = patch.stock_bytes;
        let shape_ids = patch.shape_ids;
        let expected = patch.expected;
        afp_patcher::register_patch(
            template,
            Box::new(move |afp: &[u8], _bsi: &[u8]| {
                if !ASSETS_READY.load(Ordering::Acquire) {
                    return None;
                }
                if afp != stock.as_slice() {
                    log_warn!(
                        "SMarvelous: {} variant differs from the staged template — streaming stock",
                        template
                    );
                    return None;
                }
                let mut doc = crate::core::ap2::Ap2Doc::parse(afp)?;
                let ids = doc.clone_segment_with_new_shapes(
                    assets::FC_SRC_LABEL,
                    assets::FC_NEW_LABEL,
                    &shape_ids,
                )?;
                if ids != expected {
                    log_warn!(
                        "SMarvelous: {} allocated ids diverged from staging — streaming stock",
                        template
                    );
                    return None;
                }
                let out = doc.serialize()?;
                log_info!(
                    "SMarvelous: {} patched ({} -> {} bytes, {})",
                    template,
                    afp.len(),
                    out.len(),
                    assets::FC_NEW_LABEL
                );
                Some((out, vec![0u8; 2]))
            }),
        );
    }
    PATCHES_REGISTERED.store(true, Ordering::Release);
    ASSETS_READY.store(true, Ordering::Release);
}

pub fn deactivate() {
    ASSETS_READY.store(false, Ordering::Release);
}

pub fn reset_latches() {
    FIRST_REDRIVE_LOGGED.store(false, Ordering::Relaxed);
}

unsafe extern "C" fn fc_message_hook(actor: *mut u8, msg: u32, payload: *mut u8) -> u64 {
    let ret = match DETOUR.get() {
        Some(detour) => detour.call(actor, msg, payload),
        None => 0,
    };
    if let Err(e) = std::panic::catch_unwind(|| redrive_if_smfc(actor, msg, payload)) {
        let _ = e;
    }
    ret
}

fn redrive_if_smfc(actor: *mut u8, msg: u32, payload: *mut u8) {
    if msg != MSG_FULLCOMBO
        || actor.is_null()
        || payload.is_null()
        || !ASSETS_READY.load(Ordering::Acquire)
    {
        return;
    }
    unsafe {
        // MFC type 0 (all-Marvelous) only — anything else already shows a
        // lower-tier splash.
        let mfc_type = (payload as *const i32).read_unaligned();
        if mfc_type != 0 {
            return;
        }
        let side_info = (actor.add(ACTOR_SIDE_INFO) as *const *const u8).read_unaligned();
        if side_info.is_null() {
            return;
        }
        let side = (side_info as *const i32).read_unaligned();
        if !(0..=1).contains(&side) || !state::combo_is_all_smarv(side as usize) {
            return;
        }
        let wrapper = (actor.add(ACTOR_SPLASH_WRAPPER) as *const *const u8).read_unaligned();
        if wrapper.is_null() {
            return;
        }
        let mc_id = (wrapper.add(0x110) as *const u32).read_unaligned();
        if mc_id < 1 {
            return;
        }
        if bm2d_api::mc_op_str(mc_id, 0xF09, LABEL)
            && !FIRST_REDRIVE_LOGGED.swap(true, Ordering::Relaxed)
        {
            log_info!(
                "SMarvelous: S-MFC splash re-drive (side {}, mc {:08X})",
                side,
                mc_id
            );
        }
    }
}
