//! `services::stepmania`'s pure files: the cache layout, the injection
//! ceiling, conversion glue, identity derivation, index records and their
//! on-disk checks, the `<music>` synthesiser and the auto-sync ledger
//! format. `cache.rs` paths are relative to the game folder, so the tool
//! `set_current_dir`s there before touching them. Each imports only `crate::core::*`, its
//! siblings and the crate-root `log_*` macros.
#[path = "../../../../src/services/stepmania/cache.rs"]
pub mod cache;
#[path = "../../../../src/services/stepmania/ceiling.rs"]
pub mod ceiling;
#[path = "../../../../src/services/stepmania/convert.rs"]
pub mod convert;
#[path = "../../../../src/services/stepmania/identity.rs"]
pub mod identity;
#[path = "../../../../src/services/stepmania/model.rs"]
pub mod model;
#[path = "../../../../src/services/stepmania/musicdb.rs"]
pub mod musicdb;
#[path = "../../../../src/services/stepmania/records.rs"]
pub mod records;
#[path = "../../../../src/services/stepmania/sync_state.rs"]
pub mod sync_state;
#[path = "../../../../src/services/stepmania/thumbnail_arc.rs"]
pub mod thumbnail_arc;
