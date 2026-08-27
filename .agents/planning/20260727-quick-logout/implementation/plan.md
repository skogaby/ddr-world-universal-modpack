# Quick Logout — Implementation Plan

Status: Approved 2026-07-27

Design: `../design/detailed-design.md` (Approved 2026-07-27). Section references
(§) below point there. Repo gates apply to every step: `cargo check --target
x86_64-pc-windows-msvc` clean → `cargo fmt` (whole crate, never file args) →
`./build.sh` clean. Update `../progress.md` after each step.

**Validation convention (per maintainer, 2026-07-27):** no cabinet deploys
during Steps 1–4 — implementation steps validate by build gates plus code-level
expectations, and every runtime check is consolidated into Step 5's single
cabinet validation pass. Each of Steps 1–4 records its *expected log lines* so
Step 5 can verify them in one session. Exception clause: deploy mid-plan only if
a step surfaces something that genuinely cannot be settled statically (treat
that as a return-to-design moment, not a routine check). Deferring the A1 test
to Step 5 is safe: a negative would invalidate only the quick-logout save
benefit, not Step 4's sanitiser (which serves natural session ends too).

## Checklist

- [x] Step 1: Transition plumbing — `sequence_finish` signature, scene constants, redirect-repair accessor
- [x] Step 2: `stage_records` service + `premium_free` refactor
- [x] Step 3: `quick_logout` mod — trigger, gates, diagnostics (core end-to-end)
- [x] Step 4: Sanitised logout saves — `score_guard` semantics, scene-34 sanitiser, league strip, save policy
- [x] Step 5: Cabinet validation pass, docs, research fold-back

---

## Step 1: Transition plumbing

**Objective.** Everything the trigger call depends on, resolvable and observable
at boot before any behaviour ships.

**Guidance.**
- `core/signatures.rs`: add the `sequence_finish` definition (§4.1, exact bytes
  and description given there).
- `types/scenes.rs`: constants 32/33/34/35 + the 29/30 naming comment (§4.2).
- `services/scene_manager.rs`: `redirect_repair_available()` — an `AtomicBool`
  latched where the `advance_to_scene` hook install succeeds (§4.6).

**Tests.** `cargo check`; confirm the signature is registered in the store and
the constants compile against existing `scene::` usages. Expected log line for
Step 5: `sequence_finish` in the boot-time resolve count at a plausible offset
(cross-check against the `0x21DB90`/`0x21DF70` family).

**Integration.** Foundation only; consumed by Steps 3–4.

**Demo.** A deployable build in which nothing changed behaviourally — the new
signature, constants, and accessor exist and compile clean.

## Step 2: `stage_records` service + `premium_free` refactor

**Objective.** One shared, fail-closed decode of the play-record layout (§4.3),
including the newly decoded course-record offset, with `premium_free` consuming
it instead of its private copy.

**Guidance.**
- New `services/stage_records.rs` exactly per §4.3 (decode table, validation
  ranges, accessor API). Wire `init` into `lib.rs` before
  `custom_options_persistence::init`.
- Refactor `mods/premium_free.rs` onto the helper for the layout constants; its
  INC-patch handling and stage-counter disp8 decode stay local. Behaviour must
  be byte-for-byte identical (same log lines, same fail-closed conditions).

**Tests.** `cargo check`; a careful diff-review that the refactored
`premium_free` is behaviourally identical (same decode values, same fail-closed
conditions, same log lines — the stale-record fix is save-integrity
load-bearing, §4.3). Expected log lines for Step 5: the stage_records decode
line with the known 2026 layout values, and an unchanged Premium Free
virginise line during the regression check.

**Integration.** Uses nothing from Step 1. Supplies the session gate for Step 3
and the record writes for Step 4.

**Demo.** A deployable build whose boot will report
`stage_records: layout decoded (records work+0x590, stride 0x2B8, course rec
+0x2D8, course field +0x70)` with Premium Free riding the shared decode.

## Step 3: `quick_logout` mod — the core end-to-end behaviour

**Objective.** The full trigger: triple-9 at song select → TOTAL RESULTS →
logout save → THANK YOU → attract, with the tail diagnostics. Assumption A1
(does a forced `EAmExitRootSequence` actually save?) is *exercised* by this
code but *verified* in Step 5's cabinet pass — the FR4 diagnostics written here
are what make that verification a one-look log read.

**Guidance.**
- `mods/quick_logout.rs` per §4.7: gesture buffers, the four gates, trigger
  sequence (`add_redirect_once(30, 32)` then `finish(child, 30₁ᵢₙdₑₓ)`), `FIRED`
  latch + song-select reset, tail diagnostics (the two WARN conditions).
  Panic-free hook-path discipline throughout (§6).
- Register in `lib.rs`; enable-gate on `scene_manager::is_available() &&
  redirect_repair_available()`.
- Keep the trigger-context log (entered sides + per-side taint) even though the
  taint semantics only change in Step 4 — `score_guard::is_logout_suppressed`
  still exists at this point; read it under its old name and migrate in Step 4.

**Tests.** `cargo check`; static review of the four gates and the panic-free
discipline against §4.7/§6 (no unwrap/indexing in the callback paths, all
pointer walks null-guarded, no lock held across the `finish` call). Cabinet
items **1, 2, 6** (§7) move to Step 5; expected artifacts recorded for it:
trigger-context line, timed `29 → 32 → 33 → 34 → 35` chain, and the two WARN
conditions that must NOT appear on a clean run.

**Integration.** Consumes Step 1's signature/constants/accessor and Step 2's
`player_work` for the session gate.

**Demo.** A deployable build carrying the complete quick-logout behaviour —
the feature is code-complete for a clean session; only cabinet verification
remains.

## Step 4: Sanitised logout saves

**Objective.** The D21–D26 policy: tainted sides get score-stripped-but-forwarded
logout saves instead of suppressed ones, failing closed to suppression.

**Guidance.**
- `services/score_guard.rs`: rename `is_logout_suppressed` → `logout_taint`,
  add `mark_logout_sanitised` / `was_logout_sanitised` (reset in
  `reset_session`) (§4.4). Update the module docs — the taint model prose
  currently describes the suppression policy this step replaces.
- `services/custom_options_persistence.rs` (§4.5): resolve Ordinal 164
  non-fatally; register the scene-34 sanitiser callback where the detours
  install; rework the `savekind == 3` branch of `save_sender_trampoline` to the
  three-way policy (forward / sanitise+strip-league+forward / suppress).
- Migrate `quick_logout`'s trigger-context log to `logout_taint`.

**Tests.** `cargo check`; static review of the three-way `savekind == 3` branch
against §4.5 (specifically: `savekind == 2` untouched, suppression fallback on
*either* missing piece, league removal null-safe when the node is absent).
Cabinet items **3, 4, 5** (§7) move to Step 5; expected artifacts recorded for
it: the sanitiser virginise lines and the `SANITISED — scores stripped, profile
forwarded` / `SUPPRESSED (sanitiser unavailable)` wordings.

**Integration.** Consumes Step 2's record writes; Step 3's trigger will drive
the quick-logout variants of the Step 5 tests, while the natural-logout test
exercises this step's code with no Step 3 involvement.

**Demo.** A deployable, feature-complete build: the whole design is in the DLL,
ready for the consolidated validation pass.

## Step 5: Cabinet validation pass, docs, research fold-back

**Objective.** The single deploy: run the entire §7 checklist against the
feature-complete build, then documentation and the research fold-back.

**Guidance.**
- Deploy once (`./scripts/deploy.sh`). Run the §7 checklist **in its risk
  order**, folding in the per-step expectations recorded above:
  1. Boot log: `sequence_finish` resolved; stage_records decode line (Step 1/2).
  2. Clean-session quick logout end-to-end + backend save — **the A1 test**
     (§7.1). Either FR4 WARN firing here is a stop-and-diagnose moment (a
     diagnostic build is the sanctioned mid-plan deploy), not a workaround.
  3. Profile write-back round-trip (§7.2); latch abuse (§7.6).
  4. Premium Free stale-record regression (Step 2's deferred check).
  5. Tainted quick logout, tainted natural logout, 2P asymmetric taint
     (§7.3–7.5), verifying backend rows.
  6. PASELI close, FPS-preset timing, options-modal gesture answer (§7.7–7.9).
- Docs: AGENTS.md entry-point row (Quick Logout + the sanitise policy, in the
  house single-row style); README operator section (gesture, what it does,
  taint interaction, the Premium-Free empty-summary note); update
  `docs/quick_logout_research.md` — replace the "nothing cabinet-validated"
  banner with the validation outcomes, record the R3 answer, and note the
  implemented deviations (bare gesture, sanitise-not-suppress, league strip).
- `mod-config.json` example: add `"quick-logout": true` to the mods map.
- Write `../summary.md`; final `../progress.md` update to `done`.

**Tests.** The checklist items are the tests; log every outcome in
`../progress.md`'s deploy log.

**Integration.** Validation and documentation only — every code path landed
integrated in Steps 1–4; no orphaned code exists at any point.

**Demo.** The full feature verified on the cabinet in one session, and a fresh
reader can install, configure, trigger, and reason about Quick Logout
(including the tainted-session behaviour) from README + AGENTS.md alone.
