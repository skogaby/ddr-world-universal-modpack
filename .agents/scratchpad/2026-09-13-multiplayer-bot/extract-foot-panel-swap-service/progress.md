# Progress — extract-foot-panel-swap-service

Status: Complete (uncommitted — maintainer commits manually)

## Checklist

- [x] `layout.rs` tests written (T1–T7), fail (module absent) — `logs/layout-test-red.log`
- [x] `layout.rs` implemented, tests pass on host (temp-crate mount) — 7/7, `logs/layout-test-green.log`
- [x] `mod.rs` service (init / callbacks / API)
- [x] `services/mod.rs` declaration
- [x] `lib.rs` init wiring (step 6b0, right after `judge_hook::init`)
- [x] `autoplay.rs` slimmed to a client
- [x] `mine_render.rs` comment retarget
- [x] `cargo check --target x86_64-pc-windows-msvc` clean — `logs/cargo-check.log`
- [x] `cargo fmt` (whole crate; touched only the 5 task files)
- [x] `./build.sh` clean — `logs/build.log`
- [x] Stale-reference + local-path greps clean
- [x] Code review pass

## TDD cycles

1. RED: `layout.rs` written with only the `#[cfg(test)]` module → temp-crate mount fails to
   compile (`cannot find type Controller / BotPanelFlags`).
2. GREEN: `Controller`, `effective_controller`, the four constants, `BotPanelFlags` added → 7/7.
3. Engine-facing `mod.rs` + wiring + autoplay slim → `cargo check` / `./build.sh` clean. (No
   host test possible for the engine-facing half — cabinet AC1–AC3/AC5 per the repo's rules.)
4. Post-`cargo fmt` re-run of the host test → 7/7.

## Files changed

- NEW `src/services/foot_panel_swap/layout.rs`, `src/services/foot_panel_swap/mod.rs`
- `src/services/mod.rs` (+1 line), `src/lib.rs` (+12 lines: step 6b0)
- `src/mods/autoplay.rs` (454 → 313 lines; statics/callbacks/registrations removed, module docs
  rewritten, `required_signatures() -> &[]`, `init` gates on the service, watermark predicate
  asks the service)
- `src/mods/note_types_expansion/mine_render.rs` (comment only)

## Code review findings (all resolved or accepted)

- `swap_out` now restores ANY non-null stash regardless of the side's controller. The old
  autoplay gated the restore on `AUTOPLAY_ENABLED[side]`, so a toggle landing between the pre
  and post callbacks of one frame would have left the auto panel in the slot permanently.
  Strictly safer; behaviour otherwise identical. (Improvement, kept.)
- `swap_in` gained an `actor.is_null()` guard the old code lacked (the dispatcher never passes
  null, belt-and-braces).
- `set_perfect` deliberately does NOT clear the score-guard taint on `disable` — the old mod
  did not either; the taint is the option callback's, cleared when the option turns OFF.
- `BOT_FILL` is stored but not yet read (consumed in plan Step 3). Crate-wide
  `#![allow(dead_code)]` covers it.
- Zero `static mut` in the new service (all atomics); no `unwrap`/`expect`/unmasked indexing in
  either callback.
- Signature sweep: no `signatures.rs` change; the three swap signatures move from autoplay's
  `required_signatures` to soft `get_address` consumers in the service (report.py will show
  that reclassification — expected). `judge_hook` still soft-resolves `judge_notes` itself.

## Deviations

- None from the task file. The commit step is skipped per `AGENTS.md` (maintainer commits).

## Refactoring notes

- `judge_hook` module docs still say "Per-subscriber state lives in the subscriber's own
  `static mut` slots" — atomics are the better shape (this service uses none); not touched
  (out of scope).

## Cabinet validation pending (maintainer)

Design §7.3 item 1: autoplay ON for the human ⇒ every note Marvelous + watermark; log shows
`FootPanelSwap started` / `FootPanelSwap: registered judge swap (foot panel offset 0x…, panel
object 88 bytes)` before the mods register, and `Autoplay: side=N ON`. Autoplay OFF ⇒ silent.
