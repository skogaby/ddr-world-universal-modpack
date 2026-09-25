//! Shader Fixes — the operator surface of the runtime shader-container
//! synthesis. The synthesis itself lives in
//! `services/avs_layeredfs/shader_synthesis.rs` (container layout in
//! `shader_layout.rs`); this mod only owns the switch and one config key.
//!
//! The synthesis runs lazily inside the LayeredFS arc handler when the game
//! opens `data/arc/shader.arc` — exactly once per session, during boot,
//! regardless of mod-enable timing — and reads config directly:
//!
//! - `mods["shader-fixes"]` — master switch. Disabled ⇒ NOTHING is
//!   synthesized (no anti-aliasing, no perspective programs, no mod-menu
//!   theme programs, no 3D-scene style variants) ⇒ the game runs literal
//!   stock shader bytecode. Player Perspective's runtime ≥2-programs gate
//!   then degrades hallway cleanly; the background dancers render unlit.
//! - `shader_fixes.anti_aliasing` — the cabinet-wide ARROW ANTI-ALIASING
//!   toggle (default ON): program 0 of the arrow/judge containers uses the
//!   index-aware anti-aliasing pixel shaders, smoothing scaled lane art
//!   (Playfield/Overlay Styling). At 1:1 the AA output is identical to
//!   stock. Perspective programs carry the AA PS regardless (a hallway lane
//!   is always being scaled — exactly the case AA exists for).
//! - The 3D scene's shading is NOT this mod's knob: it is Background
//!   Dancers' "Lighting Style" / "Scene Outlines" rows
//!   (`background_dancers.style` / `.outlines`, per song —
//!   `background_dancers/style.rs`). The synthesis packs every
//!   `<material>_lit` / `<material>_cel` variant container whenever
//!   `shader-fixes` and `background-dancers` are both on; the legacy
//!   `shader_fixes.dancer_lighting` / `dancer_outlines` / `lit_models` keys
//!   are still parsed, read by the dancers mod for migration only.
//!
//! This mod's own job: one mod-overlay enum row (`Arrow Anti-Aliasing`
//! OFF/ON) whose edits rewrite the DLL-written `shader_fixes` section via
//! `config::save_json_key`. The write emits only `anti_aliasing`, so the
//! legacy scene keys are dropped on the first write (their migrated values
//! live on in `background_dancers`). Changes apply on the NEXT LAUNCH
//! (boot-time synthesis). `is_active()` reflects only the toggle; the enable
//! line logs what the synthesis actually served. Operator kill switch:
//! `layeredfs.blocklist: ["shader_fixes"]` (no blobs found ⇒ no synthesis).
//! Blob sources: `shaders/src/*.hlsl` → `data_mods/shader_fixes/blobs/`
//! (`scripts/build_shaders.sh`). RE: `docs/shader_replacement_research.md`.

use std::sync::Arc;

use crate::log_info;
use crate::mods::config;
use crate::mods::mod_trait::{Mod, ModContext};

const MOD_ID: &str = "shader-fixes";
const ROW_KEY_AA: &str = "shader-fixes-aa";

fn set_anti_aliasing(value: i32) {
    let on = value != 0;
    config::save_json_key("shader_fixes", serde_json::json!({ "anti_aliasing": on }));
    log_info!(
        "ShaderFixes: ARROW ANTI-ALIASING set to {} (applies on next launch)",
        if on { "ON" } else { "OFF" }
    );
}

fn register_overlay_row(aa: bool) {
    use crate::mods::mod_menu::{register_enum_row, EnumRowSpec};
    register_enum_row(EnumRowSpec {
        key: ROW_KEY_AA.to_string(),
        label: "Arrow Anti-Aliasing".to_string(),
        hint: "Smooths scaled lane art. Restart the game to apply.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: vec![0, 1],
        labels: vec!["OFF".to_string(), "ON".to_string()],
        initial_value: i32::from(aa),
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
        "Anti-aliased + perspective-capable lane shaders, 3D-scene style variants (synthesized at boot)"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        true
    }

    fn enable(&mut self) {
        let aa = config::get()
            .and_then(|c| c.shader_fixes.as_ref().map(|s| s.anti_aliasing))
            .unwrap_or(true);
        register_overlay_row(aa);
        self.active = true;
        // Report what synthesis ACTUALLY did at the shader.arc open (the
        // previous unconditional "synthesis ran at boot arc-open" wording
        // masked a boot where the open was never intercepted at all).
        use crate::services::avs_layeredfs::shader_synthesis::{
            outline_programs_available, status, variants_available, SynthStatus,
        };
        let synth = match status() {
            SynthStatus::Synthesized => "synthesized containers served",
            SynthStatus::Stock => "shader.arc intercepted, stock served (see shader_synthesis log)",
            SynthStatus::NotSeen => "shader.arc not opened yet (synthesis pending)",
        };
        log_info!(
            "ShaderFixes: enabled (anti_aliasing={}; scene variants {}, outline programs {}; synthesis: {})",
            aa,
            if variants_available() { "served" } else { "not served" },
            if outline_programs_available() { "served" } else { "not served" },
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
