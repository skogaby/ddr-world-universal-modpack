//! Exclusive input handling while the mod-menu overlay is open, plus the
//! hold-to-repeat thread for scalar/enum rows. Navigation and selection run
//! through the pure `model::Navigator` over the active tab's row list.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::log_info;
use crate::services::{custom_options, input_manager, widget_renderer};
use crate::types::buttons::*;

use super::model::{self, Row, RowKind, RowSource, SelectorState, TabId};
use super::render;
use super::{chrome, chrome_loader, rows, tabs, MOD_MENU_STATE};

/// Hold-to-repeat timing for scalar adjustment: wait this long after the initial
/// press before auto-repeat kicks in, then fire every `REPEAT_INTERVAL_MS`.
const REPEAT_INITIAL_DELAY_MS: u64 = 350;
const REPEAT_INTERVAL_MS: u64 = 60;
/// Repeat-thread poll tick.
const REPEAT_POLL_MS: u64 = 16;

/// Generation token for the hold-to-repeat thread. Each `start_repeat_thread`
/// bumps it and spawns a thread that captures the new value; a thread exits as
/// soon as it observes a generation other than its own. This both stops the
/// running thread on close (no separate "run" flag to get wedged) and prevents
/// a stale thread from surviving a fast close→reopen: the reopen bumps the
/// generation, so the old thread — even if still mid-sleep when the new one
/// spawns — exits on its next tick instead of double-firing `activate_selected`.
/// A panic in the loop body therefore can't permanently wedge repeats either:
/// the next open bumps the generation and spawns a fresh thread regardless.
static REPEAT_THREAD_GEN: AtomicU64 = AtomicU64::new(0);

pub(super) fn handle_exclusive_input(event: &InputEvent) -> bool {
    if event.event_type != InputEventType::Pressed {
        return true;
    }

    if event.button == button::NUM_0 {
        let Ok(mut state) = MOD_MENU_STATE.lock() else {
            return true;
        };
        if super::on_zero_pressed(&mut state) {
            drop(state);
            super::close();
        }
        return true;
    }

    if event.button == button::NUM_1 {
        switch_tab(false);
        return true;
    }
    if event.button == button::NUM_3 {
        switch_tab(true);
        return true;
    }
    // Navigation/adjustment uses the cabinet's gameplay menu buttons only —
    // the pinpad is reserved for the gesture keys (0-0-0 close, 1/3 tabs)
    // per maintainer direction 2026-08-24.
    if event.button == button::MENU_UP {
        navigate(false);
        return true;
    }
    if event.button == button::MENU_DOWN {
        navigate(true);
        return true;
    }
    if event.button == button::MENU_LEFT {
        activate_selected(false);
        return true;
    }
    if event.button == button::MENU_RIGHT {
        activate_selected(true);
        return true;
    }

    true // consume all input while open
}

/// Switch to the previous (`next = false`) or next tab, wrapping, with
/// per-tab cursor memory. Rebuilds the lists so the newly-shown tab renders
/// current values/enable state.
fn switch_tab(next: bool) {
    {
        let Ok(mut state) = MOD_MENU_STATE.lock() else {
            return;
        };
        if !state.is_open {
            return;
        }
        if next {
            state.tab_nav.switch_next();
        } else {
            state.tab_nav.switch_prev();
        }
        tabs::rebuild_tabs(&mut state);
    }
    widget_renderer::run_on_render_thread(|| {
        if let Ok(state) = MOD_MENU_STATE.lock() {
            render::refresh_all(&state);
        }
    });
}

/// Move the cursor up/down on the active tab (wrap + header/greyed skip;
/// the PLAYER tab's pinned selector joins the cycle at the top).
fn navigate(down: bool) {
    {
        let Ok(mut state) = MOD_MENU_STATE.lock() else {
            return;
        };
        if !state.is_open {
            return;
        }
        let tab = state.tab_nav.active();
        let tab_idx = tab.index();
        let mut nav = state.tab_nav.state();
        if let Some(tab_rows) = state.tab_rows.get(tab_idx) {
            let navigator = render::navigator_for(tab, tab_rows);
            if down {
                navigator.down(&mut nav);
            } else {
                navigator.up(&mut nav);
            }
        }
        *state.tab_nav.state_mut() = nav;
    }
    widget_renderer::run_on_render_thread(|| {
        if let Ok(state) = MOD_MENU_STATE.lock() {
            render::refresh_all(&state);
        }
    });
}

/// The currently-selected row on the active tab (cloned out of the state so
/// callers can act without holding the lock). `None` while the PLAYER tab's
/// pinned selector holds focus.
fn selected_row() -> Option<Row> {
    let state = MOD_MENU_STATE.lock().ok()?;
    if !state.is_open {
        return None;
    }
    let tab = state.tab_nav.active();
    let tab_rows = state.tab_rows.get(tab.index())?;
    let nav = state.tab_nav.state();
    let idx = render::navigator_for(tab, tab_rows).selected(&nav)?;
    tab_rows.get(idx).cloned()
}

/// Compute the next value for a Boolean/Scalar/Enum row from a LEFT/RIGHT
/// press — shared by the Contributed and Mirrored edit paths. `None` = no
/// change (already at a bound / same boolean state / header).
fn compute_new_value(kind: &RowKind, to_on: bool) -> Option<i32> {
    match kind {
        RowKind::Boolean { value } => {
            if *value == to_on {
                return None;
            }
            Some(to_on as i32)
        }
        RowKind::Scalar {
            value,
            min,
            max,
            step_fine,
            step_coarse,
            ..
        } => {
            // Right = increase, Left = decrease. Start held → coarse.
            let step = if coarse_held() {
                *step_coarse
            } else {
                *step_fine
            };
            let delta = if to_on { step } else { -step };
            let new_value = (value + delta).clamp(*min, *max);
            if new_value == *value {
                return None; // already at the bound
            }
            Some(new_value)
        }
        RowKind::Enum {
            index, ref values, ..
        } => {
            if values.is_empty() {
                return None;
            }
            // Clamp at the ends (no wrap); coarse is a no-op for enums.
            let last = values.len() - 1;
            let new_index = if to_on {
                (index + 1).min(last)
            } else {
                index.saturating_sub(1)
            };
            if new_index == *index {
                return None; // already at the end
            }
            Some(values[new_index])
        }
        RowKind::Header => None,
    }
}

/// LEFT/RIGHT while the PLAYER tab's pinned selector holds focus: switch the
/// configured side (LEFT = P1, RIGHT = P2) among EDITABLE sides only —
/// Locked/AllGated selectors consume the press as a no-op. Returns whether
/// the press was consumed by the selector.
fn try_switch_player_side(to_right: bool) -> bool {
    let mut switched = false;
    {
        let Ok(mut state) = MOD_MENU_STATE.lock() else {
            return false;
        };
        if !state.is_open || state.tab_nav.active() != TabId::PlayerSettings {
            return false;
        }
        let nav = state.tab_nav.state();
        let focused = state
            .tab_rows
            .get(TabId::PlayerSettings.index())
            .map(|rows_list| {
                render::navigator_for(TabId::PlayerSettings, rows_list).pinned_focused(&nav)
            })
            .unwrap_or(false);
        if !focused {
            return false;
        }
        if model::selector_state(state.player_editable) == SelectorState::Free {
            let target: u8 = if to_right { 1 } else { 0 };
            if state.player_side != target {
                state.player_side = target;
                tabs::rebuild_tabs(&mut state);
                switched = true;
            }
        }
    }
    if switched {
        widget_renderer::run_on_render_thread(|| {
            if let Ok(state) = MOD_MENU_STATE.lock() {
                render::refresh_all(&state);
            }
        });
    }
    true
}

/// Activate the selected row in response to a left/right press. For a
/// `Boolean` row, `to_on` decides the target state (left = off, right = on);
/// scalars adjust by fine/coarse step; enums cycle. Registry mod-toggle rows
/// route through the registry toggle + config-save path; contributed rows
/// fire their `on_change` and mirror the value into the registration store;
/// mirrored rows marshal to the render thread and apply through the
/// custom_options framework (gate re-checked inside the closure); THEME rows
/// drive the chrome_loader appearance state + `overlay_menu` persistence.
fn activate_selected(to_on: bool) {
    // Pinned selector focus consumes LEFT/RIGHT as a side switch.
    if try_switch_player_side(to_on) {
        return;
    }

    let Some(row) = selected_row() else {
        return;
    };

    match row.source {
        RowSource::RegistryToggle => {
            if let RowKind::Boolean { value } = row.kind {
                if value != to_on {
                    rows::toggle_registry_mod(&row.key, to_on);
                }
            }
        }
        RowSource::Contributed => {
            let Some(new_value) = compute_new_value(&row.kind, to_on) else {
                return;
            };
            // Drive the owning mod (authoritative), then mirror into the store.
            if let Some(cb) = rows::on_change_for(&row.key) {
                cb(new_value);
            }
            rows::set_row_value_and_refresh(&row.key, new_value);
        }
        RowSource::Mirrored => {
            let Some(new_value) = compute_new_value(&row.kind, to_on) else {
                return;
            };
            let side = match MOD_MENU_STATE.lock() {
                Ok(state) => state.player_side,
                Err(_) => return,
            };
            let key = row.key.clone();
            // Marshal the apply to the render thread (the framework's "user
            // edits fire on the render thread" contract) with the session
            // gate re-checked INSIDE the closure — a session lost between
            // paint and press refuses the edit and repaints greyed
            // (design §4.9). Success repaints via the value-changed observer.
            widget_renderer::run_on_render_thread(move || {
                let editable = tabs::editable_sides_now();
                if editable.get(side as usize).copied().unwrap_or(false) {
                    custom_options::set_value(&key, side, new_value);
                } else {
                    log_info!(
                        "ModMenu: refused edit of {key:?} for side {side} — session no longer active"
                    );
                    super::schedule_coalesced_refresh();
                }
            });
        }
        // THEME tab appearance rows: apply to the chrome_loader-owned
        // appearance state, persist the whole `overlay_menu` section
        // (quick_restart's save_json_key pattern — file write only, safe on
        // this input/repeat thread), re-synthesize the panel where its
        // texture depends on the value, and rebuild so the row list + the
        // repaint pick the new palette up immediately. The panel itself
        // swaps only when the new texture resolves (chrome_loader publishes
        // on resolve — the old panel never disappears first).
        RowSource::Theme => {
            let Some(new_value) = compute_new_value(&row.kind, to_on) else {
                return;
            };
            match row.key.as_str() {
                model::THEME_ROW_KEY => {
                    chrome_loader::set_active_theme(new_value as usize);
                    chrome_loader::persist_overlay_menu();
                    chrome_loader::resynthesize();
                    super::update_background_feed();
                }
                model::ANIMATE_ROW_KEY => {
                    chrome_loader::set_animate(new_value != 0);
                    chrome_loader::persist_overlay_menu();
                    super::update_background_feed();
                }
                model::OPACITY_ROW_KEY => {
                    chrome_loader::set_effective_opacity(chrome::clamp_opacity(new_value));
                    chrome_loader::persist_overlay_menu();
                    chrome_loader::resynthesize();
                    // The animated background's master fade follows opacity.
                    super::update_background_feed();
                }
                other => {
                    // Defensive: hook-reachable, never panic on a stray key.
                    log_info!("ModMenu: unknown THEME row key {other:?} ignored");
                    return;
                }
            }
            tabs::rebuild_and_refresh();
        }
    }
}

/// True if Start is currently held on either side (used to pick the coarse
/// scalar step). Start is tracked in the per-player held bitmask by
/// `input_manager`, so this needs no extra hold-tracking infrastructure.
fn coarse_held() -> bool {
    if !input_manager::is_available() {
        return false;
    }
    let p1 = input_manager::get_button_state(Player::P1);
    let p2 = input_manager::get_button_state(Player::P2);
    ((p1 | p2) & button::START) != 0
}

/// True if MENU_LEFT/MENU_RIGHT is currently held on either side. Returns the
/// direction so the repeat thread can re-fire it. `Some(true)` =
/// right/increase held, `Some(false)` = left/decrease held, `None` = neither
/// (or both, which we treat as no repeat).
fn adjust_dir_held() -> Option<bool> {
    if !input_manager::is_available() {
        return None;
    }
    let held =
        input_manager::get_button_state(Player::P1) | input_manager::get_button_state(Player::P2);
    let right = (held & button::MENU_RIGHT) != 0;
    let left = (held & button::MENU_LEFT) != 0;
    match (right, left) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None, // neither or both
    }
}

/// Whether the currently-selected row auto-repeats while a direction is held.
/// Scalar and Enum rows do (continuous adjust / entry cycling); boolean toggles
/// do not (the initial press already toggled once).
fn selected_repeats() -> bool {
    selected_row()
        .map(|r| matches!(r.kind, RowKind::Scalar { .. } | RowKind::Enum { .. }))
        .unwrap_or(false)
}

/// Spawn the hold-to-repeat thread for scalar/enum rows. Runs while its generation is
/// current (see `REPEAT_THREAD_GEN`); a later open/close bumps the generation and
/// retires it. The input layer is edge-triggered (no repeat events while held),
/// so this watches the live held state via `get_button_state` and re-fires
/// `activate_selected` on a scalar row after an initial delay, then at a steady
/// interval, for as long as the direction stays held. Idle (no scalar held)
/// costs one cheap poll per tick.
pub(super) fn start_repeat_thread() {
    // Claim a new generation; the spawned thread runs only while the global
    // generation still equals this one (a later open/close bumps it, retiring
    // any prior thread). `AcqRel` so our +1 can't race a concurrent bump.
    let my_gen = REPEAT_THREAD_GEN.fetch_add(1, Ordering::AcqRel) + 1;
    std::thread::spawn(move || {
        // Per-press repeat state, reset whenever the held direction changes or
        // releases. `armed_at` = when the current hold began; `last_fire` = when
        // we last auto-fired.
        let mut current_dir: Option<bool> = None;
        let mut armed_at = Instant::now();
        let mut last_fire = Instant::now();

        loop {
            std::thread::sleep(Duration::from_millis(REPEAT_POLL_MS));
            // Re-check the generation AFTER sleeping, before doing any work, so a
            // thread retired mid-sleep (close, or a close→reopen that bumped the
            // generation) exits without firing one extra `activate_selected`
            // against the previous menu's selection.
            if REPEAT_THREAD_GEN.load(Ordering::Acquire) != my_gen {
                break;
            }

            // Guard the per-tick body: a panic here (e.g. a poisoned lock that
            // somehow surfaced) must not unwind out of the thread — that would
            // leave the generation looking live to no thread. catch_unwind keeps
            // the loop alive; the generation check still retires it on close.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Only repeat for scalar/enum rows; a held direction on a boolean
                // row is a no-op here (the initial press already toggled it once).
                let dir = if selected_repeats() {
                    adjust_dir_held()
                } else {
                    None
                };

                match dir {
                    Some(d) => {
                        let now = Instant::now();
                        if current_dir != Some(d) {
                            // New hold (or direction change): arm; the initial
                            // press was already handled by the input event, so
                            // the first auto-repeat waits the full initial delay.
                            current_dir = Some(d);
                            armed_at = now;
                            last_fire = now;
                        } else if now.duration_since(armed_at)
                            >= Duration::from_millis(REPEAT_INITIAL_DELAY_MS)
                            && now.duration_since(last_fire)
                                >= Duration::from_millis(REPEAT_INTERVAL_MS)
                        {
                            last_fire = now;
                            activate_selected(d);
                        }
                    }
                    None => current_dir = None,
                }
            }));
        }
    });
}

/// Stop the hold-to-repeat thread (on overlay close). Bumping the generation
/// retires whatever thread is currently running on its next tick.
pub(super) fn stop_repeat_thread() {
    REPEAT_THREAD_GEN.fetch_add(1, Ordering::AcqRel);
}
