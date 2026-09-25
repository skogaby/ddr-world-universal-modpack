//! Multiplayer Bot — a computer-controlled VERSUS opponent for a single player.
//!
//! A player who cards in alone and turns on BOT OPPONENT plays a genuine 2P VERSUS song: the
//! empty pad becomes a second player whose steps are judged by the game's own judge, drawn by
//! the game's own versus HUD and shown on both results panes. The opponent plays at a skill
//! level 1..=10, or — the **Target Score** value — replays the human's loaded pacemaker
//! ghost note for note, so it finishes on the target's exact score. Mod id `multiplayer-bot`,
//! default ON (not in `DEFAULT_OFF_MODS`); inert until a player turns the row on.
//!
//! ## Phantom-side governance — READ THIS BEFORE WRITING A CABINET-WIDE POLICY
//!
//! During a bot song BOTH sides read `stage_records::side_entered == Some(true)`, but the
//! bot side's option values are whatever the last real player on that pad left in the JSON
//! cache (`versus_mirror` never engages — it only acts at song select with both sides
//! entered). Any policy that folds both sides' option values into one cabinet-wide decision
//! ("P1 governs when both entered", "any entered side ON") MUST treat a side for which
//! [`is_bot_side`] is `true` as not entered, or a human on P2 plays under the previous P1
//! player's preferences. Current consumers: premium_free, training_mode, auto-calibration's
//! census, assist_tick, announcer_mute and ddr_selection. Any new fold must add the same
//! exclusion.
//!
//! ## Mechanism
//!
//! The only detour is `extra_stage_guard`. Everything else rides existing services:
//!
//! - **Impersonation** (`impersonation.rs`, pure edge rules in `session.rs`): at the
//!   song-select → stage edge (0-idx 25 → 26/27/28; scene callbacks fire before the next
//!   sequence is built) `eligibility::evaluate` must pass: exactly one entered side, SINGLE,
//!   not a course, not event mode, not already versus, and the ENTERED side's row ON (the
//!   level also comes from the human's side). `apply` then probes every pointer, refuses unless
//!   the commit prepared both stage records for the same song, mirrors the chart identity onto
//!   the bot side, copies the human's lane `Option` fields with the gauge forced NORMAL, writes
//!   the name plate (`BOT LV<n>` / `TARGET`), sets `PlayerWork+0x4 = 1` and the `GameWork`
//!   versus word, pans SEs by side (cosmetic), arms the controller and taints the bot side. The
//!   game builds the second `GamePlayActor` natively. The first scene outside the play window
//!   {26..=30} restores the snapshot; GAMEPLAY re-entry and an in-place `song_reset` re-seed
//!   the bot. A 20 s render-thread watchdog WARNs if no scene change follows the flip.
//! - **Controller**: `services::foot_panel_swap` is the one owner of the `judgeNotes`
//!   foot-panel slot swap. `foot_panel_swap::arm_bot(bot, filler::fill)` sets that side's
//!   controller to `Bot`, which outranks `Perfect` (a stale cached `autoplay = ON` on the bot's
//!   pad loses). Its `BotFootPanel` reports press ages so the judge's event lands exactly where
//!   the planner decided — offset-agnostic on every build. Never add another judge subscriber
//!   that touches the foot-panel slot.
//! - **Planner + skill**: `filler.rs` (game thread, judge pre-callback) turns the actor's
//!   Results into `NoteView`s and runs `planner::plan_frame`, which enforces the judge's
//!   ordering rules (one live event per panel per frame, per-panel monotonic events, a decided
//!   Miss blocks the next same-panel note). Per-note decisions come from `skill.rs` (lean +
//!   two-regime jitter + per-note miss, per level), or for Target Score from `ghost.rs` over
//!   the bytes `ghost_source.rs` reads from the human's `GhostActor`. With no usable ghost the
//!   song plays at [`GHOST_FALLBACK_LEVEL`] with one WARN and a toast. The filler also counts
//!   planner-vs-judge grade mismatches for the restore tally.
//! - **Extra-stage guard** (`extra_stage_guard.rs`): one detour on `extra_stage_grant` clears
//!   the bot's entered byte around the original, so the grant's "every entered side AAA'd"
//!   rule ignores the bot. The bot can never add a grant.
//! - **Self-test** (`self_test.rs`, dev only: `layeredfs.developer_mode` plus
//!   `DDR_BOT_SELF_TEST=<1..10>`): arms the bot on the human's own lane to prove the
//!   controller on a build (`mismatch=0` in the tally).
//!
//! ## Score integrity
//!
//! The bot side carries `score_guard::set_autoplay_taint` for the whole window, so its
//! per-stage save is suppressed; at restore the taint is re-synced to "autoplay is really on
//! for that side". The human's play is never tainted by the bot. Init refuses (mod inactive)
//! when `score_guard` is unavailable — fail-closed.
//!
//! ## Invariants
//!
//! - `apply` keeps a copy of every byte it writes; a failure after the first write (the
//!   controller refusing to arm) undoes all of it. Every game pointer is
//!   `memory::is_readable`-probed first.
//! - Never gate on a side's row value alone — per-side values outlive the player; gate on the
//!   entered side.
//! - The impersonation's scene hook runs before the self-test's in the mod's one scene
//!   callback, and never holds its state lock across a call into another service.
//! - The filler is on the judge hot path: panic-free, allocation-free after a song's first
//!   frame, one `try_lock` per frame.
//!
//! ## Degradation
//!
//! No required signatures. `init` makes the mod inactive (`is_active() == false`) unless
//! `foot_panel_swap` (with its bot objects), `stage_records` (with the player `Option`
//! offset), `scene_manager` and `score_guard` are all available. A missing gate input
//! refuses every song with one WARN. A missing `extra_stage_grant` leaves the stock grant
//! rule (one WARN). A missing ghost source makes every Target Score song fall back to
//! [`GHOST_FALLBACK_LEVEL`]. Without `custom_options` there are no rows, so the bot stays off.
//!
//! ## Config and option rows
//!
//! No mod-config.json section. Per-player rows [`OPT_ID`] ("Bot Opponent (1P Only)") and
//! child [`OPT_LEVEL_ID`] ("Bot Level", 1..=11, default [`DEFAULT_LEVEL`], shown when the
//! parent is ON, rendered as `Level 1`…`Level 10` / `Target Score` text via
//! `ScalarFormat::Labeled`), both `PersistMode::Local`: kept in the JSON cache, never on the
//! wire; a cached level outside 1..=11 is clamped on load. Labels come from
//! `scripts/option_strings.py`.
//!
//! RE: `docs/multiplayer_bot_research.md`, `docs/gauge_and_judge_scoring_research.md`. Host
//! tests: `scripts/validate_multiplayer_bot.sh` (= `cargo test` in `tools/bot_sim/`, which
//! `#[path]`-mounts the pure `eligibility`, `ghost`, `planner`, `session` and `skill` files).
//! Offline simulator / tuning report: `scripts/bot_sim.sh <ssq-dir>`.

pub mod eligibility;
pub mod extra_stage_guard;
pub mod filler;
pub mod ghost;
pub mod ghost_source;
pub mod impersonation;
pub mod planner;
pub mod self_test;
pub mod session;
pub mod skill;

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{
    self, PersistMode, RegisterError, RegisterSpec, ScalarFormat, ShowWhen,
};
use crate::services::{foot_panel_swap, scene_manager, score_guard, song_reset, stage_records};
use crate::{log_info, log_warn};

pub use eligibility::BotMode;

/// `init` succeeded (every prerequisite service is up) — `is_active`.
static CAPABLE: AtomicBool = AtomicBool::new(false);

/// Option id of the bool parent row (JSON cache only — never on the wire).
pub const OPT_ID: &str = "bot_opponent";
/// Option id of the level child row (JSON cache only — never on the wire).
/// Values `1..=10` are skill levels, `11` the Target Score replay.
pub const OPT_LEVEL_ID: &str = "bot_opponent_level";
/// Default BOT LEVEL (design R1).
pub const DEFAULT_LEVEL: i32 = 5;
/// The level a Target Score song plays at when no usable ghost exists.
pub const GHOST_FALLBACK_LEVEL: u8 = 10;

/// Per-side BOT OPPONENT choice, written by the option row's change callback.
/// Only the ENTERED side's value matters — per-side option values outlive the
/// player (the JSON cache primes both sides), so consumers gate on
/// `stage_records::side_entered` and never on this alone.
static OPTION_ON: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// Per-side BOT LEVEL row value (`1..=11`), clamped on store.
static LEVEL: [AtomicI32; 2] = [AtomicI32::new(DEFAULT_LEVEL), AtomicI32::new(DEFAULT_LEVEL)];

/// The side's BOT OPPONENT row value (`false` for an out-of-range side).
pub fn option_on(side: usize) -> bool {
    OPTION_ON
        .get(side)
        .map(|a| a.load(Ordering::Acquire))
        .unwrap_or(false)
}

/// The side's BOT LEVEL row value, clamped to the row's range (default for
/// an out-of-range side).
pub fn level_value(side: usize) -> i32 {
    let raw = LEVEL
        .get(side)
        .map(|a| a.load(Ordering::Acquire))
        .unwrap_or(DEFAULT_LEVEL);
    eligibility::clamp_value(raw)
}

/// The side's bot mode — the decoded level row.
pub fn mode(side: usize) -> BotMode {
    BotMode::from_value(level_value(side))
}

/// Whether `side` is currently the PHANTOM player — the impersonated bot.
///
/// During a bot session `stage_records::side_entered` reads `Some(true)`
/// for BOTH sides (the flip is what makes the game build a second player),
/// but `versus_mirror` never engages (it only engages at song select with
/// both sides entered), so the bot side's option rows are whatever the last
/// real player on that pad left in the JSON cache. Every cabinet-wide policy
/// that folds both sides' option values into one decision by "P1 governs
/// when both entered" (premium free, training pre-shift / loop latch,
/// auto-calibration's census, assist tick's latch, announcer mute) must treat
/// a side for which this is `true` as NOT entered, or a human on P2 plays
/// under the previous P1 player's preferences. Lock-free.
pub fn is_bot_side(side: usize) -> bool {
    impersonation::active_bot_side() == Some(side)
}

/// Change callback of the BOT OPPONENT row. Fires for both sides at
/// registration (from a non-render thread) and on every edit; the body is
/// an atomic store plus one INFO — it must never panic (a panic permanently
/// no-ops the callback).
fn on_option_change(player_side: u8, new_value: i32) {
    let Some(slot) = OPTION_ON.get(player_side as usize) else {
        return;
    };
    let on = new_value != 0;
    slot.store(on, Ordering::Release);
    log_info!(
        "MultiplayerBot: side={} BOT OPPONENT {}",
        player_side,
        if on { "ON" } else { "OFF" }
    );
}

/// Change callback of the BOT LEVEL row — clamps on store (a stale cached
/// value can only ever land on a legal value).
fn on_level_change(player_side: u8, new_value: i32) {
    let Some(slot) = LEVEL.get(player_side as usize) else {
        return;
    };
    slot.store(eligibility::clamp_value(new_value), Ordering::Release);
}

/// Load-side persistence transform of the BOT LEVEL row: a JSON cache value
/// outside `1..=11` lands on the nearest legal value.
fn clamp_level_transform(_id: &str, value: i32) -> i32 {
    eligibility::clamp_value(value)
}

fn identity_transform(_id: &str, value: i32) -> i32 {
    value
}

/// Register the two option rows (design §4.3 / R1). Parent first — the
/// framework validates `ShowWhen` parents synchronously. A `Duplicate` is a
/// re-enable: reseed the atomics from the registry (the duplicate path does
/// not re-fire `on_change`) and show the rows again. Every failure is
/// fail-open: the row is the feature's only enable source, so a missing row
/// simply leaves the bot off.
fn register_rows() {
    if !custom_options::is_available() {
        log_warn!(
            "MultiplayerBot: custom_options unavailable -- no enable source, bot rows absent"
        );
        return;
    }
    let spec = RegisterSpec::bool_toggle(OPT_ID)
        .display_name("Bot Opponent (1P Only)")
        .description("Play VERSUS against a computer opponent on the empty pad")
        .default_value(0)
        .persist_mode(PersistMode::Local)
        .on_change(on_option_change);
    match custom_options::register_option(spec) {
        Ok(_handle) => {
            log_info!("MultiplayerBot: registered BOT OPPONENT (1P ONLY) option");
        }
        Err(RegisterError::Duplicate { .. }) => {
            for side in 0..2u8 {
                on_option_change(side, custom_options::get_value(side, OPT_ID).unwrap_or(0));
            }
            custom_options::set_option_available(OPT_ID, true);
        }
        Err(e) => {
            log_warn!(
                "MultiplayerBot: BOT OPPONENT row registration failed: {e} -- no enable source, bot stays off"
            );
            return;
        }
    }
    register_level_row();
}

/// Register the BOT LEVEL child row. Only after the parent is known
/// registered, and only when the scalar-row machinery is up (bool rows need
/// no scalar donor, so the parent can exist while this row cannot); missing
/// ⇒ the level stays at its default.
fn register_level_row() {
    if !custom_options::row_injection_available() {
        log_warn!(
            "MultiplayerBot: scalar row machinery unavailable -- BOT LEVEL row absent, level stays at {}",
            DEFAULT_LEVEL
        );
        return;
    }
    // An enum-like selector on the scalar donor: the value layer renders
    // TEXT (`Level 1`..`Level 10`, `Target Score`) — no per-value chip
    // textures. Fine == coarse step so Start-held presses step one value.
    let spec = RegisterSpec::scalar(
        OPT_LEVEL_ID,
        eligibility::MIN_LEVEL as i32,
        eligibility::TARGET_VALUE,
        1,
        ScalarFormat::Labeled {
            prefix: "Level ",
            terminal_value: eligibility::TARGET_VALUE,
            terminal_label: "Target Score",
        },
    )
    .display_name("Bot Level")
    .description(
        "Level 1 = beginner, Level 10 = expert; Target Score replays your pacemaker target",
    )
    .default_value(DEFAULT_LEVEL)
    .show_when(ShowWhen::Equals {
        parent_id: OPT_ID.into(),
        value: 1,
    })
    .persist_mode(PersistMode::Local)
    .persist_transform(identity_transform, clamp_level_transform)
    .on_change(on_level_change);
    match custom_options::register_option(spec) {
        Ok(_handle) => {
            log_info!("MultiplayerBot: registered BOT LEVEL option under BOT OPPONENT");
        }
        Err(RegisterError::Duplicate { .. }) => {
            for side in 0..2u8 {
                on_level_change(
                    side,
                    custom_options::get_value(side, OPT_LEVEL_ID).unwrap_or(DEFAULT_LEVEL),
                );
            }
            custom_options::set_option_available(OPT_LEVEL_ID, true);
        }
        Err(e) => {
            log_warn!(
                "MultiplayerBot: BOT LEVEL row registration failed: {e} -- level stays at {}",
                DEFAULT_LEVEL
            );
        }
    }
}

pub struct MultiplayerBotMod {
    scene_cb: Option<usize>,
    reset_cb: Option<usize>,
}

impl MultiplayerBotMod {
    pub fn new() -> Self {
        Self {
            scene_cb: None,
            reset_cb: None,
        }
    }
}

impl Default for MultiplayerBotMod {
    fn default() -> Self {
        Self::new()
    }
}

impl Mod for MultiplayerBotMod {
    fn id(&self) -> &str {
        "multiplayer-bot"
    }
    fn name(&self) -> &str {
        "Multiplayer Bot"
    }
    fn description(&self) -> &str {
        "Play VERSUS against a computer opponent: a 1-10 skill level, or a replay of your pacemaker target"
    }
    fn required_signatures(&self) -> &[&str] {
        // Everything is reached through already-derived services; checked
        // at init so the mod degrades to "absent" instead of panicking.
        &[]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let mut ok = true;
        if !foot_panel_swap::is_available() {
            log_warn!("MultiplayerBot: foot_panel_swap service unavailable -- mod inactive");
            ok = false;
        } else if !foot_panel_swap::bot_objects_ready() {
            log_warn!("MultiplayerBot: bot panel objects unavailable -- mod inactive");
            ok = false;
        }
        if !stage_records::is_available() {
            log_warn!("MultiplayerBot: stage_records unavailable -- mod inactive");
            ok = false;
        } else if stage_records::player_option_offset().is_none() {
            log_warn!("MultiplayerBot: player Option offset underived -- mod inactive");
            ok = false;
        }
        if !scene_manager::is_available() {
            log_warn!("MultiplayerBot: scene_manager unavailable -- mod inactive");
            ok = false;
        }
        if !score_guard::is_available() {
            log_warn!("MultiplayerBot: score guard unavailable -- mod inactive (fail-closed)");
            ok = false;
        }
        // Optional: the extra-stage guard's signature (fail-open, its own WARN).
        extra_stage_guard::init(ctx.signatures);
        // Optional: the Target Score ghost source (fail-open — a miss means
        // every Target song falls back to LV10 with a WARN + toast).
        ghost_source::init(ctx.signatures);
        CAPABLE.store(ok, Ordering::Release);
        ok
    }

    fn enable(&mut self) {
        if !CAPABLE.load(Ordering::Acquire) {
            return;
        }
        register_rows();
        extra_stage_guard::enable();
        let self_test = self_test::configure();
        if self.scene_cb.is_none() {
            // The impersonation runs FIRST: its flip must land on the same
            // 25 -> 26 edge, and the self-test's GAMEPLAY arm skips a side
            // whose controller is already the bot's.
            self.scene_cb = Some(scene_manager::on_scene_change(Box::new(|prev, next| {
                impersonation::on_scene_change(prev, next);
                self_test::on_scene_change(prev, next);
            })));
        }
        if self.reset_cb.is_none() && song_reset::is_available() {
            self.reset_cb = Some(song_reset::on_song_reset(|t_ms| {
                impersonation::on_song_reset(t_ms);
                self_test::on_song_reset(t_ms);
            }));
        }
        log_info!(
            "MultiplayerBot: enabled (bot rows + impersonation ready; extra-stage guard {}; self-test {})",
            if extra_stage_guard::is_installed() { "on" } else { "OFF (stock rule)" },
            if self_test != 0 { "ARMED" } else { "off" }
        );
    }

    fn disable(&mut self) {
        // There is no unregister; hide both rows until the next enable
        // (which hits `Duplicate` and shows them again).
        custom_options::set_option_available(OPT_ID, false);
        custom_options::set_option_available(OPT_LEVEL_ID, false);
        impersonation::shutdown();
        extra_stage_guard::disable();
        self_test::shutdown();
        if let Some(id) = self.scene_cb.take() {
            scene_manager::remove_callback(id);
        }
        if let Some(id) = self.reset_cb.take() {
            song_reset::remove_callback(id);
        }
        log_info!("MultiplayerBot: disabled");
    }

    fn is_active(&self) -> bool {
        CAPABLE.load(Ordering::Acquire)
    }
}
