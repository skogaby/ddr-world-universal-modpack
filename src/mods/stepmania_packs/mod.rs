//! StepMania Packs — the thin `Mod` over `services::stepmania` (design §4.15).
//!
//! Everything that matters happens at step 0' (`stepmania::early_init`, see
//! the service doc): the prep tool's `index.bin` is loaded, its validation
//! started, and the `startup.arc` musicdb transform, the resolver and the
//! folder/series registration are armed long before the mod registry
//! exists, because the game reads `musicdb.xml` within ms of gamemdx
//! loading. This mod exists so the loader is a visible, togglable
//! entry in the mod menu and so the slow-cabinet ordering is covered: its
//! `init` runs at step 7 BEFORE `series_expansion` / `folder_expansion`
//! initialise (list order in `lib.rs`) and calls `ensure_registration`,
//! which — when the game's `startup.arc` open has not happened yet — waits
//! (bounded) for the index and registers the STEPMANIA bit/series so those
//! two mods merge them into their config. On a fast cabinet the open already
//! did that and the call returns at once.
//!
//! Boot-only semantics: `early_init` is config-gated, so toggling the mod in
//! the menu takes effect at the next launch. `is_active` therefore reports
//! "this mod CAN work" — the songs root exists and LayeredFS is up — not
//! whether songs were injected this boot (AGENTS.md's `is_active` rule).
//! `enable` applies the fail-closed visibility guards (hide everything when
//! score sanitisation is unavailable; hide per-chart-timing songs when the
//! `ssq_path` chain is not up) — deferred to the first scene change when the
//! music DB is not built yet — and loads the auto-sync ledger when
//! `auto_sync` is on. `disable` only logs. No signatures are required.

use crate::log_info;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::{avs_layeredfs, scroll_remap, stepmania};

pub struct StepmaniaPacksMod;

impl StepmaniaPacksMod {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StepmaniaPacksMod {
    fn default() -> Self {
        Self::new()
    }
}

impl Mod for StepmaniaPacksMod {
    fn id(&self) -> &str {
        stepmania::MOD_ID
    }

    fn name(&self) -> &str {
        "StepMania Packs"
    }

    fn description(&self) -> &str {
        "Loads StepMania packs from stepmania/Songs/ as playable songs (no score submission); toggle applies at next boot"
    }

    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        stepmania::hide::resolve(ctx.signatures);
        if !stepmania::is_armed() {
            log_info!("StepmaniaPacks: loader not armed this boot (disabled in config or root unavailable)");
            return true;
        }
        if avs_layeredfs::is_available() {
            // Slow-cabinet direction: the game's startup.arc open may still be
            // ahead of us, and series/folder expansion initialise right after
            // this. Registering here (bounded wait) keeps both consumers in
            // agreement whichever comes first.
            let _ = stepmania::ensure_registration();
        }
        // Prefetch scheduler triggers (card-bound → preview/jacket jobs,
        // highlight dwell → speculative full bank). Every service it
        // subscribes to is up by step 7.
        stepmania::init_runtime();
        log_info!(
            "StepmaniaPacks: scroll gimmicks {} (get_offset_y remap {})",
            if stepmania::scroll_gimmicks_enabled() {
                "on"
            } else {
                "off (config)"
            },
            if scroll_remap::is_available() {
                "installed"
            } else {
                "not installed"
            }
        );
        match stepmania::status() {
            Some(s) if s.songs.is_empty() && s.validated && s.dropped == 0 => log_info!(
                "StepmaniaPacks: no index -- run {} before the game to load packs from {}",
                stepmania::PREP_TOOL,
                stepmania::SONGS_ROOT
            ),
            Some(s) => log_info!(
                "StepmaniaPacks: index {} -- {} song(s), {} pack(s), {} cached rejection(s), {} dropped by validation; series {:?}, folder bit {:?}",
                if s.validated { "validated" } else { "still validating" },
                s.songs.len(),
                s.packs,
                s.rejected,
                s.dropped,
                stepmania::registered_series(),
                stepmania::registered_folder_bit()
            ),
            None => log_info!("StepmaniaPacks: loader not armed"),
        }
        true
    }

    fn enable(&mut self) {
        // A menu toggle-on from a boot where the loader was off: make the root
        // exist now so `is_active` (and the menu's ON state) reflect the intent;
        // the loader itself arms at the next boot.
        let _ = std::fs::create_dir_all(stepmania::SONGS_ROOT);
        if stepmania::is_armed() {
            let entries = stepmania::injected_entries();
            if entries.is_empty() && stepmania::registered_series().is_some() {
                // Slow-cabinet direction: the startup.arc open (and with it
                // the music DB) is still ahead of us. Apply the guards at the
                // first scene change instead — that precedes the first
                // active-list rebuild, so nothing is ever listed unguarded.
                defer_visibility_guards();
            } else {
                apply_visibility_guards(&entries);
            }
            // Auto-sync (`stepmania_packs.auto_sync`): load the ledger the
            // prep tool wrote and register the judgement-offset provider.
            if !stepmania::auto_sync_enabled() {
                log_info!("StepmaniaPacks: auto-sync off (config) -- songs play as authored");
            } else {
                let _ = stepmania::init_sync();
            }
        } else {
            log_info!("StepmaniaPacks: enabled -- takes effect at the next boot");
        }
    }

    fn disable(&mut self) {
        log_info!("StepmaniaPacks: disabled -- injected songs stay until the next boot");
    }

    fn is_active(&self) -> bool {
        stepmania::root_exists() && avs_layeredfs::is_available()
    }
}

/// Fail-closed visibility for the songs in the music DB: hide them all when
/// score sanitisation is unavailable (F12), and hide per-chart-timing songs
/// when the `ssq_path` chain is not up (the game would ask for a `<b>.ssq`
/// that does not exist).
fn apply_visibility_guards(entries: &[std::sync::Arc<stepmania::model::SongEntry>]) {
    let mcodes: Vec<u32> = entries.iter().map(|e| e.mcode).collect();
    stepmania::hide::enforce(&mcodes);
    // Records the background validation has dropped so far (later drops are
    // hidden by the validator itself now that the DB exists).
    let dropped = stepmania::take_validation_drops();
    if !dropped.is_empty() {
        stepmania::hide::hide_unservable(&dropped, "changed or missing since the last prep run");
    }
    if !stepmania::ssq_split::is_registered() {
        let split: Vec<u32> = entries
            .iter()
            .filter(|e| e.has_split_files())
            .map(|e| e.mcode)
            .collect();
        stepmania::hide::hide_unservable(
            &split,
            "ssq_path chain unavailable; per-chart timing songs cannot be served",
        );
    }
    log_info!(
        "StepmaniaPacks: {} song(s) in the music DB this boot{}",
        entries.len(),
        if stepmania::hide::hidden_for_safety() {
            " -- HIDDEN, score sanitisation unavailable"
        } else {
            ""
        }
    );
}

/// Run [`apply_visibility_guards`] once, at the first scene change after
/// the injection. Without the scene service there is no later hook: the
/// guards run now on whatever is known, and one WARN says so.
fn defer_visibility_guards() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static APPLIED: AtomicBool = AtomicBool::new(false);
    if !crate::services::scene_manager::is_available() {
        crate::log_warn!(
            "StepmaniaPacks: music DB not built yet and no scene hook -- visibility guards run on an empty list"
        );
        apply_visibility_guards(&stepmania::injected_entries());
        return;
    }
    log_info!("StepmaniaPacks: music DB not built yet -- visibility guards deferred to the first scene change");
    crate::services::scene_manager::on_scene_change(Box::new(|_prev, _next| {
        let entries = stepmania::injected_entries();
        if entries.is_empty() || APPLIED.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = std::panic::catch_unwind(|| apply_visibility_guards(&entries));
    }));
}
