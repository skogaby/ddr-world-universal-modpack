//! Scene style — the whole 3D scene's shading (stage props AND dancers):
//! STOCK (UNLIT) / SMOOTH SHADING / CEL SHADING (config `stock`/`lit`/`cel`),
//! plus the inverted-hull outlines (on/off — since 2026-09-28 CEL and the
//! outline are DANCING STAGE UNLEASHED's own toon shading and ink,
//! `shaders/src/mdl_cel.hlsl`, `outline.rs`) and the two A3 tempo
//! switches. Owns the live values the session builder reads
//! (`effective()`, `hull_plan()`, `tempo_options()`,
//! and `movie_mode()` — the Background Movies choice the song window
//! latches at entry, `movie_mode.rs`) plus the one the director reads every
//! frame (`head_scale()` — BIG HEAD), the overlay rows under the
//! Background Dancers header,
//! and the WHOLE-section persistence of `background_dancers`
//! (`save_json_key` replaces the section, so every key is re-emitted from
//! the live mirrors on each edit).
//!
//! Every row applies from the NEXT SONG: the style is applied at item build
//! by re-pointing each render item's private material copies at the
//! synthesized `<name>_lit` / `<name>_cel` variant containers
//! (`render_item::restyle_materials`, RE §4.7), and the outline twins are
//! created per session. Nothing here depends on a relaunch — every variant
//! container is synthesized at boot whenever the two mods are on. The
//! exceptions: CUSTOM DANCERS & STAGES (next launch — the catalog is built
//! once) and BIG HEAD, the section's last row, which is LIVE — `head_scale()`
//! is read by `director::produce` every frame (gameplay and previews), so a
//! toggle shows on the next frame (`docs/big_head_mode_feasibility.md`).
//!
//! The three style values are the shape the maintainer intends to expose to
//! players later (stock / enhanced lighting / enhanced lighting + cel);
//! today they are operator experiment knobs (config + mod menu).

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use crate::mods::config;
use crate::services::avs_layeredfs::shader_layout::SceneStyle;
use crate::services::avs_layeredfs::shader_synthesis;
use crate::{log_info, log_warn};

use super::director_math::BIG_HEAD_SCALE;
use super::movie_mode::MovieMode;
use super::outline::HullPlan;

const MOD_ID: &str = "background-dancers";
const ROW_KEY_STYLE: &str = "background-dancers-scene-style";
const ROW_KEY_OUTLINES: &str = "background-dancers-scene-outlines";
const ROW_KEY_BPM_SYNC: &str = "background-dancers-bpm-sync";
const ROW_KEY_STOP_SLOW: &str = "background-dancers-stop-slow";
const ROW_KEY_CUSTOM_CONTENT: &str = "background-dancers-custom-content";
const ROW_KEY_MOVIE_MODE: &str = "background-dancers-movie-mode";
const ROW_KEY_BIG_HEAD: &str = "background-dancers-big-head";

static LIVE_STYLE: AtomicU8 = AtomicU8::new(1); // SceneStyle::row_value
static LIVE_OUTLINES: AtomicBool = AtomicBool::new(true);
static LIVE_BPM_SYNC: AtomicBool = AtomicBool::new(true);
static LIVE_STOP_SLOW: AtomicBool = AtomicBool::new(true);
/// CUSTOM DANCERS & STAGES as of the latest edit — persistence mirror only:
/// the value that GOVERNS this boot is the config value `lifecycle::init_tables`
/// read (the catalog/rows are built once), so a row edit lands next launch.
static LIVE_CUSTOM_CONTENT: AtomicBool = AtomicBool::new(true);
/// BACKGROUND MOVIES (`MovieMode::row_value`) — read by the song window at
/// its entry (next-song knob).
static LIVE_MOVIE_MODE: AtomicU8 = AtomicU8::new(3); // MovieMode::DEFAULT (StageScreens)
/// BIG HEAD — the one LIVE row: `director::produce` reads it every frame.
static LIVE_BIG_HEAD: AtomicBool = AtomicBool::new(false);
/// The two A3 ConfigBank switches as of the latest edit (`bpm_sync`,
/// `stop_slow`) — read at every session creation (next-song knobs).
pub fn tempo_options() -> super::tempo::TempoOptions {
    super::tempo::TempoOptions {
        bpm_sync: LIVE_BPM_SYNC.load(Ordering::Relaxed),
        stop_slow: LIVE_STOP_SLOW.load(Ordering::Relaxed),
    }
}

/// The Background Movies mode as of the latest edit (latched by the song
/// window at its entry).
pub fn movie_mode() -> MovieMode {
    MovieMode::from_row_value(LIVE_MOVIE_MODE.load(Ordering::Relaxed) as i32)
}

/// The factor the director applies to every dancer's `Head` subtree THIS
/// frame: [`BIG_HEAD_SCALE`] while BIG HEAD is on, else `1.0` (no-op). Live —
/// one relaxed atomic load, safe on the game thread's per-frame path.
pub fn head_scale() -> f32 {
    if LIVE_BIG_HEAD.load(Ordering::Relaxed) {
        BIG_HEAD_SCALE
    } else {
        1.0
    }
}

static ROWS_REGISTERED: AtomicBool = AtomicBool::new(false);

/// What the operator asked for (before availability gating).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Requested {
    pub style: SceneStyle,
    pub outlines: bool,
}

/// What a session can actually build this boot: the requested style if
/// the variant containers are being served (else STOCK, one WARN per
/// change), outlines only with a non-stock style AND served outline programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effective {
    pub style: SceneStyle,
    pub hulls: bool,
}

pub fn requested() -> Requested {
    Requested {
        style: SceneStyle::from_row_value(LIVE_STYLE.load(Ordering::Relaxed) as i32),
        outlines: LIVE_OUTLINES.load(Ordering::Relaxed),
    }
}

/// The hull layers a session being created now builds: nothing unless
/// `eff.hulls`, else DSU's one black hull (`outline::HullPlan::ink`).
pub fn hull_plan(eff: &Effective) -> HullPlan {
    if !eff.hulls {
        return HullPlan::none();
    }
    HullPlan::ink()
}

/// Pure gate (host-tested in `pure.rs`-style: no engine reads).
pub fn effective_for(req: Requested, variants_served: bool, outlines_served: bool) -> Effective {
    let style = if variants_served {
        req.style
    } else {
        SceneStyle::Stock
    };
    Effective {
        style,
        hulls: style != SceneStyle::Stock && req.outlines && outlines_served,
    }
}

static LAST_GATE_WARN: AtomicU8 = AtomicU8::new(0);

/// The style/hull decision for a session being created now.
pub fn effective() -> Effective {
    let req = requested();
    let served = shader_synthesis::variants_available();
    let outlines_served = shader_synthesis::outline_programs_available();
    let eff = effective_for(req, served, outlines_served);
    // One WARN per distinct degradation (the flags never change after boot).
    let code: u8 = match (
        req.style != SceneStyle::Stock && !served,
        req.outlines && eff.style != SceneStyle::Stock && !outlines_served,
    ) {
        (true, _) => 1,
        (false, true) => 2,
        _ => 0,
    };
    if code != 0 && LAST_GATE_WARN.swap(code, Ordering::Relaxed) != code {
        match code {
            1 => log_warn!(
                "BackgroundDancers: scene style {} requested but the shader variants are not served (shader-fixes off / blobs missing / shader.arc not intercepted) -- stock this boot",
                req.style.key()
            ),
            _ => log_warn!(
                "BackgroundDancers: scene outlines requested but the outline programs are not served -- no outlines this boot"
            ),
        }
    }
    eff
}

/// Seed the live values from config (+ the legacy shader_fixes keys) at
/// enable and register the two rows (once — the row API has no unregister).
pub fn init_from_config() {
    let cfg = config::get();
    let bd = cfg
        .as_ref()
        .and_then(|c| c.background_dancers.clone())
        .unwrap_or_default();
    let legacy = cfg.as_ref().and_then(|c| c.shader_fixes.as_ref());
    let style = match bd.scene_style(legacy) {
        Ok(s) => s,
        Err(bad) => {
            log_warn!(
                "BackgroundDancers: background_dancers.style = '{}' is not stock/lit/cel -- using lit",
                bad
            );
            SceneStyle::Lit
        }
    };
    let outlines = bd.scene_outlines(legacy);
    if bd.outline_style.is_some()
        || bd.outline_px.is_some()
        || bd.outline_px_stage.is_some()
        || bd.outline_layer_colors.is_some()
    {
        log_info!(
            "BackgroundDancers: outline_style / outline_px / outline_px_stage / outline_layer_colors are retired (the outline is DSU's own since 2026-09-28) -- ignored, dropped at the next row edit"
        );
    }
    LIVE_STYLE.store(style.row_value() as u8, Ordering::Relaxed);
    LIVE_OUTLINES.store(outlines, Ordering::Relaxed);
    LIVE_BPM_SYNC.store(bd.bpm_sync, Ordering::Relaxed);
    LIVE_STOP_SLOW.store(bd.stop_slow, Ordering::Relaxed);
    LIVE_CUSTOM_CONTENT.store(bd.custom_content, Ordering::Relaxed);
    let movie_mode = match bd.movie_mode.as_deref() {
        None => MovieMode::DEFAULT,
        Some(s) => MovieMode::parse(s).unwrap_or_else(|| {
            log_warn!(
                "BackgroundDancers: background_dancers.movie_mode = '{}' is not {} -- using {}",
                s,
                MovieMode::keys_list(),
                MovieMode::DEFAULT.key()
            );
            MovieMode::DEFAULT
        }),
    };
    LIVE_MOVIE_MODE.store(movie_mode.row_value() as u8, Ordering::Relaxed);
    LIVE_BIG_HEAD.store(bd.big_head, Ordering::Relaxed);
    log_info!(
        "BackgroundDancers: background movies -- {} (applies per song)",
        movie_mode.key()
    );
    log_info!(
        "BackgroundDancers: big head -- {} (x{}, live)",
        if bd.big_head { "on" } else { "off" },
        BIG_HEAD_SCALE
    );
    log_info!(
        "BackgroundDancers: scene style -- {} outlines={} (cel + outline = DSU toon; variants {}, outline programs {}; applies per song)",
        style.key(),
        outlines,
        if shader_synthesis::variants_available() {
            "served"
        } else {
            "NOT served"
        },
        if shader_synthesis::outline_programs_available() {
            "served"
        } else {
            "NOT served"
        }
    );
    if !ROWS_REGISTERED.swap(true, Ordering::Relaxed) {
        register_rows(
            style,
            outlines,
            bd.bpm_sync,
            bd.stop_slow,
            bd.custom_content,
            movie_mode,
            bd.big_head,
        );
    }
}

/// Write the whole `background_dancers` section from the live values.
fn persist_section() {
    let style = SceneStyle::from_row_value(LIVE_STYLE.load(Ordering::Relaxed) as i32);
    let section = serde_json::json!({
        "bpm_sync": LIVE_BPM_SYNC.load(Ordering::Relaxed),
        "stop_slow": LIVE_STOP_SLOW.load(Ordering::Relaxed),
        "style": style.key(),
        "outlines": LIVE_OUTLINES.load(Ordering::Relaxed),
        "custom_content": LIVE_CUSTOM_CONTENT.load(Ordering::Relaxed),
        "movie_mode": movie_mode().key(),
        "big_head": LIVE_BIG_HEAD.load(Ordering::Relaxed),
    });
    config::save_json_key("background_dancers", section);
}

fn set_style(value: i32) {
    let style = SceneStyle::from_row_value(value);
    LIVE_STYLE.store(style.row_value() as u8, Ordering::Relaxed);
    persist_section();
    log_info!(
        "BackgroundDancers: LIGHTING STYLE set to {} (applies from the next song)",
        style.key().to_ascii_uppercase()
    );
}

fn set_outlines(value: i32) {
    let on = value != 0;
    LIVE_OUTLINES.store(on, Ordering::Relaxed);
    persist_section();
    log_info!(
        "BackgroundDancers: SCENE OUTLINES set to {} (applies from the next song)",
        if on { "ON" } else { "OFF" }
    );
}

fn set_bpm_sync(value: i32) {
    let on = value != 0;
    LIVE_BPM_SYNC.store(on, Ordering::Relaxed);
    persist_section();
    log_info!(
        "BackgroundDancers: BPM SYNC set to {} (applies from the next song)",
        if on { "ON" } else { "OFF" }
    );
}

fn set_stop_slow(value: i32) {
    let on = value != 0;
    LIVE_STOP_SLOW.store(on, Ordering::Relaxed);
    persist_section();
    log_info!(
        "BackgroundDancers: STOP SLOW-MOTION set to {} (applies from the next song)",
        if on { "ON" } else { "OFF" }
    );
}

fn set_custom_content(value: i32) {
    let on = value != 0;
    LIVE_CUSTOM_CONTENT.store(on, Ordering::Relaxed);
    persist_section();
    log_info!(
        "BackgroundDancers: CUSTOM DANCERS & STAGES set to {} (the catalog is built at launch -- applies at the next launch)",
        if on { "ON" } else { "OFF" }
    );
}

fn set_movie_mode(value: i32) {
    let m = MovieMode::from_row_value(value);
    LIVE_MOVIE_MODE.store(m.row_value() as u8, Ordering::Relaxed);
    persist_section();
    log_info!(
        "BackgroundDancers: BACKGROUND MOVIES set to {} (applies from the next song)",
        m.label()
    );
}

fn set_big_head(value: i32) {
    let on = value != 0;
    LIVE_BIG_HEAD.store(on, Ordering::Relaxed);
    persist_section();
    log_info!(
        "BackgroundDancers: BIG HEAD set to {} (live)",
        if on { "ON" } else { "OFF" }
    );
}

#[allow(clippy::too_many_arguments)]
fn register_rows(
    style: SceneStyle,
    outlines: bool,
    bpm_sync: bool,
    stop_slow: bool,
    custom_content: bool,
    movie_mode: MovieMode,
    big_head: bool,
) {
    use crate::mods::mod_menu::{register_enum_row, EnumRowSpec};
    let on_off = || (vec![0, 1], vec!["OFF".to_string(), "ON".to_string()]);
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_STYLE.to_string(),
        label: "Lighting Style".to_string(),
        hint: "Shading of the 3D stage and dancers: the game's unlit look, smooth key-light shading, or Dancing Stage Unleashed's two-band toon shading. Next song.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: vec![0, 1, 2],
        labels: vec![
            "STOCK (UNLIT)".to_string(),
            "SMOOTH SHADING".to_string(),
            "CEL SHADING".to_string(),
        ],
        initial_value: style.row_value(),
        on_change: Arc::new(set_style),
    });
    let (v, l) = on_off();
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_OUTLINES.to_string(),
        label: "Scene Outlines".to_string(),
        hint: "Dancing Stage Unleashed's black ink outline around the 3D stage props and dancers (needs a shaded lighting style). Next song.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: v,
        labels: l,
        initial_value: i32::from(outlines),
        on_change: Arc::new(set_outlines),
    });
    let (v, l) = on_off();
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_BPM_SYNC.to_string(),
        label: "BPM Sync".to_string(),
        hint: "Dancers, stage and camera run at the chart's tempo (half a second of choreography per beat, cuts on beats) instead of real time. Next song.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: v,
        labels: l,
        initial_value: i32::from(bpm_sync),
        on_change: Arc::new(set_bpm_sync),
    });
    let (v, l) = on_off();
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_STOP_SLOW.to_string(),
        label: "Stop Slow-Motion".to_string(),
        hint: "Drop the scene to 1/12 speed through a chart STOP (below 10 BPM), as DDR A3 did. Next song.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: v,
        labels: l,
        initial_value: i32::from(stop_slow),
        on_change: Arc::new(set_stop_slow),
    });
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_MOVIE_MODE.to_string(),
        label: "Background Movies".to_string(),
        hint: "Movie songs: OFF hides it; THUMBNAIL: small window; STAGE SCREENS: on the stage's screens; FULLSCREEN: behind the dancers; MOVIE ONLY: no 3D scene.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: MovieMode::ALL.iter().map(|m| m.row_value()).collect(),
        labels: MovieMode::ALL.iter().map(|m| m.label().to_string()).collect(),
        initial_value: movie_mode.row_value(),
        on_change: Arc::new(set_movie_mode),
    });
    let (v, l) = on_off();
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_CUSTOM_CONTENT.to_string(),
        label: "Custom Dancers & Stages".to_string(),
        hint: "Also use community dancers/stages from data_mods/custom_models/dancers and /stages (folder name = display name) beside the stock ones. Next launch.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: v,
        labels: l,
        initial_value: i32::from(custom_content),
        on_change: Arc::new(set_custom_content),
    });
    // BIG HEAD — the LAST row of the section (rows render in registration
    // order), and the only live one.
    let (v, l) = on_off();
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_BIG_HEAD.to_string(),
        label: "Big Head".to_string(),
        hint: "Every dancer's head at 3x size, hair and head accessories included (previews too). Applies immediately.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: v,
        labels: l,
        initial_value: i32::from(big_head),
        on_change: Arc::new(set_big_head),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_matrix() {
        let req = |style, outlines| Requested { style, outlines };
        // Served: requested style, hulls need non-stock + outlines + programs.
        assert_eq!(
            effective_for(req(SceneStyle::Cel, true), true, true),
            Effective {
                style: SceneStyle::Cel,
                hulls: true
            }
        );
        assert_eq!(
            effective_for(req(SceneStyle::Lit, false), true, true),
            Effective {
                style: SceneStyle::Lit,
                hulls: false
            }
        );
        assert_eq!(
            effective_for(req(SceneStyle::Stock, true), true, true),
            Effective {
                style: SceneStyle::Stock,
                hulls: false
            }
        );
        // Outline programs missing ⇒ style stays, no hulls.
        assert_eq!(
            effective_for(req(SceneStyle::Cel, true), true, false),
            Effective {
                style: SceneStyle::Cel,
                hulls: false
            }
        );
        // Variants missing ⇒ stock, no hulls, regardless of the request.
        assert_eq!(
            effective_for(req(SceneStyle::Cel, true), false, true),
            Effective {
                style: SceneStyle::Stock,
                hulls: false
            }
        );
    }
}
