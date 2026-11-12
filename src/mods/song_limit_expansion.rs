//! Song Limit Expansion Mod — lifts the five fixed limits that cap how
//! many songs the game can load: the XML property arena (1 MiB → 64 MiB),
//! the AVS property node cap (65,535 → unlimited, via `PROPERTY_MODE_EXTEND`),
//! the step-data record pool (4,096 → 131,072 records), the arc loader's
//! chunk (512 KiB → 4 MiB) and the gs texture table (4,096 → 32,768).
//!
//! ## The gs texture table
//!
//! `Renderer::initGs` sizes the graphics system's texture table — and the
//! ResourceManager's TextureData pool — from one config field, `0x1000`.
//! Every wheel thumbnail is one resident texture (the series thumbnail arcs
//! are loaded whole at boot by `ThumbnailLoadActor`), so ~1,500 stock
//! thumbnails plus the UI leave ~2,500 slots for injected songs. When the
//! table is full `gs` texture creation returns handle 0 and the DDS
//! callback registers it anyway: `FUN_18022f220` → `FUN_180237630(0)` →
//! NULL → AV on the step-data I/O worker, which libavs's thread-task
//! handler turns into a fatal re-raise (cabinet run 2026-10-26, 14,138
//! thumbnails, crash at the HARDWARE_CHECK → PRE_SPLASH transition). A
//! handle is `slot << 17 | gen << 4 | 7`, so 32,768 slots is the format's
//! ceiling; the table costs 0xA0 bytes per slot. Patched from `ea3_boot`
//! like the record pool (initGs runs from the game's main entry, after
//! `onBoot`).
//!
//! ## The arc loader chunk
//!
//! `agcs::ArchiveFileCallback` streams every arc through a pool of four
//! fixed-size chunks and parses the header — the 16-byte cue table and the
//! name table — straight out of the FIRST chunk, with no bounds check. A
//! series thumbnail arc with ~10,500 members overflows the stock 512 KiB
//! chunk, so the member names it copies (`FUN_1802061a0` → the strlen at
//! `+0x343D`) read past the buffer and the boot dies at the first thumbnail
//! load (cabinet run 2026-10-26, 14,138 members, 750 KB header, at the
//! HARDWARE_CHECK → PRE_SPLASH transition). The chunk size and the pool
//! allocation are two immediates in the callback's ctor, run once from
//! `Application::onBoot` — the same `ea3_boot` timing as the record pool.
//! Every consumer reads the chunk-size global, so 4 MiB chunks (16 MiB
//! pool, CRT heap) cost memory only; the StepMania ceiling keeps the
//! thumbnail arc's header inside whatever chunk is live.
//!
//! ## The property node cap
//!
//! Each parser hands `property_mem_read` the mode `0x11` (`READ |
//! DUPNODE`). A property created without `PROPERTY_MODE_EXTEND` (0x1000)
//! refuses the 65,536th node with `AVS_ERROR_PROP_NODE_OVER_65535`
//! (0x80092143, logged by the game as `W:prop: read error by mem[…]`), the
//! parse returns NULL and `createList` builds an EMPTY music DB — the
//! step-data actor then reads its work list through a null pointer and the
//! boot dies (cabinet run 2026-10-26, 14,138 injected songs; the stock
//! document is ~16,200 nodes and each injected entry 12, so the cap sits
//! near 3,800 songs). The mode load is `MOV R8D, 0x11` a few bytes before
//! each READ site; the patch ORs the extend flag in. Extended mode costs 4
//! extra bytes per node plus a node-name pool — well inside the 64 MiB
//! arena.
//!
//! ## The step-data record pool
//!
//! `Application::onBoot` constructs the step-data manager (`DAT_1806f2f48`,
//! the loader every SSQ goes through) from a parameter block whose record
//! capacity is the literal `0x1000`. `CheckStepDataActor::onInit` registers
//! one record per SSQ file of every song in the music DB, and the register
//! function has NO guard for an exhausted free list: the 4,097th distinct
//! file writes a 0xA0-byte name record and a 0x40-byte record at index −1 —
//! straight into the heap block header before the arrays — and the work
//! list that follows is built on a vector whose allocation then fails
//! (begin NULL). Symptom: "NOW LOADING 0%" forever (cabinet run 2026-10-26,
//! 14,138 songs). Stock + ~1,400 injected songs stayed under the cap, which
//! is why no earlier library hit it. The three `MOV dword ptr [RBP+d], imm32`
//! writes that fill the parameter block (`0x7E`, `-1`, `0x1000`) are one
//! unique byte pattern in every supported `gamemdx` build.
//!
//! **Timing**: the manager exists before the game opens `startup.arc`, so
//! the arena patch's hook point is too late for this one. It is applied from
//! `ident_override`'s `ea3_boot` detour — the launcher has mapped `gamemdx`
//! but not yet run any of it — or by `early_apply` when the DLL's signature
//! scan finishes before the launcher even reaches `ea3_boot` (the normal
//! fast-machine case; the application counts as early while the detour has
//! not yet seen the boot). If that moment was missed (the DLL loaded after
//! the boot, or the pattern failed), the patch is still written for the
//! NEXT boot but reports the stock pool for this one, and the StepMania
//! ceiling (`services::stepmania::ceiling`) caps the injected file count to
//! what the live pool can hold — the game never overflows it.
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
//! Each parser has three patch sites:
//!   1. MOV EDX, 0x100000       (allocation size)
//!   2. MOV [RSP+0x20], 0x100000 (arena size passed to the parser)
//!   3. MOV R8D, 0x11            (property mode; derived from site 2)
//!
//! The patch rewrites the full imm32 at each site: 0x00100000 → 0x04000000
//! and 0x11 → 0x1011. Sites 1–2 are mandatory; if site 3 cannot be derived
//! the arena patch still applies and `property_extended()` reports `false`
//! so the StepMania ceiling keeps the injected count under the node cap.
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
use crate::core::song_limit_sites as sites;
use crate::mods::mod_trait::{mod_enabled_in_config, EarlyContext, Mod, ModContext};
use crate::services::stepmania::ceiling;
use crate::{log_info, log_warn};

pub const MOD_ID: &str = "song-limit-expansion";

/// Stock arena size (the imm32 at every site) — shared with the ceiling.
pub const STOCK_ARENA_BYTES: u32 = ceiling::STOCK_ARENA_BYTES as u32;
/// Patched arena size — shared with the ceiling.
pub const EXPANDED_ARENA_BYTES: u32 = ceiling::EXPANDED_ARENA_BYTES as u32;

/// Stock step-data record pool (the imm32 in the parameter block).
pub const STOCK_RECORD_POOL: u32 = ceiling::STOCK_RECORD_POOL as u32;
/// Patched record pool.
pub const EXPANDED_RECORD_POOL: u32 = ceiling::EXPANDED_RECORD_POOL as u32;

/// `PROPERTY_MODE_EXTEND` (libavs `avs-property.h`): lifts the 65,535-node
/// cap (and the 36-char node-name limit) at the cost of 4 bytes per node
/// plus a separate node-name pool.
const PROPERTY_MODE_EXTEND: u32 = 0x1000;
/// Stock `property_mem_read` mode at every parser site.
pub const STOCK_PROPERTY_MODE: u32 = sites::STOCK_PROPERTY_MODE;
/// Patched mode: stock plus the extend flag.
pub const EXPANDED_PROPERTY_MODE: u32 = STOCK_PROPERTY_MODE | PROPERTY_MODE_EXTEND;

/// Stock arc-loader chunk — shared with the ceiling.
pub const STOCK_ARC_CHUNK_BYTES: u32 = ceiling::STOCK_ARC_CHUNK_BYTES as u32;
/// Patched chunk — shared with the ceiling.
pub const EXPANDED_ARC_CHUNK_BYTES: u32 = ceiling::EXPANDED_ARC_CHUNK_BYTES as u32;

/// Stock GS texture table (and ResourceManager TextureData pool) — shared
/// with the ceiling.
pub const STOCK_TEXTURE_SLOTS: u32 = ceiling::STOCK_TEXTURE_SLOTS as u32;
/// Patched table — shared with the ceiling.
pub const EXPANDED_TEXTURE_SLOTS: u32 = ceiling::EXPANDED_TEXTURE_SLOTS as u32;

#[derive(Clone, Copy)]
struct PatchSite {
    /// Address of the imm32.
    addr: *mut u8,
    /// The value the game shipped with (restored on disable).
    stock: u32,
    /// The value the patch writes.
    expanded: u32,
}

unsafe impl Send for PatchSite {}

/// Where the patch currently stands for this process.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArenaPatchState {
    /// Never attempted.
    Untried,
    /// Scan/verify failed; bytes untouched. Not retried.
    Failed,
    /// Sites known and the expanded values written.
    Applied,
    /// Sites known, stock values restored (menu toggle off).
    Reverted,
}

struct ArenaPatch {
    state: ArenaPatchState,
    sites: Vec<PatchSite>,
    /// Whether the three `property_mem_read` mode sites are among `sites`
    /// (the arena patch stands on its own when they could not be derived).
    mode_sites: bool,
    /// Who applied it first, for the log.
    applied_by: &'static str,
}

static ARENA_PATCH: Mutex<ArenaPatch> = Mutex::new(ArenaPatch {
    state: ArenaPatchState::Untried,
    sites: Vec::new(),
    mode_sites: false,
    applied_by: "",
});

/// Where a boot-time patch (record pool, arc chunk) stands for this process.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecordPoolState {
    Untried,
    /// Scan/verify failed; bytes untouched.
    Failed,
    /// Written before `Application::onBoot` ran: the live object has the
    /// expanded value.
    AppliedEarly,
    /// Written after the object was constructed: the bytes are patched for
    /// the next boot, but THIS boot's value is the stock one.
    AppliedLate,
}

/// A patch that must land before `Application::onBoot` constructs the
/// object it sizes (see the module doc): one `song_limit_sites` family
/// (unique AOB, stock values verified) plus the value each immediate is
/// rewritten to, in the family's `imms` order.
struct BootPatch {
    /// Log name.
    what: &'static str,
    spec: &'static sites::SiteSpec,
    expanded: &'static [u32],
    /// Shown in the log line as "stock → expanded".
    stock_desc: &'static str,
    expanded_desc: &'static str,
}

struct BootPatchState {
    state: RecordPoolState,
    sites: Vec<PatchSite>,
}

const RECORD_POOL_BOOT_PATCH: BootPatch = BootPatch {
    what: "step-data record pool",
    spec: &sites::RECORD_POOL,
    expanded: &[EXPANDED_RECORD_POOL],
    stock_desc: "4096 records",
    expanded_desc: "131072 records",
};

static RECORD_POOL_PATCH: Mutex<BootPatchState> = Mutex::new(BootPatchState {
    state: RecordPoolState::Untried,
    sites: Vec::new(),
});

const ARC_CHUNK_BOOT_PATCH: BootPatch = BootPatch {
    what: "arc loader chunk",
    spec: &sites::ARC_CHUNK,
    expanded: &[
        EXPANDED_ARC_CHUNK_BYTES,
        EXPANDED_ARC_CHUNK_BYTES * sites::ARC_CHUNK_SLOTS,
    ],
    stock_desc: "512 KiB chunks (2 MiB pool)",
    expanded_desc: "4 MiB chunks (16 MiB pool)",
};

static ARC_CHUNK_PATCH: Mutex<BootPatchState> = Mutex::new(BootPatchState {
    state: RecordPoolState::Untried,
    sites: Vec::new(),
});

const TEXTURE_SLOTS_BOOT_PATCH: BootPatch = BootPatch {
    what: "gs texture table",
    spec: &sites::TEXTURE_SLOTS,
    expanded: &[EXPANDED_TEXTURE_SLOTS],
    stock_desc: "4096 textures",
    expanded_desc: "32768 textures",
};

static TEXTURE_SLOTS_PATCH: Mutex<BootPatchState> = Mutex::new(BootPatchState {
    state: RecordPoolState::Untried,
    sites: Vec::new(),
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

/// Whether the music-DB parser creates its property tree with
/// `PROPERTY_MODE_EXTEND` right now (the arena patch is applied AND the
/// mode sites were derived). Without it the tree caps at 65,535 nodes.
#[must_use]
pub fn property_extended() -> bool {
    ARENA_PATCH
        .lock()
        .map(|p| p.state == ArenaPatchState::Applied && p.mode_sites)
        .unwrap_or(false)
}

/// Current state of the record-pool patch.
pub fn record_pool_state() -> RecordPoolState {
    boot_patch_state(&RECORD_POOL_PATCH)
}

/// The step-data record pool the LIVE manager has this boot: expanded only
/// when the patch landed before `onBoot`.
#[must_use]
pub fn record_pool() -> usize {
    match record_pool_state() {
        RecordPoolState::AppliedEarly => EXPANDED_RECORD_POOL as usize,
        _ => STOCK_RECORD_POOL as usize,
    }
}

/// Current state of the arc-chunk patch.
pub fn arc_chunk_state() -> RecordPoolState {
    boot_patch_state(&ARC_CHUNK_PATCH)
}

/// The arc loader's chunk size this boot — the most header (cue table +
/// name table) one arc may carry: expanded only when the patch landed
/// before `onBoot` constructed the `ArchiveFileCallback`.
#[must_use]
pub fn arc_chunk_bytes() -> usize {
    match arc_chunk_state() {
        RecordPoolState::AppliedEarly => EXPANDED_ARC_CHUNK_BYTES as usize,
        _ => STOCK_ARC_CHUNK_BYTES as usize,
    }
}

/// Current state of the texture-table patch.
pub fn texture_slots_state() -> RecordPoolState {
    boot_patch_state(&TEXTURE_SLOTS_PATCH)
}

/// GS texture slots this boot: expanded only when the patch landed before
/// `Renderer::initGs` built the table.
#[must_use]
pub fn texture_slots() -> usize {
    match texture_slots_state() {
        RecordPoolState::AppliedEarly => EXPANDED_TEXTURE_SLOTS as usize,
        _ => STOCK_TEXTURE_SLOTS as usize,
    }
}

fn boot_patch_state(slot: &Mutex<BootPatchState>) -> RecordPoolState {
    slot.lock()
        .map(|p| p.state)
        .unwrap_or(RecordPoolState::Failed)
}

/// Scan, verify and write the record-pool imm32 once (see
/// [`apply_boot_patch`]).
pub fn apply_record_pool_patch(module: &GameModule, who: &'static str, early: bool) -> bool {
    apply_boot_patch(
        &RECORD_POOL_BOOT_PATCH,
        &RECORD_POOL_PATCH,
        module,
        who,
        early,
    )
}

/// Scan, verify and write the arc-chunk immediates once (see
/// [`apply_boot_patch`]).
pub fn apply_arc_chunk_patch(module: &GameModule, who: &'static str, early: bool) -> bool {
    apply_boot_patch(&ARC_CHUNK_BOOT_PATCH, &ARC_CHUNK_PATCH, module, who, early)
}

/// Scan, verify and write the texture-table imm32 once (see
/// [`apply_boot_patch`]).
pub fn apply_texture_slots_patch(module: &GameModule, who: &'static str, early: bool) -> bool {
    apply_boot_patch(
        &TEXTURE_SLOTS_BOOT_PATCH,
        &TEXTURE_SLOTS_PATCH,
        module,
        who,
        early,
    )
}

/// Scan, verify and write a boot-time patch once. `early` is the caller's
/// knowledge that `Application::onBoot` has not run yet (the `ea3_boot`
/// detour passes `true`); any caller is upgraded to early while the detour
/// is installed and has not yet seen the boot, since the launcher reaches
/// `ea3_boot` before the game runs any of its own code. A late application
/// still writes the bytes (next boot) but is recorded as such. Idempotent,
/// except that the detour's own call upgrades an `AppliedLate` written
/// moments earlier (the boot had been observed, the object not yet built)
/// to `AppliedEarly`.
fn apply_boot_patch(
    desc: &BootPatch,
    slot: &Mutex<BootPatchState>,
    module: &GameModule,
    who: &'static str,
    early: bool,
) -> bool {
    use crate::services::ident_override;
    let mut patch = match slot.lock() {
        Ok(p) => p,
        Err(_) => return false,
    };
    // Evaluated under the lock: the detour sets `boot_observed` before it
    // runs its callbacks, and those callbacks take this same lock before
    // the original `ea3_boot` (and so `onBoot`) can run.
    let early = early || (ident_override::is_installed() && !ident_override::boot_observed());
    match patch.state {
        RecordPoolState::AppliedLate if early => {
            patch.state = RecordPoolState::AppliedEarly;
            log_info!(
                "SongLimitExpansion: {} patch confirmed before onBoot by {} — {} this boot",
                desc.what,
                who,
                desc.expanded_desc
            );
            return true;
        }
        RecordPoolState::AppliedEarly | RecordPoolState::AppliedLate => return true,
        RecordPoolState::Failed => return false,
        RecordPoolState::Untried => {}
    }
    let hits = match sites::find(module.base, module.size, desc.spec) {
        Ok(h) => h,
        Err(reason) => {
            log_warn!(
                "SongLimitExpansion: {} -- {} stays at {}",
                reason,
                desc.what,
                desc.stock_desc
            );
            patch.state = RecordPoolState::Failed;
            return false;
        }
    };
    let mut sites_out = Vec::with_capacity(desc.spec.imms.len());
    for hit in &hits {
        for (imm, &expanded) in desc.spec.imms.iter().zip(desc.expanded) {
            sites_out.push(PatchSite {
                addr: unsafe { hit.address.add(imm.offset) as *mut u8 },
                stock: imm.stock,
                expanded,
            });
        }
    }
    let sites = sites_out;
    write_sites(&sites, true);
    patch.sites = sites;
    patch.state = if early {
        RecordPoolState::AppliedEarly
    } else {
        RecordPoolState::AppliedLate
    };
    if early {
        log_info!(
            "SongLimitExpansion: applied by {} — {} {} → {}",
            who,
            desc.what,
            desc.stock_desc,
            desc.expanded_desc
        );
    } else {
        log_warn!(
            "SongLimitExpansion: {} patched by {} AFTER the game built its loader -- {} this boot, {} from the next; injected songs are capped accordingly",
            desc.what,
            who,
            desc.stock_desc,
            desc.expanded_desc
        );
    }
    true
}

/// Register the boot-time patches at the `ea3_boot` moment (see the module
/// doc). Call from `lib.rs` step 0, before the launcher boots ea3. Config-
/// gated at hook time.
pub fn register_ea3_boot_hook() {
    crate::services::ident_override::on_ea3_boot(ea3_boot_apply);
}

fn ea3_boot_apply(early: bool) {
    let enabled = crate::mods::config::get()
        .map(|c| mod_enabled_in_config(&c.mods, MOD_ID))
        .unwrap_or(true);
    if !enabled {
        return;
    }
    match crate::core::module_resolver::get_game_module() {
        Some(module) => {
            let who = if early { "ea3_boot" } else { "init" };
            apply_record_pool_patch(&module, who, early);
            apply_arc_chunk_patch(&module, who, early);
            apply_texture_slots_patch(&module, who, early);
        }
        None if early => log_warn!(
            "SongLimitExpansion: gamemdx not mapped at ea3_boot -- step-data record pool, arc chunk and texture table stay stock this boot (patched later for the next)"
        ),
        None => {}
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
            write_sites(&patch.sites, true);
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
        Some((sites, mode_sites)) => {
            write_sites(&sites, true);
            log_info!(
                "SongLimitExpansion: applied by {} — XML arenas expanded to {} MiB{} ({} patches)",
                who,
                EXPANDED_ARENA_BYTES >> 20,
                if mode_sites {
                    ", property trees in extended mode (no 65,535-node cap)"
                } else {
                    ""
                },
                sites.len()
            );
            patch.sites = sites;
            patch.mode_sites = mode_sites;
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
            write_sites(&patch.sites, false);
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

/// Scan-and-verify the 6 arena patch sites, then derive the 3
/// `property_mem_read` mode sites from the READ hits (`song_limit_sites`).
/// Returns `None` on any arena failure (wrong hit count, unexpected byte);
/// the mode sites are best-effort — the flag says whether all three were
/// found (any miss drops them all, so the three parsers stay consistent).
/// Logs the reason.
fn scan_and_verify(base: *const u8, size: usize) -> Option<(Vec<PatchSite>, bool)> {
    let mut all = Vec::with_capacity(9);
    let mut read_hits = Vec::new();
    for (spec, is_read) in [(&sites::ARENA_ALLOC, false), (&sites::ARENA_READ, true)] {
        match sites::find(base, size, spec) {
            Ok(hits) => {
                for hit in &hits {
                    for imm in spec.imms {
                        all.push(PatchSite {
                            addr: unsafe { hit.address.add(imm.offset) as *mut u8 },
                            stock: imm.stock,
                            expanded: EXPANDED_ARENA_BYTES,
                        });
                    }
                }
                if is_read {
                    read_hits = hits;
                }
            }
            Err(reason) => {
                log_warn!("SongLimitExpansion: {} — aborting", reason);
                return None;
            }
        }
    }

    let mut mode_sites = Vec::with_capacity(read_hits.len());
    for hit in &read_hits {
        match sites::find_mode_site(base, hit) {
            Some(addr) => mode_sites.push(PatchSite {
                addr: addr as *mut u8,
                stock: STOCK_PROPERTY_MODE,
                expanded: EXPANDED_PROPERTY_MODE,
            }),
            None => {
                log_warn!(
                    "SongLimitExpansion: no `MOV R8D, 0x11` within {} bytes before the read site at +0x{:X} — property trees stay in stock mode (65,535-node cap; the StepMania ceiling drops to {} songs)",
                    sites::MODE_SCAN_BACK,
                    hit.offset,
                    ceiling::node_song_budget()
                );
                mode_sites.clear();
                break;
            }
        }
    }
    let have_mode = mode_sites.len() == read_hits.len() && !read_hits.is_empty();
    all.extend(mode_sites);

    log_info!(
        "SongLimitExpansion: found {} patch sites ({} MiB → {} MiB{})",
        all.len(),
        STOCK_ARENA_BYTES >> 20,
        EXPANDED_ARENA_BYTES >> 20,
        if have_mode {
            ", mode 0x11 → 0x1011"
        } else {
            ""
        }
    );
    Some((all, have_mode))
}

/// Write every site's expanded (or stock) value.
fn write_sites(sites: &[PatchSite], expanded: bool) {
    for site in sites {
        let value = if expanded { site.expanded } else { site.stock };
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
        "Lifts the game's song-count limits (XML arena 1→64 MiB, property nodes 65,535→unlimited, step-data records 4,096→131,072)"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn early_apply(&mut self, ctx: &EarlyContext) -> bool {
        // The record pool: normally landed at ea3_boot, or here when the
        // scan beat the launcher to it (apply_record_pool_patch decides
        // early/late from the detour's state); late means "next boot".
        if record_pool_state() == RecordPoolState::Untried {
            apply_record_pool_patch(ctx.game_module, "early_apply", false);
        }
        if arc_chunk_state() == RecordPoolState::Untried {
            apply_arc_chunk_patch(ctx.game_module, "early_apply", false);
        }
        if texture_slots_state() == RecordPoolState::Untried {
            apply_texture_slots_patch(ctx.game_module, "early_apply", false);
        }
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
