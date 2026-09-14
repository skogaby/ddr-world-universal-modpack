# Multiplayer Bot — PDD Summary

Planning completed 2026-09-13. Design and plan both approved by the maintainer.

## Artifacts

| File | Role |
|---|---|
| `rough-idea.md` | The maintainer's original request |
| `idea-honing.md` | Decision register D1–D24 (all Accepted / Assumed); `Readiness Confirmed 2026-09-13` |
| `research/orientation.md` | Blind-spot pass: sub-problems, candidate architectures, interactions, unknowns U1–U11 |
| `research/autoplay-internals.md` | Anatomy of the existing autoplay mod (the seam the bot controller extends) |
| `research/side-entry-model.md` | `PlayerWork` / `GameWork` layouts and the entry flow |
| `research/option-framework.md` | `custom_options` API shapes, texture checklist, traps |
| `research/versus-impersonation-re.md` | Ghidra: actor-count seam, versus word readers, commit/record mirroring, saves, extra-stage grant |
| `research/bot-controller-re.md` | Ghidra: `AutoFootPanel` vtable on all builds, judge algebra + grade windows, `Option` layout |
| `design/detailed-design.md` | **Approved 2026-09-13** — self-contained specification (§4 is the implementation spec) |
| `implementation/plan.md` | **Approved 2026-09-13** — 6 steps with checklist, tests and demos |
| `progress.md` | Live resume point during implementation (created with Step 1) |

## Design in one paragraph

A single player turns on **BOT OPPONENT (1P ONLY)** and picks **BOT LEVEL** 1–10. At the
song-select → stage transition the mod flips two game values — `PlayerWork[bot]+0x4 = 1` and
`GameWork+0x0 = 1` — after mirroring the human's chart identity and lane options into the
empty side, so the game natively builds a 2P versus session (second `GamePlayActor`, versus
HUD, `BOT LV<n>` name plate, two results panes). The bot's input is a DLL-owned `IFootPanel`
with a cloned vtable whose `getPressAge`/`consumePress` are ours: `getPressAge = mc −
planned_event`, so the judge grades every note exactly where a pure, host-tested skill model
(Gaussian σ(L) + miss probability p(L)) decided — build-independent. The flip is restored at
the first scene outside {26..30}; the human's saves are untouched; the bot side carries the
autoplay taint; one fail-open detour keeps the bot out of the extra-stage grant. The judge-side
swap autoplay already owns is extracted into a shared `services/foot_panel_swap` with a
per-side `Off | Perfect | Bot` controller.

## Plan in one paragraph

Step 1 extracts the shared service (autoplay regression is the oracle). Step 2 lands the three
pure cores + `scripts/validate_multiplayer_bot.sh`. Step 3 builds the cloned-vtable controller
and proves it on the cabinet by letting the bot play the HUMAN's own lane under a dev flag
(`mismatch=0` tally = the judge grades where planned). Step 4 adds the impersonation state
machine, option rows, textures and menu placement — the full feature. Step 5 adds the
`extra_stage_grant` signature + guard (sweep must be green on all four builds). Step 6 is the
cross-mod interaction pass, optional SE pan, RE note and AGENTS.md row.

## Next steps

1. Run the `code-task-generator` sop against `implementation/plan.md`, one plan step at a time,
   to produce task files under `.agents/tasks/2026-09-13-multiplayer-bot/step<NN>/`.
2. Run the `code-assist` sop on each task in order. Maintain `progress.md` here after every
   step and before any pause. Readiness gates before each hand-back: `cargo check --target
   x86_64-pc-windows-msvc` → `cargo fmt` → `./build.sh` → harness → signature sweep (when
   `signatures.rs` changed).
3. Cabinet validation is the maintainer's — the design's §7.3 checklist, item by item, as each
   step's demo lands.

## Assumptions and areas likely to need refinement

- **Skill-curve constants** (σ anchors 75 → 5 ms, p_miss 5 % → 0) are first guesses shaped by
  the game's grade windows; expect cabinet tuning after Step 3/4 using the harness `--report`
  histogram. The SHAPE (two geometric curves) is the decision, not the numbers.
- **`GameWork+0x0/+0x4` and the `PlayerWork` header** are assumed build-invariant (§2.3) —
  every other offset the mod touches is derived. The Step 3 self-test on an old build (§7.3
  item 10) is the check.
- **Wire `mode`/`battle_mode` = 1** in the human's per-stage save during bot songs is accepted
  (D24); revisit only if a backend starts reading them.
- **bemani-buddy migration** (`opt_mod_bot_opponent`, `opt_mod_bot_opponent_level`) lives in
  the other repo; until it exists the JSON cache carries the preference.
- **D22 SE panning** — TAKEN in Step 6 (`audio_manager_global` was already derived;
  `game_audio::set_versus_pan`, byte `+0x20C4`, disp32 identical on all four builds).
- **D14 training mode + bot** — Step 6's audit found the training pre-shift / loop latch would
  have governed from the BOT side's stale cache when the human is on P2; fixed by the
  `is_bot_side` exclusion (see `docs/multiplayer_bot_research.md` §9). The bot playing a looped
  section (human's LOOP SONG ON) remains cabinet-unverified.
