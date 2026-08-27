//! Core infrastructure — low-level utilities shared across the entire hook DLL.
//!
//! These modules handle the foundational concerns that all other code depends on:
//! finding the game in memory, scanning for function signatures, reading/writing
//! game memory safely, managing function hooks, and logging.
//!
//! Nothing in `core/` depends on game-specific logic — it's all generic infrastructure
//! that could be reused for hooking any Windows DLL.

pub mod afp;
pub mod arc;
pub mod crash_handler;
pub mod hook_transaction;
#[cfg(test)]
mod hook_transaction_tests;
pub mod hooks;
pub mod ifs;
pub mod logger;
pub mod memory;
pub mod memory_patch;
#[cfg(test)]
mod memory_patch_tests;
pub mod module_resolver;
pub mod platform;
pub mod profiling;
pub mod scanner;
pub mod signatures;
pub mod ssq;
pub mod xact;
