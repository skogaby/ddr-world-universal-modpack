//! Split SSQ Auto-Discovery — replaces the game's hardcoded split-chart table
//! with runtime discovery of `<basename>_<N>.ssq` files.
//!
//! ## Why
//!
//! Some songs' charts are split across `data/mdb_apx/ssq/<basename>_<N>.ssq`
//! files (`N` = 1..5 = Beginner..Challenge) because their difficulties carry
//! different tempo data. The game decides which file holds `(basename,
//! difficulty)` in ONE function, `build_ssq_path`, whose body is a hardcoded
//! `repe cmpsb` song table that grows with every revision (19 → 27 → 35
//! entries across 20250805 → 20260721). Players pinned to an older `gamemdx.dll`
//! who load newer chart data get the wrong file for split songs the old
//! binary does not list — an empty Expert/Challenge chart, or the
//! boot-blocking `ME1529 FILE CORRUPTION ERROR`. RE: `docs/split_ssq_research.md`.
//!
//! ## Mechanism
//!
//! One `GenericDetour` on `build_ssq_path` (AOB `build_ssq_path`, unique on all
//! four supported builds; the callback reads nothing at `match+N`). At
//! `enable()` the mod scans the stock SSQ directory plus every LayeredFS mod
//! folder for `_N` files, reads each one's type-3 level set, and builds a
//! Rule-A index (`resolver.rs`): highest `N ≤ d+1` whose file contains a
//! level-`d` chart, else the unsplit file. Every game consumer — boot analysis
//! pass, normal/matching play, course preload — goes through the builder, so
//! the detour covers them all. The resolver is basename-OPAQUE (no musicdb
//! lookup), which preserves the game's `toho1..toho4` randomized-basename
//! special case byte-for-byte.
//!
//! ## Divergence oracle
//!
//! Each call also runs the ORIGINAL into a scratch buffer and logs one INFO per
//! distinct `(basename, d)` whose answer differs (cap 64/session). A matched
//! binary/data pair logs nothing (or only `sabm d=4`, whose `_3`/`_5`
//! Challenge chunks are byte-identical); the target scenario logs one line per
//! newly-discovered split chart.
//!
//! ## Degradation (all fail-open)
//!
//! Signature miss ⇒ mod unregistered (also covers third-party hex-edited
//! 20250805 DLLs). Stock dir unlistable ⇒ no index, every call forwarded to
//! the original, one WARN. Unknown basename ⇒ unsplit path (what stock does
//! for an unknown song). Bad args / oversize basename ⇒ original. Runtime
//! disable ⇒ passthrough (the detour is never uninstalled — one detour per
//! target); re-enable rescans.

pub mod discovery;
pub mod resolver;

use std::collections::HashSet;
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use retour::GenericDetour;

use crate::core::hooks;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::{log_info, log_warn};
use resolver::{cstr, format_path, paths_differ, Choice, Index, MAX_BASENAME};

/// `void build_ssq_path(char out[0x100], const char* basename, int difficulty)`.
type BuildSsqPathFn = unsafe extern "C" fn(*mut u8, *const u8, i32);

static mut HOOK: Option<GenericDetour<BuildSsqPathFn>> = None;
/// Detour installed (drives `is_active`).
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
/// Runtime toggle: false ⇒ the callback forwards every call to the original.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The Rule-A index, replaced wholesale at each enable. `None` ⇒ passthrough.
static INDEX: RwLock<Option<Arc<Index>>> = RwLock::new(None);

/// Divergence-oracle dedupe + cap (design R6).
static DIVERGENCE_SEEN: Mutex<Option<HashSet<(Vec<u8>, u8)>>> = Mutex::new(None);
static DIVERGENCE_COUNT: AtomicUsize = AtomicUsize::new(0);
const DIVERGENCE_CAP: usize = 64;

/// The game's output buffer size (`local_138[256]` in every caller).
const OUT_CAP: usize = 0x100;

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

fn current_index() -> Option<Arc<Index>> {
    // Never held across the original call; poisoned ⇒ treat as absent.
    INDEX.read().ok().and_then(|g| g.clone())
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
        "SplitSsqAutoDiscovery: {} d={}: ours={} stock={}{}",
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

unsafe extern "C" fn build_ssq_path_hook(out: *mut u8, basename: *const u8, d: i32) {
    let Some(hook) = (&*addr_of!(HOOK)).as_ref() else {
        return;
    };
    // Every fallback below is "let stock answer" (design R4/R7). The body is
    // panic-free: bounded reads, infallible lookup, bool-returning formatter.
    if !ACTIVE.load(Ordering::Acquire) || out.is_null() || basename.is_null() {
        return hook.call(out, basename, d);
    }
    if !(0..5).contains(&d) {
        return hook.call(out, basename, d);
    }
    let Some(index) = current_index() else {
        return hook.call(out, basename, d);
    };
    let Some(name) = bounded_cstr(basename, MAX_BASENAME) else {
        return hook.call(out, basename, d);
    };

    let choice: Choice = index.resolve(name, d as usize);
    let mut ours = [0u8; OUT_CAP];
    if !format_path(&mut ours, name, choice) {
        return hook.call(out, basename, d);
    }

    // Divergence oracle: the original writes into OUR scratch, never into the
    // caller's buffer, so `out` only ever receives the final answer.
    let mut stock = [0u8; OUT_CAP];
    hook.call(stock.as_mut_ptr(), basename, d);
    if paths_differ(&ours, &stock) {
        log_divergence_once(name, d, &ours, &stock);
    }

    let len = cstr(&ours).len() + 1; // incl. NUL; ≤ OUT_CAP by construction
    std::ptr::copy_nonoverlapping(ours.as_ptr(), out, len);
}

pub struct SplitSsqAutoDiscoveryMod {
    builder_addr: *const u8,
}

unsafe impl Send for SplitSsqAutoDiscoveryMod {}

impl SplitSsqAutoDiscoveryMod {
    pub fn new() -> Self {
        Self {
            builder_addr: std::ptr::null(),
        }
    }

    /// Scan the disk and publish a fresh index. Returns whether an index is
    /// available afterwards.
    fn rebuild_index() -> bool {
        match discovery::scan() {
            Ok(files) => {
                let index = Index::build(&files);
                log_info!(
                    "SplitSsqAutoDiscovery: indexed {} split song(s) from {} file(s)",
                    index.song_count(),
                    files.len()
                );
                for (basename, chosen) in index.describe() {
                    log_info!(
                        "SplitSsqAutoDiscovery:   {}",
                        resolver::describe_row(&basename, &chosen)
                    );
                }
                if let Ok(mut g) = INDEX.write() {
                    *g = Some(Arc::new(index));
                    return true;
                }
                log_warn!("SplitSsqAutoDiscovery: index lock poisoned -- passthrough");
                false
            }
            Err(e) => {
                if let Ok(mut g) = INDEX.write() {
                    *g = None;
                }
                log_warn!(
                    "SplitSsqAutoDiscovery: discovery failed ({}) -- stock path builder in effect",
                    e
                );
                false
            }
        }
    }
}

impl Mod for SplitSsqAutoDiscoveryMod {
    fn id(&self) -> &str {
        "split-ssq-auto-discovery"
    }

    fn name(&self) -> &str {
        "Split SSQ Auto-Discovery"
    }

    fn description(&self) -> &str {
        "Discovers split chart files (<song>_N.ssq) on disk instead of the game's hardcoded per-build table"
    }

    fn required_signatures(&self) -> &[&str] {
        &["build_ssq_path"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        self.builder_addr = ctx.signatures.require_address("build_ssq_path");
        true
    }

    fn enable(&mut self) {
        // Index FIRST so the very first builder call (the boot pass, ~7200
        // synchronous calls inside CheckStepDataActor::onInit) is answered
        // from a complete table (design R5).
        let indexed = Self::rebuild_index();

        if !HOOK_INSTALLED.load(Ordering::Acquire) {
            let target: BuildSsqPathFn = unsafe { std::mem::transmute(self.builder_addr) };
            if let Err(error) = unsafe {
                hooks::install_enabled(std::ptr::addr_of_mut!(HOOK), target, build_ssq_path_hook)
            } {
                log_warn!(
                    "SplitSsqAutoDiscovery: build_ssq_path hook installation failed: {} -- mod inactive",
                    error
                );
                return;
            }
            HOOK_INSTALLED.store(true, Ordering::Release);
        }

        ACTIVE.store(true, Ordering::Release);
        log_info!(
            "SplitSsqAutoDiscovery: enabled ({})",
            if indexed {
                "runtime discovery in effect"
            } else {
                "no index -- passthrough to stock"
            }
        );
    }

    fn disable(&mut self) {
        ACTIVE.store(false, Ordering::Release);
        log_info!("SplitSsqAutoDiscovery: disabled (stock path builder passthrough)");
    }

    fn is_active(&self) -> bool {
        HOOK_INSTALLED.load(Ordering::Acquire)
    }
}
