//! AP2 (AFP animation binary) document model — parse, edit, serialize.
//!
//! The modpack's first full AP2 tag/timeline editor: header, string table,
//! recursive tag sections (frames, tags, frame-label name-reference arrays),
//! typed decode of the tags the S-Marvelous feature edits, and opaque
//! byte-preserving carriage of everything else. The core property everything
//! downstream stands on: **`serialize(parse(x)) == x` for any accepted `x`** —
//! every offset/length/count is recomputed at serialize time, and parse-time
//! layout metadata (region order, gap bytes, tag padding, raw carriage) makes
//! the recomputation reproduce the original layout for unmodified documents.
//!
//! Format knowledge transcribed from the bemaniutils project's parser
//! (`bemani/format/afp/swf.py`, Unlicense — the complete AP2 read
//! specification; approximate line numbers cited per item). The write side is
//! new work — no open AP2 serializer exists.
//!
//! Input contract: `Ap2Doc::parse` takes **descrambled** data (BSI applied,
//! string table already plaintext) — the `services/afp_patcher` seam hands
//! patch functions exactly that (`docs/afp_system.md` §1). The cipher helpers
//! below exist for fixtures and dev validation only.
//!
//! This module tree is **std-only / self-contained** (zero `crate::` imports)
//! so `scripts/validate_s_marvelous.sh` can mount `mod.rs` via `#[path]` into
//! a throwaway host crate and run the `#[cfg(test)]` suites on non-x86 hosts
//! (plain `cargo test` cannot run here — `retour` only compiles for x86).
//! Child `mod` declarations resolve relative to this file's real directory,
//! so the submodules mount along with it.

pub mod edit;
pub mod model;
pub mod parse;
pub mod write;

#[cfg(test)]
pub mod fixtures;
#[cfg(test)]
mod tests;

// Re-exported for consumers (nothing in the DLL uses them yet — the
// S-Marvelous AFP patches land in a later plan step); house re-export
// pattern per se_bank_synth/custom_options.
#[allow(unused_imports)]
pub use edit::{
    MultiShapeSegmentClone, NamedPlacement, SegmentCloneOpts, TagRemap, WordCloneOpts,
    WordSegmentClone,
};
#[allow(unused_imports)]
pub use model::{
    Ap2Doc, DefineSprite, FrameSpan, Label, MultColorField, OpaqueTag, PlaceObject,
    PlaceObjectParams, PlaceObjectView, RegionKind, SectionLayout, Shape, SpritePath, StringTable,
    Tag, TagSection,
};

/// Round up to 4-byte alignment (AP2 aligns tag payloads and string-table
/// appends to 4; misaligned string tables are a live-game FATAL —
/// `docs/afp_system.md` §2/§9).
pub const fn align4(n: usize) -> usize {
    (n + 3) & !3
}

// ---------------------------------------------------------------------------
// Rolling string-table cipher.
//
// DELIBERATE DUPLICATION of `src/core/afp.rs::{decode_stringtable,
// encode_stringtable}` (same rolling cipher: key starts at 128, increments
// per byte — bemaniutils swf.py `__descramble_stringtable` ~line 2670). This
// module must stay std-only with zero `crate::` imports so the
// validate_s_marvelous.sh harness can mount it standalone, so it cannot
// import `core::afp`. Keep the two implementations in sync; `core/afp.rs`
// carries the mirror comment.
// ---------------------------------------------------------------------------

/// Decode a scrambled AP2 string table to plaintext: byte `i` becomes
/// `(byte - (128 + i)) & 0xFF`.
pub fn decode_string_table(scrambled: &[u8]) -> Vec<u8> {
    scrambled
        .iter()
        .enumerate()
        .map(|(i, b)| (*b as u32).wrapping_sub(128 + i as u32) as u8)
        .collect()
}

/// Encode a plaintext AP2 string table back to cipher form: byte `i` becomes
/// `(byte + (128 + i)) & 0xFF`. Inverse of [`decode_string_table`].
pub fn encode_string_table(plain: &[u8]) -> Vec<u8> {
    plain
        .iter()
        .enumerate()
        .map(|(i, b)| (*b as u32).wrapping_add(128 + i as u32) as u8)
        .collect()
}
