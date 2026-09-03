# Context — split-ssq-auto-discovery (Steps 1–3 of the plan, run as one task)

Approval chain: plan `.agents/planning/2026-09-03-split-ssq-auto-discovery/implementation/plan.md`
(`Status: Approved 2026-09-03`) ← design `design/detailed-design.md` (`Status: Approved 2026-09-03`)
← register `idea-honing.md` (`Readiness Confirmed 2026-09-03`). Maintainer explicitly authorized an
autonomous run through code-assist in-session. No `CODEASSIST.md` in the repo (noted). Project
instructions: `AGENTS.md` (incl. Git rules: NEVER commit unless asked — commit step skipped).

## Build / test commands (run from repo root)
- `cargo check --target x86_64-pc-windows-msvc`  (fast type check)
- `cargo fmt`  (whole crate, never per-file)
- `./build.sh`  (release build, cargo-xwin)
- `./scripts/validate_signatures.sh ~/Desktop/ddr_modules`  (required: signatures.rs changes)
- `./scripts/validate_split_ssq.sh`  (NEW: host harness for the pure resolver; `cargo test` cannot
  compile retour on this ARM host)

## Requirements / acceptance criteria (from design R1–R10)
R1 Rule A; R2 basename-opaque (toho); R3 stock ∪ LayeredFS mod dirs, content via
`find_first_modfile`; R4 fail-open (unknown⇒base, no index⇒original+WARN, d∉0..4⇒original,
sig miss⇒unregistered); R5 index built synchronously in enable(); R6 divergence oracle INFO
deduped, cap 64; R7 no config, toggle default ON, live; R8 allocation-free hot path; R9 panic-free
callback; R10 entry AOB only, unique on 4 builds.

## Patterns to follow
- `src/mods/announcer_mute.rs` — detour statics, `hooks::install_enabled`, `HOOK_INSTALLED`/`is_active`,
  disable = passthrough flag (detour never uninstalled).
- `src/core/ssq/ssq_chunk.rs` — chunk header walk semantics (len 0 terminator, 0xFFFF sentinel).
- `src/services/avs_layeredfs/mod_paths.rs` — `available_mods()`, `find_first_modfile(norm_rel)`.
- `scripts/validate_auto_calibration.sh` — host harness template.

## Files to touch
- NEW `src/mods/split_ssq_auto_discovery/{mod.rs,resolver.rs,discovery.rs}`
- `src/mods/mod.rs` (module decl), `src/lib.rs` (registration)
- `src/core/signatures.rs` (`build_ssq_path`)
- NEW `scripts/validate_split_ssq.sh`
- `AGENTS.md` (Key Entry Points row)
