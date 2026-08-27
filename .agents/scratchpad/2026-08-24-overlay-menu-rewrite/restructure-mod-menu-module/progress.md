# Progress — restructure-mod-menu-module

Status: Complete (uncommitted — maintainer commits manually)

## Checklist

- [x] Create `src/mods/mod_menu/` with `mod.rs` / `rows.rs` / `input.rs` / `render.rs`
- [x] Delete `src/mods/mod_menu.rs`
- [x] `cargo check --target x86_64-pc-windows-msvc` clean (0 warnings)
- [x] Diff review: 87 delta lines, all mechanical (module decls, import redistribution,
      `pub(super)` markers, cross-module call paths, `super::config` →
      `crate::mods::config`); zero logic-line changes
- [x] `cargo fmt` — no churn
- [x] `./build.sh` clean (release DLL produced)

## Record

- Split map per plan.md; children access the parent module's private items (Rust
  descendant visibility), so `ModMenuState` fields needed no visibility changes;
  cross-module functions marked `pub(super)`.
- One deviation-free wrinkle: the `RowKind` re-export triggered `unused_imports`
  (cdylib crate; nothing external names it). Resolved with `#[allow(unused_imports)]`
  on the `pub use` block — established repo precedent
  (`src/services/custom_options/mod.rs:44`, `src/services/se_bank_synth/mod.rs:44`).
  Exact public API parity preserved.
- Commit intentionally skipped per AGENTS.md git rules (maintainer commits).

## Deviations

None.
