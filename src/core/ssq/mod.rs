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
//! The write side (`writer`, `check`) was added 2026-10-22 for the
//! StepMania pack loader: pure synthesis of SSQ files from the
//! `core::sm` model and a self-check over the produced bytes, both
//! host-tested by `scripts/validate_stepmania.sh`.
//!
//! Format reference: `docs/ssq_format.md`, `docs/ssq_mine_chunk_format.md`.

pub mod check;
pub mod ssq_chunk;
pub mod timing;
pub mod writer;
