//! SSQ chart-format primitives — pure parsing over `data/mdb_apx/ssq`
//! blobs, shared by every SSQ consumer (note_types_expansion's mine
//! injection, the chart-length service, training-mode tooling).
//!
//! Moved here from `mods/note_types_expansion/` (2026-08-16) when the
//! chart_length service became a second consumer — format code is
//! game-agnostic infrastructure per the module layering rules. The
//! original module paths remain valid via re-exports in
//! `note_types_expansion`.
//!
//! Format reference: `docs/ssq_format.md`.

pub mod ssq_chunk;
pub mod timing;
