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

/// Map an in-memory sequential index to its stable asset ID, for server
/// persistence. Used as the save-side persist transform (these options are
/// `SaveOnly`: there is no load-side inverse — menu state is seeded by
/// reading the game's own `Customize` object at SONG_SELECT entry, which
/// does its own asset-id → index reverse lookup in
/// [`seed_registry_from_game`]). Returns the input unchanged if the id isn't
/// registered here or the index is out of range (shouldn't happen in
/// practice, but keeps the failure mode safe).
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

            // Register with a plain default; the menu registry is seeded with
            // the game's current selections at every SONG_SELECT entry by
            // seed_registry_from_game (reading the Customize object the game
            // populated from the server's <customize> load block). These
            // options are SaveOnly: the DLL emits them on network save — the
            // one direction the game lacks — and never network-loads or
            // JSON-persists them. Every category uses the index-based value
            // model, so the shared save transform maps index -> asset id.
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
            .save_transform(persist_save_transform);

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

        // Seed the menu registry from the game's own Customize object on
        // EVERY SONG_SELECT (scene 25) entry — the earliest point the options
        // modal can open, and the point at which PlayerWork/Customize are
        // fully populated from the server's <customize> load block. The seed
        // is read-only (silent setter, never writes Customize) and idempotent:
        // a user edit is written into Customize on-change, so re-seeding reads
        // back the same value. There is no scene-20 apply — the game's native
        // load path is the only thing that populates Customize on card-in.
        self.scene_cb_id = Some(scene_manager::on_scene_change(Box::new(|_old, new| {
            if new == scene::SONG_SELECT {
                seed_registry_from_game(0);
                seed_registry_from_game(1);
                // Same read-only seed for the workout-profile rows, from
                // the PlayerWork header the game's <common> load populated.
                profile_fields::seed(0);
                profile_fields::seed(1);
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

/// Seed the options-menu registry from the game's own `Customize` object for
/// one player side. Called on every SONG_SELECT (scene 25) entry.
///
/// Strictly READ-ONLY with respect to game memory: each category's field is
/// read as a raw u32 asset id, reverse-mapped to its menu index (index 0 when
/// the id isn't in the discovered list — e.g. the server stored an id this
/// cabinet doesn't have), and written into the registry via
/// [`custom_options::set_value_silent`], which does NOT fire `on_change` —
/// so an unknown id can never clobber the game's (server-loaded) value
/// through `try_apply_all`. Null-guards the player-work chain: a side that
/// isn't carded in is skipped silently. Panic-free (bounds-checked reads,
/// `position().unwrap_or(0)`).
fn seed_registry_from_game(player_side: u8) {
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => return,
    };

    if state.player_work_table.is_null() || state.customize_offset == 0 {
        return;
    }

    unsafe {
        let table = state.player_work_table as *const *const u8;
        let wrapper = *table.add(player_side as usize);
        if wrapper.is_null() {
            return; // side not carded in
        }
        let player_work = *(wrapper as *const *const u8);
        if player_work.is_null() {
            return;
        }
        let customize_base = player_work.add(state.customize_offset);

        for cat in &state.categories {
            let field_ptr =
                customize_base.add(cat.def.customize_field_offset as usize) as *const u32;
            let asset_id = field_ptr.read();
            let index = cat
                .asset_ids
                .iter()
                .position(|&a| a == asset_id)
                .unwrap_or(0);
            custom_options::set_value_silent(cat.def.option_id, player_side, index as i32);
        }

        log_info!(
            "WebUiOptions: seeded {} option(s) from game Customize (side={})",
            state.categories.len(),
            player_side
        );
    }
}

/// Write every category's currently-selected asset id into the game's
/// `Customize` object for one player side. This is the ONLY writer of
/// `Customize` in the mod, invoked solely from [`on_value_changed`] (a user
/// edit in the options menu); the loaded state flows the other way — the
/// game's native `<customize>` load populates `Customize`, and
/// [`seed_registry_from_game`] reads it back into the menu registry.
fn try_apply_all(player_side: u8) -> bool {
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => return false,
    };

    if state.player_work_table.is_null() || state.customize_offset == 0 {
        return false;
    }

    unsafe {
        let table = state.player_work_table as *const *const u8;
        let wrapper = *table.add(player_side as usize);
        if wrapper.is_null() {
            return false;
        }
        let player_work = *(wrapper as *const *const u8);
        if player_work.is_null() {
            return false;
        }
        let customize_base = player_work.add(state.customize_offset);

        for cat in &state.categories {
            let seq_value = custom_options::get_value(player_side, cat.def.option_id).unwrap_or(0);
            let asset_id = if seq_value >= 0 && (seq_value as usize) < cat.asset_ids.len() {
                cat.asset_ids[seq_value as usize]
            } else {
                cat.asset_ids.first().copied().unwrap_or(1)
            };

            let field_ptr = customize_base.add(cat.def.customize_field_offset as usize) as *mut u32;
            field_ptr.write(asset_id);
        }
    }

    true
}
