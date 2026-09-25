//! Mods — one module per user-facing feature, each a struct implementing the
//! [`Mod`](mod_trait::Mod) trait.
//!
//! Layout:
//! - `mod_trait.rs` — the `Mod` trait (`id` / `name` / `init` / `enable` /
//!   `disable` / `is_active`, optional `early_apply`), `ModRegistry` (the
//!   config-gated enable pass and the `requested` vs `enabled` bookkeeping),
//!   `DEFAULT_OFF_MODS` (mods that default OFF when absent from the config
//!   `mods` map) and `LATE_BINDING_MODS` (enabled after every other mod).
//! - `config.rs` — the whole `mod-config.json` schema, its boot-time load and
//!   the section writers.
//! - `mod_menu/` — the in-game overlay menu (MODS / GLOBAL SETTINGS / PLAYER
//!   SETTINGS / APPEARANCE tabs).
//! - Every other entry is one mod: a single file, or a subdirectory once it
//!   outgrows one (`note_types_expansion/` is the reference). Each mod's
//!   entry file documents its own mechanism, config and degradation.
//!
//! Construction and registration order live in `src/lib.rs` (the order is
//! load-bearing for `early_apply` mods). This file deliberately keeps no
//! per-mod list — see `.agents/summary/components.md` for the catalogue.

pub mod announcer_mute;
pub mod anytime_speedmod;
pub mod assist_tick;
pub mod autoplay;
pub mod background_dancers;
pub mod center_arrows_single;
pub mod classic_difficulty;
pub mod config;
pub mod custom_resolution;
pub mod ddr_selection;
pub mod decorative_option_headers;
pub mod fast_bootup;
pub mod folder_expansion;
pub mod fps_unlock;
pub mod gameplay_timing_fixes;
pub mod hide_bottom_text;
pub mod mod_menu;
pub mod mod_trait;
pub mod movie_size_customization;
pub mod multiplayer_bot;
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
pub mod two_player_bpl_mode;
pub mod webui_options;
