//! Boot-only observation of game/audio requests and gameplay-clock progression.
//! No audio calls, clock writes, packet tracing, or actor-tree traversal.
//! See `docs/audio_sync_diagnostics.md` for validity flags and limitations.

pub mod model;
pub mod spans;
pub mod xact;

use std::fmt::Write as _;
use std::io::{self, BufWriter, Write};
use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use retour::GenericDetour;

use crate::core::{hooks, memory, scanner, signatures::SignatureStore};
use crate::services::{judge_hook, scene_manager, song_rate::clock_patch};
use crate::{log_info, log_warn};
use model::*;

const CAPACITY: usize = 512;
const OUTPUT_LIMIT: u64 = 32 * 1024 * 1024;
const OUTPUT: &str = "audio-sync-diagnostics-v2.csv";
// 0..=6 are v1 channels; the engine observer owns 16..=21.
const FRAME_CHANNEL: u8 = 7;
const HIT_CHANNEL: u8 = 8;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static SUBMIT_TAP: AtomicBool = AtomicBool::new(false);

// Raw kernel32 imports keep this usable on Win7 without adding Cargo features.
#[link(name = "kernel32")]
extern "system" {
    fn QueryPerformanceCounter(value: *mut i64) -> i32;
    fn QueryPerformanceFrequency(value: *mut i64) -> i32;
    fn GetCurrentThreadId() -> u32;
}

struct Runtime {
    recorder: Recorder<CAPACITY>,
    frequency: i64,
    vtable: usize,
    frame_global: usize,
    audio_manager_global: usize,
    option_table: usize,
    option_offset: usize,
    offsets_valid: bool,
    actor_valid: bool,
    dead_valid: bool,
    /// None = unavailable; Some(0) = verified stock code; otherwise AtomicU64*.
    factor: Option<usize>,
    channels: AtomicU64,
}

type PrepareFn = unsafe extern "C" fn(i32, *const u8) -> i32;
type ReadyFn = unsafe extern "C" fn(i32) -> u8;
type StopFn = unsafe extern "C" fn(i32);
type StartFn = unsafe extern "C" fn(*mut u8, i32);
type BroadcastFn = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32);
static mut PREPARE: Option<GenericDetour<PrepareFn>> = None;
static mut READY: Option<GenericDetour<ReadyFn>> = None;
static mut STOP: Option<GenericDetour<StopFn>> = None;
static mut START: Option<GenericDetour<StartFn>> = None;
static mut BROADCAST: Option<GenericDetour<BroadcastFn>> = None;

fn qpc() -> i64 {
    let mut value = 0;
    if unsafe { QueryPerformanceCounter(&mut value) } == 0 {
        -1
    } else {
        value
    }
}

fn runtime() -> Option<&'static Runtime> {
    if ACTIVE.load(Ordering::Relaxed) {
        RUNTIME.get()
    } else {
        None
    }
}

fn active() -> bool {
    runtime().is_some()
}

fn clock() -> i64 {
    if active() {
        qpc()
    } else {
        -1
    }
}

fn capture_context() -> Context {
    runtime()
        .map(|rt| rt.recorder.context())
        .unwrap_or_default()
}

fn push(mut event: Event) {
    if let Some(rt) = runtime() {
        if event.thread_id == 0 {
            event.thread_id = unsafe { GetCurrentThreadId() };
        }
        rt.recorder.record(event);
    }
}

fn emit(event: Event) {
    push(event);
}

pub(super) fn publish_channel(bit: u8) {
    if bit < 64 {
        if let Some(rt) = RUNTIME.get() {
            rt.channels.fetch_or(1u64 << bit, Ordering::Release);
        }
    }
}

struct TraceSink;
static TRACE_SINK: TraceSink = TraceSink;
impl spans::Sink for TraceSink {
    fn clock(&self) -> i64 {
        clock()
    }
    fn context(&self) -> Context {
        capture_context()
    }
    fn thread_id(&self) -> u32 {
        unsafe { GetCurrentThreadId() }
    }
    fn push(&self, event: Event) {
        push(event);
    }
}

pub(super) fn trace_sink() -> Option<&'static dyn spans::Sink> {
    if active() {
        Some(&TRACE_SINK)
    } else {
        None
    }
}

pub fn span(scope: spans::Scope, id: u64) -> spans::Span<'static> {
    spans::Span::begin(trace_sink(), scope, id)
}

pub fn frame_observer() -> Option<spans::FrameObserver<'static>> {
    trace_sink().map(spans::FrameObserver)
}

/// The normal PUS/calibration installation owns this tap. Never install it
/// early for diagnostics: that would also enable the legacy statistics work.
pub fn submit_tap_installed() {
    SUBMIT_TAP.store(true, Ordering::Release);
    if active() {
        publish_channel(HIT_CHANNEL);
    }
}

/// Read-only, pre-mod snapshot. All missing/unattested fields remain blank.
pub fn record_hit(actor: *mut u8, result: *mut u8, opcode: u32, scratch: *mut u8, entry: Event) {
    if entry.trace_id == 0 || !active() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _sampling = span(spans::Scope::HitSample, 0);
        let Some(rt) = runtime() else { return };
        if opcode.wrapping_sub(0x1028) > 6 {
            return;
        }
        let actor_known = rt.actor_valid && read::<usize>(actor as usize, 0) == Some(rt.vtable);
        let dead = if actor_known && rt.dead_valid {
            read::<u8>(actor as usize, 0x1e8).map(|b| b != 0)
        } else {
            None
        };
        let expected = read::<usize>(result as usize, 0).and_then(|note| read::<i32>(note, 8));
        let error = if opcode != 0x102e {
            read::<i32>(scratch as usize, 4)
        } else {
            None
        };
        let Some(hit) = spans::hit_event(
            opcode,
            error,
            expected,
            spans::incoming_count(actor as usize),
            dead,
            None,
        ) else {
            return;
        };
        let side = if actor_known {
            read::<i32>(actor as usize, 0x84).filter(|s| (0..=1).contains(s))
        } else {
            None
        };
        push(Event {
            kind: hit.kind,
            id: hit.id,
            detail_valid: hit.detail_valid,
            detail: hit.detail,
            qpc: entry.qpc,
            end_qpc: 0,
            side: side.unwrap_or(-1),
            sample: Sample {
                actor: actor as u64,
                ..Sample::default()
            },
            ..entry
        });
    }));
}

/// Existing wave-bank hook callout, deliberately independent of rate ever_armed.
pub fn record_bank(created: bool, file_id: i32, result: u8, path: u32) {
    if runtime().is_none() {
        return;
    }
    emit(Event {
        kind: if created {
            Kind::BankCreate
        } else {
            Kind::BankUnregister
        },
        qpc: qpc(),
        id: file_id,
        result: result as i32,
        counters: [path as u64, 0, 0, 0],
        ..Event::default()
    });
}

/// Deterministic audio clock arm evidence (clock design §7). One record per
/// arm, emitted by `audio_clock::game` on the game thread when the sanity
/// gate passes. `detail0..7` = `F0, W_k0, P_k0, Wc_k0, t_k0, lead_frames,
/// margin_frames, delta_vs_stock_micro_ms`; `counters` = `[fit_n,
/// fit_resid_sd_micro_ms, C_micro_ms, voice_generation]`; `id` = output Hz;
/// `result` = content offset (wall ms); `side` = −1 (cabinet-wide clock).
#[allow(clippy::too_many_arguments)]
pub fn record_onset(
    generation: u64,
    hz: u32,
    f0: i64,
    w: i64,
    p: i64,
    wc: i64,
    t_k: i64,
    delta_vs_stock_ms: f64,
    c_ms: f64,
    offset_ms: i32,
    fit_n: usize,
    fit_resid_sd_ms: f64,
) {
    if runtime().is_none() {
        return;
    }
    emit(Event {
        kind: Kind::Onset,
        qpc: qpc(),
        end_qpc: t_k,
        id: hz as i32,
        side: -1,
        result: offset_ms,
        detail_valid: 0xff,
        detail: [
            f0,
            w,
            p,
            wc,
            t_k,
            w - wc,
            wc - p,
            (delta_vs_stock_ms * 1000.0) as i64,
        ],
        counters: [
            fit_n as u64,
            (fit_resid_sd_ms * 1000.0).max(0.0) as u64,
            (c_ms * 1000.0).max(0.0) as u64,
            generation,
        ],
        ..Event::default()
    });
}

/// Call once at the real frame boundary, not from a widget-wrapper callback.
/// Counters = [frame sequence, poll count, jobs executed, queued jobs].
pub fn record_frame(counters: [u64; 4], entry: Event) {
    if entry.trace_id == 0 || runtime().is_none() {
        return;
    }
    let mut sampling = span(spans::Scope::FrameSample, 0);
    sampling.set_parent(entry.trace_id);
    emit(Event {
        kind: Kind::Frame,
        id: 0,
        counters,
        ..entry
    });
}

fn read<T: Copy>(base: usize, offset: usize) -> Option<T> {
    let address = base.checked_add(offset)? as *const u8;
    if base == 0 || !memory::is_readable(address, std::mem::size_of::<T>()) {
        return None;
    }
    Some(unsafe { std::ptr::read_unaligned(address.cast::<T>()) })
}

fn bytes_equal(address: usize, expected: &[u8]) -> bool {
    memory::is_readable(address as *const u8, expected.len())
        && unsafe { std::slice::from_raw_parts(address as *const u8, expected.len()) == expected }
}

fn sample(actor: *mut u8, music_count: i32) -> Option<(i32, Sample)> {
    let rt = runtime()?;
    if !rt.actor_valid || read::<usize>(actor as usize, 0)? != rt.vtable {
        return None;
    }
    let side = read::<i32>(actor as usize, 0x84)?;
    if !(0..=1).contains(&side) {
        return None;
    }
    let mut sample = Sample {
        valid: VALID_ACTOR,
        actor: actor as u64,
        music_count,
        anchor: read(actor as usize, 0x160)?,
        raw_count: read(actor as usize, 0x178)?,
        ..Sample::default()
    };
    if sample.anchor != 0 {
        sample.valid |= VALID_ANCHOR;
    }
    // Recorder additionally requires a matching observed 0x1044 in this scene.
    if let Some(tick) = read::<usize>(rt.frame_global, 0).and_then(|object| read(object, 0x1268)) {
        sample.frame_tick = tick;
        sample.valid |= VALID_TICK;
    }
    if rt.offsets_valid {
        if let (Some(sound), Some(input), Some(render), Some(bomb)) = (
            read(actor as usize, 0x16c),
            read(actor as usize, 0x170),
            read(actor as usize, 0x184),
            read(actor as usize, 0x188),
        ) {
            sample.offsets = [sound, input, render, bomb, 0];
            sample.valid |= VALID_OFFSETS;
        }
    }
    // No virtual call just to sample: validate the getter's literal body first.
    if rt.option_offset != 0 {
        let option = read::<usize>(rt.option_table, side as usize * 8)
            .and_then(|holder| read::<usize>(holder, 0))
            .and_then(|context| context.checked_add(rt.option_offset));
        if let Some(option) = option {
            let getter = read::<usize>(option, 0).and_then(|vt| read::<usize>(vt, 0x248));
            if getter.is_some_and(|p| bytes_equal(p, &[0x8b, 0x41, 0x24, 0xc3])) {
                if let (Some(value), Some(offset)) = (read(option, 0x24), sample.offsets.last_mut())
                {
                    *offset = value;
                    sample.valid |= VALID_OPTION;
                }
            }
        }
    }
    if let Some(factor) = rt.factor {
        sample.rate_q31 = if factor == 0 {
            clock_patch::IDENTITY_Q31
        } else {
            unsafe { &*(factor as *const AtomicU64) }.load(Ordering::Acquire)
        };
        sample.valid |= VALID_RATE;
    }
    Some((side, sample))
}

pub fn judge_tail(actor: *mut u8, music_count: i32, entry: Event) {
    if entry.trace_id == 0 || !active() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut sampling = span(spans::Scope::JudgeSample, 0);
        sampling.set_parent(entry.trace_id);
        if let Some((side, sample)) = sample(actor, music_count) {
            emit(Event {
                kind: Kind::GameplaySample,
                qpc: sampling.event().qpc,
                id: 0,
                trace_id: sampling.id(),
                parent_id: entry.trace_id,
                detail_valid: 0,
                side,
                sample,
                ..entry
            });
        }
    }));
}

fn qpc_if_active() -> i64 {
    clock()
}

unsafe extern "C" fn prepare_hook(slot: i32, name: *const u8) -> i32 {
    let Some(hook) = (&*addr_of!(PREPARE)).as_ref() else {
        return -1;
    };
    let now = qpc_if_active();
    let context = if now >= 0 {
        capture_context()
    } else {
        Context::default()
    };
    let mut cue = [0; 32];
    if now >= 0 && memory::is_readable(name, cue.len()) {
        for (i, byte) in cue.iter_mut().enumerate() {
            let value = *name.add(i);
            if value == 0 {
                break;
            }
            *byte = value;
        }
    }
    call_observed(
        (slot, name),
        |(slot, name)| hook.call(slot, name),
        |_, result| {
            if now >= 0 {
                let end_qpc = clock();
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    xact::on_prepare(slot, *result, cue, context)
                }));
                emit(Event {
                    kind: Kind::PrepareRequest,
                    qpc: now,
                    end_qpc,
                    id: *result,
                    result: slot,
                    name: cue,
                    origin_attempt: context.attempt,
                    origin_scene: context.scene,
                    origin_known: context.valid,
                    ..Event::default()
                });
            }
        },
    )
}

unsafe extern "C" fn ready_hook(handle: i32) -> u8 {
    let Some(hook) = (&*addr_of!(READY)).as_ref() else {
        return 0;
    };
    call_observed(
        handle,
        |handle| hook.call(handle),
        |_, result| {
            if runtime().is_some() {
                emit(Event {
                    kind: Kind::ReadyObserved,
                    qpc: qpc(),
                    id: handle,
                    result: *result as i32,
                    ..Event::default()
                });
            }
        },
    )
}

unsafe extern "C" fn stop_hook(handle: i32) {
    let Some(hook) = (&*addr_of!(STOP)).as_ref() else {
        return;
    };
    let now = qpc_if_active();
    if now >= 0 {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| xact::on_stop(handle)));
    }
    call_observed(
        handle,
        |handle| hook.call(handle),
        |_, _| {
            if now >= 0 {
                emit(Event {
                    kind: Kind::StopRequest,
                    qpc: now,
                    end_qpc: qpc(),
                    id: handle,
                    ..Event::default()
                });
            }
        },
    );
}

unsafe extern "C" fn start_hook(manager: *mut u8, handle: i32) {
    let Some(hook) = (&*addr_of!(START)).as_ref() else {
        return;
    };
    let now = qpc_if_active();
    if now >= 0 {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            xact::on_start(manager, handle)
        }));
    }
    call_observed(
        (manager, handle),
        |(manager, handle)| hook.call(manager, handle),
        |_, _| {
            if now >= 0 {
                emit(Event {
                    kind: Kind::StartRequest,
                    qpc: now,
                    end_qpc: qpc(),
                    id: handle,
                    ..Event::default()
                });
            }
        },
    );
}

unsafe extern "C" fn broadcast_hook(actor: *mut u8, msg: i32, payload: *mut u8, depth: i32) {
    let Some(hook) = (&*addr_of!(BROADCAST)).as_ref() else {
        return;
    };
    if msg != 0x1044 || runtime().is_none() {
        hook.call(actor, msg, payload, depth);
        return;
    }
    let now = qpc_if_active();
    call_observed(
        (actor, msg, payload, depth),
        |(a, m, p, d)| hook.call(a, m, p, d),
        |_, _| {
            if now < 0 {
                return;
            }
            let Some((side, sample)) = sample(actor, 0) else {
                return;
            };
            let requested = read::<i64>(payload as usize, 0);
            let unsuppressed = read::<u8>(actor as usize, 0x20).is_some_and(|f| f & 0x20 == 0);
            emit(Event {
                kind: Kind::AnchorDelivered,
                qpc: now,
                end_qpc: qpc(),
                side,
                result: i32::from(unsuppressed && requested == Some(sample.anchor)),
                sample,
                counters: [requested.unwrap_or(0) as u64, 0, 0, 0],
                ..Event::default()
            });
        },
    );
}

fn unique(signatures: &SignatureStore, name: &str) -> Option<*const u8> {
    if signatures.get_all_matches(name).len() != 1 {
        return None;
    }
    signatures.get_address(name)
}

fn resolve_factor(signatures: &SignatureStore) -> Option<usize> {
    let patch = signatures.get_address("song_rate_clock_patch")? as usize;
    if bytes_equal(patch, &clock_patch::CLOCK_PATCH_BYTES) {
        return Some(0);
    }
    if !clock_patch::is_installed()
        || !bytes_equal(patch, &[0xe9])
        || !memory::is_readable(patch as *const u8, 5)
    {
        return None;
    }
    let stub = unsafe { scanner::decode_rip_relative((patch + 1) as *const u8) } as usize;
    // Compare the actual generator, not a second hand-maintained stub layout
    // (including the audio-clock call-out the installed stub was built with).
    let expected = clock_patch::build_clock_stub_with_callout(
        stub,
        patch + 8,
        clock_patch::installed_callout(),
    )
    .ok()?;
    if !bytes_equal(stub, expected.bytes.get(..expected.factor_offset)?) {
        return None;
    }
    let factor = stub.checked_add(expected.factor_offset)?;
    if factor % std::mem::align_of::<AtomicU64>() != 0
        || !memory::is_readable(factor as *const u8, 8)
    {
        return None;
    }
    Some(factor)
}

/// Call after config, derived signatures, scene/judge services and song_rate init.
/// Disabled config exits BEFORE any diagnostic allocation, subscription or hook.
pub fn init(signatures: &SignatureStore) -> bool {
    if !crate::mods::config::get()
        .and_then(|c| c.diagnostics.as_ref())
        .is_some_and(|d| d.audio_sync)
    {
        return false;
    }
    if RUNTIME.get().is_some() {
        return ACTIVE.load(Ordering::Relaxed);
    }
    let mut frequency = 0;
    if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
        log_warn!("audio_sync_diag: QPC frequency unavailable; disabled");
        return false;
    }
    let address = |name| signatures.get_address(name).map_or(0, |p| p as usize);
    let frame_global = address("frame_tick_global");
    let anchor = address("song_rate_clock_anchor");
    let prefix = anchor.checked_sub(26).unwrap_or(0);
    let clock_layout = bytes_equal(prefix, &[0x48, 0x8b, 0x05])
        && bytes_equal(
            prefix + 7,
            &[
                0x48, 0x8b, 0x98, 0x68, 0x12, 0, 0, 0x2b, 0x99, 0x6c, 1, 0, 0, 0x2b, 0x99, 0x60, 1,
                0, 0,
            ],
        )
        && unsafe { scanner::decode_rip_relative((prefix + 3) as *const u8) } as usize
            == frame_global;
    let vtable = address("gameplay_actor_vtable");
    let update = read::<usize>(vtable, 0x30).unwrap_or(0);
    let raw_store = unique(signatures, "audio_sync_raw_count_store").map_or(0, |p| p as usize);
    let actor_valid = clock_layout
        && vtable != 0
        && update != 0
        && anchor >= update
        && anchor < update.saturating_add(0x2000)
        && raw_store > anchor
        && raw_store < update.saturating_add(0x2000)
        && signatures.get_address("judge_rebuild_anchor").is_some();
    let offset_site = unique(signatures, "audio_sync_offset_layout")
        .map(|p| (p, 19, 10))
        .or_else(|| unique(signatures, "audio_sync_offset_layout_v1").map(|p| (p, 20, 11)));
    let offsets_valid = offset_site.is_some_and(|(site, stride, disp)| {
        [
            b"SOUND_OFFSET\0".as_slice(),
            b"INPUT_OFFSET\0",
            b"RENDER_OFFSET\0",
            b"BOMB_FRAME_OFFSET\0",
        ]
        .iter()
        .enumerate()
        .all(|(i, key)| {
            let name = unsafe { scanner::decode_rip_relative(site.add(i * stride + disp)) };
            bytes_equal(name as usize, key)
        })
    });
    let scene_available = scene_manager::is_available();
    let _ = RUNTIME.set(Runtime {
        recorder: Recorder::new(
            frequency,
            if scene_available {
                scene_manager::current_scene()
            } else {
                -1
            },
        ),
        frequency,
        vtable,
        frame_global,
        audio_manager_global: address("audio_manager_global"),
        option_table: address("player_option_table"),
        option_offset: signatures.player_option_offset().unwrap_or(0),
        offsets_valid,
        actor_valid,
        dead_valid: unique(signatures, "judge_submit")
            .is_some_and(|p| bytes_equal(p as usize + 41, &[0x0f, 0xb6, 0x89, 0xe8, 1, 0, 0])),
        factor: resolve_factor(signatures),
        channels: AtomicU64::new(0),
    });
    ACTIVE.store(true, Ordering::Release);
    let Some(rt) = runtime() else {
        return false;
    };
    let mut channels = 0u64;
    macro_rules! install {
        ($name:literal, $slot:ident, $ty:ty, $callback:ident, $bit:expr) => {
            if let Some(target) = unique(signatures, $name) {
                match unsafe {
                    hooks::install_enabled(
                        addr_of_mut!($slot),
                        std::mem::transmute::<*const u8, $ty>(target),
                        $callback,
                    )
                } {
                    Ok(()) => channels |= 1 << $bit,
                    Err(error) => log_warn!("audio_sync_diag: {} unavailable: {}", $name, error),
                }
            } else {
                log_warn!("audio_sync_diag: {} unavailable/non-unique", $name);
            }
        };
    }
    install!("song_play_by_bank", PREPARE, PrepareFn, prepare_hook, 0);
    install!("song_is_prepared", READY, ReadyFn, ready_hook, 1);
    install!("song_stop_by_handle", STOP, StopFn, stop_hook, 2);
    install!("audio_start_prepared", START, StartFn, start_hook, 3);
    install!(
        "update_broadcast",
        BROADCAST,
        BroadcastFn,
        broadcast_hook,
        4
    );
    if actor_valid && judge_hook::is_available() {
        channels |= 1 << 5;
    }
    if crate::services::widget_renderer::frame_dispatch_available() {
        channels |= 1 << FRAME_CHANNEL;
    }
    if SUBMIT_TAP.load(Ordering::Acquire) {
        channels |= 1 << HIT_CHANNEL;
    }
    if scene_available {
        scene_manager::on_scene_change(Box::new(|previous, scene| {
            if runtime().is_some() {
                emit(Event {
                    kind: Kind::Scene,
                    qpc: qpc(),
                    scene,
                    id: previous,
                    ..Event::default()
                });
            }
        }));
        channels |= 1 << 6;
    }
    channels |= xact::channel_bits();
    rt.channels.fetch_or(channels, Ordering::Release);
    log_info!("audio_sync_diag: channels=0x{:X} actor_layout={} offsets={} rate={} QPC={} cap={} bytes; no audible_start channel",
        channels, actor_valid, offsets_valid, rt.factor.is_some(), frequency, OUTPUT_LIMIT);
    if let Err(error) = std::thread::Builder::new().name("audio-sync-diag".into()).spawn(|| {
        let result = std::panic::catch_unwind(writer);
        ACTIVE.store(false, Ordering::Release);
        if let Some(rt) = RUNTIME.get() {
            let (full, busy) = rt.recorder.drops();
            let loss = rt.recorder.losses();
            log_info!("audio_sync_diag: stopped; full={} contention={} suppressed_examples={} span_invocations={} span_contention={} invalid_spans={} sample_capacity={} context_contention={}",
                full, busy, loss.suppressed_examples, loss.span_invocations, loss.span_contention,
                loss.invalid_spans, loss.sample_capacity, loss.context_contention);
        }
        match result {
            Ok(Ok(())) => {
                let drops = RUNTIME.get().map(|rt| rt.recorder.drops()).unwrap_or_default();
                log_info!("audio_sync_diag: output cap reached; recording stopped; full_drops={} contention_drops={}", drops.0, drops.1);
            },
            Ok(Err(error)) => log_warn!("audio_sync_diag: writer failed; recording stopped: {}", error),
            Err(_) => log_warn!("audio_sync_diag: writer panicked; recording stopped"),
        }
    }) {
        ACTIVE.store(false, Ordering::Release);
        log_warn!("audio_sync_diag: writer thread unavailable: {}", error);
        return false;
    }
    true
}

fn writer() -> io::Result<()> {
    let Some(rt) = RUNTIME.get() else {
        return Ok(());
    };
    let mut writer = CappedWriter::new(
        BufWriter::with_capacity(32 * 1024, std::fs::File::create(OUTPUT)?),
        OUTPUT_LIMIT,
    );
    let header = format!("# audio-sync/v2 qpc_frequency={} channels={} actor_layout={} offsets={} rate={} dead_layout={} slow_us=250 scope_examples_per_second=4 total_examples_per_second=64 reserved_events=128\n{}\n",
        rt.frequency, rt.channels.load(Ordering::Acquire) | xact::channel_bits(), rt.actor_valid, rt.offsets_valid, rt.factor.is_some(), rt.dead_valid, CSV_COLUMNS);
    writer.write_record(header.as_bytes())?;
    let mut batch = [Event::default(); 256];
    let mut line = String::with_capacity(2048);
    let mut previous: [Option<Event>; 2] = [None; 2];
    let mut summary = qpc();
    let mut span_summary = summary;
    loop {
        let count = rt.recorder.drain(&mut batch);
        for event in batch.iter().take(count) {
            format_event(event, rt.frequency, &mut previous, &mut line);
            if !writer.write_record(line.as_bytes())? {
                writer.inner.flush()?;
                return Ok(());
            }
        }
        let now = qpc();
        if elapsed_ms(span_summary, now, rt.frequency).is_some_and(|ms| ms >= 1000.0) {
            let count = rt.recorder.summaries(&mut batch);
            for event in batch.iter().take(count) {
                format_event(event, rt.frequency, &mut previous, &mut line);
                if !writer.write_record(line.as_bytes())? {
                    writer.inner.flush()?;
                    return Ok(());
                }
            }
            span_summary = now;
        }
        if elapsed_ms(summary, now, rt.frequency).is_some_and(|ms| ms >= 10_000.0) {
            let (full, busy) = rt.recorder.drops();
            let loss = rt.recorder.losses();
            line.clear();
            let _ = writeln!(
                line,
                "# loss qpc={} full={} contention={} bytes={} suppressed_examples={} span_invocations={} span_contention={} invalid_spans={} sample_capacity={} context_contention={} channels={}",
                now, full, busy, writer.bytes, loss.suppressed_examples, loss.span_invocations,
                loss.span_contention, loss.invalid_spans, loss.sample_capacity, loss.context_contention,
                rt.channels.load(Ordering::Acquire) | xact::channel_bits()
            );
            if !writer.write_record(line.as_bytes())? {
                writer.inner.flush()?;
                return Ok(());
            }
            log_info!(
                "audio_sync_diag: bytes={} full_drops={} contention_drops={}",
                writer.bytes,
                full,
                busy
            );
            summary = now;
        }
        writer.inner.flush()?;
        std::thread::sleep(Duration::from_millis(250));
    }
}
