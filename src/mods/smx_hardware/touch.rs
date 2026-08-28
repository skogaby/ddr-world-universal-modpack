//! Game-window touch capture for the SMX overlay.
//!
//! Subclasses the game window's WndProc (`SetWindowLongPtrW` +
//! `GWLP_WNDPROC`, chaining the original) and registers the window for
//! WM_TOUCH. Presses are hit-tested against `overlay_model` geometry in
//! 1280×720 space and drive the shared button state in `overlay`.
//!
//! Three delivery paths are handled as first-class citizens, each with a
//! one-shot arrival diagnostic so the first cabinet deploy doubles as the
//! delivery probe (the rig runs under CrossOver/Wine, where a touchscreen
//! most likely arrives as MOUSE input; WM_TOUCH is the real-Windows path):
//! - WM_TOUCH (`GetTouchInputInfo`; coords in 1/100 screen px → client)
//! - WM_POINTERDOWN/UP (screen px in lParam → client)
//! - WM_LBUTTONDOWN/UP (client px; SpiceManiaX's "debug" path)
//!
//! Presses are tracked per CONTACT (touch id / pointer id / the one mouse
//! button): release always releases the button the contact pressed,
//! regardless of where the finger lifted (fixes SpiceManiaX's stuck-press
//! on drag-off). All state changes are atomics; the WndProc body is
//! panic-contained and forwards every message except WM_TOUCH (which only
//! exists because we registered for it — the game has no handler).

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::Mutex;

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::Touch::{
    CloseTouchInputHandle, GetTouchInputInfo, RegisterTouchWindow, UnregisterTouchWindow,
    HTOUCHINPUT, TOUCHEVENTF_DOWN, TOUCHEVENTF_UP, TOUCHINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, EnumWindows, GetClientRect, GetWindowThreadProcessId, IsWindowVisible,
    SetWindowLongPtrW, GWLP_WNDPROC,
};

use crate::{log_info, log_warn};

use super::{input_inject, overlay, overlay_model};

const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;
const WM_SYSCOMMAND: u32 = 0x0112;
const SC_CLOSE: usize = 0xF060;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_TOUCH: u32 = 0x0240;
const WM_POINTERDOWN: u32 = 0x0246;
const WM_POINTERUP: u32 = 0x0247;

type WndProcFn = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// The subclassed window (0 = none).
static GAME_HWND: AtomicIsize = AtomicIsize::new(0);
/// The original WndProc (restored on disable).
static ORIGINAL_PROC: AtomicIsize = AtomicIsize::new(0);
/// Whether the subclass is installed.
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Enable gate for the message handlers (disable() clears it even though
/// the subclass may stay while restore fails).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Install attempt pacing (attempt every N frames until success).
static FRAME_COUNTER: AtomicU32 = AtomicU32::new(0);
static SEARCH_WARNED: AtomicBool = AtomicBool::new(false);

/// contact id → pressed button index (the stuck-press fix: release by
/// contact, never by position). Mouse uses contact id `u32::MAX`.
static CONTACTS: Mutex<Vec<(u32, usize)>> = Mutex::new(Vec::new());

// One-shot delivery diagnostics (the deploy-#16 probe).
static SEEN_TOUCH: AtomicBool = AtomicBool::new(false);
static SEEN_POINTER: AtomicBool = AtomicBool::new(false);
static SEEN_MOUSE: AtomicBool = AtomicBool::new(false);
static FIRST_HIT_LOGGED: AtomicBool = AtomicBool::new(false);
/// One-shot: the window is closing — SMX teardown already ran.
static CLOSE_HANDLED: AtomicBool = AtomicBool::new(false);

/// Per-frame tick (render thread, via the mod's on_frame callback):
/// paced attempts to find + subclass the game window until installed.
pub fn tick() {
    if !ACTIVE.load(Ordering::Acquire) || INSTALLED.load(Ordering::Acquire) {
        return;
    }
    // One attempt every ~2 s at 60 fps (EnumWindows isn't free).
    if FRAME_COUNTER.fetch_add(1, Ordering::Relaxed) % 120 != 0 {
        return;
    }
    try_install();
}

/// Arm the capture (mod enable). The subclass installs lazily from
/// [`tick`] once the game window exists.
pub fn activate() {
    ACTIVE.store(true, Ordering::Release);
}

/// Restore the original WndProc + unregister touch (mod disable).
pub fn deactivate() {
    ACTIVE.store(false, Ordering::Release);
    if !INSTALLED.swap(false, Ordering::AcqRel) {
        return;
    }
    let hwnd = HWND(GAME_HWND.load(Ordering::Acquire) as *mut std::ffi::c_void);
    let original = ORIGINAL_PROC.load(Ordering::Acquire);
    if hwnd.0.is_null() || original == 0 {
        return;
    }
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, original);
        let _ = UnregisterTouchWindow(hwnd);
    }
    if let Ok(mut c) = CONTACTS.lock() {
        c.clear();
    }
    log_info!("SmxTouch: window subclass removed");
}

// ── Window discovery + subclass ──────────────────────────────────────

struct FindState {
    pid: u32,
    best: HWND,
    best_area: i64,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut FindState);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == state.pid && IsWindowVisible(hwnd).as_bool() {
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_ok() {
            let area = (rect.right - rect.left) as i64 * (rect.bottom - rect.top) as i64;
            // The game window: the process's biggest visible client area.
            if area > state.best_area {
                state.best_area = area;
                state.best = hwnd;
            }
        }
    }
    true.into()
}

fn try_install() {
    unsafe {
        let mut state = FindState {
            pid: GetCurrentProcessId(),
            best: HWND::default(),
            best_area: 0,
        };
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut state as *mut FindState as isize));
        // Require a plausibly game-sized window (rules out consoles /
        // tiny helper windows while the real one isn't up yet).
        if state.best.0.is_null() || state.best_area < 320 * 240 {
            if !SEARCH_WARNED.swap(true, Ordering::Relaxed) {
                log_info!("SmxTouch: game window not found yet -- will keep looking");
            }
            return;
        }
        let hwnd = state.best;

        // Opt into WM_TOUCH (real-Windows path). Failure is fine — the
        // mouse path still works (the expected case under Wine).
        if RegisterTouchWindow(hwnd, Default::default()).is_err() {
            log_info!("SmxTouch: RegisterTouchWindow failed (mouse path only)");
        }

        let original =
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wnd_proc as *const () as usize as isize);
        if original == 0 {
            log_warn!("SmxTouch: SetWindowLongPtrW failed -- touch capture unavailable");
            let _ = UnregisterTouchWindow(hwnd);
            return;
        }
        ORIGINAL_PROC.store(original, Ordering::Release);
        GAME_HWND.store(hwnd.0 as isize, Ordering::Release);
        INSTALLED.store(true, Ordering::Release);
        log_info!(
            "SmxTouch: game window subclassed (hwnd={:p}, client area {} px)",
            hwnd.0,
            state.best_area
        );
    }
}

// ── Coordinate mapping ───────────────────────────────────────────────

/// Client-pixel point → the 1280×720 model space.
fn client_to_model(hwnd: HWND, x: i32, y: i32) -> Option<(f32, f32)> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect).ok()? };
    let w = (rect.right - rect.left) as f32;
    let h = (rect.bottom - rect.top) as f32;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    Some((x as f32 * 1280.0 / w, y as f32 * 720.0 / h))
}

fn screen_to_model(hwnd: HWND, sx: i32, sy: i32) -> Option<(f32, f32)> {
    let mut pt = POINT { x: sx, y: sy };
    unsafe {
        if !ScreenToClient(hwnd, &mut pt).as_bool() {
            return None;
        }
    }
    client_to_model(hwnd, pt.x, pt.y)
}

// ── Press handling ───────────────────────────────────────────────────

/// A contact went down at model-space (x, y).
fn handle_down(contact: u32, x: f32, y: f32) {
    let visible = [overlay::is_visible(0), overlay::is_visible(1)];
    let Some(index) = overlay_model::hit_test(overlay::buttons(), visible, overlay::scale(), x, y)
    else {
        return;
    };
    if let Ok(mut c) = CONTACTS.lock() {
        if c.iter().any(|(id, _)| *id == contact) {
            return; // duplicate down for a tracked contact
        }
        if c.len() < 16 {
            c.push((contact, index));
        }
    }
    if let Some(button) = overlay::set_button_state(index, true) {
        if !FIRST_HIT_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SmxTouch: first button hit (P{} {:?} at {:.0},{:.0})",
                button.player + 1,
                button.kind,
                x,
                y
            );
        }
        if button.kind == overlay_model::ButtonKind::CardIn {
            input_inject::on_card_button(button.player);
        }
    }
}

/// A contact lifted — release whatever it pressed (position ignored).
fn handle_up(contact: u32) {
    let index = CONTACTS.lock().ok().and_then(|mut c| {
        c.iter()
            .position(|(id, _)| *id == contact)
            .map(|i| c.swap_remove(i).1)
    });
    if let Some(index) = index {
        overlay::set_button_state(index, false);
    }
}

// ── The subclass WndProc ─────────────────────────────────────────────

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let handled =
        std::panic::catch_unwind(|| handle_message(hwnd, msg, wparam, lparam)).unwrap_or(false);
    if handled {
        return LRESULT(0);
    }
    let original = ORIGINAL_PROC.load(Ordering::Acquire);
    if original != 0 {
        CallWindowProcW(
            Some(std::mem::transmute::<isize, WndProcFn>(original)),
            hwnd,
            msg,
            wparam,
            lparam,
        )
    } else {
        LRESULT(0)
    }
}

/// Returns true when the message was fully consumed (WM_TOUCH only —
/// it exists solely because we registered for it; everything else is
/// observed and forwarded).
unsafe fn handle_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
    // Window close (the X button / Alt-F4): deploy #17 showed the click
    // produced NO reaction anywhere — no WM_CLOSE reached this proc and
    // spice2x never began shutdown (the long-standing "X doesn't close
    // the game" hang, present since the SMX mod landed; that run had NO
    // SMX devices, so the HID reader threads are exonerated). Take
    // ownership of the close instead: on SC_CLOSE or WM_CLOSE, log
    // (which path fired = the diagnostic), stop the SMX transport, give
    // the normal close path 1.5 s, then force-exit like spice2x's own
    // ctrl-C "force shutdown" endgame.
    if (msg == WM_CLOSE || (msg == WM_SYSCOMMAND && (wparam.0 & 0xFFF0) == SC_CLOSE))
        && !CLOSE_HANDLED.swap(true, Ordering::AcqRel)
    {
        log_info!(
            "SmxTouch: close requested (msg={:#x}) -- stopping SMX transport, exiting in 1.5 s unless the game shuts down first",
            msg
        );
        crate::services::smx::transport::shutdown();
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            crate::log_info!("SmxTouch: game still alive after close request -- force exit");
            // TerminateProcess, not process::exit: the whole problem is
            // teardown wedging under Wine — skip teardown entirely
            // (spice2x's own "force shutdown" endgame).
            unsafe {
                let _ = windows::Win32::System::Threading::TerminateProcess(
                    windows::Win32::System::Threading::GetCurrentProcess(),
                    0,
                );
            }
        });
        return false; // forward: let any legit close path run first
    }
    if msg == WM_DESTROY && !CLOSE_HANDLED.swap(true, Ordering::AcqRel) {
        // Teardown began without a close message we saw — still stop the
        // transport so no SMX thread can wedge process exit.
        log_info!("SmxTouch: WM_DESTROY -- stopping SMX transport");
        crate::services::smx::transport::shutdown();
        return false;
    }
    if !ACTIVE.load(Ordering::Acquire) {
        return false;
    }
    match msg {
        WM_LBUTTONDOWN | WM_LBUTTONUP => {
            if !SEEN_MOUSE.swap(true, Ordering::Relaxed) {
                log_info!("SmxTouch: delivery -- mouse events arriving");
            }
            let x = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
            if msg == WM_LBUTTONDOWN {
                if let Some((mx, my)) = client_to_model(hwnd, x, y) {
                    handle_down(u32::MAX, mx, my);
                }
            } else {
                handle_up(u32::MAX);
            }
            false
        }
        WM_POINTERDOWN | WM_POINTERUP => {
            if !SEEN_POINTER.swap(true, Ordering::Relaxed) {
                log_info!("SmxTouch: delivery -- WM_POINTER events arriving");
            }
            let id = (wparam.0 & 0xFFFF) as u32;
            let sx = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
            let sy = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
            if msg == WM_POINTERDOWN {
                if let Some((mx, my)) = screen_to_model(hwnd, sx, sy) {
                    handle_down(id, mx, my);
                }
            } else {
                handle_up(id);
            }
            false
        }
        WM_TOUCH => {
            let count = (wparam.0 & 0xFFFF) as u32;
            if !SEEN_TOUCH.swap(true, Ordering::Relaxed) {
                log_info!(
                    "SmxTouch: delivery -- WM_TOUCH events arriving (n={})",
                    count
                );
            }
            let n = count.min(10) as usize;
            let mut touches = [TOUCHINPUT::default(); 10];
            let handle = HTOUCHINPUT(lparam.0 as *mut std::ffi::c_void);
            if GetTouchInputInfo(
                handle,
                &mut touches[..n],
                std::mem::size_of::<TOUCHINPUT>() as i32,
            )
            .is_ok()
            {
                for t in &touches[..n] {
                    // TOUCHINPUT coords are 1/100 of a screen pixel.
                    let sx = t.x / 100;
                    let sy = t.y / 100;
                    if (t.dwFlags & TOUCHEVENTF_DOWN) != Default::default() {
                        if let Some((mx, my)) = screen_to_model(hwnd, sx, sy) {
                            handle_down(t.dwID, mx, my);
                        }
                    } else if (t.dwFlags & TOUCHEVENTF_UP) != Default::default() {
                        handle_up(t.dwID);
                    }
                }
                let _ = CloseTouchInputHandle(handle);
                true // consumed: the game has no WM_TOUCH handler
            } else {
                false // let the original's DefWindowProc close the handle
            }
        }
        _ => false,
    }
}
