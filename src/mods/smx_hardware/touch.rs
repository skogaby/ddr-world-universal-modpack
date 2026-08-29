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

use windows::Win32::Foundation::{GetLastError, BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MonitorFromWindow, ScreenToClient, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::Touch::{
    CloseTouchInputHandle, GetTouchInputInfo, RegisterTouchWindow, UnregisterTouchWindow,
    HTOUCHINPUT, TOUCHEVENTF_DOWN, TOUCHEVENTF_UP, TOUCHINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, EnumWindows, GetClassNameW, GetClientRect, GetWindow, GetWindowRect,
    GetWindowThreadProcessId, IsWindowVisible, SetWindowLongPtrW, GWLP_WNDPROC, GW_OWNER,
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
static TOUCH_REG_WARNED: AtomicBool = AtomicBool::new(false);

/// Windows the subclass attempt FAILED on — never retried. The
/// "biggest visible window" heuristic can land on a window we cannot
/// subclass at all: the classic case is the spice2x console window,
/// which Windows reports as belonging to this PID for compatibility
/// even though conhost.exe actually owns it, so `SetWindowLongPtrW`
/// fails with access denied forever (deploy: Windows windowed mode,
/// where no D3DProxyWindow exists and a large console out-areas the
/// 1280x720 game window). Blocklisting lets the next attempt fall
/// through to the next-biggest candidate instead of spinning.
static FAILED_HWNDS: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// contact id → pressed button index (the stuck-press fix: release by
/// contact, never by position). Mouse uses contact id `u32::MAX`.
static CONTACTS: Mutex<Vec<(u32, usize)>> = Mutex::new(Vec::new());

/// IR-frame release debounce (ms). The SMX cabinet's touchscreen is an
/// IR frame, not a glass digitizer: the beam plane sits above the glass,
/// so one physical press arrives as a down/up/down flutter as the finger
/// crosses the plane (cabinet deploy #1: the visibility toggle
/// double-fired, edge buttons multi-pressed). Releases are therefore
/// DEFERRED by this window and cancelled by a re-press: the HELD bit
/// never clears across the flutter, so edge-driven actions fire once,
/// and genuinely held buttons survive beam flicker. 0 = immediate
/// release (the pre-debounce behavior).
static DEBOUNCE_MS: AtomicU32 = AtomicU32::new(150);

/// Deferred releases: (button index, due at overlay::clock_ms()).
static PENDING_RELEASES: Mutex<Vec<(usize, u64)>> = Mutex::new(Vec::new());

// One-shot delivery diagnostics (the deploy-#16 probe).
static SEEN_TOUCH: AtomicBool = AtomicBool::new(false);
static SEEN_POINTER: AtomicBool = AtomicBool::new(false);
static SEEN_MOUSE: AtomicBool = AtomicBool::new(false);
static FIRST_HIT_LOGGED: AtomicBool = AtomicBool::new(false);
/// One-shot: the window is closing — SMX teardown already ran.
static CLOSE_HANDLED: AtomicBool = AtomicBool::new(false);

/// Per-frame tick (render thread, via the mod's on_frame callback):
/// drains due deferred releases, then paced attempts to find + subclass
/// the game window until installed.
pub fn tick() {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    drain_pending_releases();
    if INSTALLED.load(Ordering::Acquire) {
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
pub fn activate(debounce_ms: u32) {
    DEBOUNCE_MS.store(debounce_ms, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Release);
}

/// Live debounce-window update (the mod menu's "Touch Debounce" row).
/// Applies to the next release; already-queued releases keep their
/// deadline.
pub fn set_debounce_ms(ms: u32) {
    DEBOUNCE_MS.store(ms, Ordering::Relaxed);
}

/// Restore the original WndProc + unregister touch (mod disable).
pub fn deactivate() {
    ACTIVE.store(false, Ordering::Release);
    // Flush deferred releases so no button stays logically held.
    if let Ok(mut p) = PENDING_RELEASES.lock() {
        let pending = std::mem::take(&mut *p);
        drop(p);
        for (index, _) in pending {
            overlay::set_button_state(index, false);
        }
    }
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
    console: HWND,
    best: HWND,
    best_area: i64,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut FindState);
    // Never the console window: GetWindowThreadProcessId reports it as
    // ours (a Windows compat lie) but conhost.exe owns it — it can't be
    // subclassed and must not shadow the real game window.
    if hwnd == state.console {
        return true.into();
    }
    if FAILED_HWNDS
        .lock()
        .map(|f| f.contains(&(hwnd.0 as isize)))
        .unwrap_or(false)
    {
        return true.into();
    }
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

/// Best-effort window class name (diagnostics only).
fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    if n > 0 {
        String::from_utf16_lossy(&buf[..n as usize])
    } else {
        String::from("<unknown>")
    }
}

fn try_install() {
    unsafe {
        let mut state = FindState {
            pid: GetCurrentProcessId(),
            console: GetConsoleWindow(),
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
        // Owned popups lose the tiebreak to their owner: in fullscreen
        // spice2x creates a D3DProxyWindow as a WS_POPUP owned by the
        // real game window, with the IDENTICAL 1280x720 client area —
        // whichever enumerates first wins the strict `>` contest, but
        // mouse input is delivered to the OWNER (the D3D focus window;
        // fullscreen deploy #2: subclassing the proxy captured nothing).
        // Walk to the top owner, bounded, staying inside our process.
        let mut hwnd = state.best;
        for _ in 0..4 {
            let owner = GetWindow(hwnd, GW_OWNER).unwrap_or_default();
            if owner.0.is_null() || owner == state.console {
                break;
            }
            let mut owner_pid = 0u32;
            GetWindowThreadProcessId(owner, Some(&mut owner_pid));
            if owner_pid != state.pid {
                break;
            }
            log_info!(
                "SmxTouch: candidate hwnd={:p} class=\"{}\" is owned -- walking to owner hwnd={:p} class=\"{}\"",
                hwnd.0,
                class_name(hwnd),
                owner.0,
                class_name(owner)
            );
            hwnd = owner;
        }
        // The owner walk can land on an already-blocklisted window;
        // blocklist the candidate that led there too (else every pass
        // re-picks it) and bail — the WARN already fired when the
        // owner was listed.
        if FAILED_HWNDS
            .lock()
            .map(|f| f.contains(&(hwnd.0 as isize)))
            .unwrap_or(false)
        {
            if let Ok(mut f) = FAILED_HWNDS.lock() {
                if f.len() < 32 && !f.contains(&(state.best.0 as isize)) {
                    f.push(state.best.0 as isize);
                }
            }
            return;
        }

        let original =
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wnd_proc as *const () as usize as isize);
        if original == 0 {
            // Not subclassable (e.g. another process really owns it).
            // Log the identity ONCE, blocklist it, and let the next
            // attempt pick the next-best candidate.
            log_warn!(
                "SmxTouch: SetWindowLongPtrW failed on hwnd={:p} class=\"{}\" area={} (err={:?}) -- blocklisting, will try other windows",
                hwnd.0,
                class_name(hwnd),
                state.best_area,
                GetLastError()
            );
            if let Ok(mut f) = FAILED_HWNDS.lock() {
                if f.len() < 32 {
                    f.push(hwnd.0 as isize);
                }
            }
            return;
        }
        ORIGINAL_PROC.store(original, Ordering::Release);
        GAME_HWND.store(hwnd.0 as isize, Ordering::Release);
        INSTALLED.store(true, Ordering::Release);

        // Opt into WM_TOUCH (real-Windows path). Failure is fine — the
        // mouse path still works (the expected case under Wine).
        if RegisterTouchWindow(hwnd, Default::default()).is_err()
            && !TOUCH_REG_WARNED.swap(true, Ordering::Relaxed)
        {
            log_info!("SmxTouch: RegisterTouchWindow failed (mouse path only)");
        }

        log_info!(
            "SmxTouch: game window subclassed (hwnd={:p}, class=\"{}\", client area {} px)",
            hwnd.0,
            class_name(hwnd),
            state.best_area
        );
    }
}

// ── Coordinate mapping ───────────────────────────────────────────────

/// One-shot: the fullscreen (monitor-relative) mapping engaged.
static FULLSCREEN_MAP_LOGGED: AtomicBool = AtomicBool::new(false);
/// One-shot: full geometry dump at the first mapped click (every
/// fallback branch must be observable — repo learnings).
static GEOMETRY_LOGGED: AtomicBool = AtomicBool::new(false);

/// Client-pixel point → the 1280×720 model space.
///
/// Two regimes:
/// - Windowed: the game renders inside the client area — scale client
///   coords by the client size.
/// - Fullscreen: the presented image covers the MONITOR, but the game
///   window keeps its decorations, so the client rect is offset from
///   the screen origin (border + caption) and slightly smaller than
///   the monitor. Client-relative mapping then lands short — the
///   error converges to ~0 at the bottom edge and grows toward the
///   top (fullscreen deploy #4: bottom menu buttons fine, top pinpad
///   rows needed clicks BELOW the art). Map via screen coords
///   relative to the monitor rect instead.
///
/// Fullscreen detection is by the OUTER window rect covering the
/// monitor (fullscreen deploy #4 root cause: the previous
/// `client size == monitor size` gate never engaged — with Windows
/// fullscreen optimizations the desktop stays at native resolution
/// and D3D9 sizes the decorated window to the monitor, leaving the
/// client a caption-height smaller). A maximized borderless window
/// also passes, where both mappings agree — harmless.
fn client_to_model(hwnd: HWND, x: i32, y: i32) -> Option<(f32, f32)> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect).ok()? };
    let w = (rect.right - rect.left) as f32;
    let h = (rect.bottom - rect.top) as f32;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mut wr = RECT::default();
        let have_geom =
            GetMonitorInfoW(monitor, &mut mi).as_bool() && GetWindowRect(hwnd, &mut wr).is_ok();
        if have_geom && !GEOMETRY_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SmxTouch: click geometry -- client {}x{}, window ({},{})-({},{}), monitor ({},{})-({},{})",
                rect.right - rect.left,
                rect.bottom - rect.top,
                wr.left,
                wr.top,
                wr.right,
                wr.bottom,
                mi.rcMonitor.left,
                mi.rcMonitor.top,
                mi.rcMonitor.right,
                mi.rcMonitor.bottom
            );
        }
        if have_geom {
            let mw = mi.rcMonitor.right - mi.rcMonitor.left;
            let mh = mi.rcMonitor.bottom - mi.rcMonitor.top;
            let covers = wr.left <= mi.rcMonitor.left
                && wr.top <= mi.rcMonitor.top
                && wr.right >= mi.rcMonitor.right
                && wr.bottom >= mi.rcMonitor.bottom;
            if covers && mw > 0 && mh > 0 {
                let mut pt = POINT { x, y };
                if ClientToScreen(hwnd, &mut pt).as_bool() {
                    if !FULLSCREEN_MAP_LOGGED.swap(true, Ordering::Relaxed) {
                        log_info!(
                            "SmxTouch: fullscreen mapping engaged (monitor {}x{} at ({},{}), client origin offset ({},{}))",
                            mw,
                            mh,
                            mi.rcMonitor.left,
                            mi.rcMonitor.top,
                            pt.x - x - mi.rcMonitor.left,
                            pt.y - y - mi.rcMonitor.top
                        );
                    }
                    return Some((
                        (pt.x - mi.rcMonitor.left) as f32 * 1280.0 / mw as f32,
                        (pt.y - mi.rcMonitor.top) as f32 * 720.0 / mh as f32,
                    ));
                }
            }
        }
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
    // A re-press cancels the button's deferred release (IR flutter: the
    // HELD bit stays set across the down/up/down burst, so this press is
    // no edge and kind actions below don't re-fire).
    if let Ok(mut p) = PENDING_RELEASES.lock() {
        p.retain(|(i, _)| *i != index);
    }
    if let Some((button, edge)) = overlay::set_button_state(index, true) {
        if !FIRST_HIT_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SmxTouch: first button hit (P{} {:?} at {:.0},{:.0})",
                button.player + 1,
                button.kind,
                x,
                y
            );
        }
        if edge && button.kind == overlay_model::ButtonKind::CardIn {
            input_inject::on_card_button(button.player);
        }
    }
}

/// A contact lifted — release whatever it pressed (position ignored).
/// With the IR-frame debounce active the release is deferred; a
/// re-press inside the window cancels it (see [`DEBOUNCE_MS`]).
fn handle_up(contact: u32) {
    let index = CONTACTS.lock().ok().and_then(|mut c| {
        c.iter()
            .position(|(id, _)| *id == contact)
            .map(|i| c.swap_remove(i).1)
    });
    let Some(index) = index else {
        return;
    };
    let debounce = DEBOUNCE_MS.load(Ordering::Relaxed) as u64;
    if debounce == 0 {
        overlay::set_button_state(index, false);
        return;
    }
    let due = overlay::clock_ms() + debounce;
    if let Ok(mut p) = PENDING_RELEASES.lock() {
        if let Some(entry) = p.iter_mut().find(|(i, _)| *i == index) {
            entry.1 = due;
        } else if p.len() < 64 {
            p.push((index, due));
        } else {
            // Table full (shouldn't happen with 38 buttons) — fail to
            // the immediate release rather than a stuck button.
            drop(p);
            overlay::set_button_state(index, false);
        }
    } else {
        overlay::set_button_state(index, false);
    }
}

/// Release every deferred button whose debounce window has elapsed
/// (per-frame, render thread).
fn drain_pending_releases() {
    let now = overlay::clock_ms();
    // Collect first: never call set_button_state under the lock.
    let mut due = [0usize; 8];
    let mut n = 0;
    if let Ok(mut p) = PENDING_RELEASES.lock() {
        p.retain(|&(index, deadline)| {
            if deadline <= now && n < due.len() {
                due[n] = index;
                n += 1;
                false
            } else {
                true
            }
        });
    }
    for &index in &due[..n] {
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
