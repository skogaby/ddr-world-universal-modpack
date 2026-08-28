//! Resolved runtime settings for the `smx-hardware` mod (defaults applied
//! over the operator's `smx_hardware` config section).

use crate::mods::config as mod_config;

/// Effective Step 1 settings, read once at mod enable (next-launch
/// semantics). Step 3 consumes the card/overlay fields.
#[derive(Clone, Debug)]
pub struct SmxSettings {
    pub p1card: Option<String>,
    pub p2card: Option<String>,
    pub overlay_opacity: f32,
    pub overlay_enabled: bool,
    pub output_lights: bool,
    pub output_cabinet_lights: bool,
    pub force_gold_cabinet: bool,
}

impl Default for SmxSettings {
    fn default() -> Self {
        Self {
            p1card: None,
            p2card: None,
            overlay_opacity: 0.6,
            overlay_enabled: true,
            output_lights: true,
            output_cabinet_lights: true,
            force_gold_cabinet: true,
        }
    }
}

/// Load the operator's `smx_hardware` section with defaults for absent keys.
pub fn load() -> SmxSettings {
    let defaults = SmxSettings::default();
    let Some(section) = mod_config::get().and_then(|c| c.smx_hardware.as_ref()) else {
        return defaults;
    };
    SmxSettings {
        p1card: section.p1card.clone().filter(|s| !s.is_empty()),
        p2card: section.p2card.clone().filter(|s| !s.is_empty()),
        overlay_opacity: section
            .overlay_opacity
            .unwrap_or(defaults.overlay_opacity)
            .clamp(0.0, 1.0),
        overlay_enabled: section.overlay_enabled.unwrap_or(defaults.overlay_enabled),
        output_lights: section.output_lights.unwrap_or(defaults.output_lights),
        output_cabinet_lights: section
            .output_cabinet_lights
            .unwrap_or(defaults.output_cabinet_lights),
        force_gold_cabinet: section
            .force_gold_cabinet
            .unwrap_or(defaults.force_gold_cabinet),
    }
}
