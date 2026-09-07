//! Overlay rows (design §4.4 / R9): RESOLUTION (the output size, from the
//! operator's `presets`), RENDER SCALE (`= OUTPUT` / `75%` / `50%` /
//! `1280x720`), MSAA (OFF / 2X / 4X / STOCK) and SD PRESENT MODE (CROP /
//! LETTERBOX — 4:3 outputs only). All persist the WHOLE `resolution` section through
//! `config::save_json_key` (DLL-owned section — every key must round-trip)
//! and apply at the next launch; the toast/hint says so. The `fps_unlock`
//! enum-row shape.

use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;

use crate::log_info;
use crate::mods::config::{self, ResolutionConfig};
use crate::mods::mod_menu;

use super::plan::{classify_aspect, parse_dims, Aspect};
use super::MOD_ID;

pub const ROW_OUTPUT: &str = "custom-resolution-output";
pub const ROW_RENDER: &str = "custom-resolution-render";
pub const ROW_MSAA: &str = "custom-resolution-msaa";
pub const ROW_SD_PRESENT: &str = "custom-resolution-sd-present";

/// The MSAA choices: `(config string, label)`. `"auto"` (the pre-2026-09-07
/// default) is normalized to `"off"` on load.
const MSAA_CHOICES: [(&str, &str); 4] = [
    ("off", "OFF"),
    ("2x", "2X"),
    ("4x", "4X"),
    ("stock", "STOCK (GAME DECIDES)"),
];

/// The SD present-mode choices: `(config string, label)`.
const SD_PRESENT_CHOICES: [(&str, &str); 2] = [
    ("crop", "CROP (960 PX CENTRE)"),
    ("letterbox", "LETTERBOX (FULL HUD)"),
];

/// The RENDER SCALE choices: `(config string, label)`.
const RENDER_CHOICES: [(&str, &str); 4] = [
    ("output", "= OUTPUT"),
    ("75%", "75%"),
    ("50%", "50%"),
    ("1280x720", "1280x720"),
];

struct RowState {
    cfg: ResolutionConfig,
    /// Output choices parallel to the RESOLUTION row's values (0..n).
    outputs: Vec<String>,
    /// Render choices parallel to the RENDER SCALE row's values (0..n).
    renders: Vec<String>,
}

static STATE: Lazy<Mutex<RowState>> = Lazy::new(|| {
    Mutex::new(RowState {
        cfg: ResolutionConfig::default(),
        outputs: Vec::new(),
        renders: Vec::new(),
    })
});

fn label_for_output(s: &str) -> String {
    match parse_dims(s).and_then(classify_aspect) {
        Some(Aspect::Sd4x3) => format!("{s} (SD 4:3)"),
        Some(Aspect::Wide16x9) => s.to_string(),
        None => format!("{s} (unsupported)"),
    }
}

/// Load the section, normalize the choice lists (valid `WxH` presets, the
/// current value appended when unlisted), and seed `STATE`.
fn load() {
    let cfg = config::get()
        .and_then(|c| c.resolution.clone())
        .unwrap_or_default();
    let mut outputs: Vec<String> = cfg
        .presets
        .iter()
        .filter(|p| parse_dims(p).is_some())
        .map(|p| p.trim().to_string())
        .collect();
    outputs.dedup();
    if !outputs
        .iter()
        .any(|o| o.eq_ignore_ascii_case(cfg.output.trim()))
    {
        outputs.push(cfg.output.trim().to_string());
    }
    let mut renders: Vec<String> = RENDER_CHOICES.iter().map(|(v, _)| v.to_string()).collect();
    if !renders
        .iter()
        .any(|r| r.eq_ignore_ascii_case(cfg.render.trim()))
    {
        renders.push(cfg.render.trim().to_string());
    }
    let mut cfg = cfg;
    if cfg.msaa.trim().eq_ignore_ascii_case("auto") {
        cfg.msaa = "off".to_string();
    }
    if let Ok(mut st) = STATE.lock() {
        st.cfg = cfg;
        st.outputs = outputs;
        st.renders = renders;
    }
}

/// Index of `value` in a `(config, label)` table (case-insensitive), 0 if absent.
fn choice_index(table: &[(&str, &str)], value: &str) -> i32 {
    table
        .iter()
        .position(|(v, _)| v.eq_ignore_ascii_case(value.trim()))
        .unwrap_or(0) as i32
}

fn set_msaa(index: i32) {
    if let Ok(mut st) = STATE.lock() {
        if let Some((v, _)) = MSAA_CHOICES.get(index.max(0) as usize) {
            st.cfg.msaa = v.to_string();
        }
    }
    persist();
    log_info!("CustomResolution: msaa set via overlay (applies on next launch)");
}

fn set_sd_present(index: i32) {
    if let Ok(mut st) = STATE.lock() {
        if let Some((v, _)) = SD_PRESENT_CHOICES.get(index.max(0) as usize) {
            st.cfg.sd_present = v.to_string();
        }
    }
    persist();
    log_info!("CustomResolution: sd_present set via overlay (applies on next launch)");
}

fn persist() {
    let cfg = match STATE.lock() {
        Ok(st) => st.cfg.clone(),
        Err(_) => return,
    };
    config::save_json_key(
        "resolution",
        serde_json::json!({
            "output": cfg.output,
            "render": cfg.render,
            "presets": cfg.presets,
            "sd_present": cfg.sd_present,
            "msaa": cfg.msaa,
            "test_menu_scale": cfg.test_menu_scale,
        }),
    );
}

fn set_output(index: i32) {
    if let Ok(mut st) = STATE.lock() {
        if let Some(v) = st.outputs.get(index.max(0) as usize).cloned() {
            st.cfg.output = v;
        }
    }
    persist();
    log_info!("CustomResolution: output set via overlay (applies on next launch)");
}

fn set_render(index: i32) {
    if let Ok(mut st) = STATE.lock() {
        if let Some(v) = st.renders.get(index.max(0) as usize).cloned() {
            st.cfg.render = v;
        }
    }
    persist();
    log_info!("CustomResolution: render scale set via overlay (applies on next launch)");
}

/// Register both rows under the mod's toggle. Idempotent (re-registration
/// replaces the rows).
pub fn register() {
    load();
    let (outputs, renders, cur_out, cur_render, cur_msaa, cur_sd) = match STATE.lock() {
        Ok(st) => (
            st.outputs.clone(),
            st.renders.clone(),
            st.cfg.output.trim().to_string(),
            st.cfg.render.trim().to_string(),
            st.cfg.msaa.clone(),
            st.cfg.sd_present.clone(),
        ),
        Err(_) => return,
    };

    let out_values: Vec<i32> = (0..outputs.len() as i32).collect();
    let out_labels: Vec<String> = outputs.iter().map(|o| label_for_output(o)).collect();
    let out_initial = outputs
        .iter()
        .position(|o| o.eq_ignore_ascii_case(&cur_out))
        .unwrap_or(0) as i32;
    mod_menu::register_enum_row(mod_menu::EnumRowSpec {
        key: ROW_OUTPUT.to_string(),
        label: "Resolution".to_string(),
        hint: "Back-buffer / window size. 4:3 sizes use the SD-cabinet present path. Restart the game to apply."
            .to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: out_values,
        labels: out_labels,
        initial_value: out_initial,
        on_change: Arc::new(set_output),
    });

    let ren_values: Vec<i32> = (0..renders.len() as i32).collect();
    let ren_labels: Vec<String> = renders
        .iter()
        .map(|r| {
            RENDER_CHOICES
                .iter()
                .find(|(v, _)| v.eq_ignore_ascii_case(r))
                .map(|(_, l)| l.to_string())
                .unwrap_or_else(|| r.clone())
        })
        .collect();
    let ren_initial = renders
        .iter()
        .position(|r| r.eq_ignore_ascii_case(&cur_render))
        .unwrap_or(0) as i32;
    mod_menu::register_enum_row(mod_menu::EnumRowSpec {
        key: ROW_RENDER.to_string(),
        label: "Render Scale".to_string(),
        hint: "Internal render size (ignored for 4:3 outputs, which render at 1280x720). Restart the game to apply."
            .to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: ren_values,
        labels: ren_labels,
        initial_value: ren_initial,
        on_change: Arc::new(set_render),
    });

    mod_menu::register_enum_row(mod_menu::EnumRowSpec {
        key: ROW_MSAA.to_string(),
        label: "MSAA".to_string(),
        hint: "Multisampling on the game's render surfaces. Restart the game to apply.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: (0..MSAA_CHOICES.len() as i32).collect(),
        labels: MSAA_CHOICES.iter().map(|(_, l)| l.to_string()).collect(),
        initial_value: choice_index(&MSAA_CHOICES, &cur_msaa),
        on_change: Arc::new(set_msaa),
    });

    mod_menu::register_enum_row(mod_menu::EnumRowSpec {
        key: ROW_SD_PRESENT.to_string(),
        label: "SD Present Mode".to_string(),
        hint: "4:3 outputs only: CROP shows the stock SD cabinet's 960-px centre cut of the 1280x720 picture; LETTERBOX fits the whole picture with black bars. Restart the game to apply."
            .to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: (0..SD_PRESENT_CHOICES.len() as i32).collect(),
        labels: SD_PRESENT_CHOICES.iter().map(|(_, l)| l.to_string()).collect(),
        initial_value: choice_index(&SD_PRESENT_CHOICES, &cur_sd),
        on_change: Arc::new(set_sd_present),
    });
    log_info!("CustomResolution: registered overlay rows");
}

pub fn unregister() {
    mod_menu::remove_rows_for(&[ROW_OUTPUT, ROW_RENDER, ROW_MSAA, ROW_SD_PRESENT]);
}
