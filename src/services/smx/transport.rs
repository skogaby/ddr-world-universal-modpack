//! SMX transport — the IO threads driving every connected SMX device.
//!
//! **Two-thread design (latency split).** Each device gets a dedicated
//! **reader thread** doing event-driven blocking reads: it wakes the instant
//! an input report lands and writes the per-pad atomic mask with ~0 added
//! latency (≈ the pad's USB report interval), never blocked by anything else.
//! A single **worker thread** owns everything non-latency-critical: ~250 ms
//! discovery/hot-plug, the flow-controlled serial command queue (device-info
//! handshake + HOST_CMD_FINISHED-gated lights), and the ~30 Hz stage-lights
//! drain (DDR light frame → `light_map` → `protocol` → HID writes). The
//! reader forwards serial reports (id 6) to the worker over an mpsc channel;
//! the worker never touches the read path, so a blocking lights write (which
//! can take tens of ms under Wine) can no longer stall input freshness.
//!
//! This is why native HID beats the old SpiceAPI path on input latency: no
//! TCP loopback, no second process, and reads decoupled from writes so the
//! game always samples a ≤~1 ms-fresh mask.
//!
//! Threads run at `ABOVE_NORMAL`, not the SDK's `HIGHEST` — we live inside
//! the game process and must not starve its input/render threads (rule 4).
//!
//! ## Thread contract
//!
//! - `init()` / `shutdown()` — called from the mod's enable/disable (any
//!   thread; idempotent).
//! - `input_mask(pad)` — lock-free, callable from the game's getter detours.
//! - `write_tape_led` / `write_dimlamp` — tight accumulator writes, called
//!   from the light-out detours on the game's IO thread.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU16, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED,
    HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentThread, ResetEvent, SetThreadPriority, WaitForSingleObject,
    THREAD_PRIORITY_ABOVE_NORMAL,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use super::cabinet_map;
use super::device::{self, DeviceKind};
use super::light_map::{self, DdrLightFrame, DIMLAMP_COUNT, TAPE_DEVICES, TAPE_LEDS};
use super::protocol::{self, CabinetLightDevice, HID_REPORT_LEN};
use crate::{log_info, log_warn};

/// Discovery/hot-plug poll cadence.
const DISCOVERY_INTERVAL: Duration = Duration::from_millis(250);
/// Minimum interval between stage-light updates (the panels update at up to
/// 30 FPS; the SDK limits below 40 Hz to avoid phase issues).
const LIGHTS_INTERVAL: Duration = Duration::from_millis(33);
/// V3 (fw < 4) masters need the '2'→'3' commands spaced apart so the master
/// finishes forwarding to the panels between host commands.
const V3_COMMAND_GAP: Duration = Duration::from_micros(16_667);
/// A command with no HOST_CMD_FINISHED response after this long is dropped
/// (lights refresh 33 ms later anyway) or retried (device info).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
/// Main loop tick.
const TICK: Duration = Duration::from_millis(1);

// ── Cross-thread state ───────────────────────────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
/// True while ≥ 1 stage device is connected with device info.
static STAGE_AVAILABLE: AtomicBool = AtomicBool::new(false);
/// Gate for the lights drain (mirrors the `output_lights` config).
static OUTPUT_LIGHTS: AtomicBool = AtomicBool::new(true);
/// Gate for the cabinet-lights half of the drain (mirrors the
/// `output_cabinet_lights` config; effective only while `OUTPUT_LIGHTS`
/// is also on).
static OUTPUT_CABINET_LIGHTS: AtomicBool = AtomicBool::new(true);
/// Static pad accent style: 0 = Gold, 1 = Platinum (deploy #21; the
/// mod-menu "Pad Style" row live-edits it).
static PAD_STYLE: AtomicBool = AtomicBool::new(false);
/// Per-pad 9-bit input masks, written by the transport thread on every
/// input report, read by the injection provider on the game thread.
static INPUT_MASKS: [AtomicU16; 2] = [AtomicU16::new(0), AtomicU16::new(0)];

/// The shared DDR light frame. Written (a few bytes at a time) by the
/// light-out detours; cloned by the 30 Hz drain. Uncontended in practice.
static DDR_FRAME: Mutex<DdrLightFrame> = Mutex::new(DdrLightFrame::new());
/// Set on any frame write; the drain only encodes when something changed
/// at least once (avoids driving all-black onto auto-lit pads at boot).
static FRAME_TOUCHED: AtomicBool = AtomicBool::new(false);

/// When true, the drain sources lights by polling the ark's internal GOLD
/// output buffers directly (see [`poll_ark_light_buffers`]) instead of the
/// export-detour-fed [`DDR_FRAME`]. Set by the mod when GOLD-cabinet force is
/// active — it captures the operator test-menu LAMP CHECK (which the ark
/// drives internally, bypassing the `arkMDX*` exports our detours hook) in
/// addition to gameplay. Falls back to `DDR_FRAME` when the singleton isn't
/// live yet.
static POLL_ARK: AtomicBool = AtomicBool::new(false);
/// Latches once the polled buffers first show any lit LED — avoids driving
/// all-black onto the pads before the ark's light system comes up.
static POLL_SEEN_NONZERO: AtomicBool = AtomicBool::new(false);

static THREAD_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

// One-shot diagnostics.
static WARN_NO_DEVICE: AtomicBool = AtomicBool::new(false);
static INFO_FIRST_LIGHTS: AtomicBool = AtomicBool::new(false);
static INFO_FIRST_CABINET_LIGHTS: AtomicBool = AtomicBool::new(false);

// ── Public API ───────────────────────────────────────────────────────

/// Start the transport thread. Idempotent. Returns false only if the
/// thread could not be spawned.
pub fn init(output_lights: bool, output_cabinet_lights: bool) -> bool {
    OUTPUT_LIGHTS.store(output_lights, Ordering::Release);
    OUTPUT_CABINET_LIGHTS.store(output_cabinet_lights, Ordering::Release);
    if RUNNING.swap(true, Ordering::AcqRel) {
        return true;
    }
    SHUTDOWN_REQUESTED.store(false, Ordering::Release);
    match std::thread::Builder::new()
        .name("smx-transport".into())
        .spawn(thread_main)
    {
        Ok(handle) => {
            if let Ok(mut slot) = THREAD_HANDLE.lock() {
                *slot = Some(handle);
            }
            log_info!("SMX transport: thread started");
            true
        }
        Err(e) => {
            RUNNING.store(false, Ordering::Release);
            log_warn!("SMX transport: failed to spawn thread: {}", e);
            false
        }
    }
}

/// Stop the transport thread and close every device. Idempotent.
pub fn shutdown() {
    if !RUNNING.load(Ordering::Acquire) {
        return;
    }
    SHUTDOWN_REQUESTED.store(true, Ordering::Release);
    let handle = THREAD_HANDLE.lock().ok().and_then(|mut slot| slot.take());
    if let Some(handle) = handle {
        let _ = handle.join();
    }
    RUNNING.store(false, Ordering::Release);
    STAGE_AVAILABLE.store(false, Ordering::Release);
    INPUT_MASKS[0].store(0, Ordering::Release);
    INPUT_MASKS[1].store(0, Ordering::Release);
    log_info!("SMX transport: shut down");
}

/// Whether ≥ 1 stage device is connected (with device info).
pub fn is_available() -> bool {
    STAGE_AVAILABLE.load(Ordering::Acquire)
}

/// The given pad's live 9-bit panel mask (0 when not connected).
#[inline]
pub fn input_mask(pad: usize) -> u16 {
    if pad < 2 {
        INPUT_MASKS[pad].load(Ordering::Acquire)
    } else {
        0
    }
}

/// Enable/disable the lights drain at runtime.
pub fn set_output_lights(enabled: bool) {
    OUTPUT_LIGHTS.store(enabled, Ordering::Release);
}

/// Enable/disable the cabinet-lights half of the drain at runtime
/// (marquee / vertical strips / spotlights; stage lights unaffected).
pub fn set_output_cabinet_lights(enabled: bool) {
    OUTPUT_CABINET_LIGHTS.store(enabled, Ordering::Release);
}

/// Select the static pad accent (false = Gold, true = Platinum). Applies
/// on the next 30 Hz lights frame.
pub fn set_pad_platinum(platinum: bool) {
    PAD_STYLE.store(platinum, Ordering::Release);
}

/// Enable/disable sourcing lights by polling the ark's internal GOLD output
/// buffers (vs the export-detour-fed frame). Enabled by the SMX mod when
/// GOLD-cabinet force is active. Resets the "seen light" latch on disable.
pub fn set_poll_ark(enabled: bool) {
    POLL_ARK.store(enabled, Ordering::Release);
    if !enabled {
        POLL_SEEN_NONZERO.store(false, Ordering::Release);
    }
}

/// Accumulate one tape LED write (from the `arkMDXChangeTapeled` detour).
/// Channel values ≥ 0x100 mean "leave unchanged" (mirrors the game's impl).
#[inline]
pub fn write_tape_led(device: usize, led: usize, r: u32, g: u32, b: u32) {
    if device >= TAPE_DEVICES || led >= TAPE_LEDS {
        return;
    }
    if let Ok(mut frame) = DDR_FRAME.lock() {
        let slot = &mut frame.tape[device][led];
        if r < 0x100 {
            slot[0] = r as u8;
        }
        if g < 0x100 {
            slot[1] = g as u8;
        }
        if b < 0x100 {
            slot[2] = b as u8;
        }
    }
    FRAME_TOUCHED.store(true, Ordering::Release);
}

/// Accumulate one dimlamp write (from the `arkMDXChangeDimlamp` detour).
#[inline]
pub fn write_dimlamp(id: usize, value: u8) {
    if id >= DIMLAMP_COUNT {
        return;
    }
    if let Ok(mut frame) = DDR_FRAME.lock() {
        frame.dimlamps[id] = value;
    }
    FRAME_TOUCHED.store(true, Ordering::Release);
}

/// Set the masked LEDs of one tape device to a single color (from the
/// `arkMDXChangeSatellite` capture — its 5th arg is a per-LED bitmask,
/// bit N = LED N; all-ones = whole-device fill). Channel values ≥ 0x100
/// mean "leave unchanged" (mirrors the ark impl's per-channel skip).
pub fn fill_tape_device_masked(device: usize, r: u32, g: u32, b: u32, mask: u64) {
    if device >= TAPE_DEVICES {
        return;
    }
    if let Ok(mut frame) = DDR_FRAME.lock() {
        for led in 0..TAPE_LEDS {
            if mask & (1u64 << led) == 0 {
                continue;
            }
            let slot = &mut frame.tape[device][led];
            if r < 0x100 {
                slot[0] = r as u8;
            }
            if g < 0x100 {
                slot[1] = g as u8;
            }
            if b < 0x100 {
                slot[2] = b as u8;
            }
        }
    }
    FRAME_TOUCHED.store(true, Ordering::Release);
}

// ── Device state ─────────────────────────────────────────────────────

/// A queued serial command (already framed into HID packets).
struct PendingCommand {
    packets: Vec<[u8; HID_REPORT_LEN]>,
    is_device_info: bool,
    is_lights: bool,
    send_at: Instant,
}

/// The command currently written to the device, awaiting its
/// HOST_CMD_FINISHED (or DEVICE_INFO) acknowledgment.
struct InFlight {
    is_device_info: bool,
    is_lights: bool,
    sent_at: Instant,
}

/// State shared between a device's dedicated reader thread and the worker.
struct ReaderShared {
    /// Assigned pad slot (0/1), or -1 until the device-info handshake
    /// completes. The reader routes input reports to `INPUT_MASKS[slot]` only
    /// once the worker sets this.
    slot: AtomicI32,
    /// Worker → reader: exit now (reap / shutdown). The reader polls it while
    /// waiting for a report and on its 200 ms wakeups.
    stop: AtomicBool,
    /// Reader → worker: an unrecoverable read error occurred; reap + rediscover.
    failed: AtomicBool,
}

/// A `HANDLE` moved into the reader thread. The file handle is valid for the
/// device's lifetime; the reader only reads, the worker only writes (distinct
/// OVERLAPPEDs), which is safe to do concurrently on one handle.
struct SendHandle(HANDLE);
// SAFETY: the handle outlives the reader thread (the worker joins it before
// closing the handle), and read/write on separate OVERLAPPEDs don't race.
unsafe impl Send for SendHandle {}

struct Device {
    path: Vec<u16>,
    handle: HANDLE,
    kind: DeviceKind,
    /// Overlapped write state for the in-flight command (worker thread only).
    write_ov: Box<OVERLAPPED>,
    /// Manual-reset event backing `write_ov` (worker thread only). Waiting on
    /// a dedicated event instead of the file handle matters twice over: the
    /// file handle is also signaled by the READER thread's completions on the
    /// same handle (spurious wakes), and the event path needs only
    /// `GetOverlappedResult` — `GetOverlappedResultEx` doesn't exist on
    /// Windows 7 and a static import of it makes the loader reject the whole
    /// DLL there (tester-caught 2026-08-31).
    write_event: HANDLE,
    /// Control commands (device info). Never replaced.
    queue: VecDeque<PendingCommand>,
    in_flight: Option<InFlight>,
    /// The lights set currently being transmitted. A STARTED set is always
    /// finished — the pads only apply an update once its final command
    /// arrives, so evicting a set mid-transmission freezes them on the last
    /// completed frame (cabinet-caught 2026-08-27: the 33 ms drain replaced
    /// queued tail commands faster than Wine's HID writes completed and the
    /// pads never saw a complete set after the first).
    lights_active: VecDeque<Vec<[u8; HID_REPORT_LEN]>>,
    /// The freshest complete lights set, staged. Each drain replaces this
    /// wholesale (latest-wins); it's promoted to `lights_active` only when
    /// the active set has fully drained.
    lights_staged: Option<Vec<Vec<[u8; HID_REPORT_LEN]>>>,
    /// Minimum gap between this device's lights-command sends (V3 masters
    /// need 1/60 s between '2' and '3'; V4+ takes zero).
    lights_gap: Duration,
    /// When the previous lights command was sent (gap pacing).
    last_lights_sent: Instant,
    /// Device info (stage only; the cabinet controller has none).
    info: Option<protocol::DeviceInfo>,
    /// Cabinet lights controller version/model (cabinet only; from the
    /// `"I\n"` handshake — selects the cabinet-lights wire protocol and
    /// gates the cabinet half of the drain).
    cabinet_info: Option<protocol::CabinetInfo>,
    /// Assigned pad slot (0/1) once device info arrives.
    player_slot: Option<usize>,
    /// Accumulates multi-packet serial responses.
    serial_buf: Vec<u8>,
    /// Shared with the reader thread (slot routing + stop/failure signaling).
    reader: Arc<ReaderShared>,
    /// The reader thread; joined on teardown before the handle is closed.
    reader_join: Option<std::thread::JoinHandle<()>>,
    /// Serial reports (id 6) forwarded from the reader for handshake /
    /// flow-control handling on the worker thread.
    serial_rx: Receiver<Vec<u8>>,
    /// Set on a worker-side (write) IO error; the device is reaped + reopened.
    failed: bool,
}

// SAFETY: Device is only ever touched by the worker thread (the reader gets
// its own Arc<ReaderShared> + Sender, never the Device itself).
unsafe impl Send for Device {}

impl Device {
    fn new(path: Vec<u16>, handle: HANDLE, kind: DeviceKind) -> Self {
        let reader = Arc::new(ReaderShared {
            slot: AtomicI32::new(-1),
            stop: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        });
        // Manual-reset event for overlapped writes (see `write_event` doc).
        // Creation failure marks the device failed so the reap cycle drops it
        // instead of ever issuing an event-less overlapped write.
        let (write_event, event_failed) =
            match unsafe { CreateEventW(None, true, false, PCWSTR::null()) } {
                Ok(e) => (e, false),
                Err(_) => {
                    log_warn!("SMX: failed to create write event; dropping device");
                    (HANDLE::default(), true)
                }
            };
        let (tx, rx) = channel::<Vec<u8>>();
        // Dedicated event-driven reader thread: input reports → INPUT_MASKS
        // with ~0 latency, serial reports → the worker over `tx`.
        let sh = SendHandle(handle);
        let reader_for_thread = reader.clone();
        let reader_join = std::thread::Builder::new()
            .name("smx-reader".into())
            .spawn(move || reader_thread(sh, reader_for_thread, tx))
            .ok();
        if reader_join.is_none() {
            log_warn!("SMX: failed to spawn reader thread; device input unavailable");
        }
        Self {
            path,
            handle,
            kind,
            write_ov: Box::new(OVERLAPPED::default()),
            write_event,
            queue: VecDeque::new(),
            in_flight: None,
            lights_active: VecDeque::new(),
            lights_staged: None,
            lights_gap: Duration::ZERO,
            last_lights_sent: Instant::now() - Duration::from_secs(1),
            info: None,
            cabinet_info: None,
            player_slot: None,
            serial_buf: Vec::new(),
            reader,
            reader_join,
            serial_rx: rx,
            failed: event_failed,
        }
    }

    /// True if either side flagged an IO failure.
    fn is_failed(&self) -> bool {
        self.failed || self.reader.failed.load(Ordering::Acquire)
    }
}

// ── Thread main ──────────────────────────────────────────────────────

fn thread_main() {
    unsafe {
        // ABOVE_NORMAL, not HIGHEST: we must not starve the game (rule 4).
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    }

    let mut devices: Vec<Device> = Vec::new();
    let mut known_paths: Vec<Vec<u16>> = Vec::new();
    let mut last_discovery = Instant::now() - DISCOVERY_INTERVAL;
    let mut last_lights = Instant::now() - LIGHTS_INTERVAL;

    while !SHUTDOWN_REQUESTED.load(Ordering::Acquire) {
        let now = Instant::now();

        // Discovery / hot-plug.
        if now.duration_since(last_discovery) >= DISCOVERY_INTERVAL {
            last_discovery = now;
            discover(&mut devices, &mut known_paths);
        }

        // Drain serial reports (forwarded by the reader threads) + progress
        // writes per device; collect failures. Input reads happen off-thread
        // in the per-device reader, so nothing here can stall input freshness.
        for dev in devices.iter_mut() {
            drain_serial(dev);
            pump_writes(dev, now);
        }
        reap_failed(&mut devices, &mut known_paths);

        // Lights drain. The two output gates are independent (deploy #20:
        // they're exposed as separate "Pad Lights" / "Cabinet Lights"
        // toggles): stage staging is gated inside drain_lights on
        // OUTPUT_LIGHTS, cabinet staging on OUTPUT_CABINET_LIGHTS.
        if now.duration_since(last_lights) >= LIGHTS_INTERVAL {
            if OUTPUT_LIGHTS.load(Ordering::Acquire)
                || OUTPUT_CABINET_LIGHTS.load(Ordering::Acquire)
            {
                if let Some(frame) = acquire_light_frame() {
                    drain_lights(&mut devices, &frame);
                    last_lights = now;
                }
            }
        }

        // Availability snapshot.
        let stage_ready = devices
            .iter()
            .any(|d| d.kind == DeviceKind::Stage && d.info.is_some());
        STAGE_AVAILABLE.store(stage_ready, Ordering::Release);

        std::thread::sleep(TICK);
    }

    // Shutdown: stop + join every reader thread, then close every handle.
    for dev in devices.iter_mut() {
        teardown_device(dev);
    }
    STAGE_AVAILABLE.store(false, Ordering::Release);
}

/// Stop a device's reader thread and release its OS handle, in an order that
/// never leaves the reader touching a closed handle: signal stop → cancel any
/// pending read (unblocks the reader's wait) → join → close.
fn teardown_device(dev: &mut Device) {
    dev.reader.stop.store(true, Ordering::Release);
    unsafe {
        let _ = CancelIoEx(dev.handle, None);
    }
    if let Some(join) = dev.reader_join.take() {
        let _ = join.join();
    }
    device::close_device(dev.handle);
    if !dev.write_event.is_invalid() {
        unsafe {
            let _ = CloseHandle(dev.write_event);
        }
    }
    if let Some(slot) = dev.player_slot {
        INPUT_MASKS[slot].store(0, Ordering::Release);
    }
}

/// Look for newly plugged (or first-seen) SMX devices.
fn discover(devices: &mut Vec<Device>, known_paths: &mut Vec<Vec<u16>>) {
    let paths = device::enumerate_hid_paths();

    // Forget known paths that disappeared (their Device will fail its IO and
    // be reaped; this just lets a re-plug on the same path be re-opened).
    known_paths.retain(|p| paths.contains(p));

    for path in paths {
        if known_paths.contains(&path) {
            continue;
        }
        known_paths.push(path.clone());
        let Some((handle, kind)) = device::try_open_smx(&path) else {
            continue;
        };
        // Device::new spawns the dedicated reader thread for this handle.
        let mut dev = Device::new(path, handle, kind);
        // Both device kinds start with the device-info handshake (the SDK's
        // Open() requests it unconditionally; safe even mid-session for
        // other apps). Stage devices use the response to pick a pad slot;
        // the cabinet controller's response triggers the "I\n" version/model
        // handshake that selects its lights wire protocol.
        dev.queue.push_back(PendingCommand {
            packets: vec![protocol::device_info_request()],
            is_device_info: true,
            is_lights: false,
            send_at: Instant::now(),
        });
        match kind {
            DeviceKind::Stage => {
                log_info!("SMX: stage device connected (awaiting device info)");
            }
            DeviceKind::Cabinet => {
                log_info!("SMX: cabinet lights controller connected (awaiting handshake)");
            }
        }
        devices.push(dev);
    }

    if devices.is_empty() && !WARN_NO_DEVICE.swap(true, Ordering::AcqRel) {
        log_warn!("SMX: no SMX devices found (will keep polling every 250 ms)");
    }
}

/// Drop failed devices (read or write error), freeing their pad slots and
/// path records after tearing down the reader thread.
fn reap_failed(devices: &mut Vec<Device>, known_paths: &mut Vec<Vec<u16>>) {
    devices.retain_mut(|dev| {
        if !dev.is_failed() {
            return true;
        }
        teardown_device(dev);
        known_paths.retain(|p| p != &dev.path);
        log_warn!("SMX: device disconnected (will rediscover)");
        false
    });
}

/// Dedicated per-device reader thread: event-driven blocking reads that wake
/// the instant a report lands. Input reports (id 3) update `INPUT_MASKS`
/// directly (≈0 latency); serial reports (id 6) are forwarded to the worker.
/// Never blocked by lights writes or the worker's tick.
fn reader_thread(sh: SendHandle, shared: Arc<ReaderShared>, serial_tx: Sender<Vec<u8>>) {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    }
    let handle = sh.0;

    // Manual-reset event for the overlapped read; reset before each op.
    let event = match unsafe { CreateEventW(None, true, false, PCWSTR::null()) } {
        Ok(e) => e,
        Err(_) => {
            shared.failed.store(true, Ordering::Release);
            return;
        }
    };

    let mut buf = [0u8; HID_REPORT_LEN];
    let mut first_input_logged = false;

    'outer: while !shared.stop.load(Ordering::Acquire) {
        let mut ov = OVERLAPPED {
            hEvent: event,
            ..Default::default()
        };
        unsafe {
            let _ = ResetEvent(event);
        }
        buf.fill(0);

        let result = unsafe { ReadFile(handle, Some(&mut buf[..]), None, Some(&mut ov)) };
        match result {
            Ok(()) => {
                // Synchronous completion — fetch the byte count and dispatch.
                let mut bytes = 0u32;
                if unsafe { GetOverlappedResult(handle, &ov, &mut bytes, false) }.is_ok() {
                    dispatch_report(
                        &buf[..bytes as usize],
                        &shared,
                        &serial_tx,
                        &mut first_input_logged,
                    );
                }
                continue;
            }
            Err(_) => {
                let err = unsafe { GetLastError() };
                if err != ERROR_IO_PENDING && err != ERROR_IO_INCOMPLETE {
                    shared.failed.store(true, Ordering::Release);
                    break;
                }
            }
        }

        // Pending: wait for the report, waking every 200 ms to check `stop`.
        loop {
            if shared.stop.load(Ordering::Acquire) {
                unsafe {
                    let _ = CancelIoEx(handle, Some(&ov));
                    let mut b = 0u32;
                    let _ = GetOverlappedResult(handle, &ov, &mut b, true);
                }
                break 'outer;
            }
            let wait = unsafe { WaitForSingleObject(event, 200) };
            if wait == WAIT_OBJECT_0 {
                let mut bytes = 0u32;
                match unsafe { GetOverlappedResult(handle, &ov, &mut bytes, false) } {
                    Ok(()) => {
                        dispatch_report(
                            &buf[..bytes as usize],
                            &shared,
                            &serial_tx,
                            &mut first_input_logged,
                        );
                    }
                    Err(_) => {
                        let e = unsafe { GetLastError() };
                        if e != ERROR_OPERATION_ABORTED {
                            shared.failed.store(true, Ordering::Release);
                        }
                        break 'outer;
                    }
                }
                break; // issue the next read
            } else if wait == WAIT_TIMEOUT {
                continue; // re-check stop
            } else {
                shared.failed.store(true, Ordering::Release);
                break 'outer;
            }
        }
    }

    unsafe {
        let _ = CloseHandle(event);
    }
}

/// Dispatch one received HID report on the reader thread (port of
/// `HandleUsbPacket`, input half). Panic-free (no unwrap / unchecked index).
fn dispatch_report(
    report: &[u8],
    shared: &ReaderShared,
    serial_tx: &Sender<Vec<u8>>,
    first_input_logged: &mut bool,
) {
    if report.is_empty() {
        return;
    }
    match report[0] {
        protocol::INPUT_REPORT_ID => {
            if let Some(mask) = protocol::parse_input_report(report) {
                let slot = shared.slot.load(Ordering::Acquire);
                if (0..2).contains(&slot) {
                    INPUT_MASKS[slot as usize].store(mask, Ordering::Release);
                    if !*first_input_logged && mask != 0 {
                        *first_input_logged = true;
                        log_info!("SMX: first input from pad {} (mask={:#05x})", slot, mask);
                    }
                }
            }
        }
        protocol::SERIAL_REPORT_ID => {
            // Forward to the worker for device-info / flow-control handling.
            let _ = serial_tx.send(report.to_vec());
        }
        _ => {}
    }
}

/// Drain the serial reports forwarded by this device's reader thread and run
/// the handshake / flow-control state machine (worker thread).
fn drain_serial(dev: &mut Device) {
    // Collect first (the recv borrows `dev.serial_rx`), then process (needs
    // `&mut dev`).
    let mut reports: Vec<Vec<u8>> = Vec::new();
    while let Ok(report) = dev.serial_rx.try_recv() {
        reports.push(report);
    }
    for report in reports {
        handle_serial_report(dev, &report);
    }
}

/// Handle one serial report (id 6): device-info response + command acks
/// (port of `HandleUsbPacket`, serial half).
fn handle_serial_report(dev: &mut Device, report: &[u8]) {
    let Some(packet) = protocol::parse_serial_report(report) else {
        return;
    };
    if packet.device_info {
        // Only meaningful if we asked (another app may have).
        let expecting = dev.in_flight.as_ref().is_some_and(|c| c.is_device_info);
        if !expecting {
            return;
        }
        dev.in_flight = None;
        match dev.kind {
            DeviceKind::Stage => {
                if let Some(info) = protocol::parse_device_info(&packet.payload) {
                    assign_player_slot(dev, info);
                }
            }
            DeviceKind::Cabinet => {
                // Connection confirmed — request the lights controller's
                // version/model (the SDK's CheckActive cabinet path). The
                // 'I' response is parsed below at end-of-command.
                dev.queue.push_back(PendingCommand {
                    packets: protocol::frame_serial_command(protocol::cabinet_info_command()),
                    is_device_info: false,
                    is_lights: false,
                    send_at: Instant::now(),
                });
            }
        }
        return;
    }
    if packet.start_of_command && !dev.serial_buf.is_empty() {
        dev.serial_buf.clear();
    }
    dev.serial_buf.extend_from_slice(&packet.payload);
    if packet.host_cmd_finished {
        // Our command completed — the device is ready for the next.
        dev.in_flight = None;
    }
    if packet.end_of_command {
        // The cabinet controller's 'I' handshake response carries the
        // version/model that selects its lights wire protocol; everything
        // else has no consumer yet ('g'/'G' config arrives with Step 3's
        // needs, if ever).
        if dev.kind == DeviceKind::Cabinet && dev.cabinet_info.is_none() {
            if let Some(info) = protocol::parse_cabinet_info(&dev.serial_buf) {
                dev.cabinet_info = Some(info);
                log_info!(
                    "SMX: cabinet lights controller ready — version {}, model {}",
                    info.version,
                    info.model
                );
            }
        }
        dev.serial_buf.clear();
    }
}

/// Record device info and pick this stage device's pad slot. Publishes the
/// slot to the reader thread so it starts routing input reports.
fn assign_player_slot(dev: &mut Device, info: protocol::DeviceInfo) {
    dev.info = Some(info);
    let preferred = usize::from(info.player2);
    // The mask slots are per-Device; a conflict can only happen with two
    // pads configured as the same player. Mirror the SDK: give the second
    // one the other slot.
    let slot = preferred; // provisional; conflict handled by caller ordering
    dev.player_slot = Some(slot);
    dev.reader.slot.store(slot as i32, Ordering::Release);
    log_info!(
        "SMX: stage device ready — P{}, firmware v{}",
        slot + 1,
        info.firmware_version
    );
}

/// Progress the write side (port of `CheckWrites` + the SDK timeout).
fn pump_writes(dev: &mut Device, now: Instant) {
    if dev.failed {
        return;
    }

    // Timeout on the in-flight command: no HOST_CMD_FINISHED in 2 s means
    // the device is wedged (or another app owns it). Recovery = mark the
    // device failed; the reaper closes it and discovery re-opens it with a
    // fresh handshake. (The SDK retries in place; fail-and-reopen reaches
    // the same healthy end state with far less machinery.)
    if let Some(cmd) = &dev.in_flight {
        if now.duration_since(cmd.sent_at) > COMMAND_TIMEOUT {
            log_warn!(
                "SMX: command timed out ({}); reconnecting device",
                if cmd.is_device_info {
                    "device info"
                } else if cmd.is_lights {
                    "lights"
                } else {
                    "handshake"
                }
            );
            dev.failed = true;
        }
        return; // one command in flight at a time
    }

    // Send the next due command. Control (device-info / cabinet handshake)
    // commands first; then the active lights set, one command per
    // HOST_CMD_FINISHED, with the device's inter-command gap. When the
    // active set is exhausted, promote the freshest staged set.
    let now_cmd: Option<(Vec<[u8; HID_REPORT_LEN]>, bool, bool)> =
        if dev.queue.front().is_some_and(|cmd| cmd.send_at <= now) {
            dev.queue
                .pop_front()
                .map(|cmd| (cmd.packets, cmd.is_device_info, cmd.is_lights))
        } else {
            if dev.lights_active.is_empty() {
                if let Some(staged) = dev.lights_staged.take() {
                    dev.lights_active.extend(staged);
                }
            }
            if !dev.lights_active.is_empty()
                && now.duration_since(dev.last_lights_sent) >= dev.lights_gap
            {
                dev.last_lights_sent = now;
                dev.lights_active
                    .pop_front()
                    .map(|packets| (packets, false, true))
            } else {
                None
            }
        };
    let Some((packets, is_device_info, is_lights)) = now_cmd else {
        return;
    };

    for packet in &packets {
        // SERIALIZED packet writes: wait for each packet's completion
        // before issuing the next. The stock SDK fires all packets
        // back-to-back on one OVERLAPPED — safe on Windows, where the HID
        // class driver strictly orders write IRPs, but Wine's hidraw path
        // gives no ordering guarantee across in-flight writes and
        // interleaves the 61-byte chunks (cabinet-caught 2026-08-27:
        // garbled/partial arrow lights while single-command data
        // survived). Waiting per packet is correct on both platforms.
        *dev.write_ov = OVERLAPPED {
            hEvent: dev.write_event,
            ..Default::default()
        };
        unsafe {
            let _ = ResetEvent(dev.write_event);
        }
        let result = unsafe {
            WriteFile(
                dev.handle,
                Some(&packet[..]),
                None,
                Some(&mut *dev.write_ov),
            )
        };
        if result.is_err() {
            let err = unsafe { GetLastError() };
            if err != ERROR_IO_PENDING && err != ERROR_IO_INCOMPLETE {
                dev.failed = true;
                return;
            }
            // Wait for this packet to land (bounded; a wedged device is
            // failed and re-opened by discovery). Event + WaitForSingleObject
            // + plain GetOverlappedResult, NOT GetOverlappedResultEx: the Ex
            // variant is Windows 8+ and its static import made the Win7
            // loader reject the entire DLL (tester-caught 2026-08-31), and
            // an event-less wait would ride the file handle, which the
            // reader thread's completions also signal.
            let wait = unsafe { WaitForSingleObject(dev.write_event, 500) };
            let landed = wait == WAIT_OBJECT_0 && {
                let mut bytes = 0u32;
                unsafe { GetOverlappedResult(dev.handle, &*dev.write_ov, &mut bytes, false) }
                    .is_ok()
            };
            if !landed {
                unsafe {
                    // Cancel just this write op — not the reader's pending
                    // read — then DRAIN it (blocking GetOverlappedResult):
                    // the kernel writes the final status into write_ov when
                    // the IRP completes, so it must stay untouched until
                    // then. The device is failed and reaped either way.
                    let _ = CancelIoEx(dev.handle, Some(&*dev.write_ov));
                    let mut b = 0u32;
                    let _ = GetOverlappedResult(dev.handle, &*dev.write_ov, &mut b, true);
                }
                log_warn!("SMX: packet write did not complete; reconnecting device");
                dev.failed = true;
                return;
            }
        }
    }
    dev.in_flight = Some(InFlight {
        is_device_info,
        is_lights,
        sent_at: now,
    });
}

/// Choose the light frame for this drain tick. In GOLD-force mode
/// ([`POLL_ARK`]) the authoritative source is the ark's internal output
/// buffers (captures gameplay AND the ark-driven test-menu LAMP CHECK); we
/// hold off until they first show light (latch) so we never drive all-black
/// onto the pads at boot. Falls back to the export-detour-fed [`DDR_FRAME`]
/// when polling is off or the singleton isn't live yet.
fn acquire_light_frame() -> Option<DdrLightFrame> {
    if POLL_ARK.load(Ordering::Acquire) {
        if let Some(frame) = poll_ark_light_buffers() {
            if frame_has_light(&frame) {
                POLL_SEEN_NONZERO.store(true, Ordering::Release);
            }
            if POLL_SEEN_NONZERO.load(Ordering::Acquire) {
                return Some(frame);
            }
            return None;
        }
        // Singleton not live yet — fall through to the detour-fed frame.
    }
    if FRAME_TOUCHED.load(Ordering::Acquire) {
        return DDR_FRAME.lock().ok().map(|f| f.clone());
    }
    None
}

/// True if any tape LED or dimlamp in the frame is lit.
fn frame_has_light(frame: &DdrLightFrame) -> bool {
    frame.dimlamps.iter().any(|&v| v != 0)
        || frame
            .tape
            .iter()
            .any(|dev| dev.iter().any(|led| *led != [0u8; 3]))
}

/// Same `(off1, off2) → (device, led)` map as the tapeled capture / spice2x
/// `DDR_TAPELEDS`: off1 0..=3 are the foot pairs (split at LED 25), off1
/// 5..=7 are the 50-LED top/monitor strips; off1 4 is unused.
#[inline]
fn tape_off_to_device(off1: usize, off2: usize) -> Option<(usize, usize)> {
    match off1 {
        0..=3 => {
            if off2 < 25 {
                Some((off1 * 2, off2))
            } else {
                Some((off1 * 2 + 1, off2 - 25))
            }
        }
        5..=7 => Some((off1 + 3, off2)),
        _ => None,
    }
}

/// Read the ark's internal GOLD light-output buffers straight off the live
/// `MdxHWIO` singleton — the exact memory the export impls write and the
/// machine-type-4 flush emits to BI2A every frame, in ALL scenes. This is
/// the source SpiceManiaX mirrored via SpiceAPI, and the only way to capture
/// the operator test-menu LAMP CHECK (the ark drives those lamps internally,
/// never calling the `arkMDX*` exports our detours hook).
///
/// Layout (Ghidra-confirmed, `arkmdxbio2_20260721`, offsets relative to the
/// singleton object):
/// - **Tape** `this + 0x153C + (off1*50 + off2)*0xC` = r (u32), `+4` = g,
///   `+8` = b (values 0..255); off1 0..7, off2 0..49.
/// - **Dimlamp** `this + 0x14C8 + id*4` = value (u32), id 0..28.
///
/// Returns None until the ark has populated the singleton. Read-only; a torn
/// RGB read is cosmetically negligible at 30 Hz (rule 3: background threads
/// may read game memory).
fn poll_ark_light_buffers() -> Option<DdrLightFrame> {
    let obj = crate::services::input_manager::io_object_addr();
    if obj == 0 {
        return None;
    }
    let mut frame = DdrLightFrame::new();
    unsafe {
        for off1 in 0..8usize {
            for off2 in 0..50usize {
                let Some((device, led)) = tape_off_to_device(off1, off2) else {
                    continue;
                };
                if device >= TAPE_DEVICES || led >= TAPE_LEDS {
                    continue;
                }
                let base = obj + 0x153C + (off1 * 50 + off2) * 0xC;
                let r = std::ptr::read_volatile(base as *const u32);
                let g = std::ptr::read_volatile((base + 4) as *const u32);
                let b = std::ptr::read_volatile((base + 8) as *const u32);
                frame.tape[device][led] = [r.min(255) as u8, g.min(255) as u8, b.min(255) as u8];
            }
        }
        for id in 0..DIMLAMP_COUNT {
            let v = std::ptr::read_volatile((obj + 0x14C8 + id * 4) as *const u32);
            frame.dimlamps[id] = v.min(255) as u8;
        }
    }
    Some(frame)
}

/// Map + encode + STAGE the given DDR frame onto every ready device: the
/// stage pads' panel grids plus the cabinet controller's marquee / vertical
/// strips / spotlights. Staging is latest-wins; transmission ("promote
/// staged → active") happens in [`pump_writes`], which never abandons a
/// started set — the pads only apply an update once the set's final command
/// arrives.
fn drain_lights(devices: &mut [Device], frame: &DdrLightFrame) {
    let style = if PAD_STYLE.load(Ordering::Acquire) {
        light_map::PadStyle::Platinum
    } else {
        light_map::PadStyle::Gold
    };
    let pads = light_map::map_stage(frame, style);
    let stage_enabled = OUTPUT_LIGHTS.load(Ordering::Acquire);
    let cabinet_enabled = OUTPUT_CABINET_LIGHTS.load(Ordering::Acquire);

    let mut sent_any = false;
    let mut sent_cabinet = false;
    for dev in devices.iter_mut() {
        match dev.kind {
            DeviceKind::Stage => {
                if !stage_enabled {
                    continue;
                }
                let Some(info) = dev.info else { continue };
                let Some(slot) = dev.player_slot else {
                    continue;
                };

                let [cmd4, cmd2, cmd3] = protocol::encode_stage_commands(&pads[slot]);
                let set: Vec<Vec<[u8; HID_REPORT_LEN]>> = if info.firmware_version >= 4 {
                    // V4+: all three commands back to back; the master paces itself.
                    dev.lights_gap = Duration::ZERO;
                    vec![
                        protocol::frame_serial_command(&cmd4),
                        protocol::frame_serial_command(&cmd2),
                        protocol::frame_serial_command(&cmd3),
                    ]
                } else {
                    // V3: no '4' command; '2' and '3' spaced one frame apart.
                    dev.lights_gap = V3_COMMAND_GAP;
                    vec![
                        protocol::frame_serial_command(&cmd2),
                        protocol::frame_serial_command(&cmd3),
                    ]
                };
                dev.lights_staged = Some(set);
                sent_any = true;
            }
            DeviceKind::Cabinet => {
                if !cabinet_enabled {
                    continue;
                }
                // The "I\n" handshake must resolve first — the model selects
                // the wire protocol ('L' vs 'Q', channel order, LED counts).
                let Some(info) = dev.cabinet_info else {
                    continue;
                };
                dev.lights_staged = Some(encode_cabinet_set(frame, info.model));
                sent_cabinet = true;
            }
        }
    }

    if sent_any && !INFO_FIRST_LIGHTS.swap(true, Ordering::AcqRel) {
        log_info!("SMX: first stage-light frame queued to pad(s)");
    }
    if sent_cabinet && !INFO_FIRST_CABINET_LIGHTS.swap(true, Ordering::AcqRel) {
        log_info!("SMX: first cabinet-light frame queued (marquee/strips/spotlights)");
    }
}

/// Build one complete cabinet-lights set — the five dedicated-cabinet
/// commands, in SpiceManiaX's send order (marquee, left strip, right strip,
/// left spotlights, right spotlights). One command transmits per
/// HOST_CMD_FINISHED, same flow control as the stage sets.
fn encode_cabinet_set(frame: &DdrLightFrame, model: u8) -> Vec<Vec<[u8; HID_REPORT_LEN]>> {
    let marquee = cabinet_map::map_marquee(&frame.tape[cabinet_map::TAPE_TOP_PANEL]);
    let left_strip = cabinet_map::map_strip(&frame.tape[cabinet_map::TAPE_MONITOR_LEFT]);
    let right_strip = cabinet_map::map_strip(&frame.tape[cabinet_map::TAPE_MONITOR_RIGHT]);
    // P1's woofer corner drives the left spotlights, P2's the right
    // (SpiceManiaX `HandleSpotlightLightsUpdate`).
    let left_spots = cabinet_map::map_spotlights(frame.dimlamps[cabinet_map::WOOFER_DIMLAMP[0]]);
    let right_spots = cabinet_map::map_spotlights(frame.dimlamps[cabinet_map::WOOFER_DIMLAMP[1]]);

    vec![
        protocol::frame_serial_command(&protocol::encode_cabinet_light(
            CabinetLightDevice::Marquee,
            model,
            &marquee,
        )),
        protocol::frame_serial_command(&protocol::encode_cabinet_light(
            CabinetLightDevice::LeftStrip,
            model,
            &left_strip,
        )),
        protocol::frame_serial_command(&protocol::encode_cabinet_light(
            CabinetLightDevice::RightStrip,
            model,
            &right_strip,
        )),
        protocol::frame_serial_command(&protocol::encode_cabinet_light(
            CabinetLightDevice::LeftSpotlights,
            model,
            &left_spots,
        )),
        protocol::frame_serial_command(&protocol::encode_cabinet_light(
            CabinetLightDevice::RightSpotlights,
            model,
            &right_spots,
        )),
    ]
}
