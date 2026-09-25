//! The song-rate engine — plays a song at a non-100 % speed (SONG SPEED) by
//! streaming time-stretched or resampled audio through XACT and scaling the
//! gameplay clock to match. Every failure degrades to stock 100 % playback.
//!
//! ## Flow
//!
//! 1. **Arm (scene 26).** `runtime`'s permanent scene callback gathers the
//!    per-side desired rate / preserve-pitch / sync-movie atomics (written by the
//!    option rows) and `lifecycle::classify_scene26` decides eligibility:
//!    services ready, not a course, a side entered, a supported non-identity rate
//!    (or a training arm at 100 %), a known stage. Versus arms one shared rate
//!    with P1 governing. Arming is song-agnostic — the next dance bank the game
//!    loads is the generation's song. A non-identity arm tentatively sets the
//!    song-rate movie suppressor unless sync-background-video is effective.
//! 2. **Bind.** `wavebank_hook`'s `wavebank_create` detour qualifies the create
//!    and `binding::prepare_binding` builds a virtual XWB over the stock bank's
//!    resident bytes (from the FileManager row) and exposes a redirect token in a
//!    transaction slot.
//! 3. **Generate and serve.** One `generator` thread per bound generation decodes
//!    the source and runs WSOLA (pitch preserved) or the plain resampler (from
//!    `core/xact`) into the binding's ring; `io_callback_hook`'s readFile /
//!    getOverlappedResult detours serve the virtual bank from it. The detours
//!    never synthesize; a producer failure fills silence.
//! 4. **Commit, exactly once.** `transaction::call_create` commits after the
//!    original create succeeds, in a fixed order: score containment (rate ledger
//!    and session taint through `score_guard`) → movie suppression confirmed
//!    (skipped when sync-background-video is latched) → rate snapshot published →
//!    the Q31 clock factor, always last. Identity (training) commits skip the
//!    score and movie legs. A late failure retires the binding and the factor
//!    never leaves identity.
//! 5. **Teardown.** The `wavebank_unregister` detour retires the binding before
//!    the original destroys the bank; a maintenance drain reclaims it.
//!
//! Both detour pairs are pair-or-neither: `wavebank_create` + `wavebank_unregister`,
//! and the XACT readFile + getOverlappedResult callbacks (the stock overlapped
//! callback would report a deferred read as complete). A second-hook failure
//! rolls the first back. The SONG SPEED row is offered only when the clock
//! patch, both pairs, the movie policy and `score_guard`'s full sanitization are
//! all available (`runtime::integration_ready`).
//!
//! ## Identity
//!
//! `clock_patch::RateSnapshot` (read with `clock_patch::snapshot`) is the
//! authoritative identity of the running rate: generation, requested percent,
//! participant mask, exact effective `RateRatio` and whether it committed.
//! Consumers (assist tick via `tick_domain`, Real Speed via `real_speed`, movie
//! sync via `runtime::movie_rate_directive`, the statistics CSV export) derive
//! from a committed non-identity snapshot (`is_non_identity_commit`), never from
//! the option value; everything else takes the stock path.
//!
//! ## Previews
//!
//! `preview` binds song-select preview banks through the same create detour and
//! IO pair when the gameplay path resolves to stock. Previews only serve audio:
//! they never publish Q31, touch score state, suppress the movie or enter the
//! lifecycle, and every failure falls back to a stock preview.
//!
//! ## Submodules
//!
//! - `lifecycle` — pure phases, rate domain, scene-26 eligibility, transition
//!   engine; `runtime` — Windows glue (scene callback, input gathering, sinks).
//! - `binding` — virtual-bank preflight, ring and serve dispatch;
//!   `generator` — the producer thread; `io_callback_hook` — the XACT IO pair.
//! - `wavebank_hook` — the create/unregister pair and bind composition;
//!   `transaction` — the exactly-once commit; `xact_runtime` — slot table, TLS
//!   frames, maintenance queue, bank timeline.
//! - `clock_patch` — the permanent identity clock stub and rate publication.
//! - `preview` — song-select preview rates; `selected_song` — highlighted-song
//!   identity for training mode; `real_speed`, `tick_domain` — rate-aware
//!   derivations for Real Speed and assist tick.
//!
//! See `docs/song_playback_speed.md` and `docs/xact_streaming_research.md`. Host
//! tests: `scripts/validate_song_playback_speed.sh`.

pub mod binding;
pub mod clock_patch;
pub mod generator;
#[cfg(windows)]
pub mod io_callback_hook;
pub mod lifecycle;
pub mod preview;
pub mod real_speed;
#[cfg(windows)]
pub mod runtime;
pub mod selected_song;
pub mod tick_domain;
pub mod transaction;
pub mod wavebank_hook;
pub mod xact_runtime;

#[cfg(test)]
mod binding_tests;
#[cfg(test)]
mod clock_patch_tests;
#[cfg(test)]
mod generator_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod preview_tests;
#[cfg(test)]
mod real_speed_tests;
#[cfg(test)]
mod selected_song_tests;
#[cfg(test)]
mod tick_domain_tests;
#[cfg(test)]
mod transaction_tests;
#[cfg(test)]
mod wavebank_hook_tests;
#[cfg(test)]
mod xact_runtime_tests;
