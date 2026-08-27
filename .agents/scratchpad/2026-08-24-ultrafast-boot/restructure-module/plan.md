# Plan — task-01 restructure-module

Status: Approved 2026-08-24 (verified upstream approval chain; auto mode)

## Test scenarios
This is a pure file move; no new behavior, so no new tests. The "test" is the
gate suite: `cargo check --target x86_64-pc-windows-msvc` must pass identically
before and after, and the git diff must show only a rename.

## Implementation approach
1. `git mv src/mods/fast_bootup.rs src/mods/fast_bootup/mod.rs`
2. `cargo check` — expect zero edits needed (`pub mod fast_bootup;` resolves
   the directory form; `mods::fast_bootup::FastBootupMod` path unchanged)
