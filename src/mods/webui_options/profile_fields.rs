//! In-game **WEIGHT** and **DISPLAY BURNED CALORIES** profile fields.
//!
//! A self-contained submodule of the [WebUI Options](super) mod that adds two
//! player-profile settings the stock game only lets you set through Konami's
//! web portal:
//!
//! - **`is_disp_weight`** — an OFF/ON toggle ("display burned calories in-game").
//! - **`weight`** — body weight in **kg**, fed into the game's calorie
//!   calculation; shown only when the calorie toggle is ON (parent/child).
//!
//! The mechanism mirrors the cosmetic `mod_customize_*` design exactly: the
//! game's own `<common>` profile load stays the single source of truth (server
//! → ess → `ReflectPlayerWork` → `PlayerWork`), and this submodule only adds the
//! *save* direction the stock game lacks:
//!
//! - [`register`] adds the two option rows to the Mods tab.
//! - [`seed`] reads `PlayerWork` at SONG_SELECT and mirrors the values into the
//!   menu registry (read-only w.r.t. game memory).
//! - the `on_change` writers push a user edit into `PlayerWork`.
//! - persistence is [`PersistMode::SaveOnly`]: the framework auto-emits
//!   `<mod_weight>` / `<mod_is_disp_weight>` s32 children on `playerdata_save`
//!   (the option ids drive the `mod_{id}` wire names), and the backend writes
//!   them into its native `weight` / `is_disp_weight` columns.
//!
//! Offsets are hardcoded (verified stable on gamemdx 20260324 & 20260616); the
//! `player_work_table` base is resolved at runtime by the parent mod. See
//! `docs/calorie_weight_profile_research.md` for the full RE basis.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::services::custom_options::{self, PersistMode, RegisterSpec, ScalarFormat, ShowWhen};
use crate::{log_info, log_warn};

/// `PlayerWork + 0x24` — body weight, s32 (kg; `0` = unset → game assumes 60).
const WEIGHT_OFFSET: usize = 0x24;
/// `PlayerWork + 0x28` — `is_disp_weight`, u8/bool (`0`/`1`).
const IS_DISP_WEIGHT_OFFSET: usize = 0x28;

/// Parent option id → wire field `mod_is_disp_weight`.
const OPT_IS_DISP: &str = "is_disp_weight";
/// Child option id → wire field `mod_weight`.
const OPT_WEIGHT: &str = "weight";

const WEIGHT_MIN: i32 = 30;
const WEIGHT_MAX: i32 = 200;
/// Displayed when the profile reads `weight == 0` ("unset"); the game itself
/// assumes 60 kg in that case (calorie calc unset branch).
const WEIGHT_DEFAULT_WHEN_UNSET: i32 = 60;

/// Register-once latch: `register()` is called from `enable()`, which can run
/// again if the mod is toggled off/on at runtime. The options persist for the
/// process (they are not torn down on `disable()`), so a second `register()`
/// must be a no-op rather than re-attempting registration (which would log
/// spurious `Duplicate` errors).
static REGISTERED: AtomicBool = AtomicBool::new(false);

/// Walk `player_work_table[side]` → `*wrapper` = `PlayerWork` for one side,
/// null-guarded at every hop. Returns `None` if the table is unresolved or the
/// side isn't carded in — every caller then no-ops (never writes/reads game
/// memory), mirroring `webui_options::{seed_registry_from_game, try_apply_all}`.
fn player_work(side: u8) -> Option<*mut u8> {
    if side > 1 {
        return None;
    }
    let table = super::player_work_table();
    if table.is_null() {
        return None;
    }
    // SAFETY: `player_work_table` points at the game's per-side wrapper array,
    // valid for the process lifetime; each hop is null-checked before deref.
    unsafe {
        let table = table as *const *const u8;
        let wrapper = *table.add(side as usize);
        if wrapper.is_null() {
            return None; // side not carded in
        }
        let player_work = *(wrapper as *const *const u8);
        if player_work.is_null() {
            return None;
        }
        Some(player_work as *mut u8)
    }
}

/// Write the edited body weight (kg) into `PlayerWork + 0x24` for `side`. The
/// value is the framework-clamped menu value (`WEIGHT_MIN..=WEIGHT_MAX`).
fn on_weight_changed(side: u8, new_value: i32) {
    let Some(pw) = player_work(side) else { return };
    // SAFETY: `pw` is a validated PlayerWork base; `+0x24` is an in-header s32.
    unsafe {
        (pw.add(WEIGHT_OFFSET) as *mut i32).write_unaligned(new_value);
    }
    log_info!("profile_fields: wrote weight={new_value} (side={side})");
}

/// Write the calorie-display toggle into `PlayerWork + 0x28` for `side` as a
/// single byte (`0`/`1`).
fn on_is_disp_changed(side: u8, new_value: i32) {
    let Some(pw) = player_work(side) else { return };
    let byte: u8 = if new_value != 0 { 1 } else { 0 };
    // SAFETY: `pw` is a validated PlayerWork base; `+0x28` is a u8/bool flag.
    unsafe {
        pw.add(IS_DISP_WEIGHT_OFFSET).write_unaligned(byte);
    }
    log_info!("profile_fields: wrote is_disp_weight={byte} (side={side})");
}

/// Register the two option rows on the Mods tab. Idempotent (latched).
///
/// The parent (`is_disp_weight`) is registered **before** the child (`weight`):
/// the framework validates the child's `ShowWhen::Equals` reference against the
/// already-registered options synchronously and rejects it with
/// `RegisterError::UnknownParent` otherwise. A registration failure is logged
/// and skipped — it must never abort the parent mod's cosmetics.
pub fn register() {
    if REGISTERED.swap(true, Ordering::SeqCst) {
        return; // already registered (mod re-enabled at runtime)
    }

    // Parent: OFF/ON "display burned calories" toggle.
    let disp = RegisterSpec::bool_toggle(OPT_IS_DISP)
        .display_name("Display Burned Calories")
        .description("Show calories burned during play (requires PLAYER WEIGHT)")
        .default_value(0)
        .on_change(on_is_disp_changed)
        .persist_mode(PersistMode::SaveOnly);
    if let Err(e) = custom_options::register_option(disp) {
        log_warn!("profile_fields: failed to register {OPT_IS_DISP}: {e}");
    }

    // Child: WEIGHT (kg) scalar, visible only when the toggle is ON.
    let weight = RegisterSpec::scalar(
        OPT_WEIGHT,
        WEIGHT_MIN,
        WEIGHT_MAX,
        1,
        ScalarFormat::Unit { unit: "kg" },
    )
    .display_name("Player Weight")
    .description("Body weight used for the calorie calculation")
    .step_coarse(10)
    .default_value(WEIGHT_DEFAULT_WHEN_UNSET)
    .on_change(on_weight_changed)
    .show_when(ShowWhen::Equals {
        parent_id: OPT_IS_DISP.into(),
        value: 1,
    })
    .persist_mode(PersistMode::SaveOnly);
    if let Err(e) = custom_options::register_option(weight) {
        log_warn!("profile_fields: failed to register {OPT_WEIGHT}: {e}");
    }

    log_info!("profile_fields: registered {OPT_IS_DISP} + {OPT_WEIGHT}");
}

/// Seed both rows from the game's own `PlayerWork` for one player side.
///
/// Called from the parent mod's SONG_SELECT (scene 25) callback, the point at
/// which `PlayerWork` is fully populated from the server's `<common>` load.
/// Strictly **read-only** w.r.t. game memory, and uses
/// [`custom_options::set_value_silent`] (which does NOT fire `on_change`), so it
/// can never write back into `PlayerWork` or loop. A side not carded in is
/// skipped silently. A read-back `weight == 0` seeds the display to
/// [`WEIGHT_DEFAULT_WHEN_UNSET`] (60) — matching the game's own unset assumption
/// — without touching memory.
pub fn seed(player_side: u8) {
    let Some(pw) = player_work(player_side) else {
        return;
    };

    // SAFETY: `pw` is a validated PlayerWork base; both fields are in-header.
    let (raw_weight, raw_disp) = unsafe {
        (
            (pw.add(WEIGHT_OFFSET) as *const i32).read_unaligned(),
            pw.add(IS_DISP_WEIGHT_OFFSET).read_unaligned(),
        )
    };

    let weight_seed = if raw_weight == 0 {
        WEIGHT_DEFAULT_WHEN_UNSET
    } else {
        raw_weight.clamp(WEIGHT_MIN, WEIGHT_MAX)
    };
    let disp_seed = if raw_disp != 0 { 1 } else { 0 };

    custom_options::set_value_silent(OPT_WEIGHT, player_side, weight_seed);
    custom_options::set_value_silent(OPT_IS_DISP, player_side, disp_seed);

    log_info!(
        "profile_fields: seeded weight={weight_seed} (raw={raw_weight}) is_disp_weight={disp_seed} (side={player_side})"
    );
}
