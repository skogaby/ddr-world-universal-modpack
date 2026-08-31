//! Training Mode (v1: section practice) — Step-2 surface.
//!
//! The thin real consumer of the song-rate training-arm surface: while the
//! mod is enabled, EVERY eligible song (ordinary solo/doubles, any rate —
//! including 100%) gets a song-rate binding armed, so gestures can seek
//! even on songs entered without bounds pre-set. At 100% the binding is an
//! identity passthrough (`plan_identity_bank` +
//! `ServeMode::IdentityPassthrough` — byte-identical audio, no producer
//! thread, Q31 identity, no movie suppression, no score taint). Arming
//! alone is not an alteration and never taints (design §4.1/§4.2); an
//! armed-but-untouched 100% song submits normally.
//!
//! Eligibility is NOT duplicated here: the mod keeps a STANDING request
//! (`song_rate::runtime::set_training_arm`) and the scene-26 classifier
//! applies the identical gate set it applies to rate arms — course/Dan
//! and unknown sessions fail closed to identity inside `classify_scene26`
//! (training design §4.1; the request weakens nothing). Versus arms since
//! the 2026-08-31 lift: P1 governs (the classifier's versus policy) and
//! the three bound rows are MIRRORED across sides via `versus_mirror`
//! (P1 seeds; last writer wins), so both players share one training
//! session — every alteration (bounds, gestures, loops) moves the ONE
//! shared timeline and taints BOTH entered sides' scores. TIMELINE
//! PLACEMENT mirrors too (one strip — divergent placements were a
//! fiction); AUTOPLAY is the autoplay mod's genuinely per-side row and
//! stays independent.
//!
//! Step 2 adds the A/B pinpad markers ([`bounds`] — the middle row 4-5-6,
//! single-press since 2026-08-18: 4 sets A, 6 sets B, 5 clears, gameplay
//! only, each confirmed by a short bottom-center text toast — the shared
//! [`crate::services::toast`] service) and
//! restart-from-A: `quick_restart_or_fail::trigger_restart` consults
//! [`active_section_start`] and seeks to A behind the
//! [`TRAINING_LEAD_MS`] silent approach instead of restarting at 0.
//!
//! Step 3 adds the section-bound rows: SONG START TIME (s) / SONG END
//! TIME (s) (`training_start_time` / `training_end_time`, 0–200 s, fine
//! 5 / coarse 30, defaults 0 and 200 — both absolute timestamps per the
//! R2 amendment of 2026-08-14, mutually nudged so the play window keeps
//! `MIN_SECTION`), session-scoped via `PersistMode::Session` — they
//! serialize nothing and reset to their defaults at card-in, because
//! section practice is a per-session tool, not a profile preference.
//! Values land in per-side atomics in [`bounds`] for the gameplay-entry
//! bound resolution.
//!
//! Step 4 adds LOOP SONG (`training_loop_song`, a plain Session bool —
//! NOT song-scoped; it survives song switches and resets at card-in):
//! LOOP OFF with a section end writes the ControlMessageActor end
//! thresholds so the game runs its OWN stock tail early (banner →
//! results with the partial stats — [`bounds`]'s apply behind
//! [`section_math::end_policy`]); LOOP ON never writes them — the
//! [`driver`]'s loop leg fires the shipped in-place reset back to the
//! section start at a fire bound clamped strictly below BOTH live
//! thresholds, grinding the section until quick-fail/quick-restart.
//!
//! Step 5 adds score containment (design §4.7/R5): every point a training
//! session alters the current song taints the entered/pressing side in
//! `score_guard` — bound engagement and the loop latch at resolution
//! ([`bounds::try_resolve_row_bounds`]), marker gestures
//! (`bounds::set_marker`), and any completed in-place reset landing at
//! t > 0 or during a session-active song (the
//! [`song_reset::on_song_reset`] subscriber registered in `enable()` —
//! the restart-from-A / altered-song-replay re-taint). The shipped
//! per-stage suppression + sanitised-logout machinery enforces it
//! verbatim; an armed-but-untouched song (and an honest press-1 replay)
//! stays clean.
//!
//! Step 7 adds FF/RW scrobbling (the amended R12): single-press pinpad
//! **7 = rewind** / **9 = fast-forward** by
//! `training_mode.{rw,ff}_increment_ms` (default 5000 ms, normalized
//! 250..=60000 at enable) during eligible gameplay, dispatched through
//! the shipped seek transaction with NO approach lead — a pure timeline
//! adjuster, music-player style (maintainer amendment 2026-08-15; the
//! Step-6 timeline cursor tracks it, and each scrub flashes a
//! [`scrub_indicator`] icon: RW left / FF right, the toast's fade
//! envelope). Any scrub taints the pressing side (AC 7); one transaction
//! in flight across scrub + loop driver.
//!
//! The TRAINING OPTIONS group heading that originally shipped here
//! (Step 8 of
//! `.agents/planning/2026-08-13-training-mode/implementation/plan.md`)
//! now lives in the decorative-option-headers mod, which owns every
//! header row on the MODS tab.

pub mod bounds;
pub mod driver;
mod scrub_indicator;
pub mod section_math;
mod strip_hud;
pub mod strip_synth;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, EnumValue, PersistMode, RegisterSpec, ScalarFormat};
use crate::services::{
    input_manager, scene_manager, score_guard, song_rate, song_reset, stage_records, versus_mirror,
};
use crate::types::buttons::InputEvent;
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

pub use bounds::active_section_start;

/// The standard silent approach lead before section-start content (design
/// §4.3 — a code constant per R12, deliberately not configurable in v1):
/// gives section-start notes scroll-in time, since sections (unlike chart
/// starts) have notes immediately at the seek target.
pub const TRAINING_LEAD_MS: u64 = 2_500;

/// The SONG START TIME bound row (design §4.1, R2 amendment 2026-08-14);
/// label texture `seop_item_training_start_time`. `PersistMode::Session`
/// — serializes nothing, resets at card-in.
const OPT_START_TIME: &str = "training_start_time";
/// The SONG END TIME bound row; label texture
/// `seop_item_training_end_time`.
const OPT_END_TIME: &str = "training_end_time";
/// The LOOP SONG row (Step 4, design §4.1): bool, default OFF, label
/// texture `seop_item_training_loop_song` (+ `seop_image_training_loop_song_{off,on}`
/// previews; the ON/OFF value ribbons are stock atlas sprites). A PLAIN
/// Session row (breakdown decision #3): NOT song-scoped — it survives
/// song switches within a session and takes no part in the highlight
/// seeder or the digest stamp; card-in resets it to OFF.
const OPT_LOOP_SONG: &str = "training_loop_song";
/// Bound-row range/stepping (design §4.1 as amended): 0–200 s absolute
/// timestamps, fine 5, coarse 30. START defaults 0 (natural start); END
/// defaults to the cap (= natural end for every real chart).
const BOUND_ROW_MIN_S: i32 = 0;
const BOUND_ROW_MAX_S: i32 = section_math::BOUND_ROW_MAX_S;
/// The stepper's nudge distance only — the highlight seeder's END seed
/// is whole-second (off this grid; the stepper is add-then-clamp).
const BOUND_ROW_STEP_S: i32 = section_math::BOUND_ROW_STEP_S;
const BOUND_ROW_COARSE_S: i32 = 30;
const START_TIME_DEFAULT_S: i32 = 0;
const END_TIME_DEFAULT_S: i32 = BOUND_ROW_MAX_S;
/// TIMELINE PLACEMENT (Step 6, R7/R11; round-4 UX amendment 2026-08-15:
/// OFF/LEFT/RIGHT — this row ALONE dictates whether the chart-strip HUD
/// renders, replacing the training-session-active visibility predicate;
/// OFF hides it, LEFT/RIGHT pick the screen edge). The ONE training row
/// that persists with the profile (`PersistMode::Full` -> wire
/// `mod_training_progress_pos`). Default OFF (fresh profiles opt in).
const OPT_PROGRESS_POS: &str = "training_progress_pos";
const PLACEMENT_OFF: i32 = 0;
const PLACEMENT_LEFT: i32 = 1;
const PLACEMENT_RIGHT: i32 = 2;
/// The rows mirrored across sides in versus (`versus_mirror`): everything
/// that shapes the ONE shared training timeline, plus TIMELINE PLACEMENT —
/// the HUD is ONE strip, so divergent per-side placements were a fiction
/// (maintainer call 2026-08-31; it is `PersistMode::Full`, so in versus
/// both profiles save the shared placement at logout, consistent with the
/// mirror-wide persistence ruling).
const MIRRORED_OPTIONS: [&str; 4] = [
    OPT_START_TIME,
    OPT_END_TIME,
    OPT_LOOP_SONG,
    OPT_PROGRESS_POS,
];
/// Whether the mod enabled successfully this boot (readiness gate passed).
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// SONG START TIME row change callback: store the side's atomic, nudge
/// the END row up if the edit closed the play window below `MIN_SECTION`
/// (R2 amendment — the sibling write is SILENT: no callback dispatch, no
/// recursion; the row's per-frame render tick re-reads the registry, so
/// the nudged value appears on screen the same frame), then refresh the
/// sticky bind-time pre-shift — the modal is open AT song select, so
/// edits must land on the mapping before scene 26's bank create consumes
/// it.
fn on_start_time_change(side: u8, value: i32) {
    bounds::set_row_start_time(side, value);
    let end = custom_options::get_value(side, OPT_END_TIME).unwrap_or(END_TIME_DEFAULT_S);
    let nudged = section_math::nudge_end_after_start(value, end);
    if nudged != end {
        custom_options::set_value_silent(OPT_END_TIME, side, nudged);
        bounds::set_row_end_time(side, nudged);
    }
    refresh_pre_shift();
    // Versus mirror: propagate the EDITED row; the silent END nudge above
    // re-derives on the other side (its on_change runs the same pure
    // nudge over the same mirrored inputs — copying it would race the
    // re-derivation).
    versus_mirror::mirror_edit(OPT_START_TIME, side, value);
}

/// SONG END TIME row change callback: store the side's atomic and bump
/// the START row down if the edit closed the play window (silent sibling
/// write, same discipline as [`on_start_time_change`]; a START bump also
/// moves the pre-shift, so refresh it too). A value AT the abstract
/// default/cap clears the rows' song stamp — that is the card-in session
/// reset (the new player must see freshly seeded timestamps, not the
/// abstract cap; the stepper cannot reach the cap once the row is
/// re-bounded to a real song, so no user edit trips this).
fn on_end_time_change(side: u8, value: i32) {
    bounds::set_row_end_time(side, value);
    if value >= END_TIME_DEFAULT_S {
        bounds::clear_rows_digest();
    }
    let start = custom_options::get_value(side, OPT_START_TIME).unwrap_or(START_TIME_DEFAULT_S);
    let nudged = section_math::nudge_start_after_end(start, value);
    if nudged != start {
        custom_options::set_value_silent(OPT_START_TIME, side, nudged);
        bounds::set_row_start_time(side, nudged);
        refresh_pre_shift();
    }
    // Versus mirror (see on_start_time_change for the nudge rationale).
    versus_mirror::mirror_edit(OPT_END_TIME, side, value);
}

/// LOOP SONG row change callback: mirror the side's atomic. No nudge, no
/// pre-shift interaction — the loop consumes the same resolved bounds the
/// other rows produce, and latches per song at resolution.
fn on_loop_song_change(side: u8, value: i32) {
    bounds::set_row_loop_song(side, value != 0);
    versus_mirror::mirror_edit(OPT_LOOP_SONG, side, value);
}

/// TIMELINE PLACEMENT row change callback: mirror the side's atomic in
/// strip_hud (applies at the next song — the HUD latches the entered
/// side's value at GAMEPLAY entry). The raw enum value carries straight
/// through (0 OFF / 1 LEFT / 2 RIGHT — strip_hud::Placement's encoding).
fn on_progress_pos_change(side: u8, value: i32) {
    strip_hud::set_placement(side, value);
    versus_mirror::mirror_edit(OPT_PROGRESS_POS, side, value);
}

/// The highlight seeder's write half (R2 second amendment 2026-08-14 —
/// song-scoped bounds): a NEW highlighted song re-bounds and re-seeds
/// both sides' rows to its own timeline. The rows' RANGE itself is
/// clamped to the song — END steps over `[MIN_SECTION, seed_end]` and
/// START over `[0, seed_end − MIN_SECTION]`, where `seed_end` is the
/// song's length rounded UP to the next whole second
/// ([`section_math::seed_end_seconds`]: on the chart-derived path this
/// equals the music wheel's LENGTH readout exactly — 2026-08-18 UX
/// amendment removing the old 5 s pad — and stays at/above the real end,
/// so the row cannot even express a timestamp past the song) — then the
/// values re-seed to START 0 / END =
/// `seed_end`, and the rows are stamped with the song's digest. The
/// bounds/stepper/position-marker all read the registry live, so an open
/// menu repaints the same frame. Registry value writes are SILENT (no
/// callback dispatch) with the bounds atomics mirrored manually, exactly
/// like the nudge writes; `set_scalar_bounds`' own clamp dispatches are
/// harmless (the seed overwrites right after). The pre-shift refresh at
/// the end retires any previous song's mapping (START is 0 again).
pub(crate) fn seed_rows_for_highlight(digest: u64, audio_len_ms: u32) {
    let seed_end = section_math::seed_end_seconds(audio_len_ms);
    // Per-song row ranges (degenerate lengths keep a non-inverted range;
    // parse rejects zero-duration banks so seed_end >= MIN_SECTION for
    // every real publication).
    let start_max = (seed_end - section_math::MIN_SECTION_S).max(0);
    let end_min = section_math::MIN_SECTION_S.min(seed_end);
    custom_options::set_scalar_bounds(OPT_START_TIME, BOUND_ROW_MIN_S, start_max);
    custom_options::set_scalar_bounds(OPT_END_TIME, end_min, seed_end);
    for side in 0u8..2 {
        custom_options::set_value_silent(OPT_START_TIME, side, START_TIME_DEFAULT_S);
        custom_options::set_value_silent(OPT_END_TIME, side, seed_end);
        bounds::set_row_start_time(side, START_TIME_DEFAULT_S);
        bounds::set_row_end_time(side, seed_end);
    }
    bounds::stamp_rows(digest, seed_end);
    refresh_pre_shift();
}

/// The side whose SONG START TIME row drives the upcoming song's
/// pre-shift: the GOVERNING side (design §4.2's side-choice class —
/// doubles/solo carry exactly one entered side; versus carries two and
/// P1 governs, matching the scene-26 classifier — the bound rows are
/// mirrored across sides by `versus_mirror`, so the choice is
/// value-neutral). Entered state unavailable ⇒ P1-preferring side with a
/// nonzero start (assist_tick's "P1 or the only enabled side" class —
/// correct for every eligible session shape).
fn pre_shift_side() -> Option<usize> {
    let entered = [
        stage_records::side_entered(0),
        stage_records::side_entered(1),
    ];
    match (entered[0], entered[1]) {
        (Some(true), Some(false)) => Some(0),
        (Some(false), Some(true)) => Some(1),
        (Some(true), Some(true)) => Some(0),
        _ => (0..2).find(|&side| bounds::row_start_time(side) > 0),
    }
}

/// Keep the sticky bind-time pre-shift current (design §4.3/R15): the
/// effective (audio-length-clamped) SONG START TIME seconds of the
/// governing side, converted content→wall at the side's DESIRED rate (the
/// committed exact ratio does not exist yet; the driver's adjust
/// re-derives from the live mapping, so the epsilon never reaches the
/// clock), with the standard approach lead. Start 0 / no side ⇒ `(0, 0)`
/// (no mapping — the bind stays stock). Refreshed on start-row edits and
/// at the scene 25/26 boundaries (the latter also catches SONG SPEED
/// edits changing the wall conversion).
fn refresh_pre_shift() {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let armed = pre_shift_side().map(|side| {
        let audio_len = song_rate::selected_song::selected_song().map(|info| info.audio_len_ms);
        let start_s =
            section_math::effective_bound_seconds(bounds::row_start_time(side), audio_len);
        let content_ms = u64::from(start_s.max(0) as u32) * 1_000;
        let percent = song_rate::runtime::desired_percent(side);
        (
            section_math::pre_shift_wall_ms(content_ms, percent),
            TRAINING_LEAD_MS,
        )
    });
    match armed {
        Some((shift_wall_ms, lead_ms)) if shift_wall_ms > 0 => {
            // Stamped with the rows' song (R2 second amendment): the bind
            // declines the mapping if it creates a DIFFERENT song than
            // the rows describe (the fast-confirm race).
            song_rate::runtime::set_initial_content_mapping_ms(
                shift_wall_ms,
                lead_ms,
                bounds::rows_digest(),
            );
        }
        _ => {
            song_rate::runtime::set_initial_content_mapping_ms(0, 0, 0);
        }
    }
}

/// Scene hook for the pre-shift refresh (called from `bounds`'s scene
/// callback — one scene callback per mod): entries to SONG SELECT and to
/// the stage-loading interstitial (the last boundary before the bank
/// create consumes the mapping).
pub(crate) fn on_scene_for_pre_shift(next: i32) {
    if next == scene::SONG_SELECT || next == scene::SONG_TO_STAGE_INTERSTITIAL {
        refresh_pre_shift();
    }
}

/// Register the two section-bound rows (design §4.1). Best-effort: row
/// injection being unavailable degrades to the Step-2 gesture-only surface
/// (one WARN) rather than refusing the whole mod — unlike SONG SPEED, the
/// mod is useful without its rows. `Duplicate` on re-enable is success.
fn register_bound_rows() -> bool {
    if !custom_options::row_injection_available() {
        log_warn!(
            "TrainingMode: custom-option row injection unavailable -- SONG START/END TIME rows absent (gestures still work)"
        );
        return false;
    }
    for (id, display, desc, min, max, default, on_change) in [
        (
            OPT_START_TIME,
            "Song Start Time",
            "Training Mode: where the song starts",
            BOUND_ROW_MIN_S,
            // START can never sit closer than MIN_SECTION to the end.
            BOUND_ROW_MAX_S - section_math::MIN_SECTION_S,
            START_TIME_DEFAULT_S,
            on_start_time_change as fn(u8, i32),
        ),
        (
            OPT_END_TIME,
            "Song End Time",
            "Training Mode: where the song ends",
            // The lowest valid section end (START floor 0 + MIN_SECTION).
            section_math::MIN_SECTION_S,
            BOUND_ROW_MAX_S,
            END_TIME_DEFAULT_S,
            on_end_time_change,
        ),
    ] {
        let spec =
            RegisterSpec::scalar(id, min, max, BOUND_ROW_STEP_S, ScalarFormat::MinutesSeconds)
                .display_name(display)
                .description(desc)
                .step_coarse(BOUND_ROW_COARSE_S)
                .default_value(default)
                .persist_mode(PersistMode::Session)
                .on_change(on_change);
        match custom_options::register_option(spec) {
            Ok(_) => {}
            Err(custom_options::RegisterError::Duplicate { .. }) => {}
            Err(error) => {
                log_warn!(
                    "TrainingMode: {id} registration failed ({error:?}) -- bound rows degraded"
                );
                return false;
            }
        }
    }
    // The LOOP SONG toggle (Step 4) — same Session/degradation shape as
    // the bound rows; registered after them so the MODS tab reads
    // START / END / LOOP top to bottom.
    let loop_spec = RegisterSpec::bool_toggle(OPT_LOOP_SONG)
        .display_name("Loop Song")
        .description("Training Mode: loop the selected section until you exit")
        .default_value(0)
        .persist_mode(PersistMode::Session)
        .on_change(on_loop_song_change);
    match custom_options::register_option(loop_spec) {
        Ok(_) | Err(custom_options::RegisterError::Duplicate { .. }) => {}
        Err(error) => {
            log_warn!(
                "TrainingMode: {OPT_LOOP_SONG} registration failed ({error:?}) -- bound rows degraded"
            );
            return false;
        }
    }
    // TIMELINE PLACEMENT (Step 6; round-4 amendment): OFF/LEFT/RIGHT,
    // default OFF — this row ALONE controls the HUD's visibility (OFF
    // hides it; LEFT/RIGHT show it on that edge). All three ribbons are
    // stock atlas entries. The one training row that persists with the
    // profile. Registration failure degrades to OFF (one WARN) — the
    // HUD simply stays hidden without the row.
    let placement_spec = RegisterSpec::enum_values(
        OPT_PROGRESS_POS,
        vec![
            EnumValue::with_display(PLACEMENT_OFF, "seop_op_off", "OFF"),
            EnumValue::with_display(PLACEMENT_LEFT, "seop_op_left", "LEFT"),
            EnumValue::with_display(PLACEMENT_RIGHT, "seop_op_right", "RIGHT"),
        ],
    )
    .display_name("Timeline Placement")
    .description("Training Mode: which screen edge shows the timeline HUD")
    .default_value(PLACEMENT_OFF)
    .on_change(on_progress_pos_change);
    match custom_options::register_option(placement_spec) {
        Ok(_) | Err(custom_options::RegisterError::Duplicate { .. }) => {}
        Err(error) => {
            log_warn!(
                "TrainingMode: {OPT_PROGRESS_POS} registration failed ({error:?}) -- HUD stays hidden"
            );
        }
    }

    // Seed the per-side atomics from the registry (re-enable path:
    // Duplicate does not re-fire callbacks and disable() reset the
    // atomics). Plain setter seeding — the registry pair is always
    // window-consistent, so the callbacks' nudge logic has nothing to do
    // here and silent writes during registration are avoided.
    for side in 0u8..2 {
        bounds::set_row_start_time(
            side,
            custom_options::get_value(side, OPT_START_TIME).unwrap_or(START_TIME_DEFAULT_S),
        );
        bounds::set_row_end_time(
            side,
            custom_options::get_value(side, OPT_END_TIME).unwrap_or(END_TIME_DEFAULT_S),
        );
        bounds::set_row_loop_song(
            side,
            custom_options::get_value(side, OPT_LOOP_SONG).unwrap_or(0) != 0,
        );
        strip_hud::set_placement(
            side,
            custom_options::get_value(side, OPT_PROGRESS_POS).unwrap_or(PLACEMENT_OFF),
        );
    }
    custom_options::set_option_available(OPT_START_TIME, true);
    custom_options::set_option_available(OPT_END_TIME, true);
    custom_options::set_option_available(OPT_LOOP_SONG, true);
    custom_options::set_option_available(OPT_PROGRESS_POS, true);
    true
}

/// Taint the entered side(s)' per-stage save (Step 5, design §4.7/R5) —
/// the `on_song_reset(t > 0)` subscriber's body, and (since the
/// 2026-08-31 versus-training lift) the shared taint target for EVERY
/// training alteration: gestures, loop latches, and row-derived bounds
/// all move the ONE shared timeline, so in versus BOTH entered sides'
/// scores are altered regardless of which player pressed/resolved. Runs
/// on the frame thread: guarded reads + atomic stores only (no locks, no
/// allocation, panic-free).
///
/// Prefers exactly the sides `stage_records::side_entered` reports true
/// (solo/doubles carry one entered side; versus carries two); when no
/// side positively reads entered — decode unavailable or torn — both
/// sides taint conservatively (idempotent; a clean side's flag clears at
/// the next song's `reset_song_taint`).
pub(super) fn taint_entered_sides() {
    let entered = [
        stage_records::side_entered(0),
        stage_records::side_entered(1),
    ];
    if entered.iter().any(|side| *side == Some(true)) {
        for (side, was_entered) in entered.iter().enumerate() {
            if *was_entered == Some(true) {
                score_guard::set_training_taint(side);
            }
        }
    } else {
        score_guard::set_training_taint(0);
        score_guard::set_training_taint(1);
    }
}

pub struct TrainingModeMod {
    input_cb: Option<usize>,
    scene_cb: Option<usize>,
    reset_cb: Option<usize>,
}

impl TrainingModeMod {
    pub fn new() -> Self {
        Self {
            input_cb: None,
            scene_cb: None,
            reset_cb: None,
        }
    }
}

impl Mod for TrainingModeMod {
    fn id(&self) -> &str {
        "training-mode"
    }
    fn name(&self) -> &str {
        "Training Mode"
    }
    fn description(&self) -> &str {
        "Section practice: skip/omit/loop a song section with live gestures (v1 in progress)"
    }
    fn required_signatures(&self) -> &[&str] {
        // The mod consumes the song-rate service surface (atomics only);
        // readiness is gated live in enable() instead of at registration.
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        // Chart-strip timeline (Step 6): resolve the strip's optional
        // color-sourcing anchors (selector AOB + RTTI vtables). Every
        // miss degrades the strip, never the mod.
        strip_hud::init(_ctx.signatures);
        true
    }

    fn enable(&mut self) {
        // Without the streaming integration an arm request would EarlyFail
        // every song (one refusal WARN each) — refuse to enable instead,
        // exactly like the SONG SPEED row hides itself.
        if !song_rate::runtime::integration_ready() {
            log_warn!(
                "TrainingMode: song_rate streaming integration unavailable -- refusing to enable"
            );
            return;
        }

        song_rate::runtime::set_training_arm(true);

        // Section-bound rows (Step 3): SONG START/END TIME, session-
        // scoped. Best-effort — the gesture surface below works without them.
        let rows = register_bound_rows();

        // A/B gesture surface (Step 2) + FF/RW scrub (Step 7): input +
        // scene callbacks; both no-op unless GESTURES_ACTIVE and the
        // scene is GAMEPLAY. The scrub increments latch from
        // `training_mode.{ff,rw}_increment_ms` here (normalized, one
        // INFO when out of range); the indicator icons pre-load so even
        // the first flash shows.
        bounds::load_scrub_increments();
        scrub_indicator::prime();
        if input_manager::is_available() && scene_manager::is_available() {
            let id = input_manager::on_input_event(Arc::new(|event: &InputEvent| {
                bounds::on_input_event(event);
            }));
            self.input_cb = Some(id);
            let id = scene_manager::on_scene_change(Box::new(bounds::on_scene_change));
            self.scene_cb = Some(id);
            bounds::GESTURES_ACTIVE.store(true, Ordering::Release);
        } else {
            log_warn!(
                "TrainingMode: input/scene manager unavailable -- A/B gestures inactive (arming still on)"
            );
        }

        // Score containment (Step 5, design §4.7/R5): any completed reset
        // landing at t > 0 means the run plays altered content — the
        // LOAD-BEARING restart-from-A re-taint (`trigger_restart` calls
        // `score_guard::reset_song_taint()` at the trigger, BEFORE the seek
        // to A lands; this subscriber re-taints on completion). Also covers
        // the silent-start adjust and loop iterations to A > 0 uniformly.
        //
        // A t = 0 reset re-taints only while the per-song session-active
        // latch is set (design §4.1's predicate, latched per song): a plain
        // press-1 replay of an UNTOUCHED song stays clean and submits —
        // but a press-1 during a session-altered song (a B-only early end
        // whose truncated thresholds persist across the in-place reset, or
        // a whole-song grind) must not launder the taint the trigger's
        // `reset_song_taint()` just wiped. Fail-closed on score integrity.
        self.reset_cb = Some(song_reset::on_song_reset(|t_ms| {
            if t_ms > 0 || bounds::training_session_active() {
                taint_entered_sides();
            }
        }));

        ACTIVE.store(true, Ordering::Release);
        // Chart-strip timeline HUD (Step 6): scene/judge lifecycle. All
        // failures inside are fail-open (design §6) — the session never
        // blocks on the strip.
        strip_hud::activate();
        // Seed the pre-shift from the just-seeded row atomics (a re-enable
        // mid-session may already hold a nonzero skip).
        refresh_pre_shift();
        // Versus mirror (2026-08-31 lift): the bound rows hold one shared
        // value while both sides are entered — P1 seeds, edits propagate
        // via the on_change tails. Registered last so an already-engaged
        // mirror's immediate P1→P2 seed lands on registered rows.
        versus_mirror::register(&MIRRORED_OPTIONS);
        log_info!(
            "TrainingMode: enabled -- eligible songs arm a binding; press 4/5/6 to set A/clear/B and 7/9 to scrub RW/FF during gameplay, press 1 restarts from A; bound rows {}",
            if rows { "available" } else { "absent" }
        );
    }

    fn disable(&mut self) {
        // Zero footprint when off: no standing arm request, no pre-shift,
        // no gestures, no markers, no toast, no bound rows — ordinary 100%
        // plays are literally stock again.
        versus_mirror::unregister(&MIRRORED_OPTIONS);
        bounds::GESTURES_ACTIVE.store(false, Ordering::Release);
        bounds::clear_session_state("mod disabled");
        crate::services::toast::dismiss();
        scrub_indicator::dismiss();
        if let Some(id) = self.input_cb.take() {
            input_manager::remove_callback(id);
        }
        if let Some(id) = self.scene_cb.take() {
            scene_manager::remove_callback(id);
        }
        if let Some(id) = self.reset_cb.take() {
            song_reset::remove_callback(id);
        }
        // Hide the rows (next form rebuild) and zero the consumer atomics;
        // the registry values survive for a re-enable's re-seed.
        custom_options::set_option_available(OPT_START_TIME, false);
        custom_options::set_option_available(OPT_END_TIME, false);
        custom_options::set_option_available(OPT_LOOP_SONG, false);
        custom_options::set_option_available(OPT_PROGRESS_POS, false);
        for side in 0u8..2 {
            bounds::set_row_start_time(side, START_TIME_DEFAULT_S);
            bounds::set_row_end_time(side, END_TIME_DEFAULT_S);
            bounds::set_row_loop_song(side, false);
        }
        song_rate::runtime::set_training_arm(false);
        song_rate::runtime::set_initial_content_mapping_ms(0, 0, 0);
        strip_hud::deactivate();
        ACTIVE.store(false, Ordering::Release);
        log_info!("TrainingMode: disabled");
    }

    fn is_active(&self) -> bool {
        ACTIVE.load(Ordering::Acquire)
    }
}
