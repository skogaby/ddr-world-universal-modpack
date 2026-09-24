//! HUD layout hooks — the ONE owner of the detours on the gameplay
//! `LayoutActor`'s marker builder (`hud_layout_builder_entry`) and its marker
//! setter (`hud_layout_setter`).
//!
//! The builder (`FUN_18006bd40` on 20260825) runs once per gameplay
//! `LayoutActor`, after every package on its load list is resident: it reads
//! World's layout root (`dance_common` → `dance_root`) and fills the shared
//! (`LayoutActor + 0x98`) and per-side (`+0xE0 + side*0x48`) marker maps HUD
//! actors position themselves from. The setter
//! `void(parent, const char* key, const i32 coord[6])` overwrites
//! `map[parent+0x28][key]`.
//!
//! Two consumers share them (one-detour rule):
//!
//! * center_arrows_single — a builder PRE subscriber (captures the pass
//!   state) and a setter PRE subscriber (shifts the active 1P side's
//!   lane-relative X);
//! * ddr_selection — a builder POST subscriber (the legacy-skin marker
//!   post-pass) that writes keys through [`set_marker`], which runs the setter
//!   subscribers before the original — so center-arrows' shift also applies
//!   to every key the post-pass overwrites.
//!
//! Detours install on the first [`acquire`] and stay installed for the
//! session; subscribers gate themselves with their own flags. Game thread
//! only (the builder runs inside `LayoutActor::onUpdate`). Callbacks are
//! plain `fn` pointers run under `catch_unwind`.

use std::ffi::{c_char, CStr};
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::signatures::{HudLayoutSideSites, HudLayoutSites, SignatureStore};
use crate::{log_info, log_warn};

/// Builder subscriber: `(layout_actor)`.
pub type BuilderFn = fn(*mut u8);
/// Setter subscriber: `(parent, key, coord)` — may rewrite `coord`.
pub type SetterFn = fn(*mut u8, &CStr, &mut [i32; 6]);

type HudBuilderFn = unsafe extern "C" fn(*mut u8);
type HudSetterFn = unsafe extern "C" fn(*mut u8, *const c_char, *mut i32);

static mut BUILDER_HOOK: Option<GenericDetour<HudBuilderFn>> = None;
static mut SETTER_HOOK: Option<GenericDetour<HudSetterFn>> = None;

struct SitesSync(HudLayoutSites);
unsafe impl Send for SitesSync {}
unsafe impl Sync for SitesSync {}

static SITES: OnceLock<SitesSync> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

struct Subscribers {
    builder_pre: Vec<BuilderFn>,
    builder_post: Vec<BuilderFn>,
    setter_pre: Vec<SetterFn>,
}

static SUBSCRIBERS: Mutex<Subscribers> = Mutex::new(Subscribers {
    builder_pre: Vec::new(),
    builder_post: Vec::new(),
    setter_pre: Vec::new(),
});

/// Resolve the sites (lib.rs service init). Installs nothing.
pub fn init(signatures: &SignatureStore) -> bool {
    match signatures.hud_layout_sites() {
        Some(sites) => {
            log_info!(
                "HudLayoutHooks: builder @ {:p}, setter @ {:p}, side extras {}",
                sites.builder,
                sites.setter,
                if sites.side.is_some() {
                    "resolved"
                } else {
                    "UNRESOLVED"
                }
            );
            let _ = SITES.set(SitesSync(sites));
            true
        }
        None => {
            log_warn!("HudLayoutHooks: builder / setter unresolved -- layout hooks unavailable");
            false
        }
    }
}

/// Whether the builder / setter pair resolved.
pub fn is_available() -> bool {
    SITES.get().is_some()
}

/// The builder's per-side extras (style / reverse / judge_position vslot).
pub fn side_sites() -> Option<HudLayoutSideSites> {
    SITES.get()?.0.side
}

/// Install both detours (once per session; idempotent). `false` when the
/// sites are missing or an install failed.
pub fn acquire() -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let Some(sites) = SITES.get() else {
        return false;
    };
    let _guard = INSTALL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    unsafe {
        let target: HudBuilderFn = std::mem::transmute(sites.0.builder);
        if let Err(e) =
            hooks::install_enabled(std::ptr::addr_of_mut!(BUILDER_HOOK), target, builder_hook)
        {
            log_warn!("HudLayoutHooks: builder detour failed: {:?}", e);
            return false;
        }
        let target: HudSetterFn = std::mem::transmute(sites.0.setter);
        if let Err(e) =
            hooks::install_enabled(std::ptr::addr_of_mut!(SETTER_HOOK), target, setter_hook)
        {
            log_warn!("HudLayoutHooks: setter detour failed: {:?}", e);
            if let Some(d) = (*std::ptr::addr_of_mut!(BUILDER_HOOK)).take() {
                let _ = d.disable();
            }
            return false;
        }
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!(
        "HudLayoutHooks: detours installed (builder @ {:p}, setter @ {:p})",
        sites.0.builder,
        sites.0.setter
    );
    true
}

fn subscribe<T: Copy + PartialEq>(pick: fn(&mut Subscribers) -> &mut Vec<T>, f: T) {
    let mut s = SUBSCRIBERS.lock().unwrap_or_else(|e| e.into_inner());
    let v = pick(&mut s);
    if !v.contains(&f) {
        v.push(f);
    }
}

/// Run `f(layout_actor)` before the original builder.
pub fn subscribe_builder_pre(f: BuilderFn) {
    subscribe(|s| &mut s.builder_pre, f);
}

/// Run `f(layout_actor)` after the original builder (the marker maps are
/// complete; write keys through [`set_marker`]).
pub fn subscribe_builder_post(f: BuilderFn) {
    subscribe(|s| &mut s.builder_post, f);
}

/// Run `f(parent, key, coord)` before every original setter call (World's
/// own and [`set_marker`]'s).
pub fn subscribe_setter_pre(f: SetterFn) {
    subscribe(|s| &mut s.setter_pre, f);
}

fn snapshot<T: Copy>(pick: fn(&Subscribers) -> &Vec<T>) -> Vec<T> {
    let s = SUBSCRIBERS.lock().unwrap_or_else(|e| e.into_inner());
    pick(&s).clone()
}

fn run_setter_subscribers(parent: *mut u8, key: &CStr, coord: &mut [i32; 6]) {
    for f in snapshot(|s| &s.setter_pre) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(parent, key, coord)));
    }
}

/// Write `coord` under `key` into the marker map of `parent` through World's
/// own setter, after the setter subscribers (center-arrows' lane shift).
/// Game thread, inside a builder POST subscriber. `false` when the detour is
/// not installed.
pub fn set_marker(parent: *mut u8, key: &CStr, coord: [i32; 6]) -> bool {
    let Some(hook) = (unsafe { (*addr_of!(SETTER_HOOK)).as_ref() }) else {
        return false;
    };
    let mut c = coord;
    run_setter_subscribers(parent, key, &mut c);
    unsafe { hook.call(parent, key.as_ptr(), c.as_mut_ptr()) };
    true
}

unsafe extern "C" fn builder_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(BUILDER_HOOK)).as_ref() else {
        return;
    };
    for f in snapshot(|s| &s.builder_pre) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(actor)));
    }
    hook.call(actor);
    for f in snapshot(|s| &s.builder_post) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(actor)));
    }
}

unsafe extern "C" fn setter_hook(parent: *mut u8, key: *const c_char, coord: *mut i32) {
    let Some(hook) = (*addr_of!(SETTER_HOOK)).as_ref() else {
        return;
    };
    if !parent.is_null() && !key.is_null() && !coord.is_null() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut c = [0i32; 6];
            std::ptr::copy_nonoverlapping(coord, c.as_mut_ptr(), 6);
            run_setter_subscribers(parent, CStr::from_ptr(key), &mut c);
            std::ptr::copy_nonoverlapping(c.as_ptr(), coord, 6);
        }));
    }
    hook.call(parent, key, coord);
}
