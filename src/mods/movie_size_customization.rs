//! Movie Size Customization — standalone VIDEO SIZE option row.
//!
//! Exposes the game's `Customize` movie-size field (`Customize + 0x30`,
//! normally editable only from Konami's WebUI) as an in-game options-menu
//! row. Split out of `webui_options` so operators can disable the cosmetic
//! WebUI pickers while keeping the video-size control (the two mods are
//! independent toggles; this one owns ONLY this row).
//!
//! ## Value model
//!
//! Same index-based model the WebUI cosmetics use: the registered row
//! stores a 0-based index (0=FULLSCREEN, 1=ON, 2=OFF) while the game field
//! and the network-save wire carry the 1-based enum the game defined
//! (1=fullscreen, 2=on, 3=off — `docs/player_customization_system_research.md`,
//! `Customize+0x30` semantics: 0–1 select the fullscreen Flash layer,
//! 2–3 the sized layer).
//!
//! - **Apply:** a user edit (`on_change`) writes `index + 1` to
//!   `Customize + 0x30` for that side. This is the row's only writer of
//!   game memory.
//! - **Sync:** on every SONG_SELECT (scene 25) entry the menu registry is
//!   re-seeded by READING `Customize + 0x30` (populated by the game's own
//!   native `<customize>` load) via `set_value_silent` — never fires
//!   `on_change`, so an unknown stored value can't clobber game state. Under
//!   the `SaveOnly` JSON fallback (below) the direction flips for a cached /
//!   player-chosen value, which is written into `Customize + 0x30` instead.
//! - **Persistence:** `PersistMode::SaveOnly` with a `±1` persist transform
//!   pair — the DLL emits `mod_customize_movie_size` (1/2/3) on the network
//!   save; the backend stores its native `cust_movie_size` column and the
//!   value returns through the game's own `<customize>` load block. No
//!   network load. With `persist_network=false` the row rides the
//!   `mod-config.json` cache instead (`custom_options::save_only_json_fallback`).
//!
//! ## Textures
//!
//! The `option_id` stays `customize_movie_size`, so the row reuses all the
//! textures already shipped for it under
//! `data_mods/custom_options/select_music_option_lang_*/tex/`: the
//! `seop_item_customize_movie_size` label, the three
//! `seop_image_customize_movie_size_<key>` preview images, and the net-new
//! `seop_op_fullscreen` ribbon (`seop_op_on`/`seop_op_off` are stock). The
//! `option_menu_settings` placement entry in `mod-config.json` keys on the
//! same id and is likewise unchanged.
//!
//! ## Degradation
//!
//! Both required signatures are shared derivations (`player_work_table`,
//! `customize_offset`); a miss skips the mod cleanly. Every pointer hop of
//! the PlayerWork→Customize walk is null-guarded (a side that isn't carded
//! in is skipped silently); all callbacks are panic-free.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, EnumValue, PersistMode, RegisterSpec};
use crate::services::scene_manager;
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// The option id (and thus wire field `mod_customize_movie_size`, texture
/// names, and `option_menu_settings` key) — kept identical to the original
/// webui_options-owned registration so nothing downstream changes.
const OPTION_ID: &str = "customize_movie_size";

/// Offset of the movie-size field within the game's `Customize` object.
const CUSTOMIZE_FIELD_OFFSET: usize = 0x30;

/// The three values, in menu-index order. `key` doubles as the value-ribbon
/// suffix (`seop_op_<key>`, flat/shared namespace — on/off are stock) and
/// the preview-box suffix (`seop_image_customize_movie_size_<key>`,
/// option-scoped).
const VALUE_KEYS: [&str; 3] = ["fullscreen", "on", "off"];

/// Resolved `player_work_table` base pointer (as usize; 0 = unresolved) and
/// `customize_offset`, stashed at init for the capture-free callbacks.
static PLAYER_WORK_TABLE: AtomicUsize = AtomicUsize::new(0);
static CUSTOMIZE_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Menu index (0..=2) → game/wire value (1..=3). Out-of-range input passes
/// through unchanged (keeps the failure mode safe; shouldn't happen).
fn persist_save_transform(_id: &str, value: i32) -> i32 {
    if (0..VALUE_KEYS.len() as i32).contains(&value) {
        value + 1
    } else {
        value
    }
}

/// Inverse of [`persist_save_transform`]: game/wire value (1..=3) → menu index
/// (0..=2); anything else → index 0 (the same mapping the game→menu mirror
/// applies). Consulted only by the JSON prime under the `SaveOnly` JSON
/// fallback (`persist_network=false`).
fn persist_load_transform(_id: &str, wire: i32) -> i32 {
    if (1..=VALUE_KEYS.len() as i32).contains(&wire) {
        wire - 1
    } else {
        0
    }
}

/// Resolve one side's `Customize` base. Null-guards every hop; `None` when
/// the signatures didn't stash or the side isn't carded in.
fn customize_base(player_side: u8) -> Option<*mut u8> {
    let table = PLAYER_WORK_TABLE.load(Ordering::Acquire);
    let offset = CUSTOMIZE_OFFSET.load(Ordering::Acquire);
    if table == 0 || offset == 0 || player_side >= 2 {
        return None;
    }
    unsafe {
        let table = table as *const *const u8;
        let wrapper = *table.add(player_side as usize);
        if wrapper.is_null() {
            return None; // side not carded in
        }
        let player_work = *(wrapper as *const *const u8);
        if player_work.is_null() {
            return None;
        }
        Some(player_work.add(offset) as *mut u8)
    }
}

/// User edit in the options menu (or a persistence load landing) — the writer
/// of the game field, also reused by [`sync_with_game`]'s JSON-fallback leg.
fn on_value_changed(player_side: u8, new_value: i32) {
    let Some(base) = customize_base(player_side) else {
        return;
    };
    let game_value = persist_save_transform(OPTION_ID, new_value.clamp(0, 2)) as u32;
    unsafe {
        (base.add(CUSTOMIZE_FIELD_OFFSET) as *mut u32).write(game_value);
    }
}

/// Reconcile the menu registry with the game's `Customize` object for one
/// side, on SONG_SELECT entry. Default direction: a read-only seed — unknown
/// stored values map to index 0 and, via the silent setter, can never write
/// back into game memory. Under the `SaveOnly` JSON fallback
/// (`persist_network=false`) a value the cache primed or the player chose
/// ([`custom_options::value_is_authoritative`]) is written INTO the game
/// instead, since the game's own load can't bring it back.
fn sync_with_game(player_side: u8) {
    let Some(base) = customize_base(player_side) else {
        return;
    };
    if custom_options::save_only_json_fallback()
        && custom_options::value_is_authoritative(OPTION_ID, player_side)
    {
        if let Some(v) = custom_options::get_value(player_side, OPTION_ID) {
            on_value_changed(player_side, v);
        }
        return;
    }
    let game_value = unsafe { (base.add(CUSTOMIZE_FIELD_OFFSET) as *const u32).read() };
    custom_options::set_value_silent(
        OPTION_ID,
        player_side,
        persist_load_transform(OPTION_ID, game_value as i32),
    );
}

pub struct MovieSizeCustomizationMod {
    initialized: bool,
    scene_cb_id: Option<usize>,
}

impl MovieSizeCustomizationMod {
    pub fn new() -> Self {
        Self {
            initialized: false,
            scene_cb_id: None,
        }
    }
}

impl Mod for MovieSizeCustomizationMod {
    fn id(&self) -> &str {
        "movie-size-customization"
    }

    fn name(&self) -> &str {
        "Movie Size Customization"
    }

    fn description(&self) -> &str {
        "In-game VIDEO SIZE option (background movie fullscreen/on/off)"
    }

    fn required_signatures(&self) -> &[&str] {
        &["player_work_table", "customize_offset"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let pwt = ctx.signatures.require_address("player_work_table");
        let cust_off = ctx.signatures.require_address("customize_offset") as usize;

        if cust_off == 0 {
            log_warn!("MovieSizeCustomization: customize_offset resolved to 0 -- mod disabled");
            return false;
        }

        PLAYER_WORK_TABLE.store(pwt as usize, Ordering::Release);
        CUSTOMIZE_OFFSET.store(cust_off, Ordering::Release);

        self.initialized = true;
        log_info!(
            "MovieSizeCustomization: init (customize_offset=0x{:X})",
            cust_off
        );
        true
    }

    fn enable(&mut self) {
        if !self.initialized {
            return;
        }

        if !custom_options::is_available() {
            log_warn!("MovieSizeCustomization: custom_options service unavailable");
            return;
        }

        let values: Vec<EnumValue> = VALUE_KEYS
            .iter()
            .enumerate()
            .map(|(index, key)| {
                EnumValue::with_preview(index as i32, format!("seop_op_{key}"), *key)
                    .display_label(key.to_uppercase())
            })
            .collect();

        let spec = RegisterSpec::enum_values(OPTION_ID, values)
            .display_name("Video Size")
            .description("Background movie size; previewable in the in-game options menu")
            .in_game_only()
            .default_value(0)
            .on_change(on_value_changed)
            .persist_mode(PersistMode::SaveOnly)
            .persist_transform(persist_save_transform, persist_load_transform);

        match custom_options::register_option(spec) {
            Ok(_handle) => {
                log_info!("MovieSizeCustomization: registered {}", OPTION_ID);
            }
            Err(e) => {
                log_warn!(
                    "MovieSizeCustomization: failed to register {}: {}",
                    OPTION_ID,
                    e
                );
                return;
            }
        }

        // Sync the menu registry with the game's own Customize object on
        // EVERY SONG_SELECT (scene 25) entry — the earliest point the options
        // modal can open, and the point at which PlayerWork/Customize are
        // fully populated from the server's <customize> load block. With
        // network persistence on the sync is a read-only, idempotent seed (a
        // user edit writes Customize on-change, so re-seeding reads back the
        // same value); under the SaveOnly JSON fallback it drives the cached
        // / player-chosen value into Customize instead.
        self.scene_cb_id = Some(scene_manager::on_scene_change(Box::new(|_old, new| {
            if new == scene::SONG_SELECT {
                sync_with_game(0);
                sync_with_game(1);
            }
        })));

        log_info!("MovieSizeCustomization: enabled");
    }

    fn disable(&mut self) {
        if let Some(id) = self.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        log_info!("MovieSizeCustomization: disabled");
    }
}
