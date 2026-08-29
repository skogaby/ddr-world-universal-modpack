//! Resolved runtime settings for the `smx-hardware` mod (defaults applied
//! over the operator's `smx_hardware` config section) + the section
//! persist helper for the mod-menu-editable knobs.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use crate::mods::config as mod_config;

/// Effective settings, read once at mod enable (next-launch semantics for
/// the fixed fields; opacity/scale/light toggles are also live-editable
/// from the mod menu's SMX HARDWARE section, which persists back).
#[derive(Clone, Debug)]
pub struct SmxSettings {
    pub p1card: Option<String>,
    pub p2card: Option<String>,
    pub overlay_opacity: f32,
    /// Overlay scale percent (50..=150; 100 = authored layout).
    pub overlay_scale: i32,
    /// Static pad accent is Platinum (silver/chrome) instead of Gold.
    pub pad_platinum: bool,
    pub overlay_enabled: bool,
    pub output_lights: bool,
    pub output_cabinet_lights: bool,
    pub force_gold_cabinet: bool,
    /// Touch-overlay release debounce ms (IR-frame flutter absorber;
    /// 0 = off).
    pub touch_debounce_ms: u32,
}

impl Default for SmxSettings {
    fn default() -> Self {
        Self {
            p1card: None,
            p2card: None,
            overlay_opacity: 0.6,
            overlay_scale: 100,
            pad_platinum: false,
            overlay_enabled: true,
            output_lights: true,
            output_cabinet_lights: true,
            force_gold_cabinet: true,
            touch_debounce_ms: 150,
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
        overlay_scale: section
            .overlay_scale
            .unwrap_or(defaults.overlay_scale)
            .clamp(50, 150),
        pad_platinum: match section.pad_style.as_deref() {
            None | Some("gold") => false,
            Some("platinum") => true,
            Some(other) => {
                crate::log_warn!("SmxHardware: unknown pad_style {:?} -- using gold", other);
                false
            }
        },
        overlay_enabled: section.overlay_enabled.unwrap_or(defaults.overlay_enabled),
        output_lights: section.output_lights.unwrap_or(defaults.output_lights),
        output_cabinet_lights: section
            .output_cabinet_lights
            .unwrap_or(defaults.output_cabinet_lights),
        force_gold_cabinet: section
            .force_gold_cabinet
            .unwrap_or(defaults.force_gold_cabinet),
        touch_debounce_ms: section
            .touch_debounce_ms
            .unwrap_or(defaults.touch_debounce_ms)
            .clamp(0, 1000),
    }
}

// ── Section persistence (mod-menu rows) ──────────────────────────────
//
// `save_json_key` replaces the whole top-level section, so every field is
// serialized on each change (the quick_restart/overlay_menu pattern). The
// operator-authored fields (cards, gold force, overlay_enabled) are
// carried from the enable-time snapshot; the live knobs come from their
// runtime state.

/// Enable-time snapshot for the persist carry-through.
static SNAPSHOT: Mutex<Option<SmxSettings>> = Mutex::new(None);
/// Live mirrors of the light toggles (the persist reads them; transport
/// owns the effective gates).
static LIGHTS_STAGE: AtomicBool = AtomicBool::new(true);
static LIGHTS_CABINET: AtomicBool = AtomicBool::new(true);
/// Live mirror of the pad accent style (true = Platinum).
static PAD_PLATINUM: AtomicBool = AtomicBool::new(false);
/// Live mirror of the touch release-debounce window (ms).
static TOUCH_DEBOUNCE: AtomicU32 = AtomicU32::new(150);

/// Latch the enable-time settings (call from the mod's enable, after
/// `load()`).
pub fn latch(settings: &SmxSettings) {
    LIGHTS_STAGE.store(settings.output_lights, Ordering::Relaxed);
    LIGHTS_CABINET.store(settings.output_cabinet_lights, Ordering::Relaxed);
    PAD_PLATINUM.store(settings.pad_platinum, Ordering::Relaxed);
    TOUCH_DEBOUNCE.store(settings.touch_debounce_ms, Ordering::Relaxed);
    if let Ok(mut s) = SNAPSHOT.lock() {
        *s = Some(settings.clone());
    }
}

/// Live light-toggle mirrors (mod-menu rows write these alongside the
/// transport gates so the persist snapshot stays truthful).
pub fn set_lights_stage(on: bool) {
    LIGHTS_STAGE.store(on, Ordering::Relaxed);
}

pub fn set_lights_cabinet(on: bool) {
    LIGHTS_CABINET.store(on, Ordering::Relaxed);
}

/// Live pad-accent mirror (the mod-menu "Pad Style" row).
pub fn set_pad_platinum(platinum: bool) {
    PAD_PLATINUM.store(platinum, Ordering::Relaxed);
}

/// Live touch-debounce mirror (the mod-menu "Touch Debounce" row).
pub fn set_touch_debounce(ms: u32) {
    TOUCH_DEBOUNCE.store(ms, Ordering::Relaxed);
}

/// Persist the whole `smx_hardware` section from the live state (mod-menu
/// row edits). File write only — safe off the render thread.
pub fn persist() {
    let base = SNAPSHOT
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_default();
    let opacity = super::overlay::opacity_percent() as f32 / 100.0;
    mod_config::save_json_key(
        "smx_hardware",
        serde_json::json!({
            "p1card": base.p1card.clone().unwrap_or_default(),
            "p2card": base.p2card.clone().unwrap_or_default(),
            "overlay_enabled": base.overlay_enabled,
            "overlay_opacity": (opacity * 100.0).round() / 100.0,
            "overlay_scale": super::overlay::scale_percent(),
            "pad_style": if PAD_PLATINUM.load(Ordering::Relaxed) { "platinum" } else { "gold" },
            "output_lights": LIGHTS_STAGE.load(Ordering::Relaxed),
            "output_cabinet_lights": LIGHTS_CABINET.load(Ordering::Relaxed),
            "force_gold_cabinet": base.force_gold_cabinet,
            "touch_debounce_ms": TOUCH_DEBOUNCE.load(Ordering::Relaxed),
        }),
    );
}
