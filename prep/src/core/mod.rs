//! The DLL's pure pipeline, mounted from the repository so the tool converts
//! with exactly the code the hook ships. Each `mod.rs` below declares its
//! children relative to its own directory, and every module here imports only
//! `crate::core::{audio,sm,ssq,sync,xact,art}` plus the crate-root `log_*`
//! macros — the purity contract `scripts/validate_stepmania.sh` relies on too.
pub mod art;
#[path = "../../../src/core/audio/mod.rs"]
pub mod audio;
#[path = "../../../src/core/sm/mod.rs"]
pub mod sm;
#[path = "../../../src/core/ssq/mod.rs"]
pub mod ssq;
#[path = "../../../src/core/sync/mod.rs"]
pub mod sync;
#[path = "../../../src/core/xact/mod.rs"]
pub mod xact;
