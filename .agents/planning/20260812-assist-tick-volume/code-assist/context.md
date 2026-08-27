# Context: Assist Tick Volume Option (code-assist run)

Task: implement all steps of `.agents/planning/20260812-assist-tick-volume/implementation/plan.md`
(Status: Approved 2026-08-12), which decomposes
`.agents/planning/20260812-assist-tick-volume/design/detailed-design.md` (Status: Approved
2026-08-12). Mode: auto. **Maintainer directives for this run:** no git commits by the
agent (maintainer commits themselves); code-task-generator deliberately skipped — the
session-approved design + plan stand in for the task-file approval gate (approval basis:
the maintainer approved both documents in this same session, so the "no code before a
human reviewed the approach" purpose is satisfied).

## Requirements

R1–R11 in the design document. Condensed acceptance criteria:

1. Scalar child row `assist_tick_volume` ("TICK EFFECT VOLUME (%)"), visible iff that
   side's `assist_tick` == 1 (`ShowWhen::Equals`), same-frame toggle (framework).
2. Range 25–175, fine 5, coarse 10, default 100, `ScalarFormat::Integer` (bare number).
3. Linear-amplitude gain, i32 headroom, i16 saturation; >100 % may soft-clip (accepted).
4. 100 % path must not touch the samples (byte-identical track).
5. FR-5 chosen side's value applies (one track per song).
6. Latched at GAMEPLAY entry beside LATCHED_ENABLED; applies next song; in-place restart
   keeps the latch.
7. PersistMode::Full (default), wire `mod_assist_tick_volume`, load_transform clamp+snap-5.
8. Fail-open to unity volume (row absent) if scalar machinery unavailable / registration
   fails; Duplicate re-enable reseeds atomics.
9. gen_option_labels.py: LABELS entry + WIDE Preview; regenerate; PNGs committed under
   `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`.
10. bemani-buddy: model JSON → codegen → migration → db model/queries → handler → .sqlx.
11. README (row_order example + id list + feature row) and AGENTS.md entry updates.

## Build & test commands

Modpack repo (run from repo root):

- `cargo check --target x86_64-pc-windows-msvc` — fast gate
- `cargo fmt` — whole crate, NEVER pass file args
- `./build.sh` — release build via cargo-xwin (the readiness gate)
- `./scripts/validate_se_bank_synth.sh` — host harness for se_bank_synth (needs sibling
  `ddr-chart-tools` checkout; present at the expected sibling location)
- `python3 scripts/gen_option_labels.py` — regenerates option textures (Pillow 11.2.1 OK)

bemani-buddy repo (sibling checkout; run from its root):

- `cargo build` / `cargo test` / `cargo clippy --workspace --all-targets` / `cargo fmt`
- `cargo run -p codegen -- <input> <output-dir>` — model JSON → wire structs
- `.sqlx` regeneration needs a local MySQL (`DATABASE_URL`) — availability to be checked
  at Step 4; escalate if absent.

Test conventions: hook-path code has no unit tests (validation = cabinet deploy + logs —
consolidated into plan Step 6, maintainer-run). The approved plan's per-step tests are the
build gates; `scale_pcm` is additionally covered by the design's R4 identity-shortcut
review and the Step 6 audibility legs. TDD's test-first cycle is therefore satisfied at
the plan level (cabinet checklist written before code), not via new unit tests — repo
convention, recorded here per the sop's audit requirement.

## Implementation paths

- `scripts/gen_option_labels.py` — LABELS (after line 74), PREVIEWS (after the
  assist_tick/on panel, ~line 257)
- `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/` — two new PNGs
- `src/mods/assist_tick.rs` — constants (near OPT_ID line 135), statics (near
  ASSIST_TICK_ENABLED line 229), SongState (+volume_percent, line 256/302/321),
  on_scene_change latch (line 343), Action::Anchor + tick_clock (407–496),
  spawn_synthesis (502), rebuild_for (801–904), enable() registration (1172–1198),
  disable() resets (1218–1221), on_option_change vicinity (788)
- `src/services/se_bank_synth/containers.rs` — new `scale_pcm`
- `src/services/se_bank_synth/mod.rs` — re-export (line 45 `pub use containers::{...}`)
- bemani-buddy: `models/ddr_world/playdata_3.json`, codegen output
  `crates/bemani-protocol/src/ddr_world/playdata_3.rs`, `migrations/0NN_*.sql` (next after
  012), `crates/db/src/models/ddr_world/profile.rs`, `crates/db/src/mysql/ddr_world/profile.rs`,
  `crates/game-server/src/handlers/ddr_world/playdata.rs`, `.sqlx/`
- Docs: `README.md`, `AGENTS.md`

## Notes / hazards

- `src/widgets/bounce.rs` is untracked and unrelated (maintainer's) — do not touch/stage.
- bemani-buddy working tree carries the uncommitted `mod_song_speed` change (migration
  012) — stack on it, never revert/clobber.
- Repo rules: no println (log_* macros only), `cargo fmt` whole-crate only, static muts
  via addr_of, one detour per target (not relevant here — zero new hooks).
- Registration order: child AFTER the parent's registration match; skip child on the
  parent's hard-failure arm.
