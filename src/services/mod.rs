//! Game system integrations — bridges between our hook DLL and DDR World's subsystems.
//!
//! Each service wraps a specific game system and exposes a safe Rust API:
//!
//! - **widget_renderer** — Creates and manages native text/image widgets in the
//!   game's own render pipeline. Handles render list registration, font capture,
//!   and deferred game-thread operations via `run_on_render_thread`.
//!
//! - **texture_resolver** — Resolves texture names (e.g., "paseli_logo") to texture
//!   IDs and UV coordinates using the game's `get_bitmap_info` callback. Must be
//!   called from the game thread.
//!
//! - **scene_manager** — Hooks scene transitions to provide callbacks when the game
//!   changes screens (attract mode, gameplay, menus, etc.). Also supports scene
//!   redirects (e.g., skip intros).
//!
//! - **input_manager** — Polls arcade button state from arkmdxbio2.dll and fires
//!   press/release event callbacks for P1, P2, and operator buttons.
//!
//! - **bm2d_api** — Typed wrappers around libafp named exports for MovieClip
//!   manipulation (find children, set masks, drive scroll, control visibility).
//!   Also hosts the AFP-layer wrapper set (create/setup/play/destroy raw
//!   layers from a BM2D package) used by the background preview overlay.
//!
//! - **bm2d_package** — On-demand load / lookup / release of BM2D data
//!   packages through the game's own `bm2d::data::Manager` (name-keyed,
//!   refcounted mod-side). Provides the package handles the AFP-layer
//!   wrappers instantiate clips from.
//!
//! - **asset_loader** — On-demand customizer texture loading via the engine's
//!   FileManager → ResourceManager pipeline. Loads an asset `.arc` by path,
//!   resolves the inner PNG's bindable texture handle by bare stem, and
//!   releases it again. Used by the WebUI Options preview overlay.
//!
//! - **afp_patcher** — Hooks AFP stream creation to inject binary patches into
//!   AFP templates at load time (e.g., adding scroll children to filter panels).
//!
//! - **series_filter_scroll** — Drives scroll behavior for the VERSION filter panel
//!   when it overflows. Hooks the panel builder for event-driven activation.
//!
//! - **game_audio** — Mod-owned XACT bank registration and cue playback through
//!   the game's own audio engine. Claims a free sound-bank slot on the game's
//!   audio manager (writing only the slot's bank pointer, never its file id,
//!   which is what makes the bank survive every song load) and plays cues by
//!   name through the game's own sound-effect façade — so a mod sound shares
//!   the music's mix and its exact output latency.
//!
//! - **judge_hook** — Shared single-detour dispatcher for `GamePlayActor::judgeNotes`.
//!   Mods register pre/post-judge callbacks with a priority; the service runs
//!   them in order around the original function call. Prevents the retour
//!   detour-stacking problem when multiple mods need to intercept the judge.
//!
//! - **render_notes_hook** — Shared single-detour dispatcher for
//!   `ArrowRenderer::render_notes` (the per-frame, per-side lane note draw).
//!   Same model as judge_hook. Subscribers: note_types_expansion's mine pass
//!   (post @ Normal) and player_perspective's pass rewrite (pre + post @ Late).
//!
//! - **cull_window** — Multi-contributor extension of the note collector's
//!   (and guideline draw's) 720.0 top culling window via verified disp32
//!   redirects to a mod-owned float slot. Contributors: playfield_styling
//!   (min playfield scale) and player_perspective (hallway draw distance);
//!   effective bound = `max(720, distance)/min(scale, 1)` (multiplicative —
//!   the fill's scale and the perspective map stack).
//!
//! - **custom_options** — Custom player options framework. Mods register option
//!   rows that appear in the game's native options menu (including a mod-owned
//!   Page6 "Mods" tab) with first-class UI, per-player state, change callbacks,
//!   and optional persistence.
//!
//! - **custom_options_persistence** — ess.dll save/load bridge for the custom
//!   options framework. Emits `<mod_*>` kbin children under `<option>` on save
//!   and parses them back on load.
//!
//! - **options_scroll** — Per-(side, page) scroll driver for the options menu.
//!   Activates when a tab's row count exceeds native viewport capacity.
//!
//! - **se_bank_synth** — Pure-CPU synthesis of the assist-tick mod's
//!   pre-mixed tick track in the engine's own container formats: MS-ADPCM
//!   mono encoder, fixed-header one-entry XWB writer (exposing the rewritable
//!   sample segment), SE-profile XSB writer, and the sample-exact clap mixer.
//!   No game ABI — callable from any thread (per-song synthesis runs on a
//!   background thread; the engine calls consuming its output live in
//!   `game_audio` and are game-thread-only).
//!
//! - **score_guard** — Shared taint state for fraudulent (Autoplay) or
//!   incomplete (Quick Failure) score uploads. Owns no detour; fed by the
//!   `autoplay` and `quick_restart_or_fail` mods and read by the
//!   `custom_options_persistence` save trampoline, which enforces the
//!   per-side save policy on the ess.dll profile-save path.
//!
//! - **stage_records** — Shared, fail-closed decode of the per-stage
//!   play-record layout (GameWork global, player_work table, record
//!   base/stride, course record/field offsets) from the
//!   `stage_record_accessor` signature bytes. Consumed by `premium_free`
//!   (stale-record virginise), the logout-save sanitiser in
//!   `custom_options_persistence`, and `quick_logout`'s session gate.
//!
//! - **song_reset** — In-place rewind of the live gameplay run (no actor
//!   teardown): stop + replay the song cue, re-broadcast the engine's own
//!   0x1043/0x1044 song-start protocol (which rebuilds all per-note judge
//!   state at playhead 0), zero the score/combo/judge accumulators and
//!   restore the gauge. Triggered by `quick_restart_or_fail`; the
//!   `request_reset(t_ms)` shape is the Training Mode foundation.

pub mod afp_patcher;
pub mod analyze_hook;
pub mod asset_loader;
pub mod avs_layeredfs;
pub mod bm2d_api;
pub mod bm2d_package;
pub mod chart_length;
pub mod cull_window;
pub mod custom_options;
pub mod custom_options_persistence;
pub mod game_audio;
pub mod input_manager;
pub mod judge_hook;
pub mod mfplat_vih_fix;
pub mod movie_policy;
pub mod movie_sync;
pub mod ntdll_state_shim;
pub mod options_scroll;
pub mod overlay_draw;
pub mod render_notes_hook;
pub mod scene_manager;
pub mod score_guard;
pub mod se_bank_synth;
pub mod series_filter_scroll;
pub mod smx;
pub mod song_rate;
pub mod song_reset;
pub mod stage_records;
pub mod texture_resolver;
pub mod toast;
pub mod widget_renderer;

#[cfg(test)]
mod movie_policy_tests;
#[cfg(test)]
mod score_guard_tests;
