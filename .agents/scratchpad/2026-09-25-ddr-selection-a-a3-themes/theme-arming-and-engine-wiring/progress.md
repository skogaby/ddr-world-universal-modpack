# Progress — theme-arming-and-engine-wiring

- [x] Baseline `cargo check` clean
- [x] `package_helper.rs` / `intro.rs` on the single naming source (`policy::package_name`)
- [x] `mod.rs`: `arm` writes `policy::engine_skin`; dev knob 1..=8; module doc
- [x] `stage_frame.rs`: slots 1..=8, `tex_number` prefix, reach check to slot 8, compile-time buffer
      bound
- [x] `movie_sel.rs`: `_sel` movies for eras only
- [x] Docs / logs (`song_info.rs`, `options.rs`)
- [x] Readiness gate: `cargo check` clean, `cargo fmt`, `./build.sh` clean (release DLL built),
      harness 151 passed
- [ ] Cabinet spike (plan Step 1 Demo) — maintainer; procedure in the feature `progress.md`

## Deviations
- The option row's footer description is unchanged (user-facing text is plan Step 9's).

Status: Complete (uncommitted — maintainer commits manually; cabinet spike pending)
