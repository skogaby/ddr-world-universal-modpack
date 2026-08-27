//! Registry of NoteType implementations.
//!
//! Holds a list of registered types and dispatches to them from the shared
//! Analyze and judge callbacks. Enforces `note_kind()` uniqueness at
//! register time so two types can't collide on the same kind byte (+0x00
//! on the per-note record).

use crate::core::memory;
use crate::log_warn;
use crate::mods::note_types_expansion::note_type::{NoteType, RenderBinding};
use crate::mods::note_types_expansion::notes_vec::GameNotesVec;
use crate::mods::note_types_expansion::timing::TempoConverter;
use crate::types::game_note::{actor_results_range, for_each_result, result};

pub struct NoteTypeRegistry {
    types: Vec<Box<dyn NoteType>>,
}

impl NoteTypeRegistry {
    pub fn new() -> Self {
        Self { types: Vec::new() }
    }

    /// Register a new NoteType. Rejects duplicate `note_kind()` values
    /// with a warning — the later registration is dropped. This is a
    /// programmer error, caught early; in practice each feature registers
    /// its types at mod enable() and never overlaps.
    pub fn register(&mut self, nt: Box<dyn NoteType>) {
        let new_kind = nt.note_kind();
        if let Some(existing) = self.types.iter().find(|t| t.note_kind() == new_kind) {
            log_warn!(
                "NoteTypeRegistry: rejecting '{}' -- note_kind {} already registered by '{}'",
                nt.id(),
                new_kind,
                existing.id(),
            );
            return;
        }
        self.types.push(nt);
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Returns true if any registered type uses this note-kind byte value.
    pub fn handles_kind(&self, kind: i8) -> bool {
        self.types.iter().any(|t| t.note_kind() == kind)
    }

    /// Walk the `GamePlayActor`'s Results vector and mark every unjudged
    /// entry whose underlying Note has a kind registered with this
    /// registry as "judged at its own tick".
    ///
    /// For each such entry, on its first-frame encounter (guarded by
    /// the idempotent `judge-timestamp < 0` test), writes:
    ///
    ///   1. `result.judgeTimestamp = note.music_count` — any non-
    ///      negative value satisfies the `judge-timestamp >= 0` gate
    ///      the main judge loop and the autoplay panel updater use as
    ///      their "already handled, skip this" test. Writing the
    ///      note's own tick (rather than a large sentinel like
    ///      `INT32_MAX`) also keeps a second cursor walker — the one
    ///      that drives the on-screen score-update broadcast —
    ///      moving correctly.
    ///
    ///   2. `result.grade = MISS` — defense in depth: the main judge
    ///      loop also skips entries whose grade is not the "unjudged"
    ///      sentinel. The specific grade value here is decorative —
    ///      the Result entry's grade is not what the score formula
    ///      reads.
    ///
    /// The chart-wide score/combo bookkeeping (shock-arrow-count
    /// denominator, OK-slot and combo counter numerator) is handled
    /// lazily in each note type's `on_judge_tick`, per-mine at the
    /// frame its tick is crossed. This matches native shock-arrow
    /// semantics (resolve at playhead-crossing) and makes mines past
    /// the chart cutoff transparent — unreachable mines contribute to
    /// neither numerator nor denominator.
    ///
    /// Idempotent: entries already marked (judge-timestamp >= 0) are
    /// left alone on subsequent calls. The callback only writes on
    /// first-frame entry for each expansion-kind Result.
    ///
    /// `unsafe` because it dereferences raw game-memory pointers.
    pub unsafe fn mark_handled_results_skipped(&self, actor: *mut u8) {
        if self.types.is_empty() {
            return;
        }
        let (begin, end) = actor_results_range(actor);
        for_each_result(begin, end, |entry, note| {
            let note_kind = (*note).kind;
            if !self.handles_kind(note_kind) {
                return;
            }
            let timestamp_slot = entry.add(result::OFFSET_JUDGE_TIMESTAMP);
            // Only mark entries that are currently unjudged
            // (judge-timestamp < 0, the engine's chart-load default).
            // Rewriting an already-judged entry would corrupt the
            // engine's accounting.
            let current_ts = memory::read_i32(timestamp_slot);
            if current_ts < 0 {
                memory::write_i32(timestamp_slot, (*note).music_count);
                memory::write_u32(entry.add(result::OFFSET_GRADE), result::GRADE_MISS);
            }
        });
    }

    /// Look up a registered type's render binding by kind.
    pub fn binding_for_kind(&self, kind: i8) -> Option<RenderBinding> {
        self.types
            .iter()
            .find(|t| t.note_kind() == kind)
            .map(|t| t.render_binding())
    }

    /// Dispatch `on_chart_loaded` to every registered type in order. Each
    /// type's result is logged but does not abort the pass — a failure in
    /// one type does not prevent others from injecting their notes.
    ///
    /// After all types have dispatched, re-sorts the note-record vector
    /// by (beat_count, music_count) to restore the invariant that the
    /// post-parse Analyze pass establishes. Injected mines (appended at
    /// the tail) get interleaved with the vanilla-sorted regular notes
    /// so the game's render/judge walkers see a consistent ordering.
    pub fn on_chart_loaded(
        &mut self,
        ssq_blob: &[u8],
        tempo: &TempoConverter,
        notes_vec: &mut GameNotesVec,
        difficulty_code: u16,
    ) {
        let mut any_injected = false;
        for nt in self.types.iter_mut() {
            let id = nt.id();
            match nt.on_chart_loaded(ssq_blob, tempo, notes_vec, difficulty_code) {
                Ok(n) => {
                    if n > 0 {
                        any_injected = true;
                        crate::log_info!(
                            "NoteType '{}': injected {} note(s) for difficulty 0x{:04X}",
                            id,
                            n,
                            difficulty_code,
                        );
                    }
                }
                Err(e) => {
                    log_warn!(
                        "NoteType '{}': on_chart_loaded failed for difficulty 0x{:04X}: {:?}",
                        id,
                        difficulty_code,
                        e,
                    );
                }
            }
        }
        if any_injected {
            notes_vec.sort_by_beat_and_music_count();
            crate::log_info!(
                "NoteTypesExpansion: re-sorted Notes vector ({} entries) for difficulty 0x{:04X}",
                notes_vec.len(),
                difficulty_code,
            );
        }
    }

    /// Dispatch `on_judge_tick` to every registered type.
    pub fn on_judge_tick(&mut self, actor: *mut u8, music_count: i32, foot_panel: *mut u8) {
        for nt in self.types.iter_mut() {
            nt.on_judge_tick(actor, music_count, foot_panel);
        }
    }

    /// Clear per-chart state across all registered types. Returns `true`
    /// if any type actually had state to clear (callers use this to log
    /// stale-state drops without spamming the common empty case).
    pub fn reset_all(&mut self) -> bool {
        let mut any_had_state = false;
        for nt in self.types.iter_mut() {
            any_had_state |= nt.reset();
        }
        any_had_state
    }
}
