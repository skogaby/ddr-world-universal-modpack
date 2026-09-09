//! Passive, boot-only XACT diagnostics. No additional engine/device calls.
//!
//! Detail payloads (bits 0..7 mean the corresponding cell is available):
//! scheduled:    sound,event,wave,cue,bank,deadline_ms,pump_ms,event_start_target
//! voice_start:  wave,event,voice,Start_target,cue,sound,flags,branch
//!               branch: -1 unknown, 0 skipped, 1 Start succeeded, 2 Start failed.
//!               QPC pair encloses Start; it is not an exact call-site/DAC stamp.
//! sound_stop:   sound,cue,bank,immediate,0,0,0,0
//! cue_destroyed:cue,sound,bank,0,0,0,0,0 (void destructor, result is not HRESULT)
//! output_cursor:backend,DSbuffer,play,write,accumulated_play_bytes,ring_bytes,Hz,align
//!               bit 8: continuous since last emitted sample; never per-song time.
//! engine_status:channels,correlation_epoch,critical_update_losses,0,0,0,0,0; result = Status below.
//! scheduled/voice_start QPC pairs bracket original execution, excluding the
//! pre-call ownership probe; counter0 is that probe's ticks iff counter3 bit0.
//!
//! All ownership offsets are supported only behind xact_sites' PE/code identity
//! gate. Live reads occur inside the engine's serialized call paths, not on the
//! CSV writer. Unknown types/ownership stay explicitly unmatched (trace_id=0).
//!
//! Shared seams (one detour per target): the deterministic audio clock
//! (`services::audio_clock`) subscribes to the cursor hook (every mix pass),
//! the streaming submission hook (song-voice identity) and the stop/destroy
//! hooks, and asks this module to install two more engine detours on its
//! behalf — the source-node produce (`0x43CAC0`) and the in-memory wave
//! submission (`0x419DB0`). The factory bootstrap therefore runs when EITHER
//! the diagnostics are on or the `gameplay-timing-fixes` mod is enabled in
//! config; the CSV recording stays gated on the diagnostics alone.

#[path = "xact_model.rs"]
mod xact_model;
#[path = "xact_sites.rs"]
mod xact_sites;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::GenericDetour;
use windows::core::PCSTR;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleA, GetModuleHandleExA, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
};
use windows::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
use windows::Win32::System::Threading::GetCurrentProcess;

use super::model::{Context, Event, Kind};
use crate::core::{memory, module_resolver::GameModule};
use crate::services::audio_clock;
use crate::{log_info, log_warn};
use xact_model::{
    observed, observed_timed, Correlation, Cursor, CursorSampler, Origin, Owners, Token,
};
use xact_sites::Sites;

const FACTORY_BIT: u8 = 16;
const ENGINE_BITS: u64 = (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20) | (1 << 21);
const ENGINE_NAME: PCSTR = PCSTR(b"xactengine2_10.dll\0".as_ptr());

#[repr(u32)]
enum Status {
    Unavailable = 0,
    Armed = 1,
    Installed = 2,
    MissedWindow = 3,
    Unsupported = 4,
    InstallFailed = 5,
    FactoryUnavailable = 6,
    ManagerUnavailable = 7,
}

type FactoryFn = unsafe extern "C" fn(*mut *mut u8);
type ScheduleFn = unsafe extern "C" fn(*mut u8, *mut u8);
type SubmitFn = unsafe extern "C" fn(*mut u8, i32, *const f32) -> i32;
type SoundStopFn = unsafe extern "C" fn(*mut u8, i32) -> i32;
type DestroyFn = unsafe extern "C" fn(*mut u8);
type CursorFn = unsafe extern "C" fn(*mut u8) -> i32;
/// Source-node per-pass produce (`this` = node); the return value is
/// forwarded untouched.
type ProduceFn = unsafe extern "C" fn(*mut u8) -> u64;

static FACTORY: OnceLock<GenericDetour<FactoryFn>> = OnceLock::new();
static ENGINE: OnceLock<Engine> = OnceLock::new();
static MAP: OnceLock<Correlation> = OnceLock::new();
static INIT: AtomicBool = AtomicBool::new(false);
static ENTERED: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);
static CHANNELS: AtomicU64 = AtomicU64::new(0);
static STATUS: AtomicU32 = AtomicU32::new(Status::Unavailable as u32);
static REPORTED: AtomicU32 = AtomicU32::new(u32::MAX);
static REPORTED_LOST: AtomicU64 = AtomicU64::new(0);

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
    fn QueryPerformanceFrequency(out: *mut i64) -> i32;
    fn QueryPerformanceCounter(out: *mut i64) -> i32;
}

fn qpc() -> i64 {
    let mut value = 0;
    if unsafe { QueryPerformanceCounter(&mut value) } == 0 {
        -1
    } else {
        value
    }
}

fn diag_enabled_in_config() -> bool {
    crate::mods::config::get()
        .and_then(|c| c.diagnostics.as_ref())
        .is_some_and(|d| d.audio_sync)
}

struct Engine {
    base: usize,
    sites: Sites,
    // A GetModuleHandleEx reference is intentionally retained with these detours.
    _module: usize,
    frequency: i64,
    schedule: GenericDetour<ScheduleFn>,
    submit: GenericDetour<SubmitFn>,
    stop: GenericDetour<SoundStopFn>,
    destroy: GenericDetour<DestroyFn>,
    cursor: GenericDetour<CursorFn>,
    cursor_state: Mutex<CursorSampler>,
    cursor_lost: AtomicBool,
    /// Audio-clock-only detours (installed iff the mod is enabled in config).
    produce: Option<GenericDetour<ProduceFn>>,
    memory_submit: Option<GenericDetour<SubmitFn>>,
}

fn read<T: Copy>(base: usize, offset: usize) -> Option<T> {
    if base == 0 {
        return None;
    }
    let p = base.checked_add(offset)? as *const u8;
    if !memory::is_readable(p, std::mem::size_of::<T>()) {
        return None;
    }
    // Caller owns a live engine call, not a retained pointer on another thread.
    Some(unsafe { p.cast::<T>().read_unaligned() })
}

fn pointer(address: usize) -> Option<usize> {
    read(address, 0)
}

fn recording() -> bool {
    READY.load(Ordering::Acquire) && super::active()
}

pub fn channel_bits() -> u64 {
    CHANNELS.load(Ordering::Acquire)
}

fn channels(bits: u64) {
    CHANNELS.fetch_or(bits, Ordering::Release);
    for bit in FACTORY_BIT..=21 {
        if bits & (1 << bit) != 0 {
            super::publish_channel(bit);
        }
    }
}

fn event(kind: Kind, now: i64, token: Option<Token>) -> Event {
    let mut event = Event {
        kind,
        qpc: now,
        id: -1,
        side: -1,
        origin_scene: -1,
        thread_id: unsafe { GetCurrentThreadId() },
        ..Event::default()
    };
    if let Some(token) = token.filter(|t| MAP.get().is_some_and(|m| m.current(*t))) {
        event.trace_id = token.generation;
        event.origin_attempt = token.origin.attempt;
        event.origin_scene = token.origin.scene;
        event.origin_known = token.origin.valid;
        event.id = token.handle;
        event.name = token.name;
    }
    event
}

fn report_status() {
    if !super::active() {
        return;
    }
    let status = STATUS.load(Ordering::Acquire);
    let previous = REPORTED.swap(status, Ordering::Relaxed);
    let lost = MAP.get().map_or(0, Correlation::lost);
    if REPORTED_LOST.swap(lost, Ordering::Relaxed) == lost && previous == status {
        return;
    }
    let mut value = event(Kind::EngineStatus, super::clock(), None);
    value.result = status as i32;
    value.detail[0] = channel_bits() as i64;
    value.detail_valid = 1;
    if let Some(map) = MAP.get() {
        value.detail[1] = map.epoch() as i64;
        value.detail[2] = lost as i64;
        value.detail_valid |= 6;
    }
    super::push(value);
}

pub fn on_prepare(slot: i32, handle: i32, name: [u8; 32], context: Context) {
    if !super::active() {
        return;
    }
    if STATUS.load(Ordering::Acquire) == Status::Armed as u32 {
        // A prepare proves Initialize already happened. Never scan/install here.
        STATUS.store(Status::MissedWindow as u32, Ordering::Release);
    }
    report_status();
    if recording() {
        if let (Some(map), Some(engine)) = (MAP.get(), ENGINE.get()) {
            // Bind the stable cue immediately. Waiting until Start leaves a
            // multi-second unbound interval vulnerable to unrelated cue cleanup.
            let owners = super::runtime()
                .and_then(|rt| read::<usize>(rt.audio_manager_global, 0))
                .and_then(|manager| cue_owners(engine, manager, handle));
            map.prepare_bound(
                slot,
                handle,
                name,
                Origin {
                    attempt: context.attempt,
                    scene: context.scene,
                    epoch: context.epoch,
                    valid: context.valid,
                },
                owners,
            );
        }
    }
}

fn cue_owners(engine: &Engine, manager: usize, handle: i32) -> Option<Owners> {
    if !(0..256).contains(&handle) {
        return None;
    }
    // manager_valid pins LEA(handle+5), SHL 5, cue+0 and prepared+0x10.
    // The game owns the cue here, but has not taken the engine lock. Its bank
    // and interface are stable; defer mutable sound/track reads to engine hooks.
    let owners = read::<usize>(manager, 0xa0 + handle as usize * 0x20).and_then(|cue| {
        let vt = read::<usize>(cue, 0)?;
        if read::<usize>(vt, 0)? != engine.base + engine.sites.cue_play {
            return None;
        }
        Some(Owners {
            cue,
            bank: read(cue, 0x240)?,
            sound: 0,
        })
    });
    owners.filter(|o| read::<usize>(manager, 0xa0 + handle as usize * 0x20) == Some(o.cue))
}

pub fn on_start(manager: *mut u8, handle: i32) {
    if !recording() || !(0..256).contains(&handle) {
        return;
    }
    let (Some(engine), Some(map)) = (ENGINE.get(), MAP.get()) else {
        return;
    };
    if let Some(owners) = cue_owners(engine, manager as usize, handle) {
        map.bind(handle, owners);
        return;
    }
    map.stop_handle(handle);
}

pub fn on_stop(handle: i32) {
    if super::active() {
        if let Some(map) = MAP.get() {
            map.stop_handle(handle);
        }
    }
}

pub fn on_bank_unregister() {
    if super::active() {
        if let Some(map) = MAP.get() {
            map.invalidate_all();
        }
    }
}

unsafe extern "C" fn schedule_hook(sound: *mut u8, node: *mut u8) {
    let Some(engine) = ENGINE.get() else {
        return;
    };
    if !recording() {
        engine.schedule.call(sound, node);
        return;
    }
    observed_timed(
        super::clock,
        |now| {
            // The scheduler also processes frequent automation; emit wave nodes only.
            let vt = read::<usize>(node as usize, 0)?;
            if read::<usize>(vt, 8)? != engine.base + engine.sites.event_start
                || read::<usize>(vt, 0x80)? != engine.base + engine.sites.event_getter
            {
                return None;
            }
            let owners =
                xact_sites::event_owners(node as usize, engine.base, &engine.sites, &pointer)
                    .filter(|o| o.sound == sound as usize);
            let token = owners.and_then(|o| MAP.get()?.lookup(o));
            let mut value = event(Kind::Scheduled, now, token);
            value.detail[0] = sound as i64;
            value.detail[1] = node as i64;
            value.detail[7] = (engine.base + engine.sites.event_start) as i64;
            value.detail_valid = 0x83;
            if let Some(wave) = read::<usize>(node as usize, 0x60) {
                value.detail[2] = wave as i64;
                value.detail_valid |= 4;
            }
            if let Some(o) = owners {
                value.detail[3] = o.cue as i64;
                value.detail[4] = o.bank as i64;
                value.detail_valid |= 0x18;
            }
            if let Some(deadline) = read::<i32>(node as usize, 0x28) {
                value.detail[5] = deadline as i64;
                value.detail_valid |= 0x20;
            }
            if let Some(pump) =
                read::<usize>(sound as usize, 0x30).and_then(|e| read::<i32>(e, 0x74))
            {
                value.detail[6] = pump as i64;
                value.detail_valid |= 0x40;
            }
            Some((value, token))
        },
        || engine.schedule.call(sound, node),
        |snapshot, _, timing| {
            if let Some(Some((mut value, token))) = snapshot {
                value.qpc = timing.original_begin;
                value.end_qpc = timing.original_end;
                if let Some(ticks) = timing.pre_ticks() {
                    value.counters[0] = ticks as u64;
                    value.counters[3] = 1;
                }
                emit_checked(value, token);
            }
        },
    );
}

fn emit_checked(mut value: Event, token: Option<Token>) {
    report_status();
    if token.is_some_and(|t| !MAP.get().is_some_and(|m| m.current(t))) {
        value.trace_id = 0;
        value.origin_known = false;
        value.origin_attempt = 0;
        value.origin_scene = -1;
        value.id = -1;
        value.name = [0; 32];
    }
    super::push(value);
}

/// Audio-clock voice identity (PRE-original, so the pending node is armed
/// before the render thread can drain the posted Start): walk the wave's
/// reciprocal ownership chain to its cue/bank and the voice wrapper to its
/// engine source node.
fn identify_voice_for_clock(engine: &Engine, wave: usize) {
    if !audio_clock::is_available() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // `wave+0x70` bit 2 = the submission's "skip Start" branch (both the
        // streaming 0x25ED0 and in-memory 0x419DB0 test it before calling
        // V->Start): no voice starts, so there is nothing to identify.
        if read::<u16>(wave, 0x70).is_none_or(|flags| flags & 4 != 0) {
            return;
        }
        let Some(owned) = xact_sites::walk_wave(wave, engine.base, &engine.sites, pointer) else {
            return;
        };
        let Some(node) = xact_sites::voice_node(owned.voice, engine.base, &engine.sites, pointer)
        else {
            return;
        };
        audio_clock::engine::on_voice_start(owned.owners.bank, owned.owners.cue, node);
    }));
}

unsafe extern "C" fn memory_submit_hook(wave: *mut u8, flag: i32, spatial: *const f32) -> i32 {
    let Some(engine) = ENGINE.get() else {
        return 0;
    };
    let Some(hook) = engine.memory_submit.as_ref() else {
        return 0;
    };
    identify_voice_for_clock(engine, wave as usize);
    hook.call(wave, flag, spatial)
}

unsafe extern "C" fn produce_hook(node: *mut u8) -> u64 {
    let Some(engine) = ENGINE.get() else {
        return 0;
    };
    let Some(hook) = engine.produce.as_ref() else {
        return 0;
    };
    let pre = audio_clock::engine::produce_pre(node as usize);
    let result = hook.call(node);
    if let Some(pre) = pre {
        audio_clock::engine::produce_post(node as usize, pre);
    }
    result
}

unsafe extern "C" fn submit_hook(wave: *mut u8, flag: i32, spatial: *const f32) -> i32 {
    let Some(engine) = ENGINE.get() else {
        return 0;
    };
    identify_voice_for_clock(engine, wave as usize);
    if !recording() {
        return engine.submit.call(wave, flag, spatial);
    }
    observed_timed(
        super::clock,
        |now| {
            let owned = xact_sites::walk_wave(wave as usize, engine.base, &engine.sites, pointer);
            let token = owned.and_then(|w| MAP.get()?.lookup(w.owners));
            let flags = read::<u16>(wave as usize, 0x70);
            let mut value = event(Kind::VoiceStart, now, token);
            value.detail[0] = wave as i64;
            value.detail_valid = 1;
            if let Some(w) = owned {
                value.detail[1] = w.event as i64;
                value.detail[2] = w.voice as i64;
                value.detail[3] = w.target as i64;
                value.detail[4] = w.owners.cue as i64;
                value.detail[5] = w.owners.sound as i64;
                value.detail_valid |= 0x3e;
            }
            if let Some(flags) = flags {
                value.detail[6] = flags as i64;
                value.detail_valid |= 0x40;
            }
            (value, token, flags)
        },
        || engine.submit.call(wave, flag, spatial),
        |snapshot, result, timing| {
            if let Some((mut value, token, flags)) = snapshot {
                value.qpc = timing.original_begin;
                value.end_qpc = timing.original_end;
                if let Some(ticks) = timing.pre_ticks() {
                    value.counters[0] = ticks as u64;
                    value.counters[3] = 1;
                }
                value.result = *result;
                value.detail[7] = xact_sites::submission_branch(flags, *result);
                if flags.is_some() {
                    value.detail_valid |= 0x80;
                }
                emit_checked(value, token);
            }
        },
    )
}

unsafe extern "C" fn sound_stop_hook(sound: *mut u8, immediate: i32) -> i32 {
    let Some(engine) = ENGINE.get() else {
        return 0;
    };
    if audio_clock::engine::installed() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(owners) =
                xact_sites::sound_owners(sound as usize, engine.base, &engine.sites, &pointer)
            {
                audio_clock::engine::on_voice_stop(owners.cue);
            }
        }));
    }
    if !recording() {
        return engine.stop.call(sound, immediate);
    }
    observed(
        || {
            let owners =
                xact_sites::sound_owners(sound as usize, engine.base, &engine.sites, &pointer);
            let token = MAP.get().and_then(|map| {
                let token = owners.and_then(|o| map.lookup(o));
                if let Some(owners) = owners {
                    map.destroy_cue(owners.cue);
                } else {
                    map.invalidate_all();
                }
                token
            });
            let now = super::clock();
            let mut value = event(Kind::SoundStop, now, token);
            value.detail[0] = sound as i64;
            value.detail[3] = immediate as i64;
            value.detail_valid = 9;
            if let Some(owners) = owners {
                value.detail[1] = owners.cue as i64;
                value.detail[2] = owners.bank as i64;
                value.detail_valid |= 6;
            }
            (value, token)
        },
        || engine.stop.call(sound, immediate),
        |snapshot, result| {
            if let Some((mut value, token)) = snapshot {
                value.end_qpc = super::clock();
                value.result = *result;
                emit_checked(value, token);
            }
        },
    )
}

unsafe extern "C" fn destroy_hook(cue: *mut u8) {
    let Some(engine) = ENGINE.get() else {
        return;
    };
    audio_clock::engine::on_voice_stop(cue as usize);
    if !recording() {
        engine.destroy.call(cue);
        return;
    }
    observed(
        || {
            let token = MAP.get().and_then(|map| map.destroy_cue(cue as usize));
            let now = super::clock();
            let mut value = event(Kind::CueDestroyed, now, token);
            value.detail[0] = cue as i64;
            value.detail_valid = 1;
            // Copy remaining IDs while the destructor owns the cue. A bound
            // token intentionally has no game-thread snapshot of mutable sound.
            if let Some(sound) = read::<usize>(cue as usize, 0x58) {
                value.detail[1] = sound as i64;
                value.detail_valid |= 2;
            }
            if let Some(bank) = read::<usize>(cue as usize, 0x240) {
                value.detail[2] = bank as i64;
                value.detail_valid |= 4;
            }
            (value, token)
        },
        || engine.destroy.call(cue),
        |snapshot, _| {
            if let Some((mut value, token)) = snapshot {
                value.end_qpc = super::clock();
                emit_checked(value, token);
            }
        },
    );
}

fn cursor_snapshot(backend: usize) -> Option<Cursor> {
    let format = read::<usize>(backend, 0x80)?;
    Some(Cursor {
        backend,
        buffer: read(backend, 0x90)?,
        play: read(backend, 0xac)?,
        write: read(backend, 0xb0)?,
        played: read(backend, 0xd0)?,
        ring: read(backend, 0xa8)?,
        hz: read(format, 4)?,
        align: read(format, 0xc)?,
    })
}

/// The ONE detour on the engine's DAC-position read (`0x435A50`, once per mix
/// pass on the render thread) — a dispatcher for two subscribers: the audio
/// clock's fit (every pass, when installed) and the diagnostics' decimated
/// cursor channel (when recording). With neither active the call is a plain
/// passthrough with no QPC.
unsafe extern "C" fn cursor_hook(backend: *mut u8) -> i32 {
    let Some(engine) = ENGINE.get() else {
        return -1;
    };
    let clock_on = audio_clock::engine::installed();
    let diag_on = recording();
    if !clock_on && !diag_on {
        return engine.cursor.call(backend);
    }
    let pre = qpc();
    let result = engine.cursor.call(backend);
    let post = qpc();
    if clock_on {
        audio_clock::engine::on_pass(backend as usize, result, pre, post);
    }
    if diag_on {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let snapshot = if result == -1 {
                None
            } else {
                cursor_snapshot(backend as usize)
            };
            let Ok(mut sampler) = engine.cursor_state.try_lock() else {
                engine.cursor_lost.store(true, Ordering::Release);
                return;
            };
            if engine.cursor_lost.swap(false, Ordering::AcqRel) {
                sampler.invalidate();
            }
            let take = sampler.observe(post, engine.frequency, snapshot);
            drop(sampler);
            if let Some(continuous) = take {
                let mut value = event(Kind::OutputCursor, pre, None);
                value.end_qpc = post;
                value.result = result;
                value.detail[0] = backend as i64;
                value.detail_valid = 1;
                if let Some(cursor) = snapshot {
                    value.detail = cursor.payload();
                    value.detail_valid = 0xff | if continuous { 0x100 } else { 0 };
                }
                super::push(value);
            }
        }));
    }
    result
}

fn snapshot(base: usize, size: usize) -> Option<Vec<u8>> {
    if size < 4096 || size > 128 * 1024 * 1024 || !memory::is_readable(base as *const u8, 4096) {
        return None;
    }
    let headers = unsafe { std::slice::from_raw_parts(base as *const u8, 4096) };
    let ranges = xact_sites::snapshot_ranges(headers, size)?;
    let mut image = vec![0; size];
    image.get_mut(..4096)?.copy_from_slice(headers);
    for range in ranges {
        let address = base.checked_add(range.start)? as *const u8;
        if !memory::is_readable(address, range.len()) {
            return None;
        }
        image
            .get_mut(range.clone())?
            .copy_from_slice(unsafe { std::slice::from_raw_parts(address, range.len()) });
    }
    Some(image)
}

/// Called early by lib.rs. Absence/missed factory never triggers a late engine scan.
/// Runs when the diagnostics OR the audio clock (`gameplay-timing-fixes`) want
/// the engine seams; with neither, nothing is scanned or hooked.
pub fn init_factory(game: &GameModule) {
    if (!diag_enabled_in_config() && !audio_clock::wants_engine())
        || INIT.swap(true, Ordering::AcqRel)
    {
        return;
    }
    let _ = MAP.set(Correlation::new());
    let outcome = std::panic::catch_unwind(|| unsafe {
        if GetModuleHandleA(ENGINE_NAME).is_ok() {
            return Err((
                Status::MissedWindow,
                "engine already loaded; refusing late installation",
            ));
        }
        let image = snapshot(game.base as usize, game.size)
            .ok_or((Status::FactoryUnavailable, "game image unavailable"))?;
        let factory = xact_sites::unique(&image, xact_sites::FACTORY)
            .filter(|p| xact_sites::factory_valid(&image, *p))
            .ok_or((
                Status::FactoryUnavailable,
                "factory signature/layout unavailable",
            ))?;
        // This narrow bootstrap runs before SignatureStore::resolve_all.
        // The local manager shape is attested on the same four-build corpus.
        let manager = xact_sites::unique(&image, xact_sites::MANAGER);
        if !manager.is_some_and(|p| xact_sites::manager_valid(&image, p)) {
            return Err((
                Status::ManagerUnavailable,
                "manager handle layout unavailable",
            ));
        }
        // Recheck immediately before patching the game wrapper, never patch an
        // engine discovered during this scan. The return hook proves our window.
        if GetModuleHandleA(ENGINE_NAME).is_ok() {
            return Err((Status::MissedWindow, "engine loaded during bootstrap scan"));
        }
        let original: FactoryFn = std::mem::transmute(game.base.add(factory));
        let hook = GenericDetour::new(original, factory_hook as FactoryFn)
            .map_err(|_| (Status::InstallFailed, "factory detour creation failed"))?;
        FACTORY
            .set(hook)
            .map_err(|_| (Status::InstallFailed, "factory already installed"))?;
        STATUS.store(Status::Armed as u32, Ordering::Release);
        FACTORY
            .get()
            .ok_or((Status::InstallFailed, "factory handle missing"))?
            .enable()
            .map_err(|_| (Status::InstallFailed, "factory detour enable failed"))?;
        channels(1 << FACTORY_BIT);
        Ok::<(), (Status, &str)>(())
    });
    match outcome {
        Ok(Ok(())) => log_info!("audio_sync_diag: XACT factory observer armed; engine channels pending pre-Initialize window (consumers: diag={} audio_clock={})", diag_enabled_in_config(), audio_clock::wants_engine()),
        Ok(Err((status, reason))) => {
            STATUS.store(status as u32, Ordering::Release);
            log_warn!("audio_sync_diag: XACT unavailable: {}", reason);
        }
        Err(_) => {
            STATUS.store(Status::InstallFailed as u32, Ordering::Release);
            log_warn!("audio_sync_diag: XACT bootstrap panicked; engine channels unavailable");
        }
    }
}

unsafe extern "C" fn factory_hook(out: *mut *mut u8) {
    let Some(hook) = FACTORY.get() else {
        return;
    };
    // Always forward original outside panic-catching observer code.
    hook.call(out);
    if ENTERED.swap(true, Ordering::AcqRel) {
        return;
    }
    let outcome = std::panic::catch_unwind(|| install_engine(out));
    match outcome {
        Ok(Ok(())) => {
            STATUS.store(Status::Installed as u32, Ordering::Release);
            log_info!("audio_sync_diag: XACT observers installed before Initialize; voice Start is an envelope, not audible onset");
        }
        Ok(Err(reason)) => {
            if STATUS.load(Ordering::Acquire) != Status::InstallFailed as u32 {
                STATUS.store(Status::Unsupported as u32, Ordering::Release);
            }
            log_warn!("audio_sync_diag: XACT observation unavailable: {}", reason);
        }
        Err(_) => {
            STATUS.store(Status::InstallFailed as u32, Ordering::Release);
            log_warn!("audio_sync_diag: XACT install panicked; observers remain passthrough");
        }
    }
}

unsafe fn install_engine(out: *mut *mut u8) -> Result<(), &'static str> {
    let object = read::<usize>(out as usize, 0).ok_or("factory returned no engine")?;
    let vt = read::<usize>(object, 0).ok_or("engine vtable unavailable")?;
    let method = read::<usize>(vt, 0).ok_or("engine method unavailable")?;
    let mut module = HMODULE::default();
    GetModuleHandleExA(
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        PCSTR(method as *const u8),
        &mut module,
    )
    .map_err(|_| "cannot retain engine module")?;
    // Never release this reference: even rollback failure must not leave dangling
    // detours. An unsupported module costs only this one process-lifetime ref.
    if GetModuleHandleA(ENGINE_NAME).ok() != Some(module) {
        return Err("factory used another engine");
    }
    let mut info = MODULEINFO::default();
    GetModuleInformation(
        GetCurrentProcess(),
        module,
        &mut info,
        std::mem::size_of::<MODULEINFO>() as u32,
    )
    .map_err(|_| "engine PE information unavailable")?;
    let base = info.lpBaseOfDll as usize;
    let image = snapshot(base, info.SizeOfImage as usize).ok_or("engine code image unreadable")?;
    let sites = xact_sites::resolve(&image)?;
    let mut frequency = 0;
    if QueryPerformanceFrequency(&mut frequency) == 0 || frequency <= 0 {
        return Err("QPC frequency unavailable");
    }
    STATUS.store(Status::InstallFailed as u32, Ordering::Release);
    let clock_wanted = audio_clock::wants_engine();
    let produce = if clock_wanted {
        Some(
            GenericDetour::new(
                std::mem::transmute::<usize, ProduceFn>(base + sites.produce),
                produce_hook as ProduceFn,
            )
            .map_err(|_| "produce detour")?,
        )
    } else {
        None
    };
    let memory_submit = if clock_wanted {
        Some(
            GenericDetour::new(
                std::mem::transmute::<usize, SubmitFn>(base + sites.memory_submit),
                memory_submit_hook as SubmitFn,
            )
            .map_err(|_| "memory submit detour")?,
        )
    } else {
        None
    };
    let engine = Engine {
        base,
        sites,
        _module: module.0 as usize,
        frequency,
        produce,
        memory_submit,
        schedule: GenericDetour::new(
            std::mem::transmute::<usize, ScheduleFn>(base + sites.schedule),
            schedule_hook as ScheduleFn,
        )
        .map_err(|_| "schedule detour")?,
        submit: GenericDetour::new(
            std::mem::transmute::<usize, SubmitFn>(base + sites.submit),
            submit_hook as SubmitFn,
        )
        .map_err(|_| "submit detour")?,
        stop: GenericDetour::new(
            std::mem::transmute::<usize, SoundStopFn>(base + sites.sound_stop),
            sound_stop_hook as SoundStopFn,
        )
        .map_err(|_| "stop detour")?,
        destroy: GenericDetour::new(
            std::mem::transmute::<usize, DestroyFn>(base + sites.cue_destroy),
            destroy_hook as DestroyFn,
        )
        .map_err(|_| "destroy detour")?,
        cursor: GenericDetour::new(
            std::mem::transmute::<usize, CursorFn>(base + sites.cursor),
            cursor_hook as CursorFn,
        )
        .map_err(|_| "cursor detour")?,
        cursor_state: Mutex::new(CursorSampler::default()),
        cursor_lost: AtomicBool::new(false),
    };
    ENGINE.set(engine).map_err(|_| "engine already installed")?;
    let engine = ENGINE.get().ok_or("engine handle missing")?;
    let enabled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        engine
            .schedule
            .enable()
            .and_then(|_| engine.submit.enable())
            .and_then(|_| engine.stop.enable())
            .and_then(|_| engine.destroy.enable())
            .and_then(|_| engine.cursor.enable())
            .and_then(|_| match engine.produce.as_ref() {
                Some(hook) => hook.enable(),
                None => Ok(()),
            })
            .and_then(|_| match engine.memory_submit.as_ref() {
                Some(hook) => hook.enable(),
                None => Ok(()),
            })
    }));
    if !matches!(enabled, Ok(Ok(()))) {
        macro_rules! undo {
            ($hook:ident) => {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| engine.$hook.disable()))
                    .is_ok_and(|result| result.is_ok())
            };
        }
        macro_rules! undo_opt {
            ($hook:ident) => {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine.$hook.as_ref().map_or(Ok(()), |h| h.disable())
                }))
                .is_ok_and(|result| result.is_ok())
            };
        }
        let rollback = [
            undo_opt!(memory_submit),
            undo_opt!(produce),
            undo!(cursor),
            undo!(destroy),
            undo!(stop),
            undo!(submit),
            undo!(schedule),
        ];
        if rollback.iter().any(|ok| !ok) {
            log_warn!("audio_sync_diag: XACT rollback incomplete; retained hooks are passthrough, module pinned");
        }
        return Err("engine hook transaction failed");
    }
    READY.store(true, Ordering::Release);
    channels(ENGINE_BITS);
    if clock_wanted {
        // Same window, same attestation: the clock's render-thread observers
        // go live here (their hooks are enabled above); the fit allocates now,
        // before Initialize creates the render thread.
        if audio_clock::engine::install(frequency) {
            log_info!("audio_clock: engine observers installed in the pre-Initialize window (cursor dispatcher + produce + streaming/in-memory identity)");
        } else {
            log_warn!("audio_clock: engine observer setup failed -- clock stays stock");
        }
    }
    Ok(())
}
