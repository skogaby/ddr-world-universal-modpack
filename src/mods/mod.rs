//! Mod implementations — each mod is a struct implementing the `Mod` trait.
//!
//! The mod system provides:
//! - **mod_trait** — The `Mod` trait that all mods implement, plus `ModRegistry`
//!   for registration, enable/disable lifecycle, and config integration.
//! - **config** — JSON persistence for mod enable/disable state.
//! - **mod_menu** — In-game overlay for toggling mods at runtime.
//!
//! ## Included mods
//!
//! - **fast_bootup** — Hooks the loading screen's per-frame update to process
//!   multiple file entries per tick instead of one.
//! - **skip_intros** — Redirects the intro/warning scene to attract mode.
//! - **timer_freeze** — Patches the timer update function to freeze the display.
//! - **autoplay** — Hooks the note judging function to auto-hit all notes.
//! - **series_expansion** — Extends the valid series range for custom songs.
//! - **song_limit_expansion** — Expands XML read buffers to support ~8x more songs.
//! - **note_types_expansion** — Framework for new note types (mines, lifts, rolls).
//! - **assist_tick** — Clap sound at each arrow's chart timestamp (StepMania's
//!   assist tick), played through the game's own audio engine.
//! - **decorative_option_headers** — Non-selectable group-heading rows on the
//!   options MODS tab (placement via `custom_options.option_menu_settings`).
//! - **per_song_judgement_offsets** — Per-side, per-song overrides of the
//!   stock JUDGEMENT OFFSET, keyed by the highlighted song on the wheel.
//! - **anytime_speedmod** — Removes the ~10 s in-song speed-mod adjustment
//!   window so the nav buttons work for the whole song (cabinet-wide).

pub mod announcer_mute;
pub mod anytime_speedmod;
pub mod assist_tick;
pub mod autoplay;
pub mod center_arrows_single;
pub mod classic_difficulty;
pub mod config;
pub mod decorative_option_headers;
pub mod fast_bootup;
pub mod folder_expansion;
pub mod fps_unlock;
pub mod mod_menu;
pub mod mod_trait;
pub mod movie_size_customization;
pub mod music_wheel_song_length;
pub mod non_native_os_support;
pub mod note_types_expansion;
pub mod overlay_element_styling;
pub mod per_song_judgement_offsets;
pub mod player_perspective;
pub mod playfield_styling;
pub mod power_user_statistics;
pub mod premium_free;
pub mod quick_logout;
pub mod quick_restart_or_fail;
pub mod real_speed_fix;
pub mod s_marvelous;
pub mod series_expansion;
pub mod shader_fixes;
pub mod skip_intros;
pub mod smx_hardware;
pub mod song_limit_expansion;
pub mod song_playback_speed;
pub mod split_ssq_auto_discovery;
pub mod timer_freeze;
pub mod timing_offsets;
pub mod training_mode;
pub mod webui_options;
