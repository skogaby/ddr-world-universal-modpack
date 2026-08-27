//! Game audio — mod-owned XACT bank registration and cue playback through the
//! game's **own** audio engine.
//!
//! DDR World's audio is COM-instantiated Microsoft XACT 2
//! (`xactengine2_10.dll`), wrapped by an in-house "audio manager" singleton.
//! *Everything* audible — menu BGM, voice, every sound effect, and the song
//! audio itself — passes through one engine instance and one final mix, which
//! is exactly why a mod sound plays through it rather than through an audio
//! path of its own: a cue played here inherits the music's exact output
//! latency, which a self-hosted client cannot.
//!
//! This service owns the whole game-ABI surface for that — the vtable
//! dispatches, the manager global, the slot table — so all of it is auditable
//! in one file. Policy (what to play and when) belongs to the consuming mod.
//!
//! ## What registration actually does
//!
//! The audio manager owns **six** sound-bank slots. A loaded bank file's
//! basename selects its slot (`bgm_menu`→0, `se_system`→1, `se_normal`→2,
//! `voice`→3, anything else→5), so **slot 4 is never produced by that
//! mapping** and stays free for the process lifetime.
//!
//! Claiming it costs one pointer write, and the reason it *stays* claimed is
//! the load-bearing detail of this whole service: the only code in the game
//! that ever destroys a sound-bank slot is a linear "find the slot whose
//! `file_id` equals this one" search that destroys nothing when nothing
//! matches. A slot whose `file_id` stays `-1` can never be matched — so
//! writing **only** the bank pointer, and never the `file_id`, is what makes
//! our bank outlive every song load, song unload and scene transition.
//!
//! ## Threading
//!
//! [`init`] resolves addresses and calls **no** game function, so it is safe on
//! the DLL init thread (calling into the game there is a documented crash class
//! in this codebase — the backing globals may not exist yet).
//! [`register_bank`] and [`play_cue`] call the live engine and are **game
//! thread only**; consumers drive them from the judge dispatcher, which is
//! itself proof that gameplay state is live.
//!
//! The engine-module presence check (which protects the vtable dispatches from
//! an unverified XACT build) therefore lives in [`register_bank`], not in
//! [`init`]: the engine is COM-instantiated inside the game's `onBoot`, which
//! provably completes *after* our init thread finishes, so a boot-time check
//! would always fail. Checking immediately before the first dispatch is also
//! strictly tighter.
//!
//! RE record:
//! `.agents/planning/20260725-assist-tick/research/bank-slot-and-anchors.md`
//! (slot safety, the two required guards) and `research/xact-bank-format.md`
//! (creation order, vtable layouts, HRESULT vocabulary).

use once_cell::sync::Lazy;
use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

// ── Game ABI ─────────────────────────────────────────────────────────

/// The game's public "play a sound effect" façade:
/// `u32 se_play(i32 bank_id /*ECX*/, const char* cue /*RDX*/, f32 pan /*XMM2*/)`.
///
/// The third argument is a **float**, so under the Microsoft x64 convention it
/// travels in **XMM2**, not in a general-purpose register — declaring it as an
/// integer would silently pass garbage. `extern "system"` maps it correctly.
/// Returns a handle into the manager's cue table, or [`SE_PLAY_FAILED`].
type SePlayFn = unsafe extern "system" fn(i32, *const c_char, f32) -> u32;

/// `IXACT2Engine::CreateSoundBank` / `CreateInMemoryWaveBank` — identical
/// shapes: `(this, pvBuffer, cbBuffer, dwFlags, dwAllocAttributes, ppOut)`.
/// `dwFlags` is masked to `& 1` by the engine and the game passes 0.
type CreateBankFn =
    unsafe extern "system" fn(*mut u8, *const u8, u32, u32, u32, *mut *mut u8) -> i32;

/// `IXACT2SoundBank::GetCueIndex(this, cueName) -> XACTINDEX`.
/// [`CUE_NOT_FOUND`] when the name is not in the bank (byte-exact `strcmp`, so
/// cue lookup is case-sensitive).
type GetCueIndexFn = unsafe extern "system" fn(*mut u8, *const c_char) -> u16;

/// `IXACT2SoundBank::Play(this, cueIndex, dwFlags, timeOffsetMs, ppCue)`.
/// `timeOffsetMs` is validated `>= 0` by the engine but is NOT a seek: it
/// only fast-forwards the cue's *event* timeline, and a wave whose event
/// time is already due starts at **sample 0** (`Wave_StartNow_NoSampleOffset`
/// — the engine has no sample-offset start primitive; live-confirmed
/// 2026-07-29). Always passed 0 here; content shifting happens in
/// [`rewrite_tick_wave`] instead. `ppCue = NULL` selects the engine's own
/// auto-release path.
type SoundBankPlayFn = unsafe extern "system" fn(*mut u8, u16, u32, i32, *mut *mut u8) -> i32;

/// `IXACT2SoundBank::Stop(this, cueIndex, dwFlags)`. The engine rejects any
/// flag bit other than bit 0 (`dwFlags & 0xFFFFFFFE` ⇒ `E_INVALIDARG`);
/// bit 0 = `XACT_FLAG_STOP_IMMEDIATE`, which is always what we want.
type SoundBankStopFn = unsafe extern "system" fn(*mut u8, u16, u32) -> i32;

/// `se_play`'s failure return. The manager *leaks* a cue rather than crashing
/// when its shared 256-entry handle table is exhausted, so this return value is
/// the only signal that a play did not happen.
const SE_PLAY_FAILED: u32 = 0xFFFF_FFFF;
/// `GetCueIndex`'s not-found sentinel.
const CUE_NOT_FOUND: u16 = 0xFFFF;

/// `IXACT2Engine*`, at the very start of the audio-manager object.
const MGR_ENGINE_PTR: usize = 0x00;
/// Offset of sound-bank slot 0's `file_id` within the manager. The slot array
/// runs `[mgr+0x08, mgr+0x68)` — proven to be exactly 6 entries three
/// independent ways (constructor memset size, the constructor's 12 `-1`
/// stores, and the destroyer's loop terminator).
const MGR_SLOT_ARRAY: usize = 0x08;
/// Stride of one `{ int file_id; IXACT2SoundBank* bank; }` slot.
const MGR_SLOT_STRIDE: usize = 0x10;
/// `int file_id` within a slot. `-1` = empty. **Never written by us** — see
/// [`register_bank`].
const SLOT_FILE_ID: usize = 0x00;
/// `IXACT2SoundBank*` within a slot. `NULL` = empty. The one field we write.
const SLOT_BANK_PTR: usize = 0x08;
/// Number of sound-bank slots on the manager.
const MGR_SLOT_COUNT: i32 = 6;
/// `file_id` value meaning "this slot is empty".
const SLOT_EMPTY_FILE_ID: i32 = -1;
/// The slot the game maps `se_normal` (the gameplay sound-effect bank) into.
/// Its bank pointer being non-null is our proof that both the slot-array layout
/// and a normal boot hold before we write anything.
const SE_NORMAL_SLOT: i32 = 2;
/// Named banks the game's slot mapper knows (`bgm_menu`, `se_system`,
/// `se_normal`, `voice`). If a future build grows a fifth, it would map to slot
/// 4 and silently collide with our bank — hence the boot-time gate.
const EXPECTED_NAMED_BANKS: u8 = 4;

/// `IXACT2Engine` vtable index for `CreateSoundBank`.
const ENGINE_VT_CREATE_SOUND_BANK: usize = 0x48;
/// `IXACT2Engine` vtable index for `CreateInMemoryWaveBank`.
const ENGINE_VT_CREATE_MEM_WAVE_BANK: usize = 0x50;
/// `IXACT2SoundBank` vtable index for `GetCueIndex`.
const SOUND_BANK_VT_GET_CUE_INDEX: usize = 0x00;
/// `IXACT2SoundBank` vtable index for `Play`. Byte offset — verified against
/// the engine binary's sound-bank vtable (slot 4; the pre-mixed-track RE
/// spike's `SoundBank_Play`).
const SOUND_BANK_VT_PLAY: usize = 0x20;
/// `IXACT2SoundBank` vtable index for `Stop` (slot 5, `SoundBank_Stop`).
const SOUND_BANK_VT_STOP: usize = 0x28;
/// `XACT_FLAG_STOP_IMMEDIATE` — the only flag bit `Stop` accepts.
const STOP_IMMEDIATE: u32 = 1;

/// The XACT engine build whose vtable indices above were read off the
/// disassembly. A cabinet shipping a different engine would have unverified
/// indices, so [`register_bank`] refuses to dispatch without this module.
const XACT_MODULE: &str = "xactengine2_10.dll";
/// NUL-terminated form of [`XACT_MODULE`], for `GetModuleHandleA`.
const XACT_MODULE_CSTR: &CStr = c"xactengine2_10.dll";

// ── Public types ─────────────────────────────────────────────────────

/// A mod-owned XACT bank pair, ready for registration.
pub struct BankRequest {
    /// Diagnostic/idempotence key. Should match the wave-bank name inside the
    /// XSB (the engine pairs banks by that internal name, case-sensitively),
    /// which makes it the useful thing to log.
    pub name: &'static str,
    /// Wave-bank (`.xwb`) bytes.
    pub xwb: Vec<u8>,
    /// Sound-bank (`.xsb`) bytes.
    pub xsb: Vec<u8>,
}

/// Opaque handle to a registered bank. `Copy` — registration is idempotent and
/// lasts the process lifetime, so there is no release path and no ownership to
/// enforce.
#[derive(Clone, Copy)]
pub struct BankHandle {
    /// The manager sound-bank slot the bank lives in; also `se_play`'s first
    /// argument.
    slot: i32,
}

/// The assist-tick mod's rewritable tick bank, ready for registration: the
/// containers from `se_bank_synth::build_tick_containers()` plus the location
/// of the wave bank's rewritable sample segment within `xwb`.
pub struct TickBankRequest {
    /// Diagnostic/idempotence key = the banks' internal name (`"asti"`).
    pub name: &'static str,
    /// Wave-bank bytes — leaked at registration; the engine reads sample
    /// bytes from this buffer for the process lifetime (never copies).
    pub xwb: Vec<u8>,
    /// Sound-bank bytes — likewise leaked.
    pub xsb: Vec<u8>,
    /// Byte offset of the rewritable sample segment within `xwb`.
    pub sample_seg_offset: usize,
    /// Length of the sample segment (runs exactly to the end of `xwb`).
    pub sample_seg_len: usize,
}

/// Opaque handle to the registered tick bank. `Copy`, process lifetime, no
/// release path — same ownership model as [`BankHandle`].
///
/// Unlike [`BankHandle`] it names no manager slot: the tick bank is
/// deliberately **not** entered into the audio manager's sound-bank slot
/// array (deviation approved 2026-07-29). The slots' only readers are the
/// game's `se_play`/`se_prepare` façade — which this bank never plays
/// through ([`play_tick_track`] needs `Play`'s `timeOffset`, which the façade
/// hardwires to 0) — and slot 5 is the game's own per-song bank slot, so
/// mid-song registration (the first judge dispatch) would find no free slot
/// while the shipped per-tick bank coexists. A slot-less bank is also
/// strictly safer under the immortal-bank rule: the game's bank destroyer
/// selects victims by searching the slots, and a bank in no slot can never
/// be found by it at all.
///
/// Raw addresses are stored as `usize` so the handle stays `Send` for the
/// consuming mod's state mutex (the `SongState::tick_actor` precedent);
/// they are only ever dereferenced here, on the game thread.
#[derive(Clone, Copy)]
pub struct TickBankHandle {
    /// The `IXACT2SoundBank*` the engine handed back — the dispatch target
    /// for `GetCueIndex`/`Play`/`Stop`.
    sound_bank: usize,
    /// First byte of the wave bank's sample segment, inside the leaked XWB.
    sample_seg: usize,
    /// Length of the sample segment.
    sample_len: usize,
}

// ── Service state ────────────────────────────────────────────────────

struct RegisteredBank {
    name: String,
    handle: BankHandle,
    /// The `IXACT2SoundBank*` the engine handed back. Kept for `GetCueIndex`
    /// diagnostics; the authoritative copy is the one in the manager slot.
    sound_bank: *mut u8,
    /// Cue names whose resolved index has already been logged, so the
    /// diagnostic is once-per-cue rather than once-per-tick.
    logged_cues: Vec<String>,
}

struct Inner {
    se_play: SePlayFn,
    /// Address of the audio-manager **global pointer** (not the object).
    /// Dereferenced on every call so a null global can never be missed — the
    /// game's own `se_play_inner` dereferences it unconditionally, so this
    /// check is ours to make.
    manager_global: *const u8,
    /// Address of the `CMP EBX,imm8` named-bank count inside the game's slot
    /// mapper. Re-read at registration time as a safety gate.
    named_bank_count_site: *const u8,
    banks: Vec<RegisteredBank>,
    /// The one registered tick bank (name + handle), or `None`. Exactly one
    /// per process by design (NFR-2) — the handle's bank is immortal and its
    /// wave content is rewritten in place per song.
    tick_bank: Option<(String, TickBankHandle)>,
}

// These are fixed addresses in the game's address space, valid for the process
// lifetime and only touched from the game thread (codebase norm).
unsafe impl Send for Inner {}

impl Inner {
    /// Dereference the manager global to the live audio-manager object, or
    /// `None` if the global (or the pointer within it) is still null.
    fn manager(&self) -> Option<*mut u8> {
        if self.manager_global.is_null() {
            return None;
        }
        let obj = unsafe { *(self.manager_global as *const *mut u8) };
        if obj.is_null() {
            None
        } else {
            Some(obj)
        }
    }
}

static AUDIO: Lazy<Mutex<Option<Inner>>> = Lazy::new(|| Mutex::new(None));
/// Latches the first playback failure so the warning is once per session, not
/// once per tick.
static PLAY_FAILURE_WARNED: AtomicBool = AtomicBool::new(false);
/// Latches the first null-manager skip, likewise. Deliberately does NOT set
/// [`REGISTER_DECLINED`]: a null manager is the one transient failure here (it
/// just means boot has not finished), so a later attempt may legitimately
/// succeed.
static NULL_MANAGER_WARNED: AtomicBool = AtomicBool::new(false);
/// Set once any *permanent* registration failure has been reported — a missing
/// engine module, a changed slot layout, no free slot, or a bank the engine
/// rejects. None of those can turn into a success later in the session, so
/// subsequent attempts decline silently and the session carries exactly one
/// warning for the cause.
static REGISTER_DECLINED: AtomicBool = AtomicBool::new(false);
/// The tick-bank twin of [`REGISTER_DECLINED`]. Deliberately separate: during
/// the transition both banks coexist, and one path's permanent failure must
/// not silence the other's diagnostic (or block its registration).
static TICK_REGISTER_DECLINED: AtomicBool = AtomicBool::new(false);
/// Latches the tick-track playback/stop/rewrite failure warnings — one per
/// session per failure class, not one per song.
static TICK_PLAY_WARNED: AtomicBool = AtomicBool::new(false);
static TICK_STOP_WARNED: AtomicBool = AtomicBool::new(false);
static TICK_REWRITE_WARNED: AtomicBool = AtomicBool::new(false);

/// Address of a slot's `file_id` field. Address arithmetic only — the read is
/// the caller's `unsafe`.
fn slot_file_id_ptr(mgr: *mut u8, slot: i32) -> *mut i32 {
    let off = MGR_SLOT_ARRAY + (slot as usize) * MGR_SLOT_STRIDE + SLOT_FILE_ID;
    mgr.wrapping_add(off) as *mut i32
}

/// Address of a slot's `IXACT2SoundBank*` field. Address arithmetic only.
fn slot_bank_ptr(mgr: *mut u8, slot: i32) -> *mut *mut u8 {
    let off = MGR_SLOT_ARRAY + (slot as usize) * MGR_SLOT_STRIDE + SLOT_BANK_PTR;
    mgr.wrapping_add(off) as *mut *mut u8
}

/// Fetch a method pointer out of a COM object's vtable.
///
/// # Safety
/// `obj` must be a live object whose first field is a vtable pointer, and
/// `index` must be a slot the game itself exercises — `IXACT2Cue`'s layout
/// provably deviates from the public XACT 3 headers, so a "reasonable" guess at
/// an unused slot is not safe. Only [`ENGINE_VT_CREATE_SOUND_BANK`],
/// [`ENGINE_VT_CREATE_MEM_WAVE_BANK`], [`SOUND_BANK_VT_GET_CUE_INDEX`],
/// [`SOUND_BANK_VT_PLAY`] and [`SOUND_BANK_VT_STOP`] are used here; the first
/// three are game-exercised, the last two were verified directly against the
/// engine binary's sound-bank vtable (the pre-mixed-track RE spike).
unsafe fn vtable_fn<T: Copy>(obj: *mut u8, index: usize) -> T {
    let vtable = *(obj as *const *const u8);
    *(vtable.add(index) as *const T)
}

/// A plain-language gloss for the HRESULTs worth naming in a log line.
fn hresult_note(hr: i32) -> &'static str {
    match hr as u32 {
        0x8AC7_0007 => "bank rejected as malformed (bad header/CRC/structure)",
        0x8AC7_0006 => "wrong bank type (in-memory wave bank must not be marked streaming)",
        0x8AC7_0002 => "engine not initialized",
        0x8AC7_0012 => "called from the XACT notification thread",
        0x8007_0057 => "invalid argument (null buffer or zero size)",
        0x8007_000E => "engine out of memory",
        _ => "see research/xact-bank-format.md Appendix B",
    }
}

// ── Public API ───────────────────────────────────────────────────────

/// Resolve the addresses this service needs. Call once during DLL init.
///
/// **Resolves addresses only — calls no game function**, so it is safe on the
/// init thread. Returns `false` (and leaves the service unavailable) if any
/// address is missing; the consuming mod then declines to init and nothing else
/// is affected.
///
/// Note this does *not* check for the XACT engine module: it is COM-loaded
/// during the game's `onBoot`, which completes after this runs (verified in the
/// boot log), so that check belongs to [`register_bank`] — immediately before
/// the first vtable dispatch it protects.
pub fn init(signatures: &SignatureStore) -> bool {
    let se_play = signatures.get_address("se_play");
    let manager_global = signatures.get_address("audio_manager_global");
    let count_site = signatures.get_address("audio_named_bank_count_site");

    let (se_play, manager_global, count_site) = match (se_play, manager_global, count_site) {
        (Some(p), Some(m), Some(c)) => (p, m, c),
        _ => {
            log_warn!(
                "GameAudio: required addresses missing (se_play={} audio_manager_global={} audio_named_bank_count_site={}) -- service disabled",
                se_play.is_some(),
                manager_global.is_some(),
                count_site.is_some()
            );
            return false;
        }
    };

    let inner = Inner {
        se_play: unsafe { std::mem::transmute::<*const u8, SePlayFn>(se_play) },
        manager_global,
        named_bank_count_site: count_site,
        banks: Vec::new(),
        tick_bank: None,
    };
    match AUDIO.lock() {
        Ok(mut g) => *g = Some(inner),
        Err(_) => {
            log_warn!("GameAudio: state mutex poisoned during init -- service disabled");
            return false;
        }
    }
    log_info!(
        "GameAudio: initialized (se_play @ {:p}, audio_manager_global @ {:p})",
        se_play,
        manager_global
    );

    true
}

/// Whether every address resolved, i.e. whether the service can be asked to do
/// anything. Consumers check this before wiring anything up.
///
/// Does not (and at init time cannot) imply that the XACT engine module is
/// loaded — [`register_bank`] checks that at first use.
pub fn is_available() -> bool {
    AUDIO.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Register a mod-owned bank pair with the game's engine and claim a free
/// sound-bank slot on the audio manager.
///
/// **GAME THREAD ONLY.** Idempotent per `name`: a repeat call returns the
/// existing handle without creating anything. Returns `None` — with exactly one
/// warning for the session — if the expected engine module is absent, the
/// manager is not ready, the free-slot assumption no longer holds, no slot is
/// free, or the engine rejects either bank.
pub fn register_bank(req: BankRequest) -> Option<BankHandle> {
    let mut guard = AUDIO.lock().ok()?;
    let inner = guard.as_mut()?;

    // 0. Idempotence, before anything can be created twice.
    if let Some(existing) = inner.banks.iter().find(|b| b.name == req.name) {
        return Some(existing.handle);
    }

    // A permanent failure has already been reported; stay quiet.
    if REGISTER_DECLINED.load(Ordering::Relaxed) {
        return None;
    }

    // 1. The vtable indices below were read off one specific XACT build; a
    //    different engine means they are unverified, so refuse to dispatch
    //    rather than call through them. Checked here rather than at init
    //    because the engine is COM-loaded during the game's onBoot, which
    //    finishes after our init thread does.
    if !xact_module_present() {
        REGISTER_DECLINED.store(true, Ordering::Relaxed);
        log_warn!(
            "GameAudio: {} not loaded -- engine vtable indices unverified; declining to register",
            XACT_MODULE
        );
        return None;
    }

    // 2. Null-check the manager. The game's own play path dereferences this
    //    global unconditionally, so the check is ours to make. This is the one
    //    transient failure here (it means boot has not finished), so it does
    //    not latch — a later attempt may legitimately succeed.
    let mgr = match inner.manager() {
        Some(m) => m,
        None => {
            if !NULL_MANAGER_WARNED.swap(true, Ordering::Relaxed) {
                log_warn!("GameAudio: audio manager global is null -- cannot register bank");
            }
            return None;
        }
    };

    unsafe {
        // 3. Safety gate: the game's slot mapper must still know exactly four
        //    named banks. A fifth would map to slot 4 and silently take over
        //    our bank's slot (leaving that game bank mute for the session).
        let named_banks = *inner.named_bank_count_site;
        if named_banks != EXPECTED_NAMED_BANKS {
            REGISTER_DECLINED.store(true, Ordering::Relaxed);
            log_warn!(
                "GameAudio: game maps {} named sound banks (expected {}) -- the free-slot assumption no longer holds; declining to register",
                named_banks,
                EXPECTED_NAMED_BANKS
            );
            return None;
        }

        // 4. Slot-layout sanity: the gameplay sound-effect bank must be
        //    present. Proves both the slot-array layout and that boot finished.
        let se_normal = *slot_bank_ptr(mgr, SE_NORMAL_SLOT);
        if se_normal.is_null() {
            REGISTER_DECLINED.store(true, Ordering::Relaxed);
            log_warn!(
                "GameAudio: slot layout check failed -- se_normal (slot {}) bank pointer is null; declining to register",
                SE_NORMAL_SLOT
            );
            return None;
        }
        log_info!(
            "GameAudio: slot layout OK (se_normal slot {} bank = {:p})",
            SE_NORMAL_SLOT,
            se_normal
        );

        // 5. COMPUTE the free slot — never hard-code one. Free means both
        //    fields untouched since the manager's constructor.
        let free_slot = (0..MGR_SLOT_COUNT).find(|&s| {
            *slot_file_id_ptr(mgr, s) == SLOT_EMPTY_FILE_ID && (*slot_bank_ptr(mgr, s)).is_null()
        });
        let slot = match free_slot {
            Some(s) => s,
            None => {
                REGISTER_DECLINED.store(true, Ordering::Relaxed);
                log_warn!(
                    "GameAudio: no free sound-bank slot among {} -- declining to register",
                    MGR_SLOT_COUNT
                );
                return None;
            }
        };
        log_info!(
            "GameAudio: claiming free sound-bank slot {} (of {})",
            slot,
            MGR_SLOT_COUNT
        );

        let engine = *(mgr.add(MGR_ENGINE_PTR) as *const *mut u8);
        if engine.is_null() {
            REGISTER_DECLINED.store(true, Ordering::Relaxed);
            log_warn!("GameAudio: engine pointer on the audio manager is null -- cannot register");
            return None;
        }

        // 6. Leak both buffers, BEFORE handing them to the engine so the
        //    pointers are stable for the bank's lifetime. XACT does not copy an
        //    in-memory wave bank's data, and SoundBank::Initialize likewise
        //    retains the XSB pointer, so freeing either would leave the engine
        //    reading freed memory. The bank lives for the process lifetime
        //    (there is deliberately no teardown path), so this leak is the
        //    correct ownership model rather than a lapse. A failed creation
        //    below leaks these few KB permanently, which is preferable to any
        //    scheme that could hand the engine a dangling pointer.
        let xwb: &'static [u8] = Box::leak(req.xwb.into_boxed_slice());
        let xsb: &'static [u8] = Box::leak(req.xsb.into_boxed_slice());

        // 7. Wave bank FIRST. `CreateSoundBank` provably does not resolve wave
        //    banks (it allocates a zeroed array and returns) — linking is by
        //    internal name, late — and an in-memory wave bank is fully prepared
        //    synchronously inside this call, so ordering it first removes any
        //    dependence on that lazy resolution for nothing.
        let create_wave_bank: CreateBankFn = vtable_fn(engine, ENGINE_VT_CREATE_MEM_WAVE_BANK);
        let mut wave_bank: *mut u8 = std::ptr::null_mut();
        let hr = create_wave_bank(engine, xwb.as_ptr(), xwb.len() as u32, 0, 0, &mut wave_bank);
        log_info!(
            "GameAudio: CreateInMemoryWaveBank('{}', {} bytes) hr=0x{:08X} bank={:p}",
            req.name,
            xwb.len(),
            hr as u32,
            wave_bank
        );
        if hr < 0 {
            REGISTER_DECLINED.store(true, Ordering::Relaxed);
            log_warn!(
                "GameAudio: CreateInMemoryWaveBank failed hr=0x{:08X} ({}) -- '{}' not registered",
                hr as u32,
                hresult_note(hr),
                req.name
            );
            return None;
        }

        // 8. Then the sound bank. Note the game's own loader ignores this
        //    HRESULT, which is why a malformed bank looks like "audio went
        //    dark" there; we log it.
        let create_sound_bank: CreateBankFn = vtable_fn(engine, ENGINE_VT_CREATE_SOUND_BANK);
        let mut sound_bank: *mut u8 = std::ptr::null_mut();
        let hr = create_sound_bank(
            engine,
            xsb.as_ptr(),
            xsb.len() as u32,
            0,
            0,
            &mut sound_bank,
        );
        log_info!(
            "GameAudio: CreateSoundBank('{}', {} bytes) hr=0x{:08X} bank={:p}",
            req.name,
            xsb.len(),
            hr as u32,
            sound_bank
        );
        if hr < 0 || sound_bank.is_null() {
            REGISTER_DECLINED.store(true, Ordering::Relaxed);
            log_warn!(
                "GameAudio: CreateSoundBank failed hr=0x{:08X} ({}) -- '{}' not registered",
                hr as u32,
                hresult_note(hr),
                req.name
            );
            return None;
        }

        // 9. Claim the slot with a SINGLE write.
        //
        //    ONLY the bank pointer. DO NOT WRITE `file_id` — leaving it at -1
        //    is the entire reason this bank survives. The one function in the
        //    game that destroys a sound-bank slot selects its victim with a
        //    linear "find the slot whose file_id equals this file id" search
        //    over all six slots, and destroys nothing when nothing matches.
        //    Our slot's file_id is -1 from the manager's constructor and can
        //    never be a live file id, so the search can never match us and the
        //    bank outlives every song load, song unload and scene transition.
        //    Writing a plausible-looking file_id here — the obvious "fix" for
        //    the half-populated slot this leaves behind — would make the
        //    destroyer target us and the mod would go silent mid-session with
        //    no error. Nothing in the game reads the two fields together
        //    except the admission guard on its own bank loader, so the
        //    half-populated state is inert.
        *slot_bank_ptr(mgr, slot) = sound_bank;

        let handle = BankHandle { slot };
        inner.banks.push(RegisteredBank {
            name: req.name.to_string(),
            handle,
            sound_bank,
            logged_cues: Vec::new(),
        });
        log_info!(
            "GameAudio: bank '{}' registered in slot {} (file_id left at {} deliberately)",
            req.name,
            slot,
            SLOT_EMPTY_FILE_ID
        );
        Some(handle)
    }
}

/// Play a cue by name from a registered bank. **GAME THREAD ONLY.**
///
/// `pan` is `-1.0` left … `0.0` centre … `+1.0` right. Returns `false` if the
/// manager is not ready or the game's façade reported failure (unknown cue, or
/// its shared cue-handle table exhausted) — warning **once per session** in
/// either case, never per call.
pub fn play_cue(bank: BankHandle, cue: &CStr, pan: f32) -> bool {
    let mut guard = match AUDIO.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let inner = match guard.as_mut() {
        Some(i) => i,
        None => return false,
    };

    // Guard: the game's play path dereferences the manager global unchecked.
    if inner.manager().is_none() {
        if !NULL_MANAGER_WARNED.swap(true, Ordering::Relaxed) {
            log_warn!("GameAudio: audio manager global is null -- skipping playback");
        }
        return false;
    }

    // Diagnostic, once per cue name: what index the bank resolves it to.
    // Deliberately info rather than warn, so that a genuinely missing cue
    // still produces exactly ONE warning for the session (the one below).
    log_cue_index_once(inner, bank, cue);

    let handle = unsafe { (inner.se_play)(bank.slot, cue.as_ptr(), pan) };
    if handle == SE_PLAY_FAILED {
        if !PLAY_FAILURE_WARNED.swap(true, Ordering::Relaxed) {
            log_warn!(
                "GameAudio: se_play(slot {}, {:?}) returned the failure sentinel -- unknown cue, muted by the game's sound-effect filter, or its cue handle table is exhausted (warned once)",
                bank.slot,
                cue
            );
        }
        return false;
    }
    true
}

// ── The rewritable tick bank ─────────────────────────────────────────
//
// The assist-tick pre-mixed track's engine surface: one immortal in-memory
// bank pair whose single wave's sample bytes are rewritten per song, played
// as one cue via a direct `SoundBank::Play` dispatch (the `se_play` façade
// hardwires `timeOffset` to 0, and the track needs the seek for late starts
// and rewind re-anchoring). Design:
// `.agents/planning/20260729-assist-tick-premixed-track/design/detailed-design.md`
// §"Components 1".

/// Register the tick bank with the game's engine. **GAME THREAD ONLY.**
///
/// Reuses [`register_bank`]'s verified-engine and manager-sanity gates and
/// its leak-then-create sequence, but — deliberately — claims **no** manager
/// sound-bank slot (see [`TickBankHandle`] for the full rationale: nothing
/// dispatches to this bank through the slots, mid-song registration would
/// find none free, and a slot-less bank is invisible to the game's bank
/// destroyer). The named-bank-count gate is likewise not consulted: it
/// protects only the free-slot assumption.
///
/// Idempotent per `name`. Returns `None` — with exactly one warning per
/// session — on any permanent failure (missing engine module, manager layout
/// check, engine rejection); a null manager is transient (boot not finished)
/// and does not latch.
pub fn register_tick_bank(req: TickBankRequest) -> Option<TickBankHandle> {
    let mut guard = AUDIO.lock().ok()?;
    let inner = guard.as_mut()?;

    // 0. Idempotence.
    if let Some((existing_name, handle)) = &inner.tick_bank {
        if existing_name == req.name {
            return Some(*handle);
        }
        // One tick bank per process by design (NFR-2); a second name is a
        // caller bug worth exactly one loud line.
        if !TICK_REGISTER_DECLINED.swap(true, Ordering::Relaxed) {
            log_warn!(
                "GameAudio: tick bank '{}' already registered -- refusing a second ('{}')",
                existing_name,
                req.name
            );
        }
        return None;
    }
    if TICK_REGISTER_DECLINED.load(Ordering::Relaxed) {
        return None;
    }

    // Sanity: the segment must lie inside the buffer and run exactly to its
    // end (the synthesized container's invariant; a violation is a caller
    // bug, permanent by nature).
    if req.sample_seg_offset + req.sample_seg_len != req.xwb.len() {
        TICK_REGISTER_DECLINED.store(true, Ordering::Relaxed);
        log_warn!(
            "GameAudio: tick bank '{}' sample segment ({} + {}) does not run to the XWB's end ({}) -- declining to register",
            req.name,
            req.sample_seg_offset,
            req.sample_seg_len,
            req.xwb.len()
        );
        return None;
    }

    // 1. Verified-engine gate (same as register_bank).
    if !xact_module_present() {
        TICK_REGISTER_DECLINED.store(true, Ordering::Relaxed);
        log_warn!(
            "GameAudio: {} not loaded -- engine vtable indices unverified; declining to register the tick bank",
            XACT_MODULE
        );
        return None;
    }

    // 2. Manager null-check (transient — boot may not have finished).
    let mgr = match inner.manager() {
        Some(m) => m,
        None => {
            if !NULL_MANAGER_WARNED.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "GameAudio: audio manager global is null -- cannot register the tick bank"
                );
            }
            return None;
        }
    };

    unsafe {
        // 3. Manager-layout sanity: the gameplay SE bank must be present in
        //    its slot. We claim no slot, but the engine pointer is read off
        //    this same object at +0x00, so the layout proof still carries.
        let se_normal = *slot_bank_ptr(mgr, SE_NORMAL_SLOT);
        if se_normal.is_null() {
            TICK_REGISTER_DECLINED.store(true, Ordering::Relaxed);
            log_warn!(
                "GameAudio: slot layout check failed -- se_normal (slot {}) bank pointer is null; declining to register the tick bank",
                SE_NORMAL_SLOT
            );
            return None;
        }

        let engine = *(mgr.add(MGR_ENGINE_PTR) as *const *mut u8);
        if engine.is_null() {
            TICK_REGISTER_DECLINED.store(true, Ordering::Relaxed);
            log_warn!(
                "GameAudio: engine pointer on the audio manager is null -- cannot register the tick bank"
            );
            return None;
        }

        // 4. Leak both buffers BEFORE handing them to the engine (it retains
        //    both pointers for the bank's lifetime and never copies — the
        //    wave buffer is exactly what rewrite_tick_wave rewrites later).
        let sample_seg_offset = req.sample_seg_offset;
        let sample_len = req.sample_seg_len;
        let xwb: &'static [u8] = Box::leak(req.xwb.into_boxed_slice());
        let xsb: &'static [u8] = Box::leak(req.xsb.into_boxed_slice());

        // 5. Wave bank first (CreateSoundBank provably does not resolve wave
        //    banks — pairing is by internal name, late).
        let create_wave_bank: CreateBankFn = vtable_fn(engine, ENGINE_VT_CREATE_MEM_WAVE_BANK);
        let mut wave_bank: *mut u8 = std::ptr::null_mut();
        let hr = create_wave_bank(engine, xwb.as_ptr(), xwb.len() as u32, 0, 0, &mut wave_bank);
        log_info!(
            "GameAudio: tick CreateInMemoryWaveBank('{}', {} bytes) hr=0x{:08X} bank={:p}",
            req.name,
            xwb.len(),
            hr as u32,
            wave_bank
        );
        if hr < 0 {
            TICK_REGISTER_DECLINED.store(true, Ordering::Relaxed);
            log_warn!(
                "GameAudio: tick CreateInMemoryWaveBank failed hr=0x{:08X} ({}) -- '{}' not registered",
                hr as u32,
                hresult_note(hr),
                req.name
            );
            return None;
        }

        // 6. Then the sound bank.
        let create_sound_bank: CreateBankFn = vtable_fn(engine, ENGINE_VT_CREATE_SOUND_BANK);
        let mut sound_bank: *mut u8 = std::ptr::null_mut();
        let hr = create_sound_bank(
            engine,
            xsb.as_ptr(),
            xsb.len() as u32,
            0,
            0,
            &mut sound_bank,
        );
        log_info!(
            "GameAudio: tick CreateSoundBank('{}', {} bytes) hr=0x{:08X} bank={:p}",
            req.name,
            xsb.len(),
            hr as u32,
            sound_bank
        );
        if hr < 0 || sound_bank.is_null() {
            TICK_REGISTER_DECLINED.store(true, Ordering::Relaxed);
            log_warn!(
                "GameAudio: tick CreateSoundBank failed hr=0x{:08X} ({}) -- '{}' not registered",
                hr as u32,
                hresult_note(hr),
                req.name
            );
            return None;
        }

        // 7. NO slot write — deliberate (see the function doc). The engine
        //    holds both banks internally, paired by name; we retain the
        //    dispatch pointer and the rewritable segment in the handle.
        let handle = TickBankHandle {
            sound_bank: sound_bank as usize,
            sample_seg: xwb.as_ptr().add(sample_seg_offset) as usize,
            sample_len,
        };
        inner.tick_bank = Some((req.name.to_string(), handle));
        log_info!(
            "GameAudio: tick bank '{}' registered (no manager slot, deliberately; sample segment {} bytes @ {:p})",
            req.name,
            sample_len,
            handle.sample_seg as *const u8
        );
        Some(handle)
    }
}

/// Overwrite the tick wave's sample bytes in place, dropping the first
/// `skip_bytes` of `encoded` — the block-aligned content shift that replaces
/// `Play`'s `timeOffset` (live-refuted as a seek 2026-07-29: the engine has
/// no sample-offset start; an already-due wave starts at sample 0). The
/// segment is filled with `encoded[skip_bytes..]` and the freed tail with
/// encoded-silence blocks, so a shifted track plays every clap `skip_bytes`
/// worth of blocks earlier and stays exactly segment-length. Compute
/// `skip_bytes` with `se_bank_synth::shift_bytes_for_ms` (whole blocks,
/// enforced here).
///
/// **GAME THREAD ONLY**, and only while no tick cue is live — the caller
/// (the mod's per-song state machine) guarantees an immediate [`stop_cue`]
/// preceded this, so no engine read of the buffer can be in flight (design
/// assumption 3).
///
/// `encoded` must be exactly the segment length (the synthesis module pads
/// with encoded silence); a mismatch or an unaligned/oversized shift is
/// refused with one warning per session.
///
/// `mute_head_bytes` (block-aligned) replaces the FIRST bytes of the served
/// region with encoded silence instead of `encoded` content — the reset
/// re-anchor's clap floor (Training Mode Step 7): after a seek/loop reset
/// to target T, claps authored for notes BEFORE T (consumed-neutral in the
/// rebuild — they will never be judged) must not sound during the silent
/// approach lead, so positions `[skip, skip+mute)` of the track are served
/// as silence. `0` = no mute (every pre-fix caller's behavior).
pub fn rewrite_tick_wave(
    h: &TickBankHandle,
    encoded: &[u8],
    skip_bytes: usize,
    mute_head_bytes: usize,
) -> bool {
    let silence = crate::services::se_bank_synth::adpcm::silence_block();
    if encoded.len() != h.sample_len
        || skip_bytes % silence.len() != 0
        || skip_bytes > h.sample_len
        || mute_head_bytes % silence.len() != 0
        || mute_head_bytes > h.sample_len - skip_bytes
    {
        if !TICK_REWRITE_WARNED.swap(true, Ordering::Relaxed) {
            log_warn!(
                "GameAudio: rewrite_tick_wave got {} bytes / skip {} / mute {} (segment {}, block {}) -- refusing (warned once)",
                encoded.len(),
                skip_bytes,
                mute_head_bytes,
                h.sample_len,
                silence.len()
            );
        }
        return false;
    }
    let kept = h.sample_len - skip_bytes;
    unsafe {
        let seg = h.sample_seg as *mut u8;
        let mut off = 0;
        while off < mute_head_bytes {
            std::ptr::copy_nonoverlapping(silence.as_ptr(), seg.add(off), silence.len());
            off += silence.len();
        }
        std::ptr::copy_nonoverlapping(
            encoded.as_ptr().add(skip_bytes + mute_head_bytes),
            seg.add(mute_head_bytes),
            kept - mute_head_bytes,
        );
        let mut off = kept;
        while off < h.sample_len {
            std::ptr::copy_nonoverlapping(silence.as_ptr(), seg.add(off), silence.len());
            off += silence.len();
        }
    }
    true
}

/// Start the tick cue from the top of the (already shifted) track. **GAME
/// THREAD ONLY.**
///
/// Dispatches `IXACT2SoundBank::Play` directly (vt+0x20) with
/// `timeOffset = 0` — the parameter is deliberately not exposed: it is NOT a
/// seek (it only fast-forwards the cue's event timeline; an already-due
/// wave starts at sample 0), so seeking is done by shifting the content in
/// [`rewrite_tick_wave`] instead. `ppCue` is NULL, selecting the engine's
/// auto-release path; cue control is retained by name via [`stop_cue`]. Pan
/// is not touched — an un-matrixed cue renders centred, which is FR-6's
/// requirement anyway.
pub fn play_tick_track(h: &TickBankHandle, cue: &CStr) -> bool {
    let bank = h.sound_bank as *mut u8;
    unsafe {
        let get_cue_index: GetCueIndexFn = vtable_fn(bank, SOUND_BANK_VT_GET_CUE_INDEX);
        let index = get_cue_index(bank, cue.as_ptr());
        if index == CUE_NOT_FOUND {
            if !TICK_PLAY_WARNED.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "GameAudio: tick cue {:?} not found in the tick bank (warned once)",
                    cue
                );
            }
            return false;
        }
        let play: SoundBankPlayFn = vtable_fn(bank, SOUND_BANK_VT_PLAY);
        let hr = play(bank, index, 0, 0, std::ptr::null_mut());
        if hr < 0 {
            if !TICK_PLAY_WARNED.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "GameAudio: tick SoundBank::Play({:?}) failed hr=0x{:08X} ({}) (warned once)",
                    cue,
                    hr as u32,
                    hresult_note(hr)
                );
            }
            return false;
        }
    }
    true
}

/// Stop every instance of the tick cue, immediately. **GAME THREAD ONLY.**
///
/// Dispatches `IXACT2SoundBank::Stop` (vt+0x28) with flags =
/// [`STOP_IMMEDIATE`] — the only flag bit the engine accepts (anything else
/// is `E_INVALIDARG`); immediate (rather than as-authored) is required so a
/// following [`rewrite_tick_wave`] can rely on the voice being gone. Safe to
/// call with no cue playing (a no-op stop returns success).
pub fn stop_cue(h: &TickBankHandle, cue: &CStr) -> bool {
    let bank = h.sound_bank as *mut u8;
    unsafe {
        let get_cue_index: GetCueIndexFn = vtable_fn(bank, SOUND_BANK_VT_GET_CUE_INDEX);
        let index = get_cue_index(bank, cue.as_ptr());
        if index == CUE_NOT_FOUND {
            if !TICK_STOP_WARNED.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "GameAudio: tick cue {:?} not found in the tick bank on stop (warned once)",
                    cue
                );
            }
            return false;
        }
        let stop: SoundBankStopFn = vtable_fn(bank, SOUND_BANK_VT_STOP);
        let hr = stop(bank, index, STOP_IMMEDIATE);
        if hr < 0 {
            if !TICK_STOP_WARNED.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "GameAudio: tick SoundBank::Stop({:?}) failed hr=0x{:08X} ({}) (warned once)",
                    cue,
                    hr as u32,
                    hresult_note(hr)
                );
            }
            return false;
        }
    }
    true
}

// ── Internals ────────────────────────────────────────────────────────

/// Resolve and log a cue's index the first time it is played from `bank`.
/// No-op on repeat calls, and on a bank we have no record of.
fn log_cue_index_once(inner: &mut Inner, bank: BankHandle, cue: &CStr) {
    let name = cue.to_string_lossy().to_string();
    let Some(entry) = inner.banks.iter_mut().find(|b| b.handle.slot == bank.slot) else {
        return;
    };
    if entry.logged_cues.iter().any(|c| *c == name) {
        return;
    }
    entry.logged_cues.push(name.clone());

    let index = unsafe {
        let get_cue_index: GetCueIndexFn = vtable_fn(entry.sound_bank, SOUND_BANK_VT_GET_CUE_INDEX);
        get_cue_index(entry.sound_bank, cue.as_ptr())
    };
    if index == CUE_NOT_FOUND {
        log_info!(
            "GameAudio: cue '{}' NOT FOUND in bank '{}' (slot {})",
            name,
            entry.name,
            bank.slot
        );
    } else {
        log_info!(
            "GameAudio: cue '{}' -> index {} in bank '{}' (slot {})",
            name,
            index,
            entry.name,
            bank.slot
        );
    }
}

/// Whether the XACT engine build these vtable indices were read from is loaded.
fn xact_module_present() -> bool {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::GetModuleHandleA;
    unsafe { GetModuleHandleA(PCSTR(XACT_MODULE_CSTR.as_ptr() as *const u8)).is_ok() }
}
