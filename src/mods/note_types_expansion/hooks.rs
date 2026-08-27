//! Hook glue for the Analyze entry point and the allocator interface.
//!
//! Installs a retour detour on the post-parse Analyze function. Inside the
//! detour we call the original first (so the game's Notes vector is fully
//! populated with regular arrows), then parse the SSQ blob out of the
//! SsqReader object and dispatch to the NoteTypeRegistry to inject any
//! synthetic notes each registered type wants to add.
//!
//! The detour callback is `extern "C"` (MS x64 ABI) because it replaces a
//! compiled member function. Member functions pass `this` in RCX under this
//! ABI, so the first parameter of the raw function pointer type is the
//! class pointer.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::mods::note_types_expansion::notes_vec::GameNotesVec;
use crate::mods::note_types_expansion::registry::NoteTypeRegistry;
use crate::mods::note_types_expansion::timing::TempoConverter;
use crate::{log_info, log_warn};

// SsqReader layout observed via Ghidra — the reader's constructor
// stores the blob pointer and size into member offsets 0x10 and 0x18,
// and callers construct it on the stack with a {vftable, 0, data, size,
// ...} pattern at 8-byte stride. Member offsets relative to the `this`
// pointer:
//
//   +0x00  vftable
//   +0x08  unused / flag
//   +0x10  data-blob pointer (the raw SSQ bytes)
//   +0x18  data-blob size in bytes
//   +0x20..+0x30     chunk-lookup pointers populated during reader init
const OFFSET_READER_DATA_PTR: usize = 0x10;
const OFFSET_READER_DATA_SIZE: usize = 0x18;

/// Convert `(mode, difficulty)` as passed to the Analyze member into the
/// 16-bit `(slot, style)` key that step chunks use for `param2`
/// (per `docs/ssq_format.md §5.1`). Returns 0 if either value is outside
/// the known range; callers treat 0 as "no matching chunk" and skip
/// injection.
///
/// Argument encoding (observable from the Analyze invocations the game
/// makes):
///   mode       — 0 = SINGLE, 1 = DOUBLE
///   difficulty — 0 = BEGINNER, 1 = BASIC, 2 = DIFFICULT, 3 = EXPERT, 4 = CHALLENGE
fn difficulty_code(mode: i32, difficulty: i32) -> u16 {
    let style_byte: u16 = match mode {
        0 => 0x14, // Single
        1 => 0x18, // Double
        _ => return 0,
    };
    let slot_byte: u16 = match difficulty {
        0 => 0x04, // Beginner
        1 => 0x01, // Basic
        2 => 0x02, // Difficult
        3 => 0x03, // Expert
        4 => 0x06, // Challenge
        _ => return 0,
    };
    (slot_byte << 8) | style_byte
}

/// Shape of the game's per-note judgment submitter (resolved via the
/// `judge_submit` signature). Arguments: `(actor, result, judge_code,
/// scratch_ptr)`. Called from inside `judgeNotes` for every regular-
/// grade, miss, shock-miss, and shock-NG judgment. Mines reuse the
/// shock-NG path (code `0x1031` with the result record's grade dword at
/// +0xC pre-set to 7) so combo break, life gauge damage, and the
/// on-screen NG display all fall out of the engine's own handling.
pub type JudgeSubmitFn =
    unsafe extern "C" fn(actor: *mut u8, result: *mut u8, judge_code: u32, scratch: *mut u8);

/// Resolved function pointers + heap handle address, populated at install
/// time. Read-only after install, so raw statics are safe.
struct ResolvedSymbols {
    agcs_heap_malloc: unsafe extern "C" fn(*const u8, usize, usize, usize) -> *mut u8,
    agcs_heap_free: unsafe extern "C" fn(*mut u8),
    app_heap_handle_addr: *const *const u8,
    judge_submit: JudgeSubmitFn,
}

unsafe impl Send for ResolvedSymbols {}
unsafe impl Sync for ResolvedSymbols {}

static SYMBOLS: OnceLock<ResolvedSymbols> = OnceLock::new();
static REGISTRY: OnceLock<Mutex<NoteTypeRegistry>> = OnceLock::new();

/// Base offset on `GamePlayActor` of the 8-element per-grade judgment-
/// count array (int32 per grade — MARVELOUS=0..NG=7). Detected from
/// the `judge_submit` function body at install time. `0` = not yet
/// detected.
static JUDGE_COUNTS_BASE_OFFSET: AtomicUsize = AtomicUsize::new(0);
/// Offset on `GamePlayActor` of the shock-arrow-count int32 field
/// (one of the three fields summed into the score formula's
/// denominator). Derived structurally as
/// `JUDGE_COUNTS_BASE_OFFSET - 4` — the engine's actor struct lays
/// out the note-count, freeze-count, and shock-arrow-count fields as
/// three consecutive int32s immediately before the judgment-count
/// array. `0` = not yet detected.
static SHOCK_ARROW_NUM_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Index into the 8-element judgment-count array for the OK grade.
/// Values follow the engine's grade enum (0=MARVELOUS, 1=PERFECT,
/// 2=GREAT, 3=GOOD, 4=BOO, 5=MISS, 6=OK, 7=NG).
pub const JUDGE_COUNT_INDEX_OK: usize = 6;

/// Return the detected judgment-count array base offset, or `None`
/// if detection hasn't run or failed.
pub fn judge_counts_base_offset() -> Option<usize> {
    match JUDGE_COUNTS_BASE_OFFSET.load(Ordering::Acquire) {
        0 => None,
        v => Some(v),
    }
}

/// Return the detected offset of the judgment-count array's OK slot,
/// or `None` if detection hasn't run or failed.
pub fn judge_counts_ok_offset() -> Option<usize> {
    judge_counts_base_offset().map(|base| base + JUDGE_COUNT_INDEX_OK * 4)
}

/// Return the detected shock-arrow-count field offset, or `None` if
/// detection hasn't run or failed.
pub fn shock_arrow_num_offset() -> Option<usize> {
    match SHOCK_ARROW_NUM_OFFSET.load(Ordering::Acquire) {
        0 => None,
        v => Some(v),
    }
}

/// Structural-adjacency offset of the current-combo int32 field,
/// relative to the judgment-count array base. Confirmed at a stable
/// +0x3c on observed builds — the engine's actor lays out eight
/// judgment-count int32s (+0x00), then freeze-OK count, fast-judge
/// count, slow-judge count, achievement-step count, use-EX-score
/// bool (1-byte + 3 pad), score, EX-score, and finally the combo
/// counter at that offset.
const COMBO_REL_OFFSET: usize = 0x3c;
/// Structural-adjacency offset of the max-combo int32 field, right
/// after the combo counter.
const MAX_COMBO_REL_OFFSET: usize = 0x40;
/// Structural-adjacency offset of the dead-flag byte, right after
/// the max-combo counter and the through-miss-combo counter.
const IS_DEAD_REL_OFFSET: usize = 0x48;

/// Return the detected current-combo int32 field offset, or `None`
/// if detection hasn't run or failed.
pub fn combo_offset() -> Option<usize> {
    judge_counts_base_offset().map(|base| base + COMBO_REL_OFFSET)
}

/// Return the detected max-combo int32 field offset, or `None` if
/// detection hasn't run or failed.
pub fn max_combo_offset() -> Option<usize> {
    judge_counts_base_offset().map(|base| base + MAX_COMBO_REL_OFFSET)
}

/// Return the detected dead-flag byte offset, or `None` if
/// detection hasn't run or failed.
pub fn is_dead_offset() -> Option<usize> {
    judge_counts_base_offset().map(|base| base + IS_DEAD_REL_OFFSET)
}

/// Scan the `judge_submit` function body for the `INC dword ptr
/// [RDI + R12*4 + disp32]` instruction that increments the per-grade
/// judgment-count slot, extract its disp32, and store both that
/// offset and `offset - 4` (the shock-arrow-count field) as statics.
///
/// The instruction encoding is fixed:
///
/// ```text
///   42 FF 84 A7 dd dd dd dd
///   ^^ ^^ ^^ ^^ ^^^^^^^^^^^
///   │  │  │  │  disp32 = judgment-count array offset
///   │  │  │  SIB:  scale=4, index=R12 (ext), base=RDI
///   │  │  ModRM: INC /0 with [SIB + disp32]
///   │  INC opcode
///   REX.X (extends SIB.index to R12)
/// ```
///
/// The prefix `42 FF 84 A7` is the full instruction up to the disp32;
/// we scan the first 256 bytes of the function for that 4-byte
/// sequence and read the 4 bytes that follow as a little-endian u32.
///
/// Returns `true` if both offsets were resolved successfully.
unsafe fn detect_actor_field_offsets(judge_submit_addr: *const u8) -> bool {
    const PREFIX: [u8; 4] = [0x42, 0xFF, 0x84, 0xA7];
    const SCAN_LEN: usize = 256;

    let body = std::slice::from_raw_parts(judge_submit_addr, SCAN_LEN);
    let mut found_disp: Option<u32> = None;
    for i in 0..body.len().saturating_sub(8) {
        if body[i..i + 4] == PREFIX {
            let disp = u32::from_le_bytes([body[i + 4], body[i + 5], body[i + 6], body[i + 7]]);
            if let Some(prior) = found_disp {
                if prior != disp {
                    log_warn!(
                        "NoteTypesExpansion hooks: ambiguous judge-count INC — multiple displacements found (prior={:#x}, new={:#x})",
                        prior, disp,
                    );
                    return false;
                }
            }
            found_disp = Some(disp);
        }
    }

    let disp = match found_disp {
        Some(d) => d as usize,
        None => {
            log_warn!(
                "NoteTypesExpansion hooks: judgment-count array offset not detected (no INC [RDI+R12*4+disp32] in first {} bytes of judge_submit)",
                SCAN_LEN,
            );
            return false;
        }
    };

    if disp < 4 {
        log_warn!(
            "NoteTypesExpansion hooks: detected judgment-count array offset {:#x} is too small to derive the adjacent shock-arrow-count field",
            disp,
        );
        return false;
    }

    JUDGE_COUNTS_BASE_OFFSET.store(disp, Ordering::Release);
    SHOCK_ARROW_NUM_OFFSET.store(disp - 4, Ordering::Release);
    log_info!(
        "NoteTypesExpansion hooks: detected judgment-count array @ +{:#x}, shock-arrow-count field @ +{:#x}",
        disp,
        disp - 4,
    );
    true
}

/// Access the mod-wide note-type registry. Lazily initializes on first call.
pub fn registry() -> &'static Mutex<NoteTypeRegistry> {
    REGISTRY.get_or_init(|| Mutex::new(NoteTypeRegistry::new()))
}

/// Return the resolved `judge_submit` function pointer for the mine-hit
/// dispatch, or `None` if `install` hasn't run yet. Post-judge callback
/// code calls this and invokes the returned function to trigger the
/// engine's shock-NG path on mine hits.
pub fn judge_submit_fn() -> Option<JudgeSubmitFn> {
    SYMBOLS.get().map(|s| s.judge_submit)
}

/// Wire up NTX's Analyze participation: stash the allocator/judge symbols,
/// detect the actor field offsets, and register the mine-injection
/// subscriber with the shared `services::analyze_hook` dispatcher (which
/// owns the single detour). Returns `false` if called twice or if the
/// shared dispatcher isn't available (mod then registers but does nothing).
///
/// The caller is expected to have validated that the addresses are non-null
/// (via `SignatureStore::require_address` or equivalent).
pub fn install(
    malloc_addr: *const u8,
    free_addr: *const u8,
    heap_handle_addr: *const u8,
    judge_submit_addr: *const u8,
) -> bool {
    unsafe {
        let malloc_fn: unsafe extern "C" fn(*const u8, usize, usize, usize) -> *mut u8 =
            std::mem::transmute(malloc_addr);
        let free_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(free_addr);
        let judge_submit_fn: JudgeSubmitFn = std::mem::transmute(judge_submit_addr);
        let resolved = ResolvedSymbols {
            agcs_heap_malloc: malloc_fn,
            agcs_heap_free: free_fn,
            app_heap_handle_addr: heap_handle_addr as *const *const u8,
            judge_submit: judge_submit_fn,
        };
        if SYMBOLS.set(resolved).is_err() {
            log_warn!("NoteTypesExpansion hooks: install called twice");
            return false;
        }

        // Derive GamePlayActor field offsets (judgment-count array
        // base, shock-arrow-count field) from the judge_submit
        // function body. These are needed so mines can contribute to
        // the score denominator (shock-arrow-count += mine_count)
        // and pre-credit as avoided (one increment of the OK slot
        // per mine) — matching shock-arrow semantics. Logged failure
        // falls through: the mod still functions for combo/gauge on
        // mine hits, but the score won't reflect mine hits until
        // detection succeeds.
        detect_actor_field_offsets(judge_submit_addr);
    }

    // Register with the shared Analyze dispatcher (owns the single detour).
    crate::services::analyze_hook::register_post(analyze_post);
    if !crate::services::analyze_hook::is_available() {
        log_warn!(
            "NoteTypesExpansion hooks: shared Analyze dispatcher unavailable -- \
             mine injection will not fire"
        );
        return false;
    }
    log_info!("NoteTypesExpansion hooks: registered Analyze post-subscriber");
    true
}

/// Post-original Analyze subscriber, registered with the shared
/// `services::analyze_hook` dispatcher (which owns the single detour and
/// has already run the original by the time this fires). On a successful
/// parse it walks the SsqReader's raw SSQ blob for note-type chunks and
/// injects. Cheap on the common path (one blob scan, one registry lock).
pub(crate) fn analyze_post(args: &crate::services::analyze_hook::AnalyzeArgs, orig_ret: u8) {
    // Safety: the args come straight from the game's Analyze call; the
    // shared dispatcher wraps this in catch_unwind.
    unsafe {
        analyze_inject(args.this, args.notes, args.mode, args.difficulty, orig_ret);
    }
}

/// The original NTX post-Analyze injection body, verbatim. Returns
/// `orig_ret` unchanged — the shared dispatcher owns the real return value;
/// the `u8` return type is retained only so the body's many early-out
/// `return orig_ret;` sites compile without edits. `measures`/`result`/
/// `radar`/`option` are unused by injection and therefore dropped.
unsafe fn analyze_inject(
    this: *mut u8,
    notes: *mut u8,
    mode: i32,
    difficulty: i32,
    orig_ret: u8,
) -> u8 {
    if orig_ret == 0 {
        return orig_ret;
    }

    if this.is_null() || notes.is_null() {
        return orig_ret;
    }
    let data_ptr = *(this.add(OFFSET_READER_DATA_PTR) as *const *const u8);
    let data_size = *(this.add(OFFSET_READER_DATA_SIZE) as *const usize);
    if data_ptr.is_null() || data_size == 0 {
        return orig_ret;
    }

    let diff_code = difficulty_code(mode, difficulty);
    if diff_code == 0 {
        return orig_ret;
    }

    let blob = std::slice::from_raw_parts(data_ptr, data_size);

    // Fast-path: scan the blob for any chunk kind that a registered type
    // cares about. This is a flat walk over chunk headers (no allocations,
    // no logging). If no relevant chunks exist (the common case for songs
    // without mines), clear any per-chart state left by the LAST
    // chunk-carrying chart and bail.
    //
    // The clear is load-bearing: `on_chart_loaded` (which normally
    // replaces the sidecars) never runs for a chunk-less chart, and the
    // mod's scene-change reset only fires when returning to the
    // attract/title range — so without it, a sidecar filled during the
    // game's boot-time all-SSQ analysis pass (or by an attract-demo
    // chart) survives into the first real song of a session, where the
    // judge tick crosses the stale mines against the new chart's
    // timeline: +1 full-combo denominator per stale mine, +1 OK/combo
    // per "avoided" one (first-song combo-inflation bug, 2026-08-23).
    if !has_any_registered_chunk(blob, diff_code) {
        if let Some(reg) = REGISTRY.get() {
            if let Ok(mut g) = reg.lock() {
                if g.reset_all() {
                    log_info!(
                        "NoteTypesExpansion: cleared stale per-chart state on analyze of \
                         chunk-less difficulty 0x{:04X} (left by a previously analyzed chart)",
                        diff_code,
                    );
                }
            }
        }
        return orig_ret;
    }

    // Slow path: a relevant chunk exists. Build the TempoConverter (two
    // small Vec allocations) and dispatch.
    let tempo = match TempoConverter::from_ssq(blob) {
        Some(t) => t,
        None => {
            log_warn!("NoteTypesExpansion: no usable tempo chunk -- skipping injection");
            return orig_ret;
        }
    };

    let symbols = match SYMBOLS.get() {
        Some(s) => s,
        None => return orig_ret,
    };
    let heap_handle = *symbols.app_heap_handle_addr;
    if heap_handle.is_null() {
        return orig_ret;
    }
    let mut notes_vec = GameNotesVec::new(
        notes,
        heap_handle,
        symbols.agcs_heap_malloc,
        symbols.agcs_heap_free,
    );

    let registry = registry();
    let mut reg = match registry.lock() {
        Ok(g) => g,
        Err(_) => return orig_ret,
    };

    if reg.is_empty() {
        return orig_ret;
    }

    reg.on_chart_loaded(blob, &tempo, &mut notes_vec, diff_code);

    orig_ret
}

/// Lightweight blob scan: returns true if the SSQ contains at least one
/// chunk whose (kind, param2) would be claimed by a registered note type.
/// Currently hardcodes the mine chunk kind (20) since that's the only
/// registered type. When future types (lifts, rolls) are added, extend the
/// match below. No allocations, no locks.
unsafe fn has_any_registered_chunk(blob: &[u8], difficulty_code: u16) -> bool {
    use crate::mods::note_types_expansion::ssq_chunk::CHUNK_HEADER_SIZE;
    let mut offset = 0usize;
    while offset + CHUNK_HEADER_SIZE <= blob.len() {
        let length = u32::from_le_bytes([
            blob[offset],
            blob[offset + 1],
            blob[offset + 2],
            blob[offset + 3],
        ]) as usize;
        if length == 0 {
            break;
        }
        if length < CHUNK_HEADER_SIZE || offset + length > blob.len() {
            break;
        }
        let kind = u16::from_le_bytes([blob[offset + 4], blob[offset + 5]]);
        let param2 = u16::from_le_bytes([blob[offset + 6], blob[offset + 7]]);
        if param2 == 0xFFFF {
            break;
        }
        // Mine chunk: kind=20, param2=difficulty_code
        if kind == 20 && param2 == difficulty_code {
            return true;
        }
        offset += length;
    }
    false
}

/// Clear per-chart state across all registered types. Called by the mod's
/// scene-change callback when gameplay ends.
pub fn dispatch_reset() {
    if let Some(reg) = REGISTRY.get() {
        if let Ok(mut g) = reg.lock() {
            g.reset_all();
        }
    }
}
