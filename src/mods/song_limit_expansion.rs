//! Song Limit Expansion Mod — Increases the XML property arena from 1 MiB
//! to 64 MiB.
//!
//! The game allocates a fixed 1 MiB arena for the AVS property tree it
//! parses `musicdb.xml` into (and `coursedb.xml`, `license.xml`;
//! `docs/stepmania_runtime_packs_feasibility.md` §4.1). The tree costs
//! roughly the text size — the stock 716 KB file fits — so 1 MiB caps out
//! around 2,000 songs. This mod patches the size to 64 MiB; the arena comes
//! from the AGCS app heap (OS-heap backed) and is freed right after the
//! parse, so the cost is transient. The size is deliberately far above the
//! StepMania loader's injection ceiling (`services::stepmania::ceiling`) so
//! the ceiling can rise with testing without touching this patch.
//!
//! Each parser has two patch sites:
//!   1. MOV EDX, 0x100000       (allocation size)
//!   2. MOV [RSP+0x20], 0x100000 (arena size passed to the parser)
//!
//! The patch rewrites the full imm32 at each site: 0x00100000 → 0x04000000.
//!
//! ## When the patch lands
//!
//! `musicdb.xml` is parsed by `Application::onBoot` within milliseconds of
//! `gamemdx` loading — usually BEFORE this DLL's full signature scan finishes,
//! so `early_apply` (which runs after `resolve_all`) can lose the race on a
//! fast cabinet. The race-free moment is the game's open of
//! `data/arc/startup.arc`: that open comes from `onBoot` itself (so `gamemdx`
//! is mapped), and `createList` parses a member of the very arc being opened,
//! so it cannot have run yet. [`register_startup_arc_hook`] (called from
//! `lib.rs` before LayeredFS installs) registers an `arc_handler` pre-repack
//! hook that applies the patch there, config-gated on this mod being enabled.
//! `early_apply` / `init` then find it already applied and adopt the sites so
//! the menu toggle's `disable` still rolls the bytes back.
//!
//! All three entry points share one module-level patch state
//! ([`apply_arena_patch`]), so the patch is applied at most once per process.

use std::sync::Mutex;

use crate::core::memory;
use crate::core::module_resolver::GameModule;
use crate::core::scanner::{scan_pattern_all, ScanResult};
use crate::mods::mod_trait::{mod_enabled_in_config, EarlyContext, Mod, ModContext};
use crate::services::stepmania::ceiling;
use crate::{log_info, log_warn};

pub const MOD_ID: &str = "song-limit-expansion";

/// Stock arena size (the imm32 at every site) — shared with the ceiling.
pub const STOCK_ARENA_BYTES: u32 = ceiling::STOCK_ARENA_BYTES as u32;
/// Patched arena size — shared with the ceiling.
pub const EXPANDED_ARENA_BYTES: u32 = ceiling::EXPANDED_ARENA_BYTES as u32;

/// XOR R8D,R8D; MOV EDX,0x100000; CALL ...
/// The imm32 starts at pattern offset 4.
const ALLOC_PATTERN: &str = "45 33 C0 BA 00 00 10 00 E8";
const ALLOC_PATCH_OFFSET: usize = 4;

/// MOV dword ptr [RSP+0x20], 0x100000
/// The imm32 starts at pattern offset 4.
const READ_PATTERN: &str = "C7 44 24 20 00 00 10 00";
const READ_PATCH_OFFSET: usize = 4;

const EXPECTED_HITS: usize = 3; // license, musicdb, coursedb

#[derive(Clone, Copy)]
struct PatchSite {
    /// Address of the imm32.
    addr: *mut u8,
}

unsafe impl Send for PatchSite {}

/// Where the patch currently stands for this process.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArenaPatchState {
    /// Never attempted.
    Untried,
    /// Scan/verify failed; bytes untouched. Not retried.
    Failed,
    /// Sites known and the 0x80 bytes written.
    Applied,
    /// Sites known, bytes restored to 0x10 (menu toggle off).
    Reverted,
}

struct ArenaPatch {
    state: ArenaPatchState,
    sites: Vec<PatchSite>,
    /// Who applied it first, for the log.
    applied_by: &'static str,
}

static ARENA_PATCH: Mutex<ArenaPatch> = Mutex::new(ArenaPatch {
    state: ArenaPatchState::Untried,
    sites: Vec::new(),
    applied_by: "",
});

/// Current state of the arena patch.
pub fn arena_patch_state() -> ArenaPatchState {
    ARENA_PATCH
        .lock()
        .map(|p| p.state)
        .unwrap_or(ArenaPatchState::Failed)
}

/// The arena size the game's parsers use right now: expanded when the patch
/// is applied, stock otherwise (never attempted, failed, or reverted).
#[must_use]
pub fn arena_bytes() -> usize {
    match arena_patch_state() {
        ArenaPatchState::Applied => EXPANDED_ARENA_BYTES as usize,
        _ => STOCK_ARENA_BYTES as usize,
    }
}

/// Scan, verify and write the six patch sites once. Idempotent: a second call
/// returns the existing outcome without rescanning. `who` names the caller in
/// the log line. Returns `true` when the bytes are (now) patched.
pub fn apply_arena_patch(module: &GameModule, who: &'static str) -> bool {
    let mut patch = match ARENA_PATCH.lock() {
        Ok(p) => p,
        Err(_) => return false,
    };
    match patch.state {
        ArenaPatchState::Applied => return true,
        ArenaPatchState::Failed => return false,
        ArenaPatchState::Reverted => {
            write_sites(&patch.sites, EXPANDED_ARENA_BYTES);
            patch.state = ArenaPatchState::Applied;
            log_info!(
                "SongLimitExpansion: re-applied by {} ({} patches)",
                who,
                patch.sites.len()
            );
            return true;
        }
        ArenaPatchState::Untried => {}
    }
    match scan_and_verify(module.base, module.size) {
        Some(sites) => {
            write_sites(&sites, EXPANDED_ARENA_BYTES);
            log_info!(
                "SongLimitExpansion: applied by {} — XML arenas expanded to {} MiB ({} patches)",
                who,
                EXPANDED_ARENA_BYTES >> 20,
                sites.len()
            );
            patch.sites = sites;
            patch.state = ArenaPatchState::Applied;
            patch.applied_by = who;
            true
        }
        None => {
            patch.state = ArenaPatchState::Failed;
            false
        }
    }
}

/// Restore the stock bytes (menu toggle off). No-op unless applied.
fn revert_arena_patch() {
    if let Ok(mut patch) = ARENA_PATCH.lock() {
        if patch.state == ArenaPatchState::Applied {
            write_sites(&patch.sites, STOCK_ARENA_BYTES);
            patch.state = ArenaPatchState::Reverted;
            log_info!("SongLimitExpansion: disabled — XML arenas restored to 1 MiB");
        }
    }
}

/// Register the race-free application point: `arc_handler`'s pre-repack hook
/// for `arc/startup.arc`. Call from `lib.rs` BEFORE `avs_layeredfs::init()`.
/// Config-gated at hook time (the config store is loaded at step 0).
pub fn register_startup_arc_hook() {
    crate::services::avs_layeredfs::arc_handler::register_pre_repack_hook(
        "arc/startup.arc",
        startup_arc_pre_repack,
    );
}

fn startup_arc_pre_repack() {
    let enabled = crate::mods::config::get()
        .map(|c| mod_enabled_in_config(&c.mods, MOD_ID))
        .unwrap_or(true);
    if !enabled {
        return;
    }
    match crate::core::module_resolver::get_game_module() {
        Some(module) => {
            // Which side of the race this cabinet is on is worth one line:
            // a fast cabinet reaches this open before early_apply, a slow
            // (CrossOver) one after it.
            if arena_patch_state() == ArenaPatchState::Applied {
                log_info!(
                    "SongLimitExpansion: startup.arc open — arena patch already in place"
                );
                return;
            }
            if !apply_arena_patch(&module, "startup.arc open") {
                log_warn!(
                    "SongLimitExpansion: arena patch unavailable at the startup.arc open — the StepMania loader injects nothing under a stock arena"
                );
            }
        }
        None => log_warn!(
            "SongLimitExpansion: startup.arc opened before gamemdx was resolvable — arena patch deferred to early_apply (racy; the StepMania loader injects nothing this boot)"
        ),
    }
}

/// Scan-and-verify the 6 patch sites. Returns `None` on any failure (wrong
/// hit count, unexpected byte); logs the reason.
fn scan_and_verify(base: *const u8, size: usize) -> Option<Vec<PatchSite>> {
    let alloc_hits = scan_pattern_all(base, size, ALLOC_PATTERN);
    if alloc_hits.len() != EXPECTED_HITS {
        log_warn!(
            "SongLimitExpansion: expected {} alloc sites, found {} — aborting",
            EXPECTED_HITS,
            alloc_hits.len()
        );
        return None;
    }

    let read_hits = scan_pattern_all(base, size, READ_PATTERN);
    if read_hits.len() != EXPECTED_HITS {
        log_warn!(
            "SongLimitExpansion: expected {} read sites, found {} — aborting",
            EXPECTED_HITS,
            read_hits.len()
        );
        return None;
    }

    let mut sites = Vec::with_capacity(EXPECTED_HITS * 2);
    record_sites(&mut sites, &alloc_hits, ALLOC_PATCH_OFFSET)?;
    record_sites(&mut sites, &read_hits, READ_PATCH_OFFSET)?;

    log_info!(
        "SongLimitExpansion: found {} patch sites ({} MiB → {} MiB)",
        sites.len(),
        STOCK_ARENA_BYTES >> 20,
        EXPANDED_ARENA_BYTES >> 20
    );
    Some(sites)
}

/// Verify each hit holds the stock imm32 at `patch_offset` and record a
/// `PatchSite` so the patch can be rolled back. `None` on the first
/// unexpected value (abort to avoid corrupting unknown memory).
fn record_sites(
    sites: &mut Vec<PatchSite>,
    hits: &[ScanResult],
    patch_offset: usize,
) -> Option<()> {
    for hit in hits {
        let addr = unsafe { hit.address.add(patch_offset) as *mut u8 };
        let val = unsafe { memory::read_u32(addr) };
        if val != STOCK_ARENA_BYTES {
            log_warn!(
                "SongLimitExpansion: unexpected imm32 0x{:08X} at site +0x{:X}",
                val,
                hit.offset
            );
            return None;
        }
        sites.push(PatchSite { addr });
    }
    Some(())
}

fn write_sites(sites: &[PatchSite], value: u32) {
    for site in sites {
        unsafe {
            let old = memory::make_writable(site.addr as *const u8, 4);
            // Code immediates are unaligned.
            (site.addr as *mut u32).write_unaligned(value);
            memory::restore_protection(site.addr as *const u8, 4, old);
        }
    }
}

pub struct SongLimitExpansionMod;

impl SongLimitExpansionMod {
    pub fn new() -> Self {
        Self
    }
}

impl Mod for SongLimitExpansionMod {
    fn id(&self) -> &str {
        MOD_ID
    }
    fn name(&self) -> &str {
        "Song Limit Expansion"
    }
    fn description(&self) -> &str {
        "Increases the number of songs that can be loaded by ~64x"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn early_apply(&mut self, ctx: &EarlyContext) -> bool {
        // Normally the startup.arc pre-repack hook has already landed the
        // patch; this is the fallback when LayeredFS missed that open.
        if arena_patch_state() == ArenaPatchState::Applied {
            log_info!("SongLimitExpansion: early_apply — already applied at the startup.arc open");
            return true;
        }
        apply_arena_patch(ctx.game_module, "early_apply")
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        match arena_patch_state() {
            ArenaPatchState::Applied | ArenaPatchState::Reverted => {
                log_info!("SongLimitExpansion: init() — sites already known");
                true
            }
            ArenaPatchState::Failed => false,
            // Config-gated off at boot, or the menu re-enabled it after a
            // fresh boot: resolve the sites now so enable() can write them.
            ArenaPatchState::Untried => {
                let applied = apply_arena_patch(ctx.game_module, "init");
                if applied {
                    // init must not change bytes before enable(): undo the
                    // write but keep the sites.
                    revert_arena_patch();
                }
                applied
            }
        }
    }

    fn enable(&mut self) {
        if let Some(module) = crate::core::module_resolver::get_game_module() {
            apply_arena_patch(&module, "enable");
        }
    }

    fn disable(&mut self) {
        revert_arena_patch();
    }
}
