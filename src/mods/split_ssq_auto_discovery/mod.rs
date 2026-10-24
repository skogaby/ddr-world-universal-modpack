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
//! A consumer of `services::ssq_path` (the ONE owner of the `build_ssq_path`
//! detour) at priority 20 — behind the StepMania loader's injected-basename
//! resolver (10), ahead of the game's own table. At `enable()` the mod scans
//! the stock SSQ directory plus every LayeredFS mod folder for `_N` files,
//! reads each one's type-3 level set, builds a Rule-A index (`resolver.rs`:
//! highest `N ≤ d+1` whose file contains a level-`d` chart, else the unsplit
//! file) and registers its resolver. Every game consumer — boot analysis
//! pass, normal/matching play, course preload — goes through the builder, so
//! the one detour covers them all. The resolver is basename-OPAQUE (no
//! musicdb lookup), which preserves the game's `toho1..toho4`
//! randomized-basename special case byte-for-byte; an indexed-but-unknown
//! basename answers the unsplit path (what stock does for an unknown song),
//! so with this mod active the original is never consulted for an answer.
//!
//! ## Divergence oracle
//!
//! While enabled, the mod turns on the service's oracle: each answered call
//! also runs the ORIGINAL into a scratch buffer and logs one INFO per
//! distinct `(basename, d)` whose answer differs (cap 64/session; prefix
//! `SsqPath:`). A matched binary/data pair logs nothing (or only `sabm d=4`,
//! whose `_3`/`_5` Challenge chunks are byte-identical); the target scenario
//! logs one line per newly-discovered split chart.
//!
//! ## Degradation (all fail-open)
//!
//! Service unavailable (signature miss — also covers third-party hex-edited
//! 20250805 DLLs) ⇒ mod unregistered. Stock dir unlistable ⇒ no index, the
//! resolver answers `None` (original in effect), one WARN. Bad args /
//! oversize basename ⇒ original (service guards). Runtime disable ⇒ the
//! resolver answers `None` (the detour is never uninstalled — one detour per
//! target); re-enable rescans.

pub mod discovery;
pub mod resolver;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::ssq_path;
use crate::{log_info, log_warn};
use resolver::{Choice, Index};

/// Consumer priority in the `ssq_path` chain (after the StepMania loader's 10).
pub const PRIORITY: u8 = 20;

/// Runtime toggle: false ⇒ the resolver answers `None` for every call.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Registered with the service (once per process).
static REGISTERED: AtomicBool = AtomicBool::new(false);
/// The Rule-A index, replaced wholesale at each enable. `None` ⇒ passthrough.
static INDEX: RwLock<Option<Arc<Index>>> = RwLock::new(None);

fn current_index() -> Option<Arc<Index>> {
    // Poisoned ⇒ treat as absent.
    INDEX.read().ok().and_then(|g| g.clone())
}

/// The `ssq_path` resolver: Rule-A answer for every basename while active
/// and indexed; `None` (original) otherwise. Panic-free (infallible lookup).
fn resolve(basename: &[u8], d: usize) -> Option<Choice> {
    if !ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    let index = current_index()?;
    Some(index.resolve(basename, d))
}

pub struct SplitSsqAutoDiscoveryMod;

impl SplitSsqAutoDiscoveryMod {
    pub fn new() -> Self {
        Self
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
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        if !ssq_path::is_available() {
            log_warn!(
                "SplitSsqAutoDiscovery: ssq_path service unavailable (build_ssq_path unresolved) -- mod not registered"
            );
            return false;
        }
        true
    }

    fn enable(&mut self) {
        // Index FIRST so the very first builder call (the boot pass, ~7200
        // synchronous calls inside CheckStepDataActor::onInit) is answered
        // from a complete table (design R5).
        let indexed = Self::rebuild_index();

        if !REGISTERED.load(Ordering::Acquire) {
            if !ssq_path::register(PRIORITY, resolve) {
                log_warn!("SplitSsqAutoDiscovery: resolver registration failed -- mod inactive");
                return;
            }
            REGISTERED.store(true, Ordering::Release);
        }

        ssq_path::set_divergence_oracle(true);
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
        ssq_path::set_divergence_oracle(false);
        log_info!("SplitSsqAutoDiscovery: disabled (stock path builder passthrough)");
    }

    fn is_active(&self) -> bool {
        REGISTERED.load(Ordering::Acquire) && ssq_path::is_installed()
    }
}
