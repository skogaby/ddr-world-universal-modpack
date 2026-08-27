//! Trait contract for a new note type (mines, and future lifts/rolls).
//!
//! Each note type owns its own SSQ chunk format, per-chart sidecar state,
//! parse logic, render binding, judge-side behavior, and reset semantics.
//! The `NoteTypeRegistry` holds one `Box<dyn NoteType>` per registered type
//! and dispatches to them from the shared hook callbacks.
//!
//! Keeping each type self-contained means that adding a new note kind in a
//! follow-up feature is a new `NoteType` impl + one `register()` call —
//! no rewrites to the parse, render, or judge scaffolding.

use crate::mods::note_types_expansion::notes_vec::GameNotesVec;
use crate::mods::note_types_expansion::timing::TempoConverter;

/// Render-time sprite binding used when the render hook encounters a note
/// with the type's `kind` value. The texture is resolved by name via the
/// existing `texture_resolver` service; UVs are baked per binding.
#[derive(Clone, Copy, Debug)]
pub struct RenderBinding {
    pub texture_name: &'static str,
    pub uv: [f32; 4], // left, top, right, bottom
}

/// Implemented by each new note type. Lifecycle:
///
/// 1. `on_chart_loaded` runs once per chart, from the Analyze hook, after
///    the vanilla parser has filled the Notes vector. The type parses its
///    own SSQ chunk, converts ticks to music_count via the supplied
///    TempoConverter, appends synthetic GameNote entries to `notes_vec`,
///    and populates its own private sidecar.
///
/// 2. `on_judge_tick` runs once per frame from the post-judge callback.
///    It queries the sidecar against the current music_count and foot panel
///    state to apply type-specific judgments (combo breaks, score penalties,
///    gauge damage, etc.).
///
/// 3. `render_binding` is called at most once per frame by the render hook
///    when it encounters a note with this type's `kind`. Cheap lookup —
///    the binding itself should be constant-shaped per chart.
///
/// 4. `reset` clears per-chart state when gameplay exits. Called via the
///    scene manager callback.
pub trait NoteType: Send {
    /// Short identifier (e.g. `"mines"`). Used in logs and, optionally,
    /// as a config-section key.
    fn id(&self) -> &'static str;

    /// The value this type writes into the kind byte (+0x00) of each
    /// injected note record. Must be unique across registered types —
    /// the registry enforces this at `register()` time.
    fn note_kind(&self) -> i8;

    /// Parse the type's SSQ chunk(s) from the raw blob, convert ticks to
    /// music_count via `tempo`, and append synthetic notes to `notes_vec`.
    /// Also populates the type's internal sidecar for judge-side use.
    ///
    /// `difficulty_code` is the 16-bit `(slot, style)` key from
    /// `docs/ssq_format.md §5.1` for the difficulty currently being parsed
    /// (e.g. `0x0318` = Double Expert). Implementations look up their chunks
    /// by `(own_kind, difficulty_code)`, mirroring the per-difficulty keying
    /// the vanilla step parser uses. This is what enables per-difficulty
    /// note-type sets (e.g. mines only on Challenge, none on Basic).
    ///
    /// Returns the number of notes injected on success, or an error if the
    /// chunk was malformed in a non-recoverable way. Implementations should
    /// prefer warn-and-skip over hard errors for per-entry issues so that
    /// a partially-malformed chart still loads.
    fn on_chart_loaded(
        &mut self,
        ssq_blob: &[u8],
        tempo: &TempoConverter,
        notes_vec: &mut GameNotesVec,
        difficulty_code: u16,
    ) -> Result<usize, NoteTypeError>;

    /// Per-frame judgment pass. `actor` is the GamePlayActor pointer;
    /// `music_count` is the current playhead; `foot_panel` is the current
    /// IFootPanel pointer (may be the vanilla user panel or Autoplay's
    /// AutoFootPanel — the type should not care which). Called from the
    /// post-judge callback registered by this mod.
    fn on_judge_tick(&mut self, actor: *mut u8, music_count: i32, foot_panel: *mut u8);

    /// Render binding for this type's `note_kind()`. The render hook caches
    /// the result per chart.
    fn render_binding(&self) -> RenderBinding;

    /// Clear all per-chart state. Called on gameplay-scene exit AND from
    /// the Analyze hook whenever a chart WITHOUT this mod's chunks is
    /// parsed (see `analyze_dispatcher` — a chunk-less chart never reaches
    /// `on_chart_loaded`, so without this call a sidecar filled by the
    /// last chunk-carrying chart would survive into the new chart and be
    /// judged against its timeline).
    ///
    /// Returns `true` if per-chart state was actually present (used by
    /// callers for forensic logging of stale-state drops).
    fn reset(&mut self) -> bool;
}

#[derive(Debug)]
pub enum NoteTypeError {
    /// The type's chunk was present but structurally invalid (wrong entry
    /// size, inconsistent length, etc.) and nothing could be salvaged.
    MalformedChunk(&'static str),
    /// The game's notes vector could not be grown (allocator failure).
    InjectionFailed(&'static str),
}
