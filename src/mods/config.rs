//! Centralized config store — reads mod-config.json once, provides typed access.
//!
//! All consumers read from `config::get()` instead of parsing the file independently.
//! Only `save_mod_states()` writes back, and it preserves all non-mods keys.

use super::folder_expansion::FolderConfig;
use super::series_expansion::SeriesConfig;
use crate::services::avs_layeredfs::LayeredFsConfig;
use crate::{log_info, log_warn};
use once_cell::sync::OnceCell;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

#[derive(Deserialize, Clone, Debug)]
pub struct CustomOptionsConfig {
    /// Gates backend-server (network) save/load of custom options. Default true.
    #[serde(default = "default_true")]
    pub persist_network: bool,
    /// Gates offline `mod-config.json` save/load of custom options. Default true.
    #[serde(default = "default_true")]
    pub persist_json: bool,
    /// Optional gamma-correction value applied to the dark lane art in the WebUI
    /// Options previews (Photoshop convention: `> 1.0` brightens, `< 1.0`
    /// darkens, `1.0` identity). When absent, the built-in per-layer default is
    /// used (see `webui_options::discovery`'s lane categories). Tunable here so
    /// operators can match Konami's web preview without a recompile.
    #[serde(default)]
    pub lane_gamma_correction: Option<f32>,
    /// Optional half-width N of the WebUI preview prefetch window: the overlay
    /// keeps `[cur-N, cur+N]` of the FOCUSED cosmetic category's assets loaded
    /// while the options modal is open, so scrolling within the focused row
    /// shows art instantly. Clamped to 0..=10; absent → the built-in default
    /// (see `webui_options::preview_overlay::DEFAULT_WINDOW_N`). Larger = more
    /// resident preview textures (≤ 2N+1 entries, one category at a time).
    #[serde(default)]
    pub preview_window: Option<i32>,
    /// Whether the two BACKGROUND rows' live previews animate (default true).
    /// `false` still shows the focused background — as a static first frame
    /// (the clip is created and left paused) — never blank chrome. For
    /// operators who find the animated previews distracting or want to shave
    /// the (small) per-frame cost of a playing clip while the modal is open.
    #[serde(default)]
    pub animate_backgrounds: Option<bool>,
    /// Operator-defined display order + per-menu placement for the modpack's
    /// custom option rows (both the in-game MODS tab and the overlay menu's
    /// mirror). Array order = display order; each entry names an option id
    /// (case-insensitive; e.g. `"premium_free"`, `"song_speed"`) with
    /// optional `overlay` / `in_game` booleans overriding the option's
    /// registered menu placement (omitted = inherit; both `false` = hidden
    /// everywhere). Listed ids render first, in this order; any registered
    /// option not listed falls to the end (keeping its registration order);
    /// entries matching no registered option are logged once and ignored
    /// (never fatal). Absent or empty ⇒ registration order, no overrides.
    /// Cabinet-wide; read once at boot, so edits take effect on the next
    /// launch. Operator-authored only — the DLL never writes this key.
    /// (Supersedes the retired `row_order` key, which is no longer read and
    /// silently ignored if present.)
    #[serde(default)]
    pub option_menu_settings: Option<Vec<OptionMenuSettingConfig>>,
    // The per-player `p1`/`p2` value blocks co-habit this section but are NOT
    // typed here — they're handled out-of-band via read-modify-write (serde
    // ignores unknown keys, so they don't break the parse).
}

/// One `custom_options.option_menu_settings` entry (serde twin of
/// `custom_options::ordering::OptionMenuSetting`, converted at init).
#[derive(Deserialize, Clone, Debug)]
pub struct OptionMenuSettingConfig {
    /// Option id (case-insensitive).
    pub id: String,
    /// Show in the overlay mod menu (`None` = registration default).
    #[serde(default)]
    pub overlay: Option<bool>,
    /// Show in the in-game options menu (`None` = registration default).
    #[serde(default)]
    pub in_game: Option<bool>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct DiagnosticsConfig {
    #[serde(default)]
    pub profiling: bool,
}

/// Global timing-offset values for the `timing-offsets` mod. These are
/// cabinet-wide (the game publishes them into one process-wide config map), so
/// they live in their own top-level section rather than the per-player
/// `custom_options` cache. Missing keys fall back to the game's stock defaults
/// (SOUND 87, INPUT 28, RENDER 17, BOMB 0). Written immediately on change via
/// `save_json_key`.
#[derive(Deserialize, Clone, Debug)]
pub struct TimingOffsetsConfig {
    #[serde(default = "default_sound_offset")]
    pub sound_offset: i32,
    #[serde(default = "default_input_offset")]
    pub input_offset: i32,
    #[serde(default = "default_render_offset")]
    pub render_offset: i32,
    #[serde(default)]
    pub bomb_frame_offset: i32,
}

impl Default for TimingOffsetsConfig {
    fn default() -> Self {
        Self {
            sound_offset: default_sound_offset(),
            input_offset: default_input_offset(),
            render_offset: default_render_offset(),
            bomb_frame_offset: 0,
        }
    }
}

fn default_sound_offset() -> i32 {
    87
}
fn default_input_offset() -> i32 {
    28
}
fn default_render_offset() -> i32 {
    17
}

/// Config for the `fps-unlock` mod. `presets` are the selectable FPS values
/// shown in the overlay enum row (operator-editable — add oddball refresh rates
/// here); `selected` is the active value, stored as a raw FPS number (not an
/// index). Both have sensible defaults so an absent/partial section still works.
/// Normalization (dedupe / sort / range-clamp / auto-add `selected`) is applied
/// in-memory by the mod, NOT written back — only `selected` is persisted on
/// change (via `save_json_key`), preserving the operator's `presets` array.
#[derive(Deserialize, Clone, Debug)]
pub struct FpsUnlockConfig {
    #[serde(default = "default_fps_presets")]
    pub presets: Vec<i32>,
    #[serde(default = "default_fps_selected")]
    pub selected: i32,
}

impl Default for FpsUnlockConfig {
    fn default() -> Self {
        Self {
            presets: default_fps_presets(),
            selected: default_fps_selected(),
        }
    }
}

fn default_fps_presets() -> Vec<i32> {
    vec![60, 120, 144, 165, 240, 360]
}
/// Stock 60 — enabling the mod is a no-op until the operator picks a higher
/// preset (decided in idea-honing Q4).
fn default_fps_selected() -> i32 {
    60
}

fn default_true() -> bool {
    true
}

/// Cabinet-wide dev knobs for the `player-perspective` mod (the per-player
/// OVERHEAD/HALLWAY/DISTANT selection lives in custom_options, NOT here).
/// Operator-edited only — the DLL never writes this section back.
#[derive(Deserialize, Clone, Debug)]
pub struct PlayerPerspectiveConfig {
    /// Hallway focal length `k` in pixels: the receptor-row→horizon
    /// distance of the perspective map (`scale(d) = k / (k + d)`). Smaller =
    /// more aggressive perspective (horizon closer to the receptors).
    #[serde(default = "default_persp_focal")]
    pub hallway_focal: f32,
    /// Draw distance in pre-perspective lane pixels: how far past the
    /// receptor row notes are collected while a side is in a perspective
    /// mode (the value contributed to the cull window). HALLWAY compresses
    /// the approach region onto the screen; DISTANT's receptor realignment
    /// shift pulls content past the stock 720 px bound on screen. Larger =
    /// notes spawn closer to the horizon / further off the entrance edge.
    #[serde(default = "default_persp_draw_distance")]
    pub hallway_draw_distance: f32,
    /// Distant focal length `k` in pixels (the map recedes toward and past
    /// the receptors; the anchor sits mid-field). Containment rule of
    /// thumb: the entrance-edge scale is `distant_zoom · k / (k − h)` with
    /// `h` ≈ half the receptor→screen-edge span (~310 px) — keep that ≤ 1
    /// so the field stays inside the stock lane rectangle.
    #[serde(default = "default_distant_focal")]
    pub distant_focal: f32,
    /// Distant base zoom about the mid-field anchor (StepMania applies 0.9
    /// at full positive tilt). Clamped to 0.1..=1.0 at latch time.
    #[serde(default = "default_distant_zoom")]
    pub distant_zoom: f32,
}

impl Default for PlayerPerspectiveConfig {
    fn default() -> Self {
        Self {
            hallway_focal: default_persp_focal(),
            hallway_draw_distance: default_persp_draw_distance(),
            distant_focal: default_distant_focal(),
            distant_zoom: default_distant_zoom(),
        }
    }
}

fn default_persp_focal() -> f32 {
    1000.0
}
fn default_persp_draw_distance() -> f32 {
    1600.0
}
fn default_distant_focal() -> f32 {
    3000.0
}
fn default_distant_zoom() -> f32 {
    0.9
}

/// Config for the `shader-fixes` mod (runtime shader-container synthesis).
/// `anti_aliasing` is the cabinet-wide ARROW ANTI-ALIASING toggle (also
/// adjustable from the mod overlay menu; applies on the NEXT LAUNCH — the
/// containers are synthesized at arc-open time during boot).
#[derive(Deserialize, Clone, Debug)]
pub struct ShaderFixesConfig {
    #[serde(default = "default_true")]
    pub anti_aliasing: bool,
}

impl Default for ShaderFixesConfig {
    fn default() -> Self {
        Self {
            anti_aliasing: true,
        }
    }
}

/// Config for the `assist-tick` mod. The section currently holds only a
/// RETIRED key: the pre-mixed tick track derives its timing from game state
/// (the cabinet's `sound_offset` + per-side options), so the old per-tick
/// path's `offset_ms` latency knob no longer exists. The key is still parsed
/// so installs that carry a tuned value (125–150 was typical) get one INFO
/// naming it ignored — it is never reinterpreted, and nothing writes this
/// section back.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct AssistTickConfig {
    /// RETIRED (parse-but-ignore). `Some` ⇒ the key is present in the file
    /// and the mod logs one INFO at enable.
    #[serde(default)]
    pub offset_ms: Option<i32>,
}

/// `song_playback_speed` — Song Playback Speed configuration.
#[derive(Deserialize)]
pub struct SongPlaybackSpeedConfig {
    /// RETIRED (parse-but-ignore). The Step 4 developer-only pre-generated
    /// diagnostic block (`song_code`/`requested_percent`/`xwb_path`) was
    /// replaced by generation driven by the SONG SPEED option. `Some` ⇒ the
    /// key is present in the file and init logs one INFO; the shape is no
    /// longer validated and the value is never used.
    #[serde(default)]
    pub diagnostic: Option<serde_json::Value>,
}

/// Config for the `quick-restart-or-fail` mod.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct QuickRestartConfig {
    /// Delay (milliseconds) between the press-1 restart gesture and the
    /// restarted song's start. `0`/absent = instant (the default). When
    /// set, the field still resets IMMEDIATELY (music stops, notes return
    /// to their pre-song approach), then the song starts after the delay —
    /// a natural-looking countdown for players who want a beat to get back
    /// in position. Clamped to 0..=10000 at use.
    #[serde(default)]
    pub restart_delay_ms: Option<i32>,
}

/// Config for the `training-mode` mod (Step 7 — the amended R12 keys made
/// real): the FF/RW scrub increments in milliseconds per pinpad-9/7 press.
/// Absent keys default to 5000; out-of-range values normalize to
/// 250..=60000 with one INFO at mod enable (see
/// `training_mode::section_math::normalize_scrub_increment_ms`).
/// Operator-edited only — the DLL never writes this section back.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct TrainingModeConfig {
    /// Fast-forward increment per pinpad-9 press (ms).
    #[serde(default)]
    pub ff_increment_ms: Option<i32>,
    /// Rewind increment per pinpad-7 press (ms).
    #[serde(default)]
    pub rw_increment_ms: Option<i32>,
}

/// Config for the `music-wheel-song-length` mod — glyph placement knobs
/// relative to the header card's `bpm_usr` anchor. Calibration aids for
/// cabinet deployment; the defaults are the shipped placement. Operator-
/// edited only — the DLL never writes this section back.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct MusicWheelSongLengthConfig {
    /// X pixel offset from the bpm_usr anchor (SpriteLayer +0xC0).
    #[serde(default)]
    pub offset_x: Option<f64>,
    /// Y pixel offset from the bpm_usr anchor (SpriteLayer +0xC8).
    #[serde(default)]
    pub offset_y: Option<f64>,
    /// Per-glyph spacing in pixels, negative tightens (SpriteLayer +0xE8).
    #[serde(default)]
    pub spacing: Option<f64>,
    /// Fixed glyph scale (SpriteLayer +0xE0).
    #[serde(default)]
    pub scale: Option<f64>,
}

/// Config for the `per-song-judgement-offsets` mod. Operator-edited only —
/// the DLL never writes this section back.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct PerSongJudgementOffsetsConfig {
    /// When true, an options-menu edit of a song's offset by EITHER player
    /// applies to BOTH sides in-sync (both session maps + both CSV columns,
    /// last writer wins). For solo home players without a persisting backend
    /// who swap cabinet sides: adjust once, keep it on either pad. Default
    /// false (per-side edits, the shipped behavior).
    #[serde(default)]
    pub mirror_players: Option<bool>,
}

/// Config for the `non-native-operating-system-support` mod. Operator-edited
/// only — the DLL never writes this section back.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct NonNativeOsSupportConfig {
    /// Background-movie graph handling:
    /// - `"suppress"` (default, and any absent/unknown value): never build
    ///   the DirectShow graph — movies absent, crash-safe under every
    ///   spice2x configuration.
    /// - `"fallback"`: build the real graph; fake the success epilogue only
    ///   when it FAILS. Converted (H.264) movies play; unplayable (VC-1)
    ///   files degrade to no-movie instead of soft-locking. WARNING: this
    ///   runs the crash-prone `RenderFile` path — under Wine/CrossOver it
    ///   requires spice2x `-audiohookdisable` (spice2x's audio wrappers
    ///   crash Wine's builtin `winmm` during DirectShow's audio-renderer
    ///   enumeration).
    #[serde(default)]
    pub movie_mode: Option<String>,
}

/// Config for the `smx-hardware` mod (`smx_hardware` section). Operator-
/// edited only — the DLL never writes this section back. Read once at mod
/// enable (next-launch semantics). The overlay/card fields are consumed
/// from Step 3 of the feature plan; they parse now so operator configs
/// written early stay valid.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct SmxHardwareConfig {
    /// P1's e-Amusement card id (hex string). Enables the P1 Insert-Card
    /// overlay button (Step 3).
    #[serde(default)]
    pub p1card: Option<String>,
    /// P2's e-Amusement card id (hex string).
    #[serde(default)]
    pub p2card: Option<String>,
    /// Touchscreen overlay opacity 0.0..=1.0 (Step 3; default 0.6).
    #[serde(default)]
    pub overlay_opacity: Option<f32>,
    /// Master touchscreen-overlay toggle (Step 3; default true).
    #[serde(default)]
    pub overlay_enabled: Option<bool>,
    /// Drive the SMX cabinet lights from DDR's light output (default true).
    /// A debug off-switch: input injection stays active when false.
    #[serde(default)]
    pub output_lights: Option<bool>,
    /// Drive the SMX cabinet's NON-stage lights — marquee, monitor-side
    /// vertical strips, spotlights — from DDR's cabinet lighting (default
    /// true). Effective only while `output_lights` is also true; stage-pad
    /// lights are unaffected by this knob.
    #[serde(default)]
    pub output_cabinet_lights: Option<bool>,
    /// Force the game into Gold-Cab light mode (default true). On this
    /// cabinet the game auto-detects a non-GOLD machine type and drives the
    /// SD `arkMDXChangeSatellite` path (cabinet-light colors on the pads,
    /// no per-arrow tape or corners). Forcing GOLD (via in-memory detours on
    /// `arkMDXGetMachineType`/`arkMDXGetPCType`) makes it drive the per-LED
    /// `arkMDXChangeTapeled` + `arkMDXChangeDimlamp` path the SMX map expects.
    /// Operator off-switch for genuine SD/HD cabinets; never written by the DLL.
    #[serde(default)]
    pub force_gold_cabinet: Option<bool>,
}

/// Config for the overlay mod menu's appearance (`overlay_menu` section).
/// DLL-WRITTEN: the THEME tab persists the whole section on any change
/// (`save_json_key`), serializing all three keys each time.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct OverlayMenuConfig {
    /// Built-in theme id (`bubbles` / `terminal` / `waveform` /
    /// `spectrum` / `tunnel` / `xmb` / `squares` / `card_swirl` /
    /// `blobs` / `ps2` / `prime_cube` / `minimal`). Unknown values —
    /// including the retired `arrows` / `wavefield` / `mandelbulb` —
    /// fall back to `bubbles` with one WARN.
    #[serde(default)]
    pub theme: Option<String>,
    /// Whether shader-animated menu backgrounds render (Step 8; the row
    /// exists but is inert until then). Default true.
    #[serde(default)]
    pub animate_background: Option<bool>,
    /// Modal panel opacity percent, 25..=100 snapped to 5 (default 80).
    /// Out-of-range values clamp+snap silently at mod-menu enable.
    #[serde(default)]
    pub opacity: Option<i32>,
}

const CONFIG_FILENAME: &str = "mod-config.json";

static CONFIG: OnceCell<ConfigFile> = OnceCell::new();

#[derive(Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub mods: HashMap<String, bool>,
    #[serde(default)]
    pub layeredfs: Option<LayeredFsConfig>,
    #[serde(default)]
    pub series_expansion: Option<SeriesConfig>,
    #[serde(default)]
    pub folder_expansion: Option<FolderConfig>,
    #[serde(default)]
    pub custom_options: Option<CustomOptionsConfig>,
    #[serde(default)]
    pub diagnostics: Option<DiagnosticsConfig>,
    #[serde(default)]
    pub timing_offsets: Option<TimingOffsetsConfig>,
    #[serde(default)]
    pub fps_unlock: Option<FpsUnlockConfig>,
    #[serde(default)]
    pub player_perspective: Option<PlayerPerspectiveConfig>,
    #[serde(default)]
    pub shader_fixes: Option<ShaderFixesConfig>,
    #[serde(default)]
    pub assist_tick: Option<AssistTickConfig>,
    #[serde(default)]
    pub song_playback_speed: Option<SongPlaybackSpeedConfig>,
    #[serde(default)]
    pub quick_restart: Option<QuickRestartConfig>,
    #[serde(default)]
    pub training_mode: Option<TrainingModeConfig>,
    #[serde(default)]
    pub music_wheel_song_length: Option<MusicWheelSongLengthConfig>,
    #[serde(default)]
    pub per_song_judgement_offsets: Option<PerSongJudgementOffsetsConfig>,
    #[serde(default)]
    pub non_native_os_support: Option<NonNativeOsSupportConfig>,
    #[serde(default)]
    pub overlay_menu: Option<OverlayMenuConfig>,
    #[serde(default)]
    pub smx_hardware: Option<SmxHardwareConfig>,
}

/// Initialize the config store. Call once, early in init sequence.
pub fn init() {
    let config = match fs::read_to_string(CONFIG_FILENAME) {
        Ok(contents) => match serde_json::from_str::<ConfigFile>(&contents) {
            Ok(c) => {
                log_info!("Config: loaded {}", CONFIG_FILENAME);
                c
            }
            Err(e) => {
                log_warn!(
                    "Config: failed to parse {}: {} — using defaults",
                    CONFIG_FILENAME,
                    e
                );
                ConfigFile {
                    mods: HashMap::new(),
                    layeredfs: None,
                    series_expansion: None,
                    folder_expansion: None,
                    custom_options: None,
                    diagnostics: None,
                    timing_offsets: None,
                    fps_unlock: None,
                    player_perspective: None,
                    shader_fixes: None,
                    assist_tick: None,
                    song_playback_speed: None,
                    quick_restart: None,
                    training_mode: None,
                    music_wheel_song_length: None,
                    per_song_judgement_offsets: None,
                    non_native_os_support: None,
                    overlay_menu: None,
                    smx_hardware: None,
                }
            }
        },
        Err(_) => {
            log_warn!("Config: {} not found — using defaults", CONFIG_FILENAME);
            ConfigFile {
                mods: HashMap::new(),
                layeredfs: None,
                series_expansion: None,
                folder_expansion: None,
                custom_options: None,
                diagnostics: None,
                timing_offsets: None,
                fps_unlock: None,
                player_perspective: None,
                shader_fixes: None,
                assist_tick: None,
                song_playback_speed: None,
                quick_restart: None,
                training_mode: None,
                music_wheel_song_length: None,
                per_song_judgement_offsets: None,
                non_native_os_support: None,
                overlay_menu: None,
                smx_hardware: None,
            }
        }
    };
    let _ = CONFIG.set(config);
}

/// Check if the config store has been initialized.
pub fn is_available() -> bool {
    CONFIG.get().is_some()
}

/// Get a reference to the parsed config. Returns None if init() hasn't been called.
pub fn get() -> Option<&'static ConfigFile> {
    CONFIG.get()
}

/// Save mod enable/disable states back to the file. Only writes the "mods" key.
/// Preserves all other top-level keys in the file unchanged.
pub fn save_mod_states(states: &HashMap<String, bool>) {
    let filtered: HashMap<&String, &bool> = states
        .iter()
        .filter(|(id, _)| id.as_str() != "mod-menu")
        .collect();

    // Read existing file to preserve non-mods keys
    let mut root = match fs::read_to_string(CONFIG_FILENAME) {
        Ok(s) => {
            serde_json::from_str::<serde_json::Value>(&s).unwrap_or_else(|_| serde_json::json!({}))
        }
        Err(_) => serde_json::json!({}),
    };

    root["mods"] = serde_json::to_value(&filtered).unwrap_or_default();

    match serde_json::to_string_pretty(&root) {
        Ok(json) => {
            if let Err(e) = fs::write(CONFIG_FILENAME, json) {
                log_warn!("Config: failed to save: {}", e);
            } else {
                log_info!("Config: saved mod states");
            }
        }
        Err(e) => log_warn!("Config: failed to serialize: {}", e),
    }
}

/// Map a player side index to its `custom_options` sub-key.
fn side_key(side: u8) -> &'static str {
    if side == 0 {
        "p1"
    } else {
        "p2"
    }
}

/// Write one player side's value block (`p1` or `p2`) into the `custom_options`
/// section, preserving the gate keys (`persist_network`/`persist_json`), the
/// *other* side's block, and all other top-level keys.
///
/// Per-side by design: the ess.dll `save_sender` fires once per carded-out
/// side, so a single-player card-out must not touch the absent player's
/// persisted block (the network path has the same per-side semantics). The
/// other side is preserved straight from the on-disk read.
///
/// Dirty-checked: if the resulting `custom_options` block is byte-identical to
/// what's already on disk, the write is skipped (avoids redundant writes when a
/// card-out produces values identical to those already persisted). Returns
/// `true` if the file was written, `false` if the write was skipped.
///
/// On a file-read failure the existing block is treated as "differs" and the
/// write proceeds (fail-safe toward persisting).
pub fn save_custom_options_values(side: u8, values: serde_json::Value) -> bool {
    let mut root = match fs::read_to_string(CONFIG_FILENAME) {
        Ok(s) => {
            serde_json::from_str::<serde_json::Value>(&s).unwrap_or_else(|_| serde_json::json!({}))
        }
        Err(_) => serde_json::json!({}),
    };

    // Snapshot the existing custom_options block for the dirty-check.
    let old_block = root.get("custom_options").cloned();

    // Ensure custom_options is an object, then set only this side's sub-key.
    // This preserves the gate keys, the other side, and any sibling keys.
    if !root["custom_options"].is_object() {
        root["custom_options"] = serde_json::json!({});
    }
    root["custom_options"][side_key(side)] = values;

    // Dirty-check: skip the write if the block is unchanged.
    if old_block.as_ref() == Some(&root["custom_options"]) {
        return false;
    }

    match serde_json::to_string_pretty(&root) {
        Ok(json) => {
            if let Err(e) = fs::write(CONFIG_FILENAME, json) {
                log_warn!("Config: failed to save custom_options values: {}", e);
                false
            } else {
                true
            }
        }
        Err(e) => {
            log_warn!("Config: failed to serialize custom_options values: {}", e);
            false
        }
    }
}

/// One-shot migration of the legacy `webui_options` offline cache into the
/// `custom_options` section. If a `webui_options` block exists and
/// `custom_options.{p1,p2}` does not yet hold data, copy `webui_options`'s
/// `p1`/`p2` sub-objects under `custom_options` and delete the old
/// `webui_options` key (preserving the gate keys and all other keys).
///
/// Idempotent: once `webui_options` is gone, subsequent runs no-op. Defensive:
/// if `webui_options` is present but not an object (malformed), the key is left
/// in place and a warning is logged rather than discarding data we can't move.
/// Called once during persistence init, before the JSON-load timer is spawned.
pub fn migrate_webui_options_to_custom_options() {
    let contents = match fs::read_to_string(CONFIG_FILENAME) {
        Ok(s) => s,
        Err(_) => return, // no file yet → nothing to migrate
    };
    let mut root = match serde_json::from_str::<serde_json::Value>(&contents) {
        Ok(v) => v,
        Err(_) => return, // unparseable → leave alone, init() already warned
    };

    // Nothing to migrate if the legacy key is absent.
    if root.get("webui_options").is_none() {
        return;
    }

    // Don't clobber existing custom_options p1/p2 data (idempotency guard
    // against a partially-migrated or hand-edited file).
    let already_migrated =
        root["custom_options"]["p1"].is_object() || root["custom_options"]["p2"].is_object();
    if already_migrated {
        // Legacy key lingering alongside already-migrated data: drop it so the
        // migration doesn't keep firing, but don't overwrite the new data.
        if let Some(obj) = root.as_object_mut() {
            obj.remove("webui_options");
        }
        write_root("migration cleanup", root);
        return;
    }

    // Validate the legacy block is an object before moving anything.
    let webui = root["webui_options"].clone();
    if !webui.is_object() {
        log_warn!(
            "Config: webui_options present but not an object — skipping migration, leaving key in place"
        );
        return;
    }

    // Ensure custom_options is an object, then move p1/p2 across.
    if !root["custom_options"].is_object() {
        root["custom_options"] = serde_json::json!({});
    }
    for key in ["p1", "p2"] {
        if webui[key].is_object() {
            root["custom_options"][key] = webui[key].clone();
        }
    }
    if let Some(obj) = root.as_object_mut() {
        obj.remove("webui_options");
    }

    write_root("migrated webui_options → custom_options", root);
    log_info!("Config: migrated webui_options → custom_options.{{p1,p2}}");
}

/// Serialize and write a root JSON value back to the config file, logging on
/// failure. Shared by the migration paths.
fn write_root(context: &str, root: serde_json::Value) {
    match serde_json::to_string_pretty(&root) {
        Ok(json) => {
            if let Err(e) = fs::write(CONFIG_FILENAME, json) {
                log_warn!("Config: failed to write during {}: {}", context, e);
            }
        }
        Err(e) => log_warn!("Config: failed to serialize during {}: {}", context, e),
    }
}

/// Re-read the `custom_options.{p1,p2}` value blocks **from disk** (not the
/// cached `OnceCell`, so it reflects any migration write that happened after
/// `init()`). Returns one `(side, option_id, wire_value)` tuple per cached
/// value. Used by the lazy JSON-load timer to prime the registry.
///
/// A missing file / section, a parse failure, or non-integer values yield an
/// empty (or partial) result rather than an error — JSON load is best-effort.
pub fn read_custom_options_values() -> Vec<(u8, String, i32)> {
    let root = match fs::read_to_string(CONFIG_FILENAME) {
        Ok(s) => match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(v) => v,
            Err(e) => {
                log_warn!(
                    "Config: failed to parse {} for JSON load: {}",
                    CONFIG_FILENAME,
                    e
                );
                return Vec::new();
            }
        },
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    for (side, key) in [(0u8, "p1"), (1u8, "p2")] {
        if let Some(obj) = root["custom_options"][key].as_object() {
            for (id, val) in obj {
                if let Some(v) = val.as_i64() {
                    out.push((side, id.clone(), v as i32));
                }
            }
        }
    }
    out
}

/// Save an arbitrary JSON value under a top-level key. Preserves all other keys.
pub fn save_json_key(key: &str, value: serde_json::Value) {
    let mut root = match fs::read_to_string(CONFIG_FILENAME) {
        Ok(s) => {
            serde_json::from_str::<serde_json::Value>(&s).unwrap_or_else(|_| serde_json::json!({}))
        }
        Err(_) => serde_json::json!({}),
    };

    root[key] = value;

    match serde_json::to_string_pretty(&root) {
        Ok(json) => {
            if let Err(e) = fs::write(CONFIG_FILENAME, json) {
                log_warn!("Config: failed to save key {}: {}", key, e);
            }
        }
        Err(e) => log_warn!("Config: failed to serialize for key {}: {}", key, e),
    }
}
