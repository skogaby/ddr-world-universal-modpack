//! The DLL's SSQ primitives, mounted from the repository so the simulator
//! parses charts with exactly the code the hook ships. `timing.rs` imports
//! `crate::core::ssq::ssq_chunk`, which this module path satisfies.
#[path = "../../../../../src/core/ssq/ssq_chunk.rs"]
pub mod ssq_chunk;
#[path = "../../../../../src/core/ssq/timing.rs"]
pub mod timing;
