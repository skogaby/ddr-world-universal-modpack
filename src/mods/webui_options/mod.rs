//! WebUI Options Mod — in-game pickers for the profile settings the stock game only lets a
//! player change through Konami's web portal: the customize cosmetics (APPEAL BOARD,
//! BACKGROUND ×2, CHARACTER P1/P2, LANE ×2, LANE COVER ×2), each with a live preview in the
//! options modal, plus the DISPLAY BURNED CALORIES / PLAYER WEIGHT workout-profile rows.
//!
//! Mod id `webui-options`, default ON (not in `DEFAULT_OFF_MODS`). It is one of the
//! `LATE_BINDING_MODS` in `src/mods/mod_trait.rs`: its `enable` does filesystem discovery and
//! preview-asset generation, so the registry enables it after every other mod. Nothing it
//! hooks is needed before the player reaches song select.
//!
//! ## Mechanism
//!
//! Zero detours. Every category is a field in the game's own per-player `Customize` object
//! (`PlayerWork + customize_offset + field`, holding a u32 asset id); the game's native
//! `<customize>` profile load fills it at card-in and the game applies it. This mod only adds
//! the missing edit + save directions:
//!
//! - **Discovery** (`discovery.rs`): at enable, each category's asset ids are scanned from
//!   `.arc` filenames on disk; a category with no assets gets no row.
//! - **Rows**: one `custom_options` scalar per category (game's native options menu only, not
//!   the overlay). The stored value is the 0-based index into the discovered id list; the
//!   display is a short prefix plus the 1-based position (`ScalarFormat::PrefixedIndex`, e.g.
//!   `Character #3`). No per-value textures.
//! - **Sync**: on every SONG_SELECT (scene 25) entry, `sync_registry_with_game` reads each
//!   side's `Customize` fields, reverse-maps id → index (index 0 when the id isn't present on
//!   this cabinet) and writes the menu registry with `set_value_silent`. With network
//!   persistence on it never writes game memory and fires no `on_change`; see the JSON
//!   fallback below for the one case where the direction flips.
//! - **Apply**: a menu edit (`on_change`) runs `try_apply_all`, which writes every category's
//!   selected asset id into `Customize` for that side.
//!
//! ## Persistence (`PersistMode::SaveOnly`)
//!
//! With `custom_options.persist_network` on (the default), the cosmetic and profile rows are
//! emitted on the network save only (as `mod_<option id>` wire fields): never read from a
//! network load and never written to or primed from the mod-config.json cache. The game's own
//! profile load is the single source of truth and the scene-25 sync mirrors it into the menu.
//! The persist transforms (`persist_save_transform` / `persist_load_transform`) convert the
//! stored index to the stable asset id and back, so the `categories` state must be populated
//! before any row is registered.
//!
//! **JSON fallback (`persist_network=false`).** A server that ignores the injected
//! `mod_customize_*` fields can never round-trip these rows, so when the operator turns the
//! network leg off the framework raises `custom_options::save_only_json_fallback()` and every
//! `SaveOnly` row rides the `custom_options.{p1,p2}` cache like a `Local` row (written on each
//! save, primed at boot through `load_transform`). The scene-25 sync then DRIVES the game for
//! any row the cache primed or the player edited (`value_is_authoritative`): the cached asset
//! id is written into `Customize`, since the game's own load would otherwise overwrite the
//! pick on every card-in. Rows nobody has chosen yet keep mirroring the game, so a value the
//! server/web UI set survives until the player first edits that row.
//!
//! ## Submodules
//!
//! - `discovery.rs` — the static category table (option id, `Customize` field offset, scan
//!   directory / filename prefix, preview recipe) and the filesystem asset-id scan.
//! - `preview_gen.rs` — builds each category's base `seop_image_<id>` chrome from its shipped
//!   `_TEMPLATE` (marker boxes cleared) before the row registers; marker lookup, gamma and
//!   source-arc search shared with the overlay.
//! - `preview_overlay.rs` — draws the focused value's real art as native sprites over the
//!   chrome while a side's options modal is open, loading a bounded prefetch window on demand
//!   through `asset_loader`.
//! - `bg_preview_overlay.rs` — animated previews for the two BACKGROUND rows, as mod-owned AFP
//!   layers over private alias packages (`bm2d_api` + `bm2d_package`).
//! - `profile_fields.rs` — the `is_disp_weight` toggle and its `weight` (kg) child, read from /
//!   written to the `PlayerWork` header; registered once per process.
//!
//! ## Invariants
//!
//! - Never write `Customize` or `PlayerWork` outside a user edit or the JSON-fallback sync of
//!   an authoritative row: the mirror leg must stay read-only and silent, or an id the cabinet
//!   lacks would overwrite the server-loaded value.
//! - Never hold the mod's `STATE` lock while calling into the registry: the registry calls the
//!   persist transforms (which take `STATE`) under its own lock. `customize_base_and_categories`
//!   snapshots and releases first.
//! - Don't hold the `STATE` lock across `register_option` (registration can fire callbacks
//!   that re-enter it). Every `PlayerWork` walk is null-guarded, so an uncarded side is a
//!   no-op.
//! - The previews must not reintroduce per-value textures: chrome plus on-demand overlay is
//!   what keeps the song-select / CAUTION preload small (`docs/scene_load_analysis.md`).
//!
//! ## Degradation
//!
//! `player_work_table` and `customize_offset` are required signatures (the registry skips the
//! mod without them; a zero `customize_offset` fails `init`). Without `custom_options`
//! `enable` does nothing. No discovered assets still registers the profile rows. Without
//! `asset_loader` the previews are chrome-only; without the AFP-layer wrappers or
//! `bm2d_package` the BACKGROUND rows are chrome-only.
//!
//! ## Config
//!
//! This mod has no config section of its own. It reads three optional keys from
//! `custom_options` in mod-config.json at enable: `lane_gamma_correction` (Photoshop-convention
//! gamma for the lane preview art, overriding the per-layer default; the pre-brightened lane
//! cache regenerates when it changes), `preview_window` (half-width N of the prefetch window
//! for both overlays, clamped 0..=10, default `preview_overlay::DEFAULT_WINDOW_N`) and
//! `animate_backgrounds` (default true; false shows the BACKGROUND preview as a paused first
//! frame). A legacy top-level `webui_options` block (the old offline value cache) is migrated
//! into `custom_options.{p1,p2}` once by `config::migrate_webui_options_to_custom_options` at
//! persistence init.
//!
//! VIDEO SIZE is not here: it is the standalone `movie_size_customization` mod. RE notes:
//! `docs/player_customization_system_research.md`, `docs/calorie_weight_profile_research.md`,
//! `docs/option_preview_image_box.md`, `docs/bm2d_background_preview_research.md`.

pub mod bg_preview_overlay;
pub mod discovery;
pub mod preview_gen;
pub mod preview_overlay;
pub mod profile_fields;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, PersistMode, RegisterSpec, ScalarFormat};
use crate::services::scene_manager;
use crate::types::scenes::scene;
use crate::{log_info, log_warn};
use once_cell::sync::Lazy;
use std::sync::Mutex;

use discovery::{CategoryDef, DiscoveredCategory};

struct SharedState {
    customize_offset: usize,
    player_work_table: *const u8,
    categories: Vec<DiscoveredCategory>,
}

unsafe impl Send for SharedState {}

static STATE: Lazy<Mutex<SharedState>> = Lazy::new(|| {
    Mutex::new(SharedState {
        customize_offset: 0,
        player_work_table: std::ptr::null(),
        categories: Vec::new(),
    })
});

/// Map an in-memory sequential index to its stable asset ID — the wire value
/// of both the network save and the JSON cache. The save-side persist
/// transform; [`persist_load_transform`] is its inverse. Returns the input
/// unchanged if the id isn't registered here or the index is out of range
/// (shouldn't happen in practice, but keeps the failure mode safe).
fn persist_save_transform(id: &str, value: i32) -> i32 {
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => return value,
    };
    let cat = match state.categories.iter().find(|c| c.def.option_id == id) {
        Some(c) => c,
        None => return value,
    };
    if value < 0 || (value as usize) >= cat.asset_ids.len() {
        return value;
    }
    cat.asset_ids[value as usize] as i32
}

/// Inverse of [`persist_save_transform`]: a cached asset id → its menu index.
/// Only ever consulted by the JSON prime under the `SaveOnly` JSON fallback
/// (`persist_network=false`); the network load never carries these rows. An
/// id this cabinet doesn't have maps to index 0 — the same degradation
/// [`sync_registry_with_game`]'s mirror applies to a server-stored id that
/// isn't installed.
fn persist_load_transform(id: &str, asset_id: i32) -> i32 {
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let cat = match state.categories.iter().find(|c| c.def.option_id == id) {
        Some(c) => c,
        None => return 0,
    };
    cat.asset_ids
        .iter()
        .position(|&a| a as i32 == asset_id)
        .unwrap_or(0) as i32
}

/// Accessor for the mod's resolved `player_work_table` base pointer, shared with
/// the [`profile_fields`] submodule so it can reach the same per-side
/// `PlayerWork` objects the cosmetics use — at the PlayerWork **header** offsets
/// (`+0x24`/`+0x28`) rather than the customize offset. Returns null if the
/// `player_work_table` signature hasn't resolved or `init()` hasn't run yet;
/// callers null-guard every hop of the walk.
pub(super) fn player_work_table() -> *const u8 {
    STATE
        .lock()
        .map(|s| s.player_work_table)
        .unwrap_or(std::ptr::null())
}

pub struct WebUiOptionsMod {
    initialized: bool,
    scene_cb_id: Option<usize>,
}

impl WebUiOptionsMod {
    pub fn new() -> Self {
        Self {
            initialized: false,
            scene_cb_id: None,
        }
    }
}

impl Mod for WebUiOptionsMod {
    fn id(&self) -> &str {
        "webui-options"
    }

    fn name(&self) -> &str {
        "WebUI Options"
    }

    fn description(&self) -> &str {
        "In-game customization options (appeal board, lanes, etc.)"
    }

    fn required_signatures(&self) -> &[&str] {
        &["player_work_table", "customize_offset"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let pwt = ctx.signatures.require_address("player_work_table");
        let cust_off = ctx.signatures.require_address("customize_offset") as usize;

        if cust_off == 0 {
            log_warn!("WebUiOptions: customize_offset resolved to 0 -- mod disabled");
            return false;
        }

        {
            let mut state = STATE.lock().unwrap();
            state.player_work_table = pwt;
            state.customize_offset = cust_off;
        }

        self.initialized = true;
        log_info!("WebUiOptions: init (customize_offset=0x{:X})", cust_off);
        true
    }

    fn enable(&mut self) {
        if !self.initialized {
            return;
        }

        if !custom_options::is_available() {
            log_warn!("WebUiOptions: custom_options service unavailable");
            return;
        }

        let categories = discovery::discover_all();
        if categories.is_empty() {
            // No cosmetic assets on this cabinet (VIDEO SIZE, which needed no
            // assets, now lives in the standalone movie_size_customization
            // mod). Still fall through: the non-cosmetic profile rows below
            // register regardless.
            log_warn!("WebUiOptions: no cosmetic categories discovered");
        }

        // Populate STATE.categories BEFORE registering options, so the
        // persist transforms can resolve asset_id ↔ seq_index lookups as
        // soon as a persistence load (network load_receiver or the lazy JSON
        // timer) fires.
        {
            let mut state = STATE.lock().unwrap();
            state.categories = categories;
        }

        // Snapshot the categories for iteration. We can't hold the STATE
        // lock during register_option because registration can trigger
        // change callbacks that re-enter STATE.
        let categories_snapshot: Vec<(&'static CategoryDef, Vec<u32>)> = {
            let state = STATE.lock().unwrap();
            state
                .categories
                .iter()
                .map(|c| (c.def, c.asset_ids.clone()))
                .collect()
        };

        for (def, asset_ids) in &categories_snapshot {
            let option_id = def.option_id;
            let count = asset_ids.len() as i32;
            if count == 0 {
                continue;
            }

            // Register with a plain default; the menu registry is synced with
            // the game's current selections at every SONG_SELECT entry by
            // sync_registry_with_game (reading the Customize object the game
            // populated from the server's <customize> load block). These
            // options are SaveOnly: the DLL emits them on network save — the
            // one direction the game lacks — and never network-loads them.
            // With persist_network=false they fall back to the JSON cache
            // (write + prime), which the same sync then drives INTO the game.
            // Every category uses the index-based value model, so the shared
            // persist transforms map index <-> asset id.
            //
            // Generate the base chrome image (the `_TEMPLATE` with its
            // marker boxes cleared) BEFORE registration, so it's on
            // disk when register_option records the base preview name
            // and the atlas flush reads it. Scalar rows always show
            // this single chrome; the preview overlay draws the
            // focused value's live art on top for categories with
            // overlay layers. The selector displays the category's
            // short label + the 1-based position (e.g. "Char #3")
            // while the stored value stays the 0-based asset index
            // (display-only prefix + offset).
            preview_gen::generate_chrome(option_id);
            let spec = RegisterSpec::scalar(
                option_id,
                0,
                count - 1,
                1,
                ScalarFormat::PrefixedIndex {
                    prefix: def.value_prefix,
                    display_offset: 1,
                },
            )
            .display_name(def.display_name)
            .description("Profile cosmetic; previewable in the in-game options menu")
            .in_game_only()
            .default_value(0)
            .on_change(on_value_changed)
            .persist_mode(PersistMode::SaveOnly)
            .persist_transform(persist_save_transform, persist_load_transform);

            match custom_options::register_option(spec) {
                Ok(_handle) => {
                    log_info!(
                        "WebUiOptions: registered {} (range 0..{}, scalar)",
                        option_id,
                        count - 1
                    );
                }
                Err(e) => {
                    log_warn!("WebUiOptions: failed to register {}: {}", option_id, e);
                }
            }
        }

        // Non-cosmetic WebUI-only profile rows (DISPLAY BURNED CALORIES + the
        // conditional WEIGHT child). Registered under the same webui-options
        // toggle and the same custom_options availability guard as the
        // cosmetics; a registration failure logs + is skipped inside register()
        // and never affects the cosmetics above.
        profile_fields::register();

        // Sync the menu registry with the game's own Customize object on
        // EVERY SONG_SELECT (scene 25) entry — the earliest point the options
        // modal can open, and the point at which PlayerWork/Customize are
        // fully populated from the server's <customize> load block. With the
        // network round-trip on, the sync is a read-only mirror (silent
        // setter, never writes Customize) and idempotent: a user edit is
        // written into Customize on-change, so re-reading yields the same
        // value. Under the SaveOnly JSON fallback the direction flips for
        // every row the cache primed / the player chose: the game's load
        // can't bring those back, so the sync writes them into Customize.
        self.scene_cb_id = Some(scene_manager::on_scene_change(Box::new(|_old, new| {
            if new == scene::SONG_SELECT {
                sync_registry_with_game(0);
                sync_registry_with_game(1);
                // Same sync for the workout-profile rows, against the
                // PlayerWork header the game's <common> load populated.
                profile_fields::sync(0);
                profile_fields::sync(1);
            }
        })));

        // Preview overlay (on-demand asset art over the chrome templates).
        // Guarded on asset_loader availability: if the FileManager/ResourceManager
        // signatures didn't resolve, skip overlay setup entirely — the preview
        // boxes then show chrome only (graceful degradation, R-8).
        if crate::services::asset_loader::is_available() {
            let overlay_categories: Vec<DiscoveredCategory> = categories_snapshot
                .iter()
                .map(|(def, asset_ids)| DiscoveredCategory {
                    def,
                    asset_ids: asset_ids.clone(),
                })
                .collect();
            // Window half-width from `custom_options.preview_window` when set
            // (clamped inside init), else the built-in default.
            let window_n = preview_window_config().unwrap_or(preview_overlay::DEFAULT_WINDOW_N);
            preview_overlay::init(overlay_categories, window_n);
        } else {
            log_warn!(
                "WebUiOptions: asset_loader unavailable — preview overlays disabled (chrome only)"
            );
        }

        // Animated BACKGROUND previews (AFP layers over the two background
        // rows, driven by the game's own AFP/BM2D runtime). Guarded on the
        // AFP-layer wrappers + package service; a miss leaves the background
        // rows chrome-only (graceful degradation, R-9).
        if crate::services::bm2d_api::afp_layers_available()
            && crate::services::bm2d_package::is_available()
        {
            let bg_categories: Vec<DiscoveredCategory> = categories_snapshot
                .iter()
                .map(|(def, asset_ids)| DiscoveredCategory {
                    def,
                    asset_ids: asset_ids.clone(),
                })
                .collect();
            // Same prefetch-window knob the static overlay uses (clamped
            // inside init).
            let bg_window_n = preview_window_config().unwrap_or(preview_overlay::DEFAULT_WINDOW_N);
            bg_preview_overlay::init(bg_categories, bg_window_n, animate_backgrounds_config());
        } else {
            log_warn!(
                "WebUiOptions: bm2d layer/package services unavailable — background previews disabled (chrome only)"
            );
        }

        log_info!("WebUiOptions: enabled");
    }

    fn disable(&mut self) {
        if let Some(id) = self.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        // Disarm the preview overlay (hides sprites + releases any resident
        // preview assets on the render thread).
        preview_overlay::shutdown();
        // Disarm the animated-background overlay (destroys any live layer +
        // releases its package on the render thread).
        bg_preview_overlay::shutdown();
        let mut state = STATE.lock().unwrap();
        state.categories.clear();
        log_info!("WebUiOptions: disabled");
    }
}

/// Resolve the operator-tunable lane gamma override from
/// `custom_options.lane_gamma_correction` in `mod-config.json`. `None` (key
/// absent, or config not yet loaded) leaves each layer's built-in default in
/// place; `Some(g)` overrides every gamma-opted preview layer. Read fresh at
/// `enable()` time — by the preview compositor and by `preview_overlay`'s
/// lane brighten cache (which keys its cache on the effective value, so a
/// config change regenerates the cached arcs on the next boot/enable).
pub(super) fn lane_gamma_override() -> Option<f32> {
    crate::mods::config::get()
        .and_then(|c| c.custom_options.as_ref())
        .and_then(|co| co.lane_gamma_correction)
}

/// Resolve the operator-tunable prefetch-window half-width from
/// `custom_options.preview_window` in `mod-config.json`. `None` (key absent,
/// or config not yet loaded) → the built-in `DEFAULT_WINDOW_N`; the value is
/// range-clamped inside `preview_overlay::init`.
fn preview_window_config() -> Option<i32> {
    crate::mods::config::get()
        .and_then(|c| c.custom_options.as_ref())
        .and_then(|co| co.preview_window)
}

/// Resolve `custom_options.animate_backgrounds` from `mod-config.json`.
/// Absent (or config not loaded) → true: background previews animate.
/// `false` → static first frame (create + pause), never blank chrome.
fn animate_backgrounds_config() -> bool {
    crate::mods::config::get()
        .and_then(|c| c.custom_options.as_ref())
        .and_then(|co| co.animate_backgrounds)
        .unwrap_or(true)
}

fn on_value_changed(player_side: u8, _new_value: i32) {
    try_apply_all(player_side);
}

/// Reconcile the options-menu registry and the game's own `Customize` object
/// for one player side. Called on every SONG_SELECT (scene 25) entry.
///
/// Default direction (network persistence on): strictly READ-ONLY with
/// respect to game memory — each category's field is read as a raw u32 asset
/// id, reverse-mapped to its menu index (index 0 when the id isn't in the
/// discovered list — e.g. the server stored an id this cabinet doesn't have),
/// and written into the registry via [`custom_options::set_value_silent`],
/// which does NOT fire `on_change` — so an unknown id can never clobber the
/// game's (server-loaded) value through `try_apply_all`.
///
/// Under the `SaveOnly` JSON fallback (`persist_network=false`), a row whose
/// value the JSON prime or the player has chosen
/// ([`custom_options::value_is_authoritative`]) flows the OTHER way: its
/// selected asset id is written into `Customize`, because the game's load
/// (whatever the server sent) is the stale copy and nothing else would ever
/// restore the player's pick. Rows nobody has chosen yet (a fresh cache)
/// still mirror the game, so a server/web-UI value survives until the player
/// first edits the row — after which the cache owns it.
///
/// Null-guards the player-work chain: a side that isn't carded in is skipped
/// silently. Panic-free (bounds-checked reads, `position().unwrap_or(0)`).
fn sync_registry_with_game(player_side: u8) {
    let Some((customize_base, categories)) = customize_base_and_categories(player_side) else {
        return;
    };

    let drive_game = custom_options::save_only_json_fallback();
    let mut mirrored = 0usize;
    let mut applied = 0usize;

    for (def, asset_ids) in &categories {
        // SAFETY: `customize_base` is the carded-in side's validated
        // PlayerWork + customize_offset; every field offset comes from the
        // static category table and addresses a u32 inside `Customize`.
        let field_ptr =
            unsafe { customize_base.add(def.customize_field_offset as usize) } as *mut u32;

        if drive_game && custom_options::value_is_authoritative(def.option_id, player_side) {
            let seq_value = custom_options::get_value(player_side, def.option_id).unwrap_or(0);
            if let Some(&asset_id) = usize::try_from(seq_value)
                .ok()
                .and_then(|i| asset_ids.get(i))
            {
                unsafe { field_ptr.write(asset_id) };
                applied += 1;
            }
            continue;
        }

        let asset_id = unsafe { field_ptr.read() };
        let index = asset_ids.iter().position(|&a| a == asset_id).unwrap_or(0);
        custom_options::set_value_silent(def.option_id, player_side, index as i32);
        mirrored += 1;
    }

    if drive_game {
        log_info!(
            "WebUiOptions: synced {} option(s) with game Customize (side={}; JSON fallback: {} applied to game, {} mirrored from game)",
            categories.len(),
            player_side,
            applied,
            mirrored
        );
    } else {
        log_info!(
            "WebUiOptions: seeded {} option(s) from game Customize (side={})",
            mirrored,
            player_side
        );
    }
}

/// Resolve one side's `Customize` base pointer plus a snapshot of the
/// discovered categories, then RELEASE the mod's `STATE` lock before the
/// caller touches the registry or game memory. The registry's persist
/// transforms take `STATE` while the registry lock is held (the save
/// snapshot, the JSON prime), so holding `STATE` across a registry call
/// here would invert the lock order. Returns `None` when the signatures are
/// unresolved or the side isn't carded in (every hop null-guarded).
fn customize_base_and_categories(
    player_side: u8,
) -> Option<(*mut u8, Vec<(&'static CategoryDef, Vec<u32>)>)> {
    let state = STATE.lock().ok()?;
    if state.player_work_table.is_null() || state.customize_offset == 0 {
        return None;
    }
    // SAFETY: `player_work_table` is the resolved 2-slot per-side table;
    // each hop is null-checked before it is dereferenced.
    let customize_base = unsafe {
        let table = state.player_work_table as *const *const u8;
        let wrapper = *table.add(player_side as usize);
        if wrapper.is_null() {
            return None; // side not carded in
        }
        let player_work = *(wrapper as *const *const u8);
        if player_work.is_null() {
            return None;
        }
        player_work.add(state.customize_offset) as *mut u8
    };
    let categories = state
        .categories
        .iter()
        .map(|c| (c.def, c.asset_ids.clone()))
        .collect();
    Some((customize_base, categories))
}

/// Write every category's currently-selected asset id into the game's
/// `Customize` object for one player side. Invoked from [`on_value_changed`]
/// (a user edit in the options menu, or a persistence load landing); the
/// other writer is [`sync_registry_with_game`]'s JSON-fallback leg. With
/// network persistence on, the loaded state flows the other way — the game's
/// native `<customize>` load populates `Customize`, and the sync reads it
/// back into the menu registry.
fn try_apply_all(player_side: u8) -> bool {
    let Some((customize_base, categories)) = customize_base_and_categories(player_side) else {
        return false;
    };

    for (def, asset_ids) in &categories {
        let seq_value = custom_options::get_value(player_side, def.option_id).unwrap_or(0);
        let asset_id = usize::try_from(seq_value)
            .ok()
            .and_then(|i| asset_ids.get(i).copied())
            .unwrap_or_else(|| asset_ids.first().copied().unwrap_or(1));

        // SAFETY: see `sync_registry_with_game` — validated base, table-driven
        // u32 field offset inside `Customize`.
        unsafe {
            let field_ptr = customize_base.add(def.customize_field_offset as usize) as *mut u32;
            field_ptr.write(asset_id);
        }
    }

    true
}
