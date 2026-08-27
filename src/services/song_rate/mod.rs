//! The song-rate service: lifecycle policy, clock publication, the
//! exactly-once wave-bank transaction, and the streaming binding
//! preflight/registry (the IO-callback detour pair that consumes them is
//! plan Step 4's final task).

pub mod binding;
pub mod clock_patch;
pub mod generator;
#[cfg(windows)]
pub mod io_callback_hook;
pub mod lifecycle;
pub mod preview;
pub mod real_speed;
#[cfg(windows)]
pub mod runtime;
pub mod selected_song;
pub mod tick_domain;
pub mod transaction;
pub mod wavebank_hook;
pub mod xact_runtime;

#[cfg(test)]
mod binding_tests;
#[cfg(test)]
mod clock_patch_tests;
#[cfg(test)]
mod generator_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod preview_tests;
#[cfg(test)]
mod real_speed_tests;
#[cfg(test)]
mod selected_song_tests;
#[cfg(test)]
mod tick_domain_tests;
#[cfg(test)]
mod transaction_tests;
#[cfg(test)]
mod wavebank_hook_tests;
#[cfg(test)]
mod xact_runtime_tests;
