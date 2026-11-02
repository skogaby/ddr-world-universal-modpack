//! Scene style — the whole 3D scene's shading (stage props AND dancers):
//! STOCK (UNLIT) / SMOOTH SHADING / CEL SHADING (config `stock`/`lit`/`cel`)
//! and, since 2026-11-02, fifteen DANCERS-ONLY DDR Universe render styles
//! (CONTRAST … X-RAY and OUTLINE ONLY, `shader_layout::SceneStyle`,
//! `docs/ddr_universe_render_styles_research.md`: the stage takes CEL in
//! them, the dancers' own outline only with NEGATIVE / MONOCHROME / SEPIA)
//! plus RANDOM (EVERY BAR) — the first row value: a different preset per
//! chart bar — plus the inverted-hull outlines (on/off — since 2026-09-28
//! CEL and the outline are DANCING STAGE UNLEASHED's own toon shading and
//! ink, `shaders/src/mdl_cel.hlsl`, `outline.rs`) and the two A3 tempo
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
//! LIGHTING STYLE is LIVE: every built instance captures a runtime look
//! switch (`style_rig.rs` — the synthesized `<name>_lit` / `<name>_cel` /
//! `<name>_u<family>` container of every family for each restyleable
//! material copy, RE §4.7) and `session::Session::style_tick` applies the
//! current style each frame — [`generation`] tells it an edit happened —
//! in gameplay and in the song-select previews alike. The hulls are built
//! per song (`hull_plan`: wherever a look MAY need one) and shown or hidden
//! per look; SCENE OUTLINES shows / hides the built ones at once and
//! decides what is built from the next song. The other rows apply from the
//! NEXT SONG, except CUSTOM DANCERS & STAGES (next launch — the catalog is
//! built once) and BIG HEAD, the section's last row, which is LIVE —
//! `head_scale()` is read by `director::produce` every frame (gameplay and
//! previews), so a toggle shows on the next frame
//! (`docs/big_head_mode_feasibility.md`). Nothing here depends on a relaunch
//! — every variant container is synthesized at boot whenever the two mods
//! are on.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use crate::mods::config;
use crate::services::avs_layeredfs::shader_layout::{SceneStyle, STYLE_TEXTURE_ARC};
use crate::services::avs_layeredfs::shader_synthesis;
use crate::services::scene3d::arc_set;
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

static LIVE_STYLE: AtomicU8 = AtomicU8::new(2); // SceneStyle::row_value (LIT until init)
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

/// What a session can actually draw this boot: the requested style gated on
/// what is served (`gate_style`), the live SCENE OUTLINES switch, and the
/// served set (RANDOM's pool and the per-bar picks are gated by it too).
/// Sessions re-read it whenever [`generation`] moves — every LIGHTING STYLE
/// / SCENE OUTLINES edit applies at once (`session::Session::style_tick`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effective {
    /// May be `Random` (resolved per bar by the session).
    pub style: SceneStyle,
    /// SCENE OUTLINES on AND the outline pair served: the hulls of the looks
    /// that outline are shown.
    pub outlines: bool,
    pub served: Served,
}

/// What this boot serves (the inputs of [`effective_for`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Served {
    /// The `_lit` / `_cel` variant containers.
    pub variants: bool,
    /// The outline pair at program 0 of every variant container.
    pub outlines: bool,
    /// The DDR Universe `_u<family>` containers.
    pub universe: bool,
    /// `shader_layout::STYLE_TEXTURE_ARC` resolves.
    pub style_textures: bool,
}

impl Effective {
    /// RANDOM (EVERY BAR)'s presets this boot.
    pub fn random_pool(&self) -> Vec<SceneStyle> {
        SceneStyle::random_pool(
            self.served.universe,
            self.served.style_textures,
            self.served.outlines,
        )
    }
}

pub fn requested() -> Requested {
    Requested {
        style: SceneStyle::from_row_value(LIVE_STYLE.load(Ordering::Relaxed) as i32),
        outlines: LIVE_OUTLINES.load(Ordering::Relaxed),
    }
}

/// Bumped by every LIGHTING STYLE / SCENE OUTLINES edit.
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// See [`GENERATION`] (a live session re-reads [`effective`] when it moves).
pub fn generation() -> u32 {
    GENERATION.load(Ordering::Acquire)
}

/// One style as this boot can draw it: STOCK without the variant containers;
/// CEL for a DDR Universe style without its containers, for a texture style
/// without the style-texture arc and for OUTLINE ONLY without the outline
/// pair; RANDOM stays RANDOM (its pool is gated instead).
pub fn gate_style(style: SceneStyle, served: Served) -> SceneStyle {
    if !served.variants {
        SceneStyle::Stock
    } else if style == SceneStyle::Random {
        style
    } else if style.is_universe() && !served.universe
        || style.texture().is_some() && !served.style_textures
        || style == SceneStyle::Outline && !served.outlines
    {
        SceneStyle::Cel
    } else {
        style
    }
}

/// The hulls a session being created now builds — DSU's one black hull
/// (`outline::HullPlan`) on each body whose look MAY need one this song;
/// the session shows or hides every hull per look. Stage props: SCENE
/// OUTLINES on with a shaded stage. Dancers: SCENE OUTLINES on with a
/// shaded style, or OUTLINE ONLY / RANDOM (the hull IS the look). A dancer
/// preview (`dancer_preview`) always builds them when served — every look
/// follows the menu live there, so any may need one.
pub fn hull_plan(eff: &Effective, dancer_preview: bool) -> HullPlan {
    if !eff.served.outlines || !eff.served.variants {
        return HullPlan::none();
    }
    if dancer_preview {
        return HullPlan::ink_on(false, true);
    }
    let stage = eff.outlines && eff.style.stage_style() != SceneStyle::Stock;
    let dancers = eff.outlines && eff.style != SceneStyle::Stock || eff.style.needs_hulls();
    HullPlan::ink_on(stage, dancers)
}

/// Pure gate (host-tested: no engine reads).
pub fn effective_for(req: Requested, served: Served) -> Effective {
    Effective {
        style: gate_style(req.style, served),
        outlines: req.outlines && served.outlines,
        served,
    }
}

static LAST_GATE_WARN: AtomicU8 = AtomicU8::new(0);

/// The style/hull decision as of now (a session reads it at creation and
/// whenever [`generation`] moves).
pub fn effective() -> Effective {
    let req = requested();
    let served = Served {
        variants: shader_synthesis::variants_available(),
        outlines: shader_synthesis::outline_programs_available(),
        universe: shader_synthesis::universe_available(),
        style_textures: arc_set::resolve_path(STYLE_TEXTURE_ARC).is_some(),
    };
    let eff = effective_for(req, served);
    // One WARN per distinct degradation (the flags never change after boot;
    // the arc check is per edit but a mid-boot change is an operator edit).
    let code: u8 = if req.style != SceneStyle::Stock && !served.variants {
        1
    } else if req.style.is_universe() && !served.universe {
        3
    } else if (req.style.texture().is_some() || req.style == SceneStyle::Random)
        && !served.style_textures
    {
        4
    } else if req.style.needs_hulls() && !served.outlines {
        5
    } else if req.outlines && eff.style != SceneStyle::Stock && !served.outlines {
        2
    } else {
        0
    };
    if code != 0 && LAST_GATE_WARN.swap(code, Ordering::Relaxed) != code {
        match code {
            1 => log_warn!(
                "BackgroundDancers: scene style {} requested but the shader variants are not served (shader-fixes off / blobs missing / shader.arc not intercepted) -- stock this boot",
                req.style.key()
            ),
            3 => log_warn!(
                "BackgroundDancers: lighting style {} requested but the DDR Universe shader containers are not served (mdl_uni_* blobs missing from data_mods/shader_fixes/blobs/) -- CEL this boot",
                req.style.key()
            ),
            4 => log_warn!(
                "BackgroundDancers: lighting style {} needs {} (deploy data_mods/background_dancers/ with the DLL) -- CEL (RANDOM: the texture styles left out)",
                req.style.key(),
                STYLE_TEXTURE_ARC
            ),
            5 => log_warn!(
                "BackgroundDancers: lighting style {} needs the outline programs, which are not served -- CEL (RANDOM: OUTLINE ONLY left out)",
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
                "BackgroundDancers: background_dancers.style = '{}' is not one of {} -- using lit",
                bad,
                SceneStyle::ALL
                    .iter()
                    .map(|s| s.key())
                    .collect::<Vec<_>>()
                    .join("/")
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
        "BackgroundDancers: scene style -- {} outlines={} (cel + outline = DSU toon; variants {}, outline programs {}, DDR Universe styles {}; applies per song)",
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
        },
        if shader_synthesis::universe_available() {
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
    GENERATION.fetch_add(1, Ordering::AcqRel);
    persist_section();
    log_info!(
        "BackgroundDancers: LIGHTING STYLE set to {} (applies now to live scenes and previews)",
        style.key().to_ascii_uppercase()
    );
}

fn set_outlines(value: i32) {
    let on = value != 0;
    LIVE_OUTLINES.store(on, Ordering::Relaxed);
    GENERATION.fetch_add(1, Ordering::AcqRel);
    persist_section();
    log_info!(
        "BackgroundDancers: SCENE OUTLINES set to {} (shown/hidden now where the outlines were built; built from the next song)",
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
        hint: "Shading of the 3D scene: unlit, smooth, Dancing Stage Unleashed toon, or a DDR Universe dancer effect (stage stays toon); RANDOM changes it every bar. Applies now.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: SceneStyle::ALL.iter().map(|s| s.row_value()).collect(),
        labels: SceneStyle::ALL.iter().map(|s| s.label().to_string()).collect(),
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
        let all = Served {
            variants: true,
            outlines: true,
            universe: true,
            style_textures: true,
        };
        let e = effective_for(req(SceneStyle::Cel, true), all);
        assert_eq!((e.style, e.outlines), (SceneStyle::Cel, true));
        let e = effective_for(req(SceneStyle::Lit, false), all);
        assert_eq!((e.style, e.outlines), (SceneStyle::Lit, false));
        // Outline programs missing ⇒ style stays, outlines off, no hulls.
        let no_ol = Served {
            outlines: false,
            ..all
        };
        let e = effective_for(req(SceneStyle::Cel, true), no_ol);
        assert_eq!((e.style, e.outlines), (SceneStyle::Cel, false));
        assert!(hull_plan(&e, false).is_none());
        // Variants missing ⇒ stock, whatever was asked (RANDOM included).
        let none = Served {
            variants: false,
            ..all
        };
        for s in [SceneStyle::Thermo, SceneStyle::Random, SceneStyle::Outline] {
            assert_eq!(effective_for(req(s, true), none).style, SceneStyle::Stock);
        }
        assert!(hull_plan(&effective_for(req(SceneStyle::Cel, true), none), false).is_none());
    }

    #[test]
    fn universe_gate() {
        let all = Served {
            variants: true,
            outlines: true,
            universe: true,
            style_textures: true,
        };
        assert_eq!(gate_style(SceneStyle::Negative, all), SceneStyle::Negative);
        assert_eq!(gate_style(SceneStyle::Random, all), SceneStyle::Random);
        let no_uni = Served {
            universe: false,
            ..all
        };
        assert_eq!(gate_style(SceneStyle::Xray, no_uni), SceneStyle::Cel);
        assert_eq!(gate_style(SceneStyle::Outline, no_uni), SceneStyle::Cel);
        let no_tex = Served {
            style_textures: false,
            ..all
        };
        assert_eq!(gate_style(SceneStyle::Mercury, no_tex), SceneStyle::Cel);
        assert_eq!(
            gate_style(SceneStyle::StencilRainbow, no_tex),
            SceneStyle::Cel
        );
        assert_eq!(
            gate_style(SceneStyle::Contrast, no_tex),
            SceneStyle::Contrast
        );
        assert_eq!(gate_style(SceneStyle::Lit, no_tex), SceneStyle::Lit);
        let no_ol = Served {
            outlines: false,
            ..all
        };
        assert_eq!(gate_style(SceneStyle::Outline, no_ol), SceneStyle::Cel);
        // RANDOM's pool follows the served set.
        let r = |served| {
            effective_for(
                Requested {
                    style: SceneStyle::Random,
                    outlines: true,
                },
                served,
            )
        };
        assert!(r(all).random_pool().contains(&SceneStyle::Thermo));
        assert!(!r(no_tex).random_pool().contains(&SceneStyle::Thermo));
        assert!(!r(no_ol).random_pool().contains(&SceneStyle::Outline));
        assert_eq!(
            r(no_uni).random_pool(),
            vec![SceneStyle::Lit, SceneStyle::Cel]
        );
    }

    #[test]
    fn hull_plans() {
        let all = Served {
            variants: true,
            outlines: true,
            universe: true,
            style_textures: true,
        };
        let plan = |style, outlines| {
            let p = hull_plan(&effective_for(Requested { style, outlines }, all), false);
            (p.stage, p.dancers, p.is_none())
        };
        // Outlines on: shaded stage + dancers (hidden per look at runtime).
        assert_eq!(plan(SceneStyle::Cel, true), (true, true, false));
        assert_eq!(plan(SceneStyle::Thermo, true), (true, true, false));
        assert_eq!(plan(SceneStyle::Stock, true), (false, false, true));
        // Outlines off: only the looks that ARE the hull build dancer hulls.
        assert_eq!(plan(SceneStyle::Cel, false), (false, false, true));
        assert_eq!(plan(SceneStyle::Outline, false), (false, true, false));
        assert_eq!(plan(SceneStyle::Random, false), (false, true, false));
        // A dancer preview always builds dancer hulls (looks follow the menu).
        let e = effective_for(
            Requested {
                style: SceneStyle::Stock,
                outlines: false,
            },
            all,
        );
        let p = hull_plan(&e, true);
        assert!(p.dancers && !p.stage);
    }
}
