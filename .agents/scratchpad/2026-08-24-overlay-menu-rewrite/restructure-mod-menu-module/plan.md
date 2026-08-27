# Plan — restructure-mod-menu-module

Status: Approved 2026-08-24 (auto mode; approval derived from the verified
code-task-generator → approved plan → approved design chain recorded in context.md)

## Test scenarios

This task introduces no new behavior; the acceptance criteria are compile/format/build
gates plus a diff review:

1. `cargo check --target x86_64-pc-windows-msvc` clean with zero edits outside
   `src/mods/mod_menu/` (covers AC-1: public API stability — the five registrants and
   lib.rs compile unmodified).
2. Diff review: every moved function body identical modulo `use` paths and
   `pub(super)` markers (AC-2).
3. `cargo fmt` (whole crate) produces no unrelated churn; `./build.sh` clean (AC-3).

No host tests exist for this module (engine-facing; no pure layer touched).

## Implementation approach

Per the split map in context.md: create `src/mods/mod_menu/` with the four files in one
pass (the source file is fully known), delete `src/mods/mod_menu.rs`, then run the
gates. Cross-module calls become `super::` / sibling-module paths; the only textual body
change is `super::config::save_mod_states` → `crate::mods::config::save_mod_states`
(module depth changed by one).

Risk: a missed cross-reference — caught immediately by `cargo check`.
