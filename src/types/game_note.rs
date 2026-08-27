//! Raw layout of the game's per-note record (the 0x60-byte struct the
//! engine's post-parse loops iterate) and the read-only helpers for
//! walking a gameplay actor's Results vector.
//!
//! The note records are owned by the game; everything here reads them.
//! The injection-side wrapper around the game's allocator-aware note
//! vector (`GameNotesVec`) lives in
//! `mods::note_types_expansion::notes_vec`, since it is bound to the
//! app-heap allocator and only that mod injects notes.
//!
//! Layout confirmed by Ghidra observations on the engine's post-parse
//! Analyze pass: the inner iteration loops walk with `ADD RBX, 0x60`,
//! the per-panel state array sits at +0x1C (dword reads in the shock
//! classifier), the per-panel length array sits at +0x3C, the first
//! signed byte (+0x00) is the note-kind discriminator checked against
//! small positive/negative integers throughout the freeze and shock
//! passes, the `beatCount` dword sits at +0x04, and `musicCount` at
//! +0x08 — the latter name confirmed by the game's own debug format
//! string embedded in gamemdx.dll:
//! `"shock ng : pressedDir=%d, musicCount=%d, note.musicCount=%d, diff=%d"`.

use std::mem;

/// Layout of the per-note record the engine's Analyze loops iterate at
/// stride 0x60 in `gamemdx.dll` (observed via Ghidra).
///
/// Total size: 0x60 bytes. `_pad*` fields exist to reach the known
/// stride; the game's compiled code iterates this struct at that stride
/// and the interior bytes in padding regions are not referenced by any
/// code path we touch.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GameNote {
    pub kind: i8,         // +0x00
    _pad1: [u8; 3],       // +0x01..+0x04
    pub beat_count: i32,  // +0x04
    pub music_count: i32, // +0x08
    _pad2: [u8; 0x10],    // +0x0C..+0x1C   (unknown interior fields)
    pub state: [i32; 8],  // +0x1C..+0x3C   (per-panel state enum)
    pub length: [i32; 8], // +0x3C..+0x5C   (per-panel freeze length)
    _pad3: [u8; 0x04],    // +0x5C..+0x60
}

const _: () = {
    // Compile-time assertion that the stride matches what the game's
    // compiled code walks with (`ADD RBX, 0x60` in the Analyze loops).
    assert!(mem::size_of::<GameNote>() == 0x60);
};

/// Panel bit convention — mirrors `docs/ssq_format.md §5.3` exactly. Kept
/// local to avoid pulling in SSQ-parsing dependencies from other modules.
pub mod panel {
    pub const P1_LEFT: u8 = 0x01;
    pub const P1_DOWN: u8 = 0x02;
    pub const P1_UP: u8 = 0x04;
    pub const P1_RIGHT: u8 = 0x08;
    pub const P2_LEFT: u8 = 0x10;
    pub const P2_DOWN: u8 = 0x20;
    pub const P2_UP: u8 = 0x40;
    pub const P2_RIGHT: u8 = 0x80;
}

/// Values the game assigns to each per-panel entry in the per-note
/// state array at offset +0x1C. NONE=0 for panels not struck at this
/// tick; TRG=1 for a trigger on the panel. REC/GEN/REP are observed but
/// not used for mines.
pub mod state {
    pub const NONE: i32 = 0;
    pub const TRG: i32 = 1;
    pub const REC: i32 = 2;
    pub const GEN: i32 = 3;
    pub const REP: i32 = 4;
}

/// Values observed in the first signed byte (+0x00) of each note
/// record, used by the engine's freeze/shock passes and render
/// collector as a note-kind discriminator. Positive small integers are
/// data kinds; negative values are control markers. Ghidra
/// cross-reference: the freeze classifier reads `(*p == DL)` against
/// `DL=0`; the render collector filters on "kind byte != 0".
pub mod kind {
    pub const ARROW: i8 = 0;
    pub const THINOUT: i8 = 1;
    pub const FREEZE_TAIL: i8 = 2;
    // Our own out-of-band kind values start at 20, outside the observed
    // vanilla range {-3, -2, -1, 0, 1, 2} plus negative control markers.
    // The NoteTypeRegistry enforces uniqueness across registered types.
    pub const MINE: i8 = 20;
}

/// Layout of the per-active-note entry the judge and renderer iterate
/// (the 0x40-byte Result record). Confirmed via Ghidra on the
/// judgeNotes inner loop (`ADD RBX, 0x40` stride, `[RBX]` dereferences
/// the underlying note pointer, the dword at `+0x08` is the
/// judge-completion timestamp, and the dword at `+0x0C` is the grade).
pub mod result {
    /// Stride between consecutive Result entries in the actor's Results
    /// vector.
    pub const STRIDE: usize = 0x40;
    /// Offset of the underlying note pointer inside a Result.
    pub const OFFSET_NOTE_PTR: usize = 0x00;
    /// Offset of the judge-completion timestamp inside a Result. A
    /// negative value (`-1` at chart load) means "not yet judged". The
    /// judge writes the player's `music_count` here when a note is
    /// judged. Several downstream consumers treat `>= 0` as "skip this
    /// entry; already handled":
    ///
    ///   * the main judge loop's skip test
    ///   * the autoplay panel updater's skip test
    ///   * the cursor walker that drives the on-screen score-update
    ///     broadcast (its predicate is "not yet reached the playhead"
    ///     = `judge-timestamp < 0 OR current_music_count < judge-timestamp`)
    ///
    /// Mines exploit the first two by writing a non-negative value
    /// here at first-frame time so both skip mine entries cleanly.
    /// The **specific value written** matters for the third consumer:
    /// writing the note's own `music_count` keeps that cursor
    /// advancing correctly as the playhead crosses each mine (same
    /// behavior as a normal judged arrow). Writing a large sentinel
    /// like `INT32_MAX` would instead freeze the cursor at the first
    /// mine, suppressing the score-update broadcast that drives the
    /// on-screen score display.
    pub const OFFSET_JUDGE_TIMESTAMP: usize = 0x08;
    /// Offset of the grade dword inside a Result. Values follow the
    /// engine's grade enum (0 = MARVELOUS, 1 = PERFECT, 2 = GREAT,
    /// 3 = GOOD, 4 = BOO, 5 = MISS, 6 = OK, 7 = NG, 0xFF = INVALID).
    /// The main judge loop skips entries whose grade is not INVALID.
    pub const OFFSET_GRADE: usize = 0x0C;
    /// Offset of the visible-judgment byte inside a Result. `0` =
    /// judgment suppressed (no on-screen display); `1` = show. The
    /// judge writes this as `(actor->0x1e8 == 0) as u8`.
    pub const OFFSET_VISIBLE: usize = 0x10;
    /// Grade value representing MISS. Mines are marked with this at
    /// first-frame time. MISS has coefficient 0 in the score formula
    /// and does not advance combo or max-combo, so marking with MISS
    /// keeps score and combo unaffected while making the entry
    /// "judged".
    pub const GRADE_MISS: u32 = 5;
    /// Offset of the Results vector's `begin` pointer on `GamePlayActor`.
    pub const ACTOR_OFFSET_RESULTS_BEGIN: usize = 0xB0;
    /// Offset of the Results vector's `end` pointer on `GamePlayActor`.
    pub const ACTOR_OFFSET_RESULTS_END: usize = 0xB8;
}

/// Iterate every Result entry in `[begin, end)` and invoke `callback`
/// with the entry pointer and its underlying Note pointer. Entries
/// whose Note pointer is null are skipped. Returns early if the range
/// is empty, misaligned, or has a reversed begin/end.
///
/// `unsafe` because it dereferences raw game-memory pointers. The
/// caller must guarantee the range is a well-formed Results vector
/// slice for the lifetime of the callback.
pub unsafe fn for_each_result(
    begin: *mut u8,
    end: *mut u8,
    mut callback: impl FnMut(*mut u8, *mut GameNote),
) {
    if begin.is_null() || end.is_null() || end <= begin {
        return;
    }
    let span = end.offset_from(begin) as usize;
    if !span.is_multiple_of(result::STRIDE) {
        return;
    }
    let count = span / result::STRIDE;
    for i in 0..count {
        let entry = begin.add(i * result::STRIDE);
        let note_slot = entry.add(result::OFFSET_NOTE_PTR) as *const *mut GameNote;
        let note = *note_slot;
        if note.is_null() {
            continue;
        }
        callback(entry, note);
    }
}

/// Read the Results vector's `[begin, end)` pointers from a
/// `GamePlayActor`. Returns `(begin, end)`, either of which may be
/// null if the actor hasn't populated the vector yet.
///
/// `unsafe` because it dereferences the actor pointer. The caller
/// must guarantee `actor` is a valid GamePlayActor for the duration
/// of the call.
pub unsafe fn actor_results_range(actor: *mut u8) -> (*mut u8, *mut u8) {
    if actor.is_null() {
        return (std::ptr::null_mut(), std::ptr::null_mut());
    }
    let begin = *(actor.add(result::ACTOR_OFFSET_RESULTS_BEGIN) as *const *mut u8);
    let end = *(actor.add(result::ACTOR_OFFSET_RESULTS_END) as *const *mut u8);
    (begin, end)
}

impl GameNote {
    /// Construct a single-panel mine note ready to inject.
    ///
    /// `panel_bits` is the one-hot bitmask (exactly one bit set) identifying
    /// which panel the mine occupies. Multi-panel mines are injected as
    /// multiple single-panel entries at the same `music_count`.
    pub fn mine(beat_count: i32, music_count: i32, panel_bits: u8) -> Self {
        let mut state = [0i32; 8];
        for (bit, panel) in state.iter_mut().enumerate() {
            if (panel_bits & (1 << bit)) != 0 {
                *panel = state::TRG;
            }
        }
        GameNote {
            kind: kind::MINE,
            _pad1: [0; 3],
            beat_count,
            music_count,
            _pad2: [0; 0x10],
            state,
            length: [0; 8],
            _pad3: [0; 4],
        }
    }
}
