//! Song Playback Speed — the player-facing SONG SPEED option.
//!
//! One scalar custom-option row (`song_speed`, 25%..=175% in steps of 5,
//! coarse step 10, default 100, `PersistMode::Full`) whose normalized
//! per-side values feed the song-rate service's desired-percent atomics
//! (`services::song_rate::runtime::set_desired_percent`), plus two boolean
//! child rows shown only while that side's SONG SPEED is not 100%
//! (`ShowWhen::NotEquals`): `preserve_pitch` ("PRESERVE SONG PITCH",
//! default ON) — OFF selects the plain-resample DSP mode (pitch follows
//! the rate) instead of the pitch-preserving WSOLA stretch — and
//! `sync_movie` ("SYNC BACKGROUND VIDEO", default OFF) — ON keeps the
//! background movie at non-100% rates and rate-locks it to the audio
//! via movie_sync's scaled reference-clock proxy (Windows and
//! CrossOver/Wine alike; under Wine, movies exist at all only in the
//! non-native mod's fallback movie mode) instead of suppressing it. The mod supplies
//! ONLY desired policy: the service's permanent scene callback resolves
//! scene-26 eligibility and arms, its two-stage LayeredFS/XACT transaction
//! owns what actually commits, and score containment is enforced by
//! `score_guard` — none of that lives here.
//!
//! Policy semantics:
//! - Next-song-only by construction: the desired atomics are read exactly
//!   once per attempt, at scene-26 arming. Mid-song option edits touch only
//!   the atomics, never the active generation.
//! - Disable = future policy off: the row hides at the next form rebuild
//!   (`set_option_available`) and both sides' desired percents reset to 100,
//!   while the current attempt (if any) runs to its definitive lifecycle
//!   boundary untouched.
//! - Values are normalized (clamp 25..=175, snap to the nearest 5) at BOTH
//!   the persistence load transform and the change callback, so a legacy or
//!   hand-edited value can never leave a side desiring an unsupported rate.
//!
//! Readiness: the row registers during `enable()` (before the one-time
//! label-atlas flush) but only when the strict row-injection predicate AND
//! the song-rate integration conjunction hold — a rate the shared service
//! cannot guarantee audio/clock/score/movie integration for must not be
//! selectable (no inert UI).
//!
//! Versus mirroring (2026-08-31): local versus no longer disables the rate —
//! the mechanism is cabinet-global (one clock factor, one dance bank), so
//! versus plays at ONE shared rate. While both sides are entered at song
//! select, the three rows are MIRRORED through the shared
//! [`versus_mirror`] service (P1 is the authoritative initial seed; last
//! writer wins; persistence is pull-based at save time, so both profiles
//! save the shared value at logout). The gameplay classifier
//! (`classify_scene26`) and the preview qualifier independently take P1's
//! values in versus, so a torn mirror can never split the rate.

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, RegisterSpec, ScalarFormat, ShowWhen};
use crate::services::song_rate::lifecycle::{
    snap_rate_percent, IDENTITY_PERCENT, MAX_RATE_PERCENT, MIN_RATE_PERCENT, RATE_PERCENT_STEP,
};
use crate::services::song_rate::preview as song_preview;
use crate::services::song_rate::real_speed;
use crate::services::song_rate::runtime as song_rate_runtime;
use crate::services::versus_mirror;
use crate::{log_info, log_warn};

const MOD_ID: &str = "song-playback-speed";
/// Registered option id; the label texture is `seop_item_song_speed` and the
/// network wire field is `mod_song_speed` (both derived by the framework).
const OPT_SONG_SPEED: &str = "song_speed";
/// The preserve-pitch child row; label texture `seop_item_preserve_pitch`,
/// wire field `mod_preserve_pitch`, previews
/// `seop_image_preserve_pitch_{off,on}` (all framework-derived).
const OPT_PRESERVE_PITCH: &str = "preserve_pitch";
/// The sync-background-video child row; label texture
/// `seop_item_sync_movie`, wire field `mod_sync_movie`, previews
/// `seop_image_sync_movie_{off,on}` (all framework-derived).
const OPT_SYNC_MOVIE: &str = "sync_movie";
const COARSE_STEP: i32 = 10;
/// Preserve-pitch default: ON (1) — the shipped pitch-preserving behavior.
const PRESERVE_PITCH_DEFAULT: i32 = 1;
/// Sync-background-video default: OFF (0) — non-100% suppresses the movie,
/// the shipped behavior (background-movie-sync design FR-1).
const SYNC_MOVIE_DEFAULT: i32 = 0;

pub struct SongPlaybackSpeedMod {
    active: bool,
}

impl SongPlaybackSpeedMod {
    pub fn new() -> Self {
        Self { active: false }
    }
}

/// The three mirrored rows — everything that latches into the single
/// cabinet-global `ArmRequest` (rate, DSP mode, movie sync). Divergent
/// children with a shared rate would silently ignore one side's choice.
const MIRRORED_OPTIONS: [&str; 3] = [OPT_SONG_SPEED, OPT_PRESERVE_PITCH, OPT_SYNC_MOVIE];

/// Change callback: normalize and store one side's desired rate. One atomic
/// store — no I/O, no game API, no locking (the option-callback contract) —
/// plus, while the versus mirror is engaged, the cross-side sync (which
/// re-enters the registry lock exactly like the judgement-offsets
/// precedent's callbacks do). The preview refresh stamp is two more atomic
/// stores (preview design §Components 7 — the debounced restart executor
/// consumes it).
fn on_song_speed_change(side: u8, value: i32) {
    let snapped = snap_rate_percent(value);
    song_rate_runtime::set_desired_percent(usize::from(side), snapped);
    song_preview::request_refresh();
    versus_mirror::mirror_edit(OPT_SONG_SPEED, side, snapped);
}

/// Persistence load transform: a persisted value (network or JSON cache)
/// from an older build or a hand-edited config normalizes into the scalar
/// domain before it reaches the value cache and the change callback.
fn load_normalize(_id: &str, value: i32) -> i32 {
    snap_rate_percent(value)
}

/// Change callback for the preserve-pitch child row: one atomic store, plus
/// the preview refresh stamp (a DSP-mode edit restarts the preview too).
fn on_preserve_pitch_change(side: u8, value: i32) {
    song_rate_runtime::set_desired_preserve_pitch(usize::from(side), value != 0);
    song_preview::request_refresh();
    versus_mirror::mirror_edit(OPT_PRESERVE_PITCH, side, value);
}

/// Change callback for the sync-background-video child row: one atomic
/// store. No preview refresh — previews carry no movie, so the flag never
/// affects them (background-movie-sync design; the desire latches at the
/// next scene-26 arm like every other rate option).
fn on_sync_movie_change(side: u8, value: i32) {
    song_rate_runtime::set_desired_sync_movie(usize::from(side), value != 0);
    versus_mirror::mirror_edit(OPT_SYNC_MOVIE, side, value);
}

/// Load transform for the preserve-pitch bool: clamp any persisted value
/// into {0, 1} (insurance against hand-edited JSON / foreign servers).
fn load_clamp_bool(_id: &str, value: i32) -> i32 {
    i32::from(value != 0)
}

impl Mod for SongPlaybackSpeedMod {
    fn id(&self) -> &str {
        MOD_ID
    }
    fn name(&self) -> &str {
        "Song Playback Speed"
    }
    fn description(&self) -> &str {
        "Per-player SONG SPEED (25%-175%): pitch-preserved audio with a matching gameplay clock; mirrored/shared in versus"
    }
    fn required_signatures(&self) -> &[&str] {
        // Empty on purpose: every load-bearing piece (clock patch, wave
        // hooks, LayeredFS transaction, score guard) belongs to the shared
        // song-rate service, which resolves and self-reports through
        // `integration_ready()`. The mod checks that conjunction at enable.
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        // Resolve the rate-aware Real Speed recompute's option-table chain
        // (design req 33; `services::song_rate::real_speed`). Fail-soft: a
        // missing derivation leaves Real Speed stock at non-identity rates
        // (warned at activate), never blocks the feature.
        let _ = real_speed::init(_ctx.signatures);
        // Stash the preview restart half's four addresses (preview design
        // §Components 6): the two vftable identity gates + the two stock
        // functions the Step-5 executor calls. All-or-nothing; a miss
        // leaves the wheel-settle preview binds fully functional and only
        // the live-edit restart unavailable (reported at enable).
        let _ = song_preview::init_restart(_ctx.signatures);
        true
    }

    fn enable(&mut self) {
        self.active = false;

        // Strict readiness: the row must be injectable (allocator + builder
        // + tab filter) AND the shared service must guarantee audio/clock/
        // score/movie integration. Refuse enable otherwise — no inert UI.
        if !custom_options::row_injection_available() {
            log_warn!("{MOD_ID}: custom-option row injection unavailable — refusing enable");
            return;
        }
        if !song_rate_runtime::integration_ready() {
            log_warn!(
                "{MOD_ID}: song-rate integration incomplete (transaction/clock/score readiness) — refusing enable"
            );
            return;
        }

        // Register once; a re-enable sees Duplicate (the framework has no
        // unregister) and that is success — availability brings the row back.
        let spec = RegisterSpec::scalar(
            OPT_SONG_SPEED,
            MIN_RATE_PERCENT,
            MAX_RATE_PERCENT,
            RATE_PERCENT_STEP,
            ScalarFormat::Unit { unit: "%" },
        )
        .display_name("Song Playback Speed")
        .description("Plays the whole song faster or slower; scores are not saved off 100%")
        .step_coarse(COARSE_STEP)
        .default_value(IDENTITY_PERCENT)
        .persist_transform(|_id, value| value, load_normalize)
        .on_change(on_song_speed_change);
        match custom_options::register_option(spec) {
            Ok(_) => {}
            Err(custom_options::RegisterError::Duplicate { .. }) => {}
            Err(error) => {
                log_warn!("{MOD_ID}: option registration failed ({error:?}) — refusing enable");
                return;
            }
        }

        // The preserve-pitch child: registered right after the parent (so
        // the ShowWhen reference resolves and the default row order keeps
        // them adjacent), hidden while the parent side sits at 100%.
        let preserve = RegisterSpec::bool_toggle(OPT_PRESERVE_PITCH)
            .display_name("Preserve Song Pitch")
            .description("Keep the song's pitch when changing playback speed")
            .default_value(PRESERVE_PITCH_DEFAULT)
            .show_when(ShowWhen::NotEquals {
                parent_id: OPT_SONG_SPEED.into(),
                value: IDENTITY_PERCENT,
            })
            .persist_transform(|_id, value| value, load_clamp_bool)
            .on_change(on_preserve_pitch_change);
        match custom_options::register_option(preserve) {
            Ok(_) => {}
            Err(custom_options::RegisterError::Duplicate { .. }) => {}
            Err(error) => {
                // The rate feature works without the child (preserved
                // default) — warn and continue rather than refusing enable.
                log_warn!("{MOD_ID}: preserve-pitch registration failed ({error:?}) — rate stays pitch-preserved");
            }
        }

        // The sync-background-video child: SECOND child row, registered
        // right after preserve_pitch (adjacent in the default row order),
        // same show rule. Governs ONLY the non-100% movie behavior: ON =
        // keep the movie and rate-lock it via movie_sync's clock proxy
        // (platform-uniform); OFF = suppress as
        // shipped. Registration failure is non-fatal: the rate feature
        // works, movies simply stay suppressed at non-100%.
        let sync_movie = RegisterSpec::bool_toggle(OPT_SYNC_MOVIE)
            .display_name("Sync Background Video")
            .description("Rate-lock the background video to the adjusted song speed")
            .default_value(SYNC_MOVIE_DEFAULT)
            .show_when(ShowWhen::NotEquals {
                parent_id: OPT_SONG_SPEED.into(),
                value: IDENTITY_PERCENT,
            })
            .persist_transform(|_id, value| value, load_clamp_bool)
            .on_change(on_sync_movie_change);
        match custom_options::register_option(sync_movie) {
            Ok(_) => {}
            Err(custom_options::RegisterError::Duplicate { .. }) => {}
            Err(error) => {
                log_warn!("{MOD_ID}: sync-movie registration failed ({error:?}) — movies stay suppressed at non-100%");
            }
        }

        // Seed the desired atomics from the authoritative registry values.
        // First enable: registration already primed them via `on_change`;
        // re-enable: Duplicate does NOT re-fire, and `disable()` reset the
        // atomics to identity/preserved, so re-seed from persisted values.
        for side in 0u8..2 {
            on_song_speed_change(
                side,
                custom_options::get_value(side, OPT_SONG_SPEED).unwrap_or(IDENTITY_PERCENT),
            );
            on_preserve_pitch_change(
                side,
                custom_options::get_value(side, OPT_PRESERVE_PITCH)
                    .unwrap_or(PRESERVE_PITCH_DEFAULT),
            );
            on_sync_movie_change(
                side,
                custom_options::get_value(side, OPT_SYNC_MOVIE).unwrap_or(SYNC_MOVIE_DEFAULT),
            );
        }

        custom_options::set_option_available(OPT_SONG_SPEED, true);
        custom_options::set_option_available(OPT_PRESERVE_PITCH, true);
        custom_options::set_option_available(OPT_SYNC_MOVIE, true);

        // Rate-aware Real Speed (req 33): the recompute is owned by THIS
        // mod — the rate feature — never by the Real Speed Fix mod, whose
        // toggle must play no part. Self-warns when unavailable.
        real_speed::activate();

        // Song-select preview rate (preview design R11): on whenever this
        // mod is enabled — no config surface, no extra rows. `init`
        // registers the scene-exit force-retire; a missing scene manager
        // degrades to natural-teardown-only coverage (the unregister
        // prelude still retires preview bindings on every wheel move /
        // song confirm) — warn once and continue.
        if !song_preview::init() {
            log_warn!(
                "{MOD_ID}: scene manager unavailable — preview bindings rely on natural teardown only"
            );
        }
        song_preview::set_feature_active(true);
        // Restart-half availability report (preview design §Components 6 /
        // §Error Handling row 1): degraded = wheel-settle previews only,
        // live edits apply at the next settle.
        if song_preview::restart_available() {
            log_info!(
                "{MOD_ID}: preview restart derivations resolved — live-edit restart half available"
            );
        } else {
            log_warn!(
                "{MOD_ID}: preview restart derivations unresolved — option edits apply at the next wheel settle"
            );
        }

        // Versus shared-rate mirroring (shared service): registered after
        // the seeding loop above so an already-engaged mirror's immediate
        // P1→P2 seed lands on primed rows.
        versus_mirror::register(&MIRRORED_OPTIONS);

        self.active = true;
        log_info!(
            "{MOD_ID}: enabled — SONG SPEED row available ({}..={} step {}/{}, default {})",
            MIN_RATE_PERCENT,
            MAX_RATE_PERCENT,
            RATE_PERCENT_STEP,
            COARSE_STEP,
            IDENTITY_PERCENT
        );
    }

    fn disable(&mut self) {
        if !self.active {
            return;
        }
        // Versus mirror off first: subsequent value churn must not cross
        // sides once the mod is disabling.
        versus_mirror::unregister(&MIRRORED_OPTIONS);
        // Future policy off: hide the row (next form rebuild) and desire
        // identity on both sides. The current attempt, if one is armed or
        // committed, deliberately runs to its definitive lifecycle boundary —
        // the service owns it, and its score/clock/movie handling must not
        // be disturbed mid-song.
        custom_options::set_option_available(OPT_SONG_SPEED, false);
        custom_options::set_option_available(OPT_PRESERVE_PITCH, false);
        custom_options::set_option_available(OPT_SYNC_MOVIE, false);
        for side in 0..2usize {
            song_rate_runtime::set_desired_percent(side, IDENTITY_PERCENT);
            song_rate_runtime::set_desired_preserve_pitch(side, true);
            song_rate_runtime::set_desired_sync_movie(side, false);
        }
        real_speed::deactivate();
        // Preview rate off with the mod: future creates stay stock and any
        // live preview binding retires (the currently playing preview
        // finishes serving through the retired list's grace period).
        song_preview::set_feature_active(false);
        song_preview::retire_now();
        self.active = false;
        log_info!(
            "{MOD_ID}: disabled — row hidden, future rates identity (active attempt unaffected)"
        );
    }

    fn is_active(&self) -> bool {
        self.active
    }
}
