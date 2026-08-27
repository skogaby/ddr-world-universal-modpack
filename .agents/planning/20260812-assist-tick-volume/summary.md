# Summary: Assist Tick Volume Option

Planning completed and approved 2026-08-12. **Implemented and cabinet-validated
end-to-end the same day — feature COMPLETE** (all 6 plan steps done; changes await the
maintainer's own commits in both repos, per their no-agent-commits directive).

## Artifacts

| File | Purpose |
|------|---------|
| `rough-idea.md` | The original feature request |
| `idea-honing.md` | Decision register — 12 decisions, all Accepted 2026-08-12; Readiness Confirmed 2026-08-12 |
| `research/orientation.md` | Code-level findings: custom_options child-row/scalar/persistence APIs, assist_tick synthesis pipeline, se_bank_synth mixer, song_speed precedent, gen_option_labels.py, bemani-buddy backend pattern (all with file:line cites) |
| `design/detailed-design.md` | **Approved 2026-08-12** (incl. same-day revision: backend codegen scope, consolidated final validation). Requirements R1–R11, architecture, components, data models, error handling, testing strategy |
| `implementation/plan.md` | **Approved 2026-08-12**. Six steps with checklist |
| `progress.md` | Live implementation resume point (seeded, nothing started) |

## Design in brief

A per-player scalar child row `assist_tick_volume` ("TICK EFFECT VOLUME (%)") on the MODS
tab, visible only while the parent `assist_tick` row is ON (`ShowWhen::Equals` — the
pacemaker_threshold mechanism). Range 25–175, fine step 5, coarse step 10, default 100 =
exact unity (byte-identical track to today). The FR-5 chosen side's value is latched at
GAMEPLAY entry and baked into the per-song pre-mixed tick track via a new pure
`se_bank_synth::scale_pcm` (linear amplitude, i32 headroom, i16 saturation) on the
existing synthesis thread — applies next song; quick-restart keeps the latch. Persistence
is the generic `PersistMode::Full` path (wire `mod_assist_tick_volume` + offline JSON
cache) with a clamp+snap `load_transform`; fail-open to unity volume if the scalar row
can't register. Assets via `scripts/gen_option_labels.py` (label + WIDE preview panel);
backend via bemani-buddy's model-JSON → codegen → migration → handler flow.

## Plan in brief

1. **Step 1** — label + preview textures (script entries, regenerate, commit PNGs)
2. **Step 2** — option row, atomics, latch, per-song threading + logging (no audio yet)
3. **Step 3** — `scale_pcm` + synthesis application (identity shortcut at 100 %)
4. **Step 4** — bemani-buddy: model JSON + codegen + migration + db/handler + `.sqlx`
   (stacks on the uncommitted `mod_song_speed` working-tree change there — do not clobber)
5. **Step 5** — README / AGENTS.md updates
6. **Step 6** — the single consolidated end-to-end cabinet validation pass
   (maintainer-run; no per-step demos, per maintainer direction)

Per-step gates: modpack `cargo check` → `cargo fmt` (whole crate) → `./build.sh`;
bemani-buddy `cargo build`/`test`/`clippy`/`fmt`; `validate_se_bank_synth.sh` unchanged.

## Watch items for implementation

- **bemani-buddy working tree is dirty** with the in-flight `mod_song_speed` change
  (migration `012_ddr_world_song_speed.sql`); Step 4 takes the next migration number and
  must coexist with it.
- R4 (exact unity at 100 %) is a hard requirement — the identity path must skip
  `scale_pcm` entirely.
- The child registration must sit **after** the parent's registration `match` and must
  not run on the parent's hard-failure arm (`ShowWhen` requires a registered parent).
- Maintain `progress.md` in this directory throughout implementation (repo convention);
  record Step 6 outcomes in its deploy-and-test log.

## Next steps

None — the feature shipped. Implementation was run directly via the code-assist sop
(code-task-generator waived by the maintainer); the run's working record is under
`code-assist/` in this directory. Outcome: modpack (textures, `assist_tick.rs` row/latch/
synthesis threading, `se_bank_synth::scale_pcm`, docs) + bemani-buddy (model JSON,
codegen, migration 013, db/handler, 5 unit tests, `.sqlx`), all gates green, cabinet
pass clean.
