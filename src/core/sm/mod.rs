//! StepMania simfile parsing — pure, host-testable, panic-free.
//!
//! Mechanism: bytes → text (`text`, encoding detection) → MSD values
//! (`msd`) → `Song` (`simfile`, with `notes` decoding each chart body).
//! The runtime converts arbitrary community packs unattended inside the
//! game process, so every path here is bounded by the input size, never
//! panics on malformed data, and isolates failures per chart: a bad
//! chart is recorded in `Song::rejected`, the song survives. Warnings
//! are returned in the model (`Vec<Warning>`), never logged.
//!
//! Provenance: ported from the sibling `ddr-chart-tools` checkout at git
//! `a733404` (`src/ssc/{mod,notes,msd}.rs`, `src/sm/mod.rs`,
//! `src/model/{mod,rational}.rs`), stripped of `anyhow`/`log`/`thiserror`.
//!
//! Fidelity policy: for an input that needs none of the runtime
//! additions (no quads, delays, warps, fakes, lifts, per-chart timing,
//! annotations, non-UTF-8 text) the resulting timing, charts and notes
//! are equivalent to the tool's — `scripts/validate_stepmania.sh`
//! proves it indirectly by byte-comparing the SSQ built from this model
//! against the tool's CLI output. Additions are layered on top as
//! documented in each file's header (design §4.1).
//!
//! Purity contract: no `windows`, no `log_*!`, no `crate::` imports
//! outside `crate::core::sm`; the only external crate is `encoding_rs`.
//! `scripts/validate_stepmania.sh` mounts this directory via `#[path]`
//! under a `core` module mirror.

pub mod model;
pub mod msd;
pub mod notes;
pub mod scroll;
pub mod simfile;
pub mod text;
pub mod title_glyphs;

use model::{Song, Warning};
use simfile::parse;

/// Largest simfile the runtime will decode (design N7).
pub const MAX_SIMFILE_BYTES: usize = 4 * 1024 * 1024;

/// Decode and parse simfile bytes in one step. Returns `None` when the
/// input exceeds [`MAX_SIMFILE_BYTES`].
#[must_use]
pub fn parse_bytes(path_hint: &str, bytes: &[u8]) -> Option<Song> {
    if bytes.len() > MAX_SIMFILE_BYTES {
        return None;
    }
    let (text, encoding) = text::decode(bytes);
    let mut song = parse(path_hint, &text);
    if encoding != text::Encoding::Utf8 {
        song.warnings
            .push(Warning(format!("text decoded as {encoding:?} (not UTF-8)")));
    }
    Some(song)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bytes_enforces_cap_and_notes_encoding() {
        assert!(parse_bytes("a.ssc", &vec![b'#'; MAX_SIMFILE_BYTES + 1]).is_none());
        let song = parse_bytes("a.ssc", b"#TITLE:\x97\x5C;").expect("under cap");
        assert_eq!(song.title, "予");
        assert_eq!(song.warnings.len(), 1);
    }
}
