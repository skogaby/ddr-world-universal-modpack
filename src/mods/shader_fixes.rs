//! Shader Fixes Mod — the user-facing surface of the runtime shader-container
//! synthesis (`services::avs_layeredfs::shader_synthesis`).
//!
//! The synthesis itself runs at arc-open time during boot (it must — the
//! game reads `data/arc/shader.arc` exactly once, regardless of mod-enable
//! timing) and consumes this mod's config directly:
//!
//! - `mods["shader-fixes"]` — master switch. Disabled ⇒ NOTHING is
//!   synthesized (anti-aliasing off AND no perspective shader programs) ⇒
//!   the game runs literal stock shader bytecode. The player-perspective
//!   mod's runtime ≥2-programs gate then degrades hallway cleanly.
//! - `shader_fixes.anti_aliasing` — the cabinet-wide ARROW ANTI-ALIASING
//!   toggle (default ON): program 0 of the arrow/judge containers uses the
//!   index-aware anti-aliasing pixel shaders, smoothing scaled lane art
//!   (Playfield/Overlay Styling). At 1:1 the AA output is identical to
//!   stock. Perspective programs carry the AA PS regardless (a hallway lane
//!   is always being scaled — exactly the case AA exists for).
//!
//! This mod's own job is just the operator surface: one mod-overlay enum
//! row (`ARROW ANTI-ALIASING` OFF/ON) persisted to the `shader_fixes`
//! config section. Changes apply on the NEXT LAUNCH (boot-time synthesis;
//! the fps_unlock precedent). Operator kill switches preserved:
//! `layeredfs.blocklist: ["shader_fixes"]` still works (no blobs found ⇒
//! no synthesis).

use std::sync::Arc;

use crate::log_info;
use crate::mods::config;
use crate::mods::mod_trait::{Mod, ModContext};

const MOD_ID: &str = "shader-fixes";
const ROW_KEY: &str = "shader-fixes-aa";

fn set_anti_aliasing(value: i32) {
    let on = value != 0;
    config::save_json_key("shader_fixes", serde_json::json!({ "anti_aliasing": on }));
    log_info!(
        "ShaderFixes: ARROW ANTI-ALIASING set to {} (applies on next launch)",
        if on { "ON" } else { "OFF" }
    );
}

fn register_overlay_row() {
    let initial = config::get()
        .and_then(|c| c.shader_fixes.as_ref().map(|s| s.anti_aliasing))
        .unwrap_or(true);
    crate::mods::mod_menu::register_enum_row(crate::mods::mod_menu::EnumRowSpec {
        key: ROW_KEY.to_string(),
        label: "Arrow Anti-Aliasing".to_string(),
        hint: "Smooths scaled lane art. Restart the game to apply.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: vec![0, 1],
        labels: vec!["OFF".to_string(), "ON".to_string()],
        initial_value: i32::from(initial),
        on_change: Arc::new(set_anti_aliasing),
    });
}

pub struct ShaderFixesMod {
    active: bool,
}

impl ShaderFixesMod {
    pub fn new() -> Self {
        Self { active: false }
    }
}

impl Mod for ShaderFixesMod {
    fn id(&self) -> &str {
        MOD_ID
    }
    fn name(&self) -> &str {
        "Shader Fixes"
    }
    fn description(&self) -> &str {
        "Anti-aliased + perspective-capable lane shaders (synthesized at boot)"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        true
    }

    fn enable(&mut self) {
        register_overlay_row();
        self.active = true;
        let aa = config::get()
            .and_then(|c| c.shader_fixes.as_ref().map(|s| s.anti_aliasing))
            .unwrap_or(true);
        // Report what synthesis ACTUALLY did at the shader.arc open (the
        // previous unconditional "synthesis ran at boot arc-open" wording
        // masked a boot where the open was never intercepted at all).
        use crate::services::avs_layeredfs::shader_synthesis::{status, SynthStatus};
        let synth = match status() {
            SynthStatus::Synthesized => "synthesized containers served",
            SynthStatus::Stock => "shader.arc intercepted, stock served (see shader_synthesis log)",
            SynthStatus::NotSeen => "shader.arc not opened yet (synthesis pending)",
        };
        log_info!(
            "ShaderFixes: enabled (anti_aliasing={}; synthesis: {})",
            aa,
            synth
        );
    }

    fn disable(&mut self) {
        // The overlay row stays registered until reboot (no unregister API);
        // synthesis consumed the config at boot, so a mid-session disable
        // has no further effect until the next launch.
        self.active = false;
        log_info!("ShaderFixes: disabled (takes full effect on next launch)");
    }

    fn is_active(&self) -> bool {
        self.active
    }
}
