//! Combo hooks — the ONE owner of every detour on World's gameplay
//! `sequence::dance::ComboActor`.
//!
//! * the digit refresh (`combo_digit_refresh`, `FUN_180066930` on 20260825 —
//!   repaints the three `dance_combo_root1..3` layers; called only by the
//!   actor's init and its msg-0x1033 case), promoted from s_marvelous;
//! * the actor's `onInitialize` / `onFinalize` / `onUpdate` / `onMessage`
//!   (RTTI slots 4 / 5 / 6 / 8, `derive_combo_actor`) — for ddr_selection's
//!   legacy combo, which re-hosts A3's single-clip `ComboActor` inside World's
//!   object (World's object and vtable stay: `song_reset` and the finalize's
//!   record write-back keep working).
//!
//! Subscribers:
//!
//! * s_marvelous — refresh POST (the all-S-Marvelous digit / tint repaint);
//! * ddr_selection — refresh OVERRIDE, init PRE (may skip) / POST, msg OVERRIDE, update
//!   OVERRIDE, finalize POST (a legacy actor never reaches World's refresh,
//!   msg-0x1033 case or update, so s_marvelous' repaint never runs for it).
//!
//! An OVERRIDE returning `true` / `Some(ret)` skips the original (and, for
//! the refresh, the POST subscribers). Detours install on the first
//! `acquire_*` and stay installed for the session; subscribers gate
//! themselves. Game thread only (actor callbacks). Callbacks are plain `fn`
//! pointers run under `catch_unwind`.

use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::signatures::{ComboActorSites, SignatureStore};
use crate::{log_info, log_warn};

/// `(actor)`.
pub type ActorFn = fn(*mut u8);
/// `(actor) -> handled`.
pub type OverrideFn = fn(*mut u8) -> bool;
/// `(actor, msg, payload) -> Some(return value)` = handled.
pub type MsgOverrideFn = fn(*mut u8, i32, *mut u8) -> Option<u64>;

type ActorRawFn = unsafe extern "C" fn(*mut u8);
type MsgRawFn = unsafe extern "C" fn(*mut u8, i32, *mut u8) -> u64;

static mut REFRESH_HOOK: Option<GenericDetour<ActorRawFn>> = None;
static mut INIT_HOOK: Option<GenericDetour<ActorRawFn>> = None;
static mut FINALIZE_HOOK: Option<GenericDetour<ActorRawFn>> = None;
static mut UPDATE_HOOK: Option<GenericDetour<ActorRawFn>> = None;
static mut MSG_HOOK: Option<GenericDetour<MsgRawFn>> = None;

struct Sites {
    refresh: Option<*const u8>,
    actor: Option<ComboActorSites>,
}
unsafe impl Send for Sites {}
unsafe impl Sync for Sites {}

static SITES: OnceLock<Sites> = OnceLock::new();
static REFRESH_INSTALLED: AtomicBool = AtomicBool::new(false);
static ACTOR_INSTALLED: AtomicBool = AtomicBool::new(false);
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

struct Subscribers {
    refresh_override: Vec<OverrideFn>,
    refresh_post: Vec<ActorFn>,
    init_pre: Vec<OverrideFn>,
    init_post: Vec<ActorFn>,
    finalize_post: Vec<ActorFn>,
    update_override: Vec<OverrideFn>,
    msg_override: Vec<MsgOverrideFn>,
}

static SUBSCRIBERS: Mutex<Subscribers> = Mutex::new(Subscribers {
    refresh_override: Vec::new(),
    refresh_post: Vec::new(),
    init_pre: Vec::new(),
    init_post: Vec::new(),
    finalize_post: Vec::new(),
    update_override: Vec::new(),
    msg_override: Vec::new(),
});

/// Resolve the sites (lib.rs service init). Installs nothing.
pub fn init(signatures: &SignatureStore) -> bool {
    let refresh = signatures.get_address("combo_digit_refresh");
    let actor = signatures.combo_actor_sites();
    log_info!(
        "ComboHooks: digit refresh {}, actor functions {}",
        if refresh.is_some() {
            "resolved"
        } else {
            "UNRESOLVED"
        },
        if actor.is_some() {
            "resolved"
        } else {
            "UNRESOLVED"
        }
    );
    let any = refresh.is_some() || actor.is_some();
    let _ = SITES.set(Sites { refresh, actor });
    any
}

/// The actor's functions and counter fields, if derived.
pub fn actor_sites() -> Option<ComboActorSites> {
    SITES.get()?.actor
}

/// Install the digit-refresh detour (idempotent). `false` when unresolved or
/// the install failed.
pub fn acquire_refresh() -> bool {
    if REFRESH_INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let Some(target) = SITES.get().and_then(|s| s.refresh) else {
        return false;
    };
    let _guard = INSTALL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if REFRESH_INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    unsafe {
        let t: ActorRawFn = std::mem::transmute(target);
        if let Err(e) =
            hooks::install_enabled(std::ptr::addr_of_mut!(REFRESH_HOOK), t, refresh_hook)
        {
            log_warn!("ComboHooks: digit refresh detour failed: {:?}", e);
            return false;
        }
    }
    REFRESH_INSTALLED.store(true, Ordering::Release);
    log_info!("ComboHooks: digit refresh detour installed @ {:p}", target);
    true
}

/// Install the init / finalize / update / msg detours (idempotent,
/// all-or-nothing). `false` when unresolved or an install failed.
pub fn acquire_actor() -> bool {
    if ACTOR_INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let Some(s) = SITES.get().and_then(|s| s.actor) else {
        return false;
    };
    let _guard = INSTALL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if ACTOR_INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    unsafe {
        let actor_hooks: [(
            *mut Option<GenericDetour<ActorRawFn>>,
            *const u8,
            ActorRawFn,
        ); 3] = [
            (std::ptr::addr_of_mut!(INIT_HOOK), s.init, init_hook),
            (
                std::ptr::addr_of_mut!(FINALIZE_HOOK),
                s.finalize,
                finalize_hook,
            ),
            (std::ptr::addr_of_mut!(UPDATE_HOOK), s.update, update_hook),
        ];
        let mut done: Vec<*mut Option<GenericDetour<ActorRawFn>>> = Vec::new();
        let rollback = |done: &[*mut Option<GenericDetour<ActorRawFn>>]| {
            for storage in done {
                if let Some(d) = (**storage).take() {
                    let _ = d.disable();
                }
            }
        };
        for (storage, target, cb) in actor_hooks {
            let t: ActorRawFn = std::mem::transmute(target);
            if let Err(e) = hooks::install_enabled(storage, t, cb) {
                log_warn!("ComboHooks: actor detour @ {:p} failed: {:?}", target, e);
                rollback(&done);
                return false;
            }
            done.push(storage);
        }
        let t: MsgRawFn = std::mem::transmute(s.msg);
        if let Err(e) = hooks::install_enabled(std::ptr::addr_of_mut!(MSG_HOOK), t, msg_hook) {
            log_warn!("ComboHooks: msg detour failed: {:?}", e);
            rollback(&done);
            return false;
        }
    }
    ACTOR_INSTALLED.store(true, Ordering::Release);
    log_info!(
        "ComboHooks: actor detours installed (init @ {:p}, finalize @ {:p}, update @ {:p}, msg @ {:p})",
        s.init,
        s.finalize,
        s.update,
        s.msg
    );
    true
}

fn subscribe<T: Copy + PartialEq>(pick: fn(&mut Subscribers) -> &mut Vec<T>, f: T) {
    let mut s = SUBSCRIBERS.lock().unwrap_or_else(|e| e.into_inner());
    let v = pick(&mut s);
    if !v.contains(&f) {
        if v.len() >= MAX_PER_LIST {
            log_warn!("ComboHooks: subscriber list full -- subscription dropped");
            return;
        }
        v.push(f);
    }
}

/// Subscribers per list (a fixed-size copy: the update / msg detours run
/// every frame — no allocation on the hot path).
const MAX_PER_LIST: usize = 4;

fn snapshot<T: Copy>(pick: fn(&Subscribers) -> &Vec<T>) -> [Option<T>; MAX_PER_LIST] {
    let s = SUBSCRIBERS.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = [None; MAX_PER_LIST];
    for (o, f) in out.iter_mut().zip(pick(&s).iter()) {
        *o = Some(*f);
    }
    out
}

/// Run `f(actor)` after World's digit refresh.
pub fn subscribe_refresh_post(f: ActorFn) {
    subscribe(|s| &mut s.refresh_post, f);
}

/// `f(actor) == true` replaces World's digit refresh (and skips the POST
/// subscribers).
pub fn subscribe_refresh_override(f: OverrideFn) {
    subscribe(|s| &mut s.refresh_override, f);
}

/// Run `f(actor)` before World's `onInitialize`; `true` skips World's init
/// (the POST subscribers still run).
pub fn subscribe_init_pre(f: OverrideFn) {
    subscribe(|s| &mut s.init_pre, f);
}
/// Run `f(actor)` after World's `onInitialize` (or after a skipped one).
pub fn subscribe_init_post(f: ActorFn) {
    subscribe(|s| &mut s.init_post, f);
}

/// Run `f(actor)` after World's `onFinalize`.
pub fn subscribe_finalize_post(f: ActorFn) {
    subscribe(|s| &mut s.finalize_post, f);
}

/// `f(actor) == true` replaces World's `onUpdate`.
pub fn subscribe_update_override(f: OverrideFn) {
    subscribe(|s| &mut s.update_override, f);
}

/// `f(actor, msg, payload) == Some(r)` replaces World's `onMessage` (returns
/// `r`).
pub fn subscribe_msg_override(f: MsgOverrideFn) {
    subscribe(|s| &mut s.msg_override, f);
}

fn run_actor(list: [Option<ActorFn>; MAX_PER_LIST], actor: *mut u8) {
    for f in list.into_iter().flatten() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(actor)));
    }
}

fn run_override(list: [Option<OverrideFn>; MAX_PER_LIST], actor: *mut u8) -> bool {
    list.into_iter().flatten().any(|f| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(actor))).unwrap_or(false)
    })
}

unsafe extern "C" fn refresh_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(REFRESH_HOOK)).as_ref() else {
        return;
    };
    if run_override(snapshot(|s| &s.refresh_override), actor) {
        return;
    }
    hook.call(actor);
    run_actor(snapshot(|s| &s.refresh_post), actor);
}

unsafe extern "C" fn init_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(INIT_HOOK)).as_ref() else {
        return;
    };
    if !run_override(snapshot(|s| &s.init_pre), actor) {
        hook.call(actor);
    }
    run_actor(snapshot(|s| &s.init_post), actor);
}

unsafe extern "C" fn finalize_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(FINALIZE_HOOK)).as_ref() else {
        return;
    };
    hook.call(actor);
    run_actor(snapshot(|s| &s.finalize_post), actor);
}

unsafe extern "C" fn update_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(UPDATE_HOOK)).as_ref() else {
        return;
    };
    if run_override(snapshot(|s| &s.update_override), actor) {
        return;
    }
    hook.call(actor);
}

unsafe extern "C" fn msg_hook(actor: *mut u8, msg: i32, payload: *mut u8) -> u64 {
    let Some(hook) = (*addr_of!(MSG_HOOK)).as_ref() else {
        return 0;
    };
    for f in snapshot(|s| &s.msg_override).into_iter().flatten() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(actor, msg, payload)))
            .unwrap_or(None);
        if let Some(r) = r {
            return r;
        }
    }
    hook.call(actor, msg, payload)
}
