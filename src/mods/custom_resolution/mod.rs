//! Custom Resolution — run DDR World at resolutions other than its fixed
//! 1280×720: native 16:9 rendering (1080p / 1440p / 4K / any 16:9 size) and
//! the 4:3 SD-cabinet output World officially dropped (640×480 through the
//! engine's own crop/letterbox present path).
//!
//! Planning record: `.agents/planning/2026-09-05-arbitrary-resolution/`
//! (design → `design/detailed-design.md`). Everything physical the engine
//! hard-codes is a boot-time immediate the game reads once, so the mod is
//! `early_apply`-style byte patching (the `fps_unlock` precedent) plus a
//! small number of detours for fixups that need runtime values. All settings
//! apply at the NEXT launch — the D3D device is created once at boot.
//!
//! Module map (filled in by the plan's steps):
//! - [`plan`] — pure config → boot-plan model, present-mode policy, scissor
//!   scaling (host-tested via `scripts/validate_custom_resolution.sh`).
//! - [`sites`] — pure immediate-site finders over byte windows (host-tested).
//! - [`patches`] — the OUTPUT (and, from plan Step 6, RENDER) imm32 sets,
//!   stock-verified, applied atomically with rollback.
//! - [`present`] — the `graphics_init` detour: PRESENT rt dims → output,
//!   window-client fit (spice2x `-w` pins the client size).
//! - [`letterbox`] — the present-mode policy detour (SD letterbox option,
//!   forced letterbox for 16:9 render ≠ output).
//! - [`logical_screen`] — the app layer's view of the screen: the ~20 game
//!   sites that size/position content "on the screen" (layer set-size loop,
//!   footer/version/attract text, TEST-menu drawers, system font) read a
//!   constant 1280×720 (the design space they were authored against), the
//!   AFP callbacks read the RENDER size, the renderer/device layer keeps the
//!   real back-buffer. Every screen-sized layer root thereby becomes a
//!   1280×720 canvas that the walker scales to the physical viewport —
//!   game text, loading art and the DLL's widgets all land right at any
//!   output (cabinet-derived 2026-09-05 after two narrower attempts).
//! - [`rows`] — the RESOLUTION / RENDER SCALE overlay rows.
//! - [`display_modes`] — the fullscreen fail-safe (`EnumDisplaySettingsW`).
//!
//! Boot flow (`early_apply`, before `Application::onBoot` reaches display
//! init): config → `plan::compute` → too-late check (screen globals still 0)
//! → display-mode validation → OUTPUT set → graphics_init detour. Any
//! failure leaves the game byte-identical to stock with one WARN.

pub mod display_modes;
pub mod letterbox;
pub mod logical_screen;
pub mod patches;
pub mod plan;
pub mod present;
pub mod rows;
pub mod sites;

use crate::core::memory;
use crate::mods::config;
use crate::mods::mod_trait::{EarlyContext, Mod, ModContext};
use crate::{log_info, log_warn};

use plan::{Aspect, Outcome, Plan, PlanInput, PresentPolicy, SdPresent};

/// Registry mod id.
pub const MOD_ID: &str = "custom-resolution";

pub struct CustomResolutionMod {
    /// The plan that was applied this boot (None = inert / rejected / failed).
    plan: Option<Plan>,
    /// The applied OUTPUT set (kept for a possible rollback on a later
    /// install failure inside `early_apply`; boot-scoped afterwards).
    output_set: Option<patches::PatchSet>,
    /// True once every piece of the plan landed.
    applied: bool,
    /// True once the overlay rows are registered (so `disable` removes them).
    rows_registered: bool,
}

// Raw pointers inside `PatchSet` point into the game image (process lifetime).
unsafe impl Send for CustomResolutionMod {}

impl Default for CustomResolutionMod {
    fn default() -> Self {
        Self::new()
    }
}

impl CustomResolutionMod {
    pub fn new() -> Self {
        Self {
            plan: None,
            output_set: None,
            applied: false,
            rows_registered: false,
        }
    }

    fn load_plan() -> Outcome {
        let cfg = config::get()
            .and_then(|c| c.resolution.clone())
            .unwrap_or_default();
        plan::compute(&PlanInput {
            output: &cfg.output,
            render: &cfg.render,
            sd_present: &cfg.sd_present,
            msaa: &cfg.msaa,
        })
    }

    /// FPS Unlock's selected target when that mod is enabled (feeds the
    /// display-mode check's refresh-rate match).
    fn fps_hint() -> Option<u32> {
        let cfg = config::get()?;
        if !cfg.mods.get("fps-unlock").copied().unwrap_or(true) {
            return None;
        }
        cfg.fps_unlock.as_ref().map(|f| f.selected.max(1) as u32)
    }

    fn describe(plan: &Plan) -> String {
        let policy = match plan.present_policy {
            PresentPolicy::Stock => "stock (1:1)".to_string(),
            PresentPolicy::ForceLetterbox => "letterbox".to_string(),
            PresentPolicy::Sd(SdPresent::Crop) => "SD crop (960-px centre)".to_string(),
            PresentPolicy::Sd(SdPresent::Letterbox) => "SD letterbox".to_string(),
        };
        format!(
            "output {}x{} ({}), render {}x{}, present {}, aa {}",
            plan.output.w,
            plan.output.h,
            match plan.aspect {
                Aspect::Wide16x9 => "16:9",
                Aspect::Sd4x3 => "4:3",
            },
            plan.render.w,
            plan.render.h,
            policy,
            if plan.force_aa_zero {
                "forced 0"
            } else {
                "stock"
            }
        )
    }
}

impl Mod for CustomResolutionMod {
    fn id(&self) -> &str {
        MOD_ID
    }
    fn name(&self) -> &str {
        "Custom Resolution"
    }
    fn description(&self) -> &str {
        "Render at 1080p/1440p/4K or the 4:3 SD-cabinet size (applies next launch)"
    }
    fn required_signatures(&self) -> &[&str] {
        // Best-effort: every piece resolves on its own and the mod reports
        // the truth through `is_active`.
        &[]
    }

    fn early_apply(&mut self, ctx: &EarlyContext) -> bool {
        let plan = match Self::load_plan() {
            Outcome::Inert => {
                log_info!("CustomResolution: stock 1280x720 configured -- inert");
                return true;
            }
            Outcome::Rejected(why) => {
                log_warn!("CustomResolution: {why} -- staying at stock 1280x720");
                return true;
            }
            Outcome::Plan(p) => p,
        };
        log_info!("CustomResolution: plan = {}", Self::describe(&plan));
        if plan.coerced_render {
            log_info!(
                "CustomResolution: 4:3 output always renders at 1280x720 (render setting ignored)"
            );
        }

        let anchors = ctx.signatures.custom_resolution_anchors();

        // Too-late check: the screen globals are BSS zeros until display init
        // runs. Non-zero here means onBoot beat us — patching now would only
        // take effect next launch and could desynchronise the surfaces.
        if let Some(sw) = anchors.screen_w_global {
            if memory::is_readable(sw, 4) && unsafe { memory::read_u32(sw) } != 0 {
                log_warn!("CustomResolution: display init already ran (screen_w != 0) -- nothing applied this boot");
                return true;
            }
        }

        if let Err(why) = display_modes::validate(plan.output, Self::fps_hint()) {
            log_warn!("CustomResolution: {why} -- staying at stock 1280x720");
            return true;
        }
        if display_modes::spice_windowed() {
            log_info!("CustomResolution: spice2x windowed mode (-w) -- display-mode check skipped");
        }

        // OUTPUT set (back-buffer + AA) then the PRESENT fixup detour. Both or
        // neither: a bigger back-buffer with a 1280×720 PRESENT viewport paints
        // the top-left corner only.
        let mut set = match patches::apply_output_set(ctx.signatures, &anchors, &plan) {
            Ok(s) => s,
            Err(why) => {
                log_warn!("CustomResolution: OUTPUT set not applied ({why}) -- staying at stock");
                return true;
            }
        };
        if let Err(why) = present::install(&anchors, &plan) {
            log_warn!("CustomResolution: {why} -- rolling back the OUTPUT set, staying at stock");
            set.rollback();
            return true;
        }
        // Present-mode policy (no-op install for Stock / SD crop). A 16:9
        // render ≠ output MUST have it — without the remap the game's
        // per-scene mode-1 selection would crop the picture.
        if let Err(why) = letterbox::install(ctx.signatures, plan.present_policy) {
            if plan.present_policy == PresentPolicy::ForceLetterbox {
                log_warn!(
                    "CustomResolution: {why} -- rolling back the OUTPUT set, staying at stock"
                );
                set.rollback();
                return true;
            }
            log_warn!(
                "CustomResolution: {why} -- SD letterbox unavailable, the stock crop applies"
            );
        }
        logical_screen::install(
            ctx.signatures,
            ctx.game_module.base,
            ctx.game_module.size,
            plan.render,
        );

        self.output_set = Some(set);
        self.plan = Some(plan);
        self.applied = true;
        log_info!("CustomResolution: early_apply complete -- effective this boot");
        true
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        // All engine work is boot-time; `init` only owns the overlay rows.
        true
    }

    fn enable(&mut self) {
        rows::register();
        self.rows_registered = true;
        if !self.applied {
            log_info!("CustomResolution: enabled -- settings apply at the next launch");
        }
    }

    fn disable(&mut self) {
        // Boot-scoped patches cannot be undone mid-session (the device and
        // surfaces already exist); disabling takes effect at the next launch.
        if self.rows_registered {
            rows::unregister();
            self.rows_registered = false;
        }
        log_info!("CustomResolution: disabled -- stock resolution at the next launch");
    }

    fn is_active(&self) -> bool {
        self.applied
    }
}
