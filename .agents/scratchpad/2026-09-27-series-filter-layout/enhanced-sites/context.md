# Context — enhanced-sites
Task: `.agents/tasks/2026-09-27-series-filter-layout/step03/task-01-enhanced-sites.code-task.md`.
Approval chain as in the step-02 task (code-task-generator line; plan/design approved). Mode: auto.
Build/test: `cargo check --target x86_64-pc-windows-msvc`; `./scripts/validate_signatures.sh
~/Desktop/ddr_modules --json <log>/sweep.json`; `scripts/sig_harness/shape_diff.py --json … --dir
~/Desktop/ddr_modules --ref 20260915 --window W --names N`.
Paths: `src/core/signatures.rs`, `src/mods/series_expansion/mod.rs`.
