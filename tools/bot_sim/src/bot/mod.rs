//! The Multiplayer Bot's pure cores, mounted from the repository. `planner.rs`
//! imports `super::skill`, which this module satisfies exactly as
//! `src/mods/multiplayer_bot/mod.rs` does in the DLL.
#[path = "../../../../src/mods/multiplayer_bot/eligibility.rs"]
pub mod eligibility;
#[path = "../../../../src/mods/multiplayer_bot/ghost.rs"]
pub mod ghost;
#[path = "../../../../src/mods/multiplayer_bot/planner.rs"]
pub mod planner;
#[path = "../../../../src/mods/multiplayer_bot/session.rs"]
pub mod session;
#[path = "../../../../src/mods/multiplayer_bot/skill.rs"]
pub mod skill;
