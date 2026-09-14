//! The `judgement_offsets.csv` grammar, shared BY CONSTRUCTION with the hook
//! DLL: this module IS `src/mods/per_song_judgement_offsets/csv.rs`, mounted
//! with `#[path]`. That file is documented dependency-free (no logging, no
//! `unsafe`, no game APIs) and is already host-tested through the same kind of
//! mount by `scripts/validate_judgement_offsets.sh`. If it ever grows a crate
//! dependency, this build breaks loudly — which is the desired signal that the
//! two sides' grammars are about to diverge.

#[path = "../../../src/mods/per_song_judgement_offsets/csv.rs"]
#[allow(dead_code)] // the updater uses a subset of the DLL module's API
mod dll_csv;

pub use dll_csv::{parse, serialize, CsvDoc};
