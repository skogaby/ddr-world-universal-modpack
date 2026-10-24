//! `ssq_path` — the ONE owner of the detour on the game's SSQ path builder,
//! with a priority resolver chain.
//!
//! `void build_ssq_path(char out[0x100], const char* basename, int d)` is
//! the game's single chooser between `data/mdb_apx/ssq/<b>.ssq` and
//! `<b>_<N>.ssq` (`N` = 1..5 = Beginner..Challenge; a `_N` file holds both
//! styles of that level). Its body is a hardcoded per-build song table
//! (RE: `docs/split_ssq_research.md`). Every game consumer — the boot
//! analysis pass (`CheckStepDataActor`, ~7200 synchronous calls), normal /
//! matching play, course preload — goes through this one function, so one
//! detour covers them all.
//!
//! ## Consumers (ascending priority; first `Some` wins)
//!
//! | priority | consumer | answers for |
//! |---|---|---|
//! | 10 | `services::stepmania::ssq_split` | injected basenames, from the index's split table |
//! | 20 | `split-ssq-auto-discovery` | every other basename, from its Rule-A disk index (passthrough while disabled) |
//! | — | the ORIGINAL (stock table) | when no resolver answers |
//!
//! The detour is installed on the first `register` (never uninstalled —
//! one detour per target); a consumer that wants to stand down returns
//! `None`. The live chain is an immutable snapshot swapped atomically at
//! registration, so the hot path is one atomic load and a slice walk.
//!
//! ## Guards and the divergence oracle
//!
//! Bad args (null pointers, `d ∉ 0..5`, oversize / unterminated basename)
//! and a `format_path` overflow forward to the original unchanged. When a
//! consumer asks for it (`set_divergence_oracle`), each answered call also
//! runs the original into a scratch buffer and logs one INFO per distinct
//! `(basename, d)` whose answer differs (cap 64 per session) — the
//! split-discovery mod's oracle, unchanged in substance.
//!
//! `Choice` and `format_path` stay in the split-discovery mod's pure
//! `resolver.rs` (its host harness mounts that file alone) and are
//! re-exported here; the chain ordering itself is `ssq_path_logic.rs`.

use std::collections::HashSet;
use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Mutex;

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::signatures::SignatureStore;
use crate::services::ssq_path_logic::Chain;
use crate::{log_info, log_warn};

use crate::mods::split_ssq_auto_discovery::resolver::paths_differ;
pub use crate::mods::split_ssq_auto_discovery::resolver::{
    cstr, format_path, Choice, MAX_BASENAME,
};

/// `fn(basename bytes, difficulty 0..4) -> Option<Choice>`; `None` = "not mine".
pub type Resolver = fn(&[u8], usize) -> Option<Choice>;

type BuildSsqPathFn = unsafe extern "C" fn(*mut u8, *const u8, i32);

/// The game's output buffer size (`local_138[256]` in every caller).
pub const OUT_CAP: usize = 0x100;

static TARGET: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Serialises registration (chain rebuild + one-time install).
static REGISTER: Mutex<()> = Mutex::new(());
/// Written once under `REGISTER`, then only read by the detour.
static mut HOOK: Option<GenericDetour<BuildSsqPathFn>> = None;
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
/// Immutable chain snapshot; each registration leaks a fresh copy and swaps
/// the pointer (registrations are a handful at boot — a reader may still be
/// walking the previous snapshot, so none is ever freed).
static CHAIN: AtomicPtr<Chain<Choice>> = AtomicPtr::new(std::ptr::null_mut());
static ORACLE: AtomicBool = AtomicBool::new(false);

static DIVERGENCE_SEEN: Mutex<Option<HashSet<(Vec<u8>, u8)>>> = Mutex::new(None);
static DIVERGENCE_COUNT: AtomicUsize = AtomicUsize::new(0);
const DIVERGENCE_CAP: usize = 64;

/// Record the builder's address. Installs nothing.
pub fn init(signatures: &SignatureStore) -> bool {
    match signatures.get_address("build_ssq_path") {
        Some(addr) => {
            TARGET.store(addr as *mut u8, Ordering::Release);
            true
        }
        None => false,
    }
}

/// Whether the builder resolved (consumers may register).
pub fn is_available() -> bool {
    !TARGET.load(Ordering::Acquire).is_null()
}

/// Whether the detour is live.
pub fn is_installed() -> bool {
    HOOK_INSTALLED.load(Ordering::Acquire)
}

/// Add `resolver` at `priority` and make sure the detour is installed.
/// Returns `false` (one WARN) when the service is unavailable or the
/// install failed; the resolver is then never consulted.
pub fn register(priority: u8, resolver: Resolver) -> bool {
    let target = TARGET.load(Ordering::Acquire);
    if target.is_null() {
        log_warn!(
            "SsqPath: build_ssq_path unresolved -- resolver (priority {}) not registered",
            priority
        );
        return false;
    }
    let _guard = REGISTER.lock().unwrap_or_else(|p| p.into_inner());

    let mut chain = current_chain().cloned().unwrap_or_default();
    chain.register(priority, resolver);
    let priorities = chain.priorities();
    let leaked: *mut Chain<Choice> = Box::into_raw(Box::new(chain));
    CHAIN.store(leaked, Ordering::Release);

    if !HOOK_INSTALLED.load(Ordering::Acquire) {
        // SAFETY: `target` is the resolved entry of a function with exactly
        // this ABI (docs/split_ssq_research.md §3); HOOK is written only here,
        // under REGISTER, before the detour can run.
        let result = unsafe {
            let original: BuildSsqPathFn = std::mem::transmute(target);
            hooks::install_enabled(addr_of_mut!(HOOK), original, build_ssq_path_hook)
        };
        match result {
            Ok(()) => {
                HOOK_INSTALLED.store(true, Ordering::Release);
                log_info!("SsqPath: build_ssq_path detour installed");
            }
            Err(e) => {
                log_warn!(
                    "SsqPath: build_ssq_path detour install failed: {} -- stock path builder in effect",
                    e
                );
                return false;
            }
        }
    }
    log_info!(
        "SsqPath: resolver registered at priority {} (chain: {:?})",
        priority,
        priorities
    );
    true
}

/// Compare every answered call against the original's answer and log the
/// differences (INFO, cap 64 per session).
pub fn set_divergence_oracle(enabled: bool) {
    ORACLE.store(enabled, Ordering::Release);
}

fn current_chain() -> Option<&'static Chain<Choice>> {
    let p = CHAIN.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        // SAFETY: snapshots are leaked, never freed (see `CHAIN`).
        Some(unsafe { &*p })
    }
}

/// Bounded C-string read: bytes up to the first NUL within `cap`, or `None`
/// when no NUL is found (oversize / unterminated ⇒ caller forwards to stock).
unsafe fn bounded_cstr<'a>(p: *const u8, cap: usize) -> Option<&'a [u8]> {
    for i in 0..cap {
        if *p.add(i) == 0 {
            return Some(std::slice::from_raw_parts(p, i));
        }
    }
    None
}

fn log_divergence_once(basename: &[u8], d: i32, ours: &[u8], stock: &[u8]) {
    if DIVERGENCE_COUNT.load(Ordering::Relaxed) >= DIVERGENCE_CAP {
        return;
    }
    let Ok(mut guard) = DIVERGENCE_SEEN.lock() else {
        return;
    };
    let seen = guard.get_or_insert_with(HashSet::new);
    if !seen.insert((basename.to_vec(), d as u8)) {
        return;
    }
    let n = DIVERGENCE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    log_info!(
        "SsqPath: {} d={}: ours={} stock={}{}",
        String::from_utf8_lossy(basename),
        d,
        String::from_utf8_lossy(cstr(ours)),
        String::from_utf8_lossy(cstr(stock)),
        if n == DIVERGENCE_CAP {
            " (cap reached; further divergences not logged)"
        } else {
            ""
        }
    );
}

/// Panic-free: bounded reads, infallible lookups, a bool-returning
/// formatter; every fallback is "let the original answer".
unsafe extern "C" fn build_ssq_path_hook(out: *mut u8, basename: *const u8, d: i32) {
    let Some(hook) = (*addr_of!(HOOK)).as_ref() else {
        return;
    };
    if out.is_null() || basename.is_null() || !(0..5).contains(&d) {
        return hook.call(out, basename, d);
    }
    let Some(chain) = current_chain() else {
        return hook.call(out, basename, d);
    };
    let Some(name) = bounded_cstr(basename, MAX_BASENAME) else {
        return hook.call(out, basename, d);
    };
    let Some(choice) = chain.resolve(name, d as usize) else {
        return hook.call(out, basename, d);
    };
    let mut ours = [0u8; OUT_CAP];
    if !format_path(&mut ours, name, choice) {
        return hook.call(out, basename, d);
    }

    if ORACLE.load(Ordering::Acquire) {
        // The original writes into OUR scratch, never into the caller's
        // buffer, so `out` only ever receives the final answer.
        let mut stock = [0u8; OUT_CAP];
        hook.call(stock.as_mut_ptr(), basename, d);
        if paths_differ(&ours, &stock) {
            log_divergence_once(name, d, &ours, &stock);
        }
    }

    let len = cstr(&ours).len() + 1; // incl. NUL; ≤ OUT_CAP by construction
    std::ptr::copy_nonoverlapping(ours.as_ptr(), out, len);
}
