//! Overlay rows (design §4.4 / R9): RESOLUTION (the output size, from the
//! operator's `presets`) and RENDER SCALE (`= OUTPUT` / `75%` / `50%` /
//! `1280x720`). Both persist the WHOLE `resolution` section through
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
    if let Ok(mut st) = STATE.lock() {
        st.cfg = cfg;
        st.outputs = outputs;
        st.renders = renders;
    }
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
    let (outputs, renders, cur_out, cur_render) = match STATE.lock() {
        Ok(st) => (
            st.outputs.clone(),
            st.renders.clone(),
            st.cfg.output.trim().to_string(),
            st.cfg.render.trim().to_string(),
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
    log_info!("CustomResolution: registered overlay rows");
}

pub fn unregister() {
    mod_menu::remove_rows_for(&[ROW_OUTPUT, ROW_RENDER]);
}
