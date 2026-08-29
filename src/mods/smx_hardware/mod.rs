//! SMX Hardware — native StepManiaX Dedicated Cabinet support.
//!
//! Talks directly to the SMX cabinet from inside the game process over raw
//! HID (the `services::smx` transport), replacing the external SpiceManiaX
//! app + SpiceAPI loopback. Step 1 scope (see
//! `.agents/planning/2026-08-27-native-smx-hardware-support/`):
//!
//! - **Input**: SMX stage panel sensors → the ark IO singleton's vtable
//!   panel getters (additive injection through `input_manager`'s seam,
//!   installed lazily once the game's IO layer is live — covers gameplay,
//!   the ark's panel counters, and the test menu; cabinet inputs keep
//!   working alongside).
//! - **Stage lights**: DDR's Gold-Cab light output (`arkMDXChangeTapeled`
//!   per-LED tape + `arkMDXChangeDimlamp` stage corners) → the SMX pads'
//!   9-panel LED grids at ~30 Hz, with SpiceManiaX's exact mapping.
//! - **Cabinet lights** (Step 2): the same captured frame's top-panel /
//!   monitor tape strips + woofer-corner dimlamps → the SMX Dedicated
//!   Cabinet's marquee, vertical strips, and spotlights (the separate
//!   "SMXArcade" HID controller, `L`/`Q` wire commands).
//!
//! - **Touch overlay** (Step 3): per-player menu-nav / pinpad /
//!   insert-card / visibility buttons rendered natively (overlay-draw
//!   quads + label TextWidgets, working in exclusive fullscreen), fed by
//!   a game-window WndProc subclass (WM_TOUCH / WM_POINTER / mouse).
//!   Presses inject through the ark IO vtable seams: menu buttons +
//!   card scans via the IO-dispatcher detour's object-byte writes,
//!   pinpad via the 10-key impl detour (see `input_manager`).
//!
//! ## Lifecycle
//!
//! Default **OFF** (hardware-specific — see `DEFAULT_OFF_MODS`). `enable()`
//! installs the light-capture detours, starts the transport thread, and
//! activates input injection; `disable()` deactivates injection, gates off
//! capture, and stops the transport (detours stay installed per the repo's
//! one-detour rule — their bodies become passthrough). `is_active()`
//! reports false if the load-bearing pieces failed so the mod menu never
//! shows a false ON (NFR-4: no connected cabinet is NOT a failure — the
//! transport hot-plugs when one appears).

pub mod cabinet_force;
pub mod config;
pub mod input_inject;
pub mod lights_read;
pub mod overlay;
pub mod overlay_atlas;
pub mod overlay_model;
pub mod touch;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::input_manager;
use crate::services::smx::transport;
use crate::{log_info, log_warn};

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The registered on_frame callback id (usize::MAX = none).
static FRAME_CB_ID: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Row keys of the mod-menu SMX HARDWARE section (removed on disable).
const MENU_ROW_KEYS: [&str; 6] = [
    "smx_overlay_opacity",
    "smx_overlay_scale",
    "smx_pad_lights",
    "smx_cabinet_lights",
    "smx_pad_style",
    "smx_touch_debounce",
];

/// Register the SMX HARDWARE section rows on the mod menu's GLOBAL
/// SETTINGS tab (grouped under this mod's header while it's enabled):
/// overlay opacity + scale (live-applied to render AND hit-testing) and
/// the two independent light-output toggles. Every change persists the
/// whole `smx_hardware` config section (quick_restart pattern).
fn register_menu_rows(settings: &config::SmxSettings) {
    use crate::mods::mod_menu;

    let on_off = || (vec![0, 1], vec!["OFF".to_string(), "ON".to_string()]);

    mod_menu::register_scalar_row(mod_menu::ScalarRowSpec {
        key: MENU_ROW_KEYS[0].to_string(),
        label: "Touch Overlay Opacity".to_string(),
        hint: "How opaque the touchscreen buttons render.".to_string(),
        parent_row_key: Some("smx-hardware".to_string()),
        min: 10,
        max: 100,
        step_fine: 5,
        step_coarse: 25,
        initial: (settings.overlay_opacity * 100.0).round() as i32,
        on_change: Arc::new(|v| {
            overlay::set_opacity_percent(v);
            config::persist();
        }),
    });
    mod_menu::register_scalar_row(mod_menu::ScalarRowSpec {
        key: MENU_ROW_KEYS[1].to_string(),
        label: "Touch Overlay Scale".to_string(),
        hint: "Button size; clusters grow from their screen corners toward center.".to_string(),
        parent_row_key: Some("smx-hardware".to_string()),
        min: 50,
        max: 150,
        step_fine: 5,
        step_coarse: 25,
        initial: settings.overlay_scale,
        on_change: Arc::new(|v| {
            overlay::set_scale_percent(v);
            config::persist();
        }),
    });
    let (values, labels) = on_off();
    mod_menu::register_enum_row(mod_menu::EnumRowSpec {
        key: MENU_ROW_KEYS[2].to_string(),
        label: "Pad Lights".to_string(),
        hint: "Mirror DDR's stage lighting onto the SMX pads.".to_string(),
        parent_row_key: Some("smx-hardware".to_string()),
        values: values.clone(),
        labels: labels.clone(),
        initial_value: settings.output_lights as i32,
        on_change: Arc::new(|v| {
            transport::set_output_lights(v != 0);
            config::set_lights_stage(v != 0);
            config::persist();
        }),
    });
    mod_menu::register_enum_row(mod_menu::EnumRowSpec {
        key: MENU_ROW_KEYS[3].to_string(),
        label: "Cabinet Lights".to_string(),
        hint: "Mirror DDR's cabinet lighting onto the SMX marquee, strips, and spotlights."
            .to_string(),
        parent_row_key: Some("smx-hardware".to_string()),
        values,
        labels,
        initial_value: settings.output_cabinet_lights as i32,
        on_change: Arc::new(|v| {
            transport::set_output_cabinet_lights(v != 0);
            config::set_lights_cabinet(v != 0);
            config::persist();
        }),
    });
    mod_menu::register_enum_row(mod_menu::EnumRowSpec {
        key: MENU_ROW_KEYS[4].to_string(),
        label: "Pad Style".to_string(),
        hint: "Static pad accent color: Gold or Platinum cabinet lighting.".to_string(),
        parent_row_key: Some("smx-hardware".to_string()),
        values: vec![0, 1],
        labels: vec!["GOLD".to_string(), "PLATINUM".to_string()],
        initial_value: settings.pad_platinum as i32,
        on_change: Arc::new(|v| {
            transport::set_pad_platinum(v != 0);
            config::set_pad_platinum(v != 0);
            config::persist();
        }),
    });
    mod_menu::register_scalar_row(mod_menu::ScalarRowSpec {
        key: MENU_ROW_KEYS[5].to_string(),
        label: "Touch Debounce".to_string(),
        hint: "IR-frame flutter absorber (ms): button releases wait this long and a re-press cancels them. 0 = off.".to_string(),
        parent_row_key: Some("smx-hardware".to_string()),
        min: 0,
        max: 1000,
        step_fine: 25,
        step_coarse: 100,
        initial: settings.touch_debounce_ms as i32,
        on_change: Arc::new(|v| {
            let ms = v.clamp(0, 1000) as u32;
            touch::set_debounce_ms(ms);
            config::set_touch_debounce(ms);
            config::persist();
        }),
    });
}

pub struct SmxHardwareMod;

impl SmxHardwareMod {
    pub fn new() -> Self {
        Self
    }
}

impl Mod for SmxHardwareMod {
    fn id(&self) -> &str {
        "smx-hardware"
    }

    fn name(&self) -> &str {
        "SMX Hardware"
    }

    fn description(&self) -> &str {
        "Native StepManiaX cabinet support: pads as stage input, DDR lights on the pads"
    }

    fn required_signatures(&self) -> &[&str] {
        // Everything resolves via GetProcAddress on the ark module at
        // enable-time (loader-agnostic; no gamemdx signatures needed).
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        true
    }

    fn enable(&mut self) {
        let settings = config::load();

        // 0. Force GOLD cabinet mode so gamemdx drives the per-LED arrow
        //    tape (`arkMDXChangeTapeled`) + stage corners
        //    (`arkMDXChangeDimlamp`) instead of the SD satellite path. On
        //    this cabinet the game auto-detects a non-GOLD machine type and
        //    would otherwise emit only `arkMDXChangeSatellite` cabinet-light
        //    data (arrows never lit correctly). Best-effort: a miss degrades
        //    to the game's own cabinet mode with a WARN, it doesn't disable
        //    the mod. Installed first so it's live before gamemdx queries the
        //    machine type on its first light frame.
        if settings.force_gold_cabinet {
            if cabinet_force::install() {
                cabinet_force::set_force_enabled(true);
                // Source lights by polling the ark's GOLD output buffers so
                // the operator test-menu LAMP CHECK (ark-driven, bypasses the
                // arkMDX* exports) reaches the pads alongside gameplay.
                transport::set_poll_ark(true);
            } else {
                log_warn!(
                    "SmxHardware: could not force GOLD cabinet mode -- pad lights may be wrong"
                );
            }
        }

        // 1. Light-output capture (Tapeled + Dimlamp detours; once).
        if !lights_read::install() {
            log_warn!("SmxHardware: lights capture unavailable -- mod inactive");
            return;
        }
        lights_read::set_capture_enabled(true);

        // 2. The SMX transport thread (discovery, input reads, lights drain).
        transport::set_pad_platinum(settings.pad_platinum);
        if !transport::init(settings.output_lights, settings.output_cabinet_lights) {
            lights_read::set_capture_enabled(false);
            log_warn!("SmxHardware: transport failed to start -- mod inactive");
            return;
        }

        // 3. Stage input injection (SMX panels → the ark vtable panel
        //    getters; the detours install lazily at the first poll after
        //    the game's IO singleton goes live) + the touch overlay's
        //    menu/pinpad/card slots.
        let cards = [
            settings
                .p1card
                .as_deref()
                .and_then(overlay_model::parse_card_id),
            settings
                .p2card
                .as_deref()
                .and_then(overlay_model::parse_card_id),
        ];
        if settings.p1card.is_some() && cards[0].is_none() {
            log_warn!("SmxHardware: p1card is not 16 hex chars -- P1 Insert-Card disabled");
        }
        if settings.p2card.is_some() && cards[1].is_none() {
            log_warn!("SmxHardware: p2card is not 16 hex chars -- P2 Insert-Card disabled");
        }
        input_inject::activate(cards);

        // 4. Touchscreen overlay (Step 3): native-rendered buttons whose
        //    presses drive menu/pinpad/card injection. The window
        //    subclass + widget allocation happen lazily from a per-frame
        //    callback once the game window / widget renderer exist.
        if settings.overlay_enabled {
            overlay::activate(
                [cards[0].is_some(), cards[1].is_some()],
                settings.overlay_opacity,
                settings.overlay_scale,
            );
            touch::activate(settings.touch_debounce_ms);
            if FRAME_CB_ID.load(Ordering::Acquire) == usize::MAX {
                let id = input_manager::on_frame(Arc::new(|| {
                    overlay::tick();
                    touch::tick();
                }));
                FRAME_CB_ID.store(id, Ordering::Release);
            }
        }

        // 5. Mod-menu SMX HARDWARE section (GLOBAL SETTINGS): overlay
        //    opacity/scale + the two light toggles, all live-applied and
        //    persisted back to the `smx_hardware` config section.
        config::latch(&settings);
        register_menu_rows(&settings);

        ACTIVE.store(true, Ordering::Release);
        log_info!(
            "SmxHardware: enabled (output_lights={}, cabinet_lights={}, force_gold={}, overlay={}; cabinet hot-plugs at runtime)",
            settings.output_lights,
            settings.output_cabinet_lights,
            settings.force_gold_cabinet,
            settings.overlay_enabled
        );
    }

    fn disable(&mut self) {
        crate::mods::mod_menu::remove_rows_for(&MENU_ROW_KEYS);
        touch::deactivate();
        overlay::deactivate();
        input_inject::deactivate();
        lights_read::set_capture_enabled(false);
        cabinet_force::set_force_enabled(false);
        transport::set_poll_ark(false);
        transport::shutdown();
        ACTIVE.store(false, Ordering::Release);
        log_info!("SmxHardware: disabled (game input/lights back to stock)");
    }

    fn is_active(&self) -> bool {
        ACTIVE.load(Ordering::Acquire)
    }
}
