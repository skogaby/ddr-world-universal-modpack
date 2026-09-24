//! Scene style — the whole 3D scene's shading (stage props AND dancers):
//! STOCK (UNLIT) / SMOOTH SHADING / CEL SHADING (config `stock`/`lit`/`cel`),
//! plus the inverted-hull outlines — on/off, INK or LAYERED style
//! (`outline.rs`) and their per-kind widths — and the two A3 tempo
//! switches. Owns the live values the session builder reads
//! (`effective()`, `hull_plan()`, `outline_widths()`, `tempo_options()`,
//! and `movie_mode()` — the Background Movies choice the song window
//! latches at entry, `movie_mode.rs`), the overlay rows under the
//! Background Dancers header (the outline style + width rows are CHILDREN of
//! SCENE OUTLINES — hidden while it is OFF, `mod_menu::set_row_show_when`),
//! and the WHOLE-section persistence of `background_dancers`
//! (`save_json_key` replaces the section, so every key is re-emitted from
//! the live mirrors on each edit).
//!
//! Every row applies from the NEXT SONG: the style is applied at item build
//! by re-pointing each render item's private material copies at the
//! synthesized `<name>_lit` / `<name>_cel` variant containers
//! (`render_item::restyle_materials`, RE §4.7), and the outline twins are
//! created per session. Nothing here depends on a relaunch — every variant
//! container is synthesized at boot whenever the two mods are on.
//!
//! The three style values are the shape the maintainer intends to expose to
//! players later (stock / enhanced lighting / enhanced lighting + cel);
//! today they are operator experiment knobs (config + mod menu).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use crate::mods::config;
use crate::services::avs_layeredfs::shader_layout::SceneStyle;
use crate::services::avs_layeredfs::shader_synthesis;
use crate::{log_info, log_warn};

use super::movie_mode::MovieMode;
use super::outline::{self, HullPlan, OutlineStyle};

const MOD_ID: &str = "background-dancers";
const ROW_KEY_STYLE: &str = "background-dancers-scene-style";
const ROW_KEY_OUTLINES: &str = "background-dancers-scene-outlines";
const ROW_KEY_OUTLINE_STYLE: &str = "background-dancers-outline-style";
const ROW_KEY_PX_DANCER: &str = "background-dancers-outline-px";
const ROW_KEY_PX_STAGE: &str = "background-dancers-outline-px-stage";
const ROW_KEY_BPM_SYNC: &str = "background-dancers-bpm-sync";
const ROW_KEY_STOP_SLOW: &str = "background-dancers-stop-slow";
const ROW_KEY_CUSTOM_CONTENT: &str = "background-dancers-custom-content";
const ROW_KEY_MOVIE_MODE: &str = "background-dancers-movie-mode";
/// Rows shown only while SCENE OUTLINES is ON (`set_row_show_when`).
const OUTLINE_CHILD_ROWS: [&str; 3] = [ROW_KEY_OUTLINE_STYLE, ROW_KEY_PX_DANCER, ROW_KEY_PX_STAGE];

/// The outline-width rows step on a 0.25-px grid (values in HUNDREDTHS of a
/// pixel — the row API is integer-valued); config values off the grid are
/// kept live until the row is edited.
const PX_GRID_MIN: i32 = 50;
const PX_GRID_MAX: i32 = 600;
const PX_GRID_STEP: i32 = 25;

static LIVE_STYLE: AtomicU8 = AtomicU8::new(1); // SceneStyle::row_value
static LIVE_OUTLINES: AtomicBool = AtomicBool::new(true);
static LIVE_OUTLINE_STYLE: AtomicU8 = AtomicU8::new(0); // OutlineStyle::row_value
static LIVE_BPM_SYNC: AtomicBool = AtomicBool::new(true);
static LIVE_STOP_SLOW: AtomicBool = AtomicBool::new(true);
/// CUSTOM DANCERS & STAGES as of the latest edit — persistence mirror only:
/// the value that GOVERNS this boot is the config value `lifecycle::init_tables`
/// read (the catalog/rows are built once), so a row edit lands next launch.
static LIVE_CUSTOM_CONTENT: AtomicBool = AtomicBool::new(true);
/// BACKGROUND MOVIES (`MovieMode::row_value`) — read by the song window at
/// its entry (next-song knob).
static LIVE_MOVIE_MODE: AtomicU8 = AtomicU8::new(3); // MovieMode::DEFAULT (StageScreens)
/// Outline rim widths (f32 bits): dancers / stage props.
static LIVE_PX_DANCER: AtomicU32 = AtomicU32::new(0x4000_0000); // 2.0
static LIVE_PX_STAGE: AtomicU32 = AtomicU32::new(0x3FC0_0000); // 1.5
/// LAYERED palette override from config (never row-edited; re-emitted by
/// `persist_section` so a row edit cannot drop it). `None` = the default
/// black / red / blue.
static LIVE_LAYER_PALETTE: Mutex<Option<Vec<[f32; 3]>>> = Mutex::new(None);

pub const DEFAULT_OUTLINE_PX_DANCER: f32 = 2.0;
pub const DEFAULT_OUTLINE_PX_STAGE: f32 = 1.5;

/// The hull rim widths (720p px) a session applies: `(dancers, stage)`.
pub fn outline_widths() -> (f32, f32) {
    (
        f32::from_bits(LIVE_PX_DANCER.load(Ordering::Relaxed)),
        f32::from_bits(LIVE_PX_STAGE.load(Ordering::Relaxed)),
    )
}

fn layer_palette() -> Option<Vec<[f32; 3]>> {
    LIVE_LAYER_PALETTE.lock().ok().and_then(|g| g.clone())
}

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

/// Nearest row value (hundredths of a px on the 0.25 grid) for a width.
fn px_to_row(px: f32) -> i32 {
    let v = (px * 100.0).round() as i32;
    let snapped = ((v - PX_GRID_MIN + PX_GRID_STEP / 2).div_euclid(PX_GRID_STEP)) * PX_GRID_STEP
        + PX_GRID_MIN;
    snapped.clamp(PX_GRID_MIN, PX_GRID_MAX)
}

fn px_grid() -> (Vec<i32>, Vec<String>) {
    let values: Vec<i32> = (PX_GRID_MIN..=PX_GRID_MAX)
        .step_by(PX_GRID_STEP as usize)
        .collect();
    let labels = values
        .iter()
        .map(|v| format!("{:.2} PX", *v as f32 / 100.0))
        .collect();
    (values, labels)
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

/// The requested outline style (INK / LAYERED) as of the latest edit.
pub fn outline_style() -> OutlineStyle {
    OutlineStyle::from_row_value(LIVE_OUTLINE_STYLE.load(Ordering::Relaxed) as i32)
}

/// The hull layers a session being created now builds: nothing unless
/// `eff.hulls`; INK = one grey layer; LAYERED = the palette (config override
/// or black / red / blue), each stroke as wide again as the ink width
/// (`outline::HullPlan`).
pub fn hull_plan(eff: &Effective) -> HullPlan {
    if !eff.hulls {
        return HullPlan::none();
    }
    let palette = layer_palette().unwrap_or_default();
    HullPlan::for_style(outline_style(), &palette)
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
    let outline_style = match bd.outline_style.as_deref() {
        None => OutlineStyle::Ink,
        Some(s) => OutlineStyle::parse(s).unwrap_or_else(|| {
            log_warn!(
                "BackgroundDancers: background_dancers.outline_style = '{}' is not ink/layered -- using ink",
                s
            );
            OutlineStyle::Ink
        }),
    };
    LIVE_STYLE.store(style.row_value() as u8, Ordering::Relaxed);
    LIVE_OUTLINES.store(outlines, Ordering::Relaxed);
    LIVE_OUTLINE_STYLE.store(outline_style.row_value() as u8, Ordering::Relaxed);
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
    let px_d = config::clamp_outline_px(bd.outline_px.unwrap_or(DEFAULT_OUTLINE_PX_DANCER));
    let px_s = config::clamp_outline_px(bd.outline_px_stage.unwrap_or(DEFAULT_OUTLINE_PX_STAGE));
    LIVE_PX_DANCER.store(px_d.to_bits(), Ordering::Relaxed);
    LIVE_PX_STAGE.store(px_s.to_bits(), Ordering::Relaxed);
    // LAYERED palette override: 1..=MAX_LAYERS entries, channels clamped.
    let palette = match bd.outline_layer_colors.as_ref() {
        None => None,
        Some(v) if v.is_empty() || v.len() > outline::MAX_LAYERS => {
            log_warn!(
                "BackgroundDancers: background_dancers.outline_layer_colors has {} entr{} (need 1..={}) -- using the default black/red/blue",
                v.len(),
                if v.len() == 1 { "y" } else { "ies" },
                outline::MAX_LAYERS
            );
            None
        }
        Some(v) => Some(v.iter().map(|c| outline::clamp_rgb(*c)).collect::<Vec<_>>()),
    };
    if let Ok(mut g) = LIVE_LAYER_PALETTE.lock() {
        *g = palette.clone();
    }
    log_info!(
        "BackgroundDancers: background movies -- {} (applies per song)",
        movie_mode.key()
    );
    log_info!(
        "BackgroundDancers: scene style -- {} outlines={} style={} (rim px dancers={} stage={}; layered palette={}; variants {}, outline programs {}; applies per song)",
        style.key(),
        outlines,
        outline_style.key(),
        px_d,
        px_s,
        match palette.as_ref() {
            Some(p) => format!("{} custom", p.len()),
            None => "default".to_string(),
        },
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
            outline_style,
            px_d,
            px_s,
            bd.bpm_sync,
            bd.stop_slow,
            bd.custom_content,
            movie_mode,
        );
    }
}

/// Write the whole `background_dancers` section from the live values.
fn persist_section() {
    let style = SceneStyle::from_row_value(LIVE_STYLE.load(Ordering::Relaxed) as i32);
    let mut section = serde_json::json!({
        "bpm_sync": LIVE_BPM_SYNC.load(Ordering::Relaxed),
        "stop_slow": LIVE_STOP_SLOW.load(Ordering::Relaxed),
        "style": style.key(),
        "outlines": LIVE_OUTLINES.load(Ordering::Relaxed),
        "outline_style": outline_style().key(),
        "outline_px": f32::from_bits(LIVE_PX_DANCER.load(Ordering::Relaxed)),
        "outline_px_stage": f32::from_bits(LIVE_PX_STAGE.load(Ordering::Relaxed)),
        "custom_content": LIVE_CUSTOM_CONTENT.load(Ordering::Relaxed),
        "movie_mode": movie_mode().key(),
    });
    // Optional key: absent means "default", so it is emitted only when set
    // (the palette is operator-authored and must survive every row edit).
    if let Some(map) = section.as_object_mut() {
        if let Some(pal) = layer_palette() {
            map.insert("outline_layer_colors".to_string(), serde_json::json!(pal));
        }
    }
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

fn set_outline_style(value: i32) {
    let s = OutlineStyle::from_row_value(value);
    LIVE_OUTLINE_STYLE.store(s.row_value() as u8, Ordering::Relaxed);
    persist_section();
    log_info!(
        "BackgroundDancers: OUTLINE STYLE set to {} (applies from the next song)",
        s.key().to_ascii_uppercase()
    );
}

fn set_px_dancer(value: i32) {
    let px = config::clamp_outline_px(value as f32 / 100.0);
    LIVE_PX_DANCER.store(px.to_bits(), Ordering::Relaxed);
    persist_section();
    log_info!(
        "BackgroundDancers: OUTLINE WIDTH (DANCERS) set to {:.2} px (applies from the next song)",
        px
    );
}

fn set_px_stage(value: i32) {
    let px = config::clamp_outline_px(value as f32 / 100.0);
    LIVE_PX_STAGE.store(px.to_bits(), Ordering::Relaxed);
    persist_section();
    log_info!(
        "BackgroundDancers: OUTLINE WIDTH (STAGE) set to {:.2} px (applies from the next song)",
        px
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

#[allow(clippy::too_many_arguments)]
fn register_rows(
    style: SceneStyle,
    outlines: bool,
    outline_style: OutlineStyle,
    px_dancer: f32,
    px_stage: f32,
    bpm_sync: bool,
    stop_slow: bool,
    custom_content: bool,
    movie_mode: MovieMode,
) {
    use crate::mods::mod_menu::{register_enum_row, set_row_show_when, EnumRowSpec};
    let on_off = || (vec![0, 1], vec!["OFF".to_string(), "ON".to_string()]);
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_STYLE.to_string(),
        label: "Lighting Style".to_string(),
        hint: "Shading of the 3D stage and dancers: the game's unlit look, smooth key-light shading, or the same light in cel bands with ink. Next song.".to_string(),
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
        hint: "Ink outline around the 3D stage props and dancers (needs a shaded lighting style). Next song.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: v,
        labels: l,
        initial_value: i32::from(outlines),
        on_change: Arc::new(set_outlines),
    });
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_OUTLINE_STYLE.to_string(),
        label: "Outline Style".to_string(),
        hint: "INK: one black stroke. LAYERED: stacked strokes like the game's UI text -- black, then red, then blue.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: vec![0, 1],
        labels: vec!["INK".to_string(), "LAYERED".to_string()],
        initial_value: outline_style.row_value(),
        on_change: Arc::new(set_outline_style),
    });
    let (v, l) = px_grid();
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_PX_DANCER.to_string(),
        label: "Outline Width (Dancers)".to_string(),
        hint: "Outline thickness on the dancers, in 720p pixels. Next song.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: v.clone(),
        labels: l.clone(),
        initial_value: px_to_row(px_dancer),
        on_change: Arc::new(set_px_dancer),
    });
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_PX_STAGE.to_string(),
        label: "Outline Width (Stage)".to_string(),
        hint: "Outline thickness on the stage props, in 720p pixels. Next song.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: v,
        labels: l,
        initial_value: px_to_row(px_stage),
        on_change: Arc::new(set_px_stage),
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
    // The three outline detail rows are CHILDREN of SCENE OUTLINES: hidden
    // while it is OFF (their values persist unchanged underneath).
    for key in OUTLINE_CHILD_ROWS {
        set_row_show_when(key, ROW_KEY_OUTLINES, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn px_grid_round_trips() {
        let (values, labels) = px_grid();
        assert_eq!(values.first(), Some(&50));
        assert_eq!(values.last(), Some(&600));
        assert_eq!(values.len(), labels.len());
        assert_eq!(labels[0], "0.50 PX");
        assert_eq!(
            labels[values.iter().position(|&v| v == 150).unwrap()],
            "1.50 PX"
        );
        // Defaults sit on the grid; off-grid config snaps to the nearest step.
        assert_eq!(px_to_row(DEFAULT_OUTLINE_PX_DANCER), 200);
        assert_eq!(px_to_row(DEFAULT_OUTLINE_PX_STAGE), 150);
        assert_eq!(px_to_row(1.3), 125);
        assert_eq!(px_to_row(1.4), 150);
        assert_eq!(px_to_row(0.1), 50);
        assert_eq!(px_to_row(9.0), 600);
        for v in &values {
            assert_eq!(px_to_row(*v as f32 / 100.0), *v);
        }
    }

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
