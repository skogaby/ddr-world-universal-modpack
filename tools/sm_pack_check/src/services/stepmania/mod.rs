//! `services::stepmania`'s pure files: identity derivation, conversion glue,
//! index records and the `<music>` synthesiser. Each imports only
//! `crate::core::*` and its siblings.
#[path = "../../../../../src/services/stepmania/convert.rs"]
pub mod convert;
#[path = "../../../../../src/services/stepmania/identity.rs"]
pub mod identity;
#[path = "../../../../../src/services/stepmania/model.rs"]
pub mod model;
#[path = "../../../../../src/services/stepmania/musicdb.rs"]
pub mod musicdb;
