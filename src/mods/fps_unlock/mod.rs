//! FPS Unlock — overrides DDR World's frame-rate target.
//!
//! The game writes a hardcoded `0x3C` (60) into a stack struct during
//! `Application::onBoot()`, which is copied into a global and consumed **once**
//! to configure the Direct3D device — it is never re-read per frame (see
//! `.agents/planning/20260627-fps-unlock/research/r2`). It becomes
//! `D3DPRESENT_PARAMETERS.FullScreen_RefreshRateInHz`: a request for a
//! fullscreen DISPLAY MODE, not a frame cap (the game vsyncs with
//! `D3DPRESENT_INTERVAL_ONE`, so it renders at whatever the display refreshes
//! at). Because the engine is delta-time based, raising this target gives
//! smooth high-refresh gameplay.
//!
//! Two paths, chosen once at boot by `pacing::mode_for`:
//!
//! - **Target ≥ 60 (the original feature, unchanged):** an AOB-resolved
//!   byte-patch of that imm32, written in the `early_apply` boot phase (before
//!   `onBoot` reads it) — the same race-critical pattern `song_limit_expansion`
//!   uses. The stock value is captured first so a disable reverts to it. 60
//!   itself patches nothing.
//! - **Target < 60 (`limiter.rs`):** the imm32 is left at stock — no monitor
//!   offers a 20/30 Hz mode, D3D9 fails `CreateDevice` for a non-enumerated
//!   fullscreen rate (a boot the operator could not recover from in the
//!   menu), and windowed mode ignores the value outright. Instead a
//!   post-original detour on the app tick's `frame_begin` paces the game loop
//!   to the target and raises the game's per-frame dt clamp so dt-driven
//!   animation keeps wall-clock speed. Nothing of this path runs — no detour,
//!   no global write — for a target ≥ 60. RE: `docs/fps_frame_limiter.md`.
//!
//! Cabinet-wide (not per-player), so it is configured via the `fps_unlock`
//! section of `mod-config.json` and adjusted in the DLL mod-overlay (an `Enum`
//! row), not the game's per-player Options screen. Changes take effect on the
//! **next launch** (the refresh value is latched into the D3D device at boot;
//! the limiter is armed from the boot selection for the same semantics).
//!
//! Graceful degradation: the patch site is load-bearing (mod self-disables if
//! the AOB doesn't resolve); the limiter needs both of its anchors or it is
//! not installed (the game then runs at stock pacing — never with a sub-60
//! refresh request); the overlay row is optional (config-file control still
//! works without it).

mod limiter;
pub mod pacing;

use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;

use crate::core::memory;
use crate::mods::config;
use crate::mods::mod_menu;
use crate::mods::mod_trait::{EarlyContext, Mod, ModContext};
use crate::{log_info, log_warn};

use pacing::{default_presets, mode_for, normalize, Mode};

/// Registry mod id (the master toggle row the enum row nests under).
const MOD_ID: &str = "fps-unlock";
/// Enum child-row key.
const ROW_KEY: &str = "fps-target";
/// The patchable imm32 sits at this offset from the AOB match start (the `0x3C`
/// in `C7 44 24 ?? 3C 00 00 00 ...`).
const IMM_OFFSET: usize = 4;
/// Expected stock FPS immediate in the binary (always 60 — the cabinet-selected
/// 75 is a separate runtime branch, not this literal). Validated before patching.
const STOCK_FPS: u32 = pacing::STOCK_FPS as u32;

/// The fullscreen refresh the D3D device is asked for with `selected` — the
/// value a display-mode check must match (custom_resolution's fail-safe).
/// Identical to `selected` for targets ≥ 60; the stock 60 below that.
pub fn requested_refresh_hz(selected: i32) -> u32 {
    pacing::requested_refresh_hz(selected)
}

/// Resolved patch site: the imm32 address plus the captured stock bytes (for the
/// OFF-revert). Raw pointer into the game image — valid for process lifetime.
struct PatchSite {
    imm_addr: *mut u8,
    stock: u32,
}
unsafe impl Send for PatchSite {}

/// Shared config-derived state. Lives in a global (not on the mod struct) so the
/// overlay row's `on_change` callback — an `Arc<dyn Fn>` that can't capture
/// `&mut self` — can read/update it. Mirrors the `timing_offsets` STATE pattern.
struct FpsState {
    /// Normalized selectable values (sorted asc, deduped, `selected` present).
    values: Vec<i32>,
    /// Display labels parallel to `values` (e.g. `"144fps"`).
    labels: Vec<String>,
    /// Active selection (raw FPS value).
    selected: i32,
    /// Operator's preset array as read from disk (pre-normalization), preserved
    /// verbatim for write-back so persisting `selected` never reorders it (Q9).
    original_presets: Vec<i32>,
}

static STATE: Lazy<Mutex<FpsState>> = Lazy::new(|| {
    Mutex::new(FpsState {
        values: default_presets(),
        labels: default_presets()
            .iter()
            .map(|v| format!("{v}fps"))
            .collect(),
        selected: STOCK_FPS as i32,
        original_presets: default_presets(),
    })
});

/// Load `fps_unlock` config, normalize it, and seed `STATE`. Idempotent; safe to
/// call from both `early_apply` and `init`.
fn load_config_into_state() {
    let cfg = config::get()
        .and_then(|c| c.fps_unlock.clone())
        .unwrap_or_default();
    let original_presets = cfg.presets.clone();
    let (values, selected) = normalize(&cfg.presets, cfg.selected);
    let labels = values.iter().map(|v| format!("{v}fps")).collect();
    if let Ok(mut st) = STATE.lock() {
        st.values = values;
        st.labels = labels;
        st.selected = selected;
        st.original_presets = original_presets;
    }
}

/// Persist the active selection to `mod-config.json` under `fps_unlock`,
/// preserving the operator's `presets` array verbatim (only `selected` changes).
fn persist() {
    let (presets, selected) = match STATE.lock() {
        Ok(st) => (st.original_presets.clone(), st.selected),
        Err(_) => return,
    };
    config::save_json_key(
        "fps_unlock",
        serde_json::json!({ "presets": presets, "selected": selected }),
    );
}

/// Overlay enum-row callback: record the new selection and persist it. Takes
/// effect on the next launch (the value is latched into the D3D device at boot).
/// Runs on the render/input thread — non-blocking, no game calls.
fn set_selected(value: i32) {
    if let Ok(mut st) = STATE.lock() {
        st.selected = value;
    }
    persist();
    log_info!("FpsUnlock: target set to {value}fps (applies on next launch)");
}

/// Register the `FPS TARGET` enum row under the master toggle (optional tier).
fn register_overlay_row() {
    let (values, labels, selected) = match STATE.lock() {
        Ok(st) => (st.values.clone(), st.labels.clone(), st.selected),
        Err(_) => return,
    };
    mod_menu::register_enum_row(mod_menu::EnumRowSpec {
        key: ROW_KEY.to_string(),
        label: "FPS Target".to_string(),
        hint: "Display refresh target. Restart the game to apply.".to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values,
        labels,
        initial_value: selected,
        on_change: Arc::new(set_selected),
    });
    log_info!("FpsUnlock: registered overlay enum row");
}

pub struct FpsUnlockMod {
    /// Resolved patch site (None until resolved, or if the AOB missed).
    patch_site: Option<PatchSite>,
    /// True once `early_apply` actually wrote a non-stock value.
    applied: bool,
    /// True once `early_apply` installed the sub-60 frame limiter.
    limiter_armed: bool,
    /// True while a menu toggle has the armed limiter suspended.
    limiter_suspended: bool,
    /// True once the overlay row is registered (so `disable` removes it).
    row_registered: bool,
}

// Raw pointers into the game image are valid for the process lifetime and only
// touched on controlled threads (matches the project's other patch mods).
unsafe impl Send for FpsUnlockMod {}

impl FpsUnlockMod {
    pub fn new() -> Self {
        Self {
            patch_site: None,
            applied: false,
            limiter_armed: false,
            limiter_suspended: false,
            row_registered: false,
        }
    }

    /// Resolve the imm32 address from the AOB match, validate the stock value,
    /// and record the patch site. Returns false (without recording) on an
    /// unexpected stock value — never patches unknown bytes.
    fn resolve_and_capture(&mut self, match_addr: *const u8) -> bool {
        let imm_addr = unsafe { match_addr.add(IMM_OFFSET) as *mut u8 };
        let stock = unsafe { memory::read_u32(imm_addr as *const u8) };
        if stock != STOCK_FPS {
            log_warn!(
                "FpsUnlock: unexpected stock FPS imm32 {stock} (expected {STOCK_FPS}) -- not patching"
            );
            return false;
        }
        self.patch_site = Some(PatchSite { imm_addr, stock });
        true
    }

    /// Write a u32 value to the imm32 site (handles 360 = 0x168 > one byte).
    fn write_patch(&self, value: i32) {
        if let Some(ref site) = self.patch_site {
            unsafe {
                let old = memory::make_writable(site.imm_addr as *const u8, 4);
                memory::write_u32(site.imm_addr, value as u32);
                memory::restore_protection(site.imm_addr as *const u8, 4, old);
            }
        }
    }

    /// Restore the captured stock value at the imm32 site.
    fn revert_patch(&self) {
        if let Some(ref site) = self.patch_site {
            unsafe {
                let old = memory::make_writable(site.imm_addr as *const u8, 4);
                memory::write_u32(site.imm_addr, site.stock);
                memory::restore_protection(site.imm_addr as *const u8, 4, old);
            }
        }
    }
}

impl Mod for FpsUnlockMod {
    fn id(&self) -> &str {
        MOD_ID
    }
    fn name(&self) -> &str {
        "FPS Unlock"
    }
    fn description(&self) -> &str {
        "Override the fullscreen display refresh (FPS) target"
    }
    fn required_signatures(&self) -> &[&str] {
        // Graceful degradation: the AOB is resolved best-effort; the mod
        // self-disables (no patch, shows [OFF] via is_active) if it's missing,
        // rather than failing registration. Matches timing-offsets / song-limit.
        &[]
    }

    fn early_apply(&mut self, ctx: &EarlyContext) -> bool {
        // Race-critical: this runs before the game's onBoot reads the FPS imm32.
        // (lib.rs only calls early_apply when the mod is enabled in config.)
        load_config_into_state();

        // fps_target_imm32 is a linear AOB, so resolve_all has already populated
        // it in EarlyContext — no manual re-scan needed.
        let Some(addr) = ctx.signatures.get_address("fps_target_imm32") else {
            log_warn!("FpsUnlock: fps_target_imm32 unresolved -- mod self-disabled (no effect)");
            return false;
        };
        if !self.resolve_and_capture(addr) {
            return false;
        }

        let selected = STATE
            .lock()
            .map(|st| st.selected)
            .unwrap_or(STOCK_FPS as i32);
        match mode_for(selected) {
            // Above stock: the original refresh-request patch, unchanged.
            Mode::Refresh(_) => {
                self.write_patch(selected);
                self.applied = true;
                log_info!(
                    "FpsUnlock: early_apply patched FPS target {STOCK_FPS} -> {selected} (effective this boot)"
                );
            }
            Mode::Stock => {
                log_info!("FpsUnlock: selected == stock ({STOCK_FPS}fps); no patch needed");
            }
            // Below stock: the refresh request stays at stock (never a
            // sub-60 display mode); the frame limiter paces the loop instead.
            // Installed here, before the frame loop starts.
            Mode::Limit(fps) => {
                let anchors = ctx.signatures.frame_limiter_anchors();
                match limiter::install(&anchors, fps) {
                    Ok(()) => {
                        self.limiter_armed = true;
                        log_info!(
                            "FpsUnlock: early_apply armed the {fps}fps frame limiter (refresh request stays at stock {STOCK_FPS}Hz)"
                        );
                    }
                    Err(e) => log_warn!(
                        "FpsUnlock: {fps}fps needs the frame limiter, which is unavailable ({e}) -- running at stock pacing"
                    ),
                }
            }
        }
        true
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        // If early_apply already resolved the site, nothing to do. Otherwise the
        // mod was config-disabled at boot (early_apply skipped) — resolve here so
        // a runtime menu toggle can register the row and is_active() is accurate.
        // (A runtime enable still only takes effect next launch — the boot value
        // is already latched into the device.)
        if self.patch_site.is_none() {
            load_config_into_state();
            match ctx.signatures.get_address("fps_target_imm32") {
                Some(addr) => {
                    let _ = self.resolve_and_capture(addr);
                }
                None => log_warn!("FpsUnlock: fps_target_imm32 unresolved at init"),
            }
        }
        true
    }

    fn enable(&mut self) {
        // The patch site is load-bearing: without it the mod can do nothing, so
        // self-disable cleanly (is_active() will report [OFF]).
        if self.patch_site.is_none() {
            log_warn!("FpsUnlock: patch site unresolved -- mod self-disabled (no effect)");
            return;
        }

        // Overlay row is the optional tier — config-file control works regardless.
        register_overlay_row();
        self.row_registered = true;

        let selected = STATE
            .lock()
            .map(|st| st.selected)
            .unwrap_or(STOCK_FPS as i32);
        if self.limiter_armed {
            // A menu OFF -> ON within the session resumes the boot-armed
            // limiter (the first enable after boot finds it already active).
            if self.limiter_suspended {
                limiter::set_active(true);
                self.limiter_suspended = false;
            }
            log_info!("FpsUnlock: enabled -- {selected}fps frame limiter active this boot");
        } else if self.applied {
            log_info!("FpsUnlock: enabled -- target {selected}fps active this boot");
        } else {
            log_info!("FpsUnlock: enabled -- target {selected}fps (applies on next launch)");
        }
    }

    fn disable(&mut self) {
        if self.row_registered {
            mod_menu::remove_rows_for(&[ROW_KEY]);
            self.row_registered = false;
        }
        // Revert the in-memory patch to stock. Inert this session (the value is
        // already latched into the D3D device) but keeps memory clean and
        // symmetric; the config toggle being off means next launch stays stock.
        self.revert_patch();
        // The limiter, unlike the refresh value, is live: suspend it now
        // (restores stock pacing and the game's own dt clamp).
        if self.limiter_armed && !self.limiter_suspended {
            limiter::set_active(false);
            self.limiter_suspended = true;
        }
        log_info!("FpsUnlock: disabled (reverted FPS target to stock)");
    }

    /// Active iff the load-bearing patch site resolved. `enable()` self-disables
    /// (returns early without registering) when the AOB didn't resolve, so this
    /// keeps the overlay from showing a false `[ON]` (and revealing the child
    /// FPS row) over an inert mod. Mirrors timing-offsets' is_active.
    fn is_active(&self) -> bool {
        self.patch_site.is_some()
    }
}
