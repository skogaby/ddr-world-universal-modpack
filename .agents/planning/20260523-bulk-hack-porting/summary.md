# Project Summary — 20260523 Bulk Hack Porting

This document summarizes the artifacts produced by the PDD process
for the bulk-hack-porting feature. All paths are relative to the
project root.

## Artifacts

```
.agents/planning/20260523-bulk-hack-porting/
├── rough-idea.md                — original idea capture
├── idea-honing.md               — Q&A clarification log (Q1-Q17 + research-phase verifications)
├── research/
│   ├── quick-restart-re.md      — direct 28→28 transition mechanism
│   ├── quick-fail-re.md         — R19 anchor verification + role-flip finding
│   ├── speed-toggle-re.md       — vanilla DDR has fine/coarse natively → DROPPED
│   ├── per-step-data-feed.md    — FUN_1800603a0 hook point discovery
│   ├── real-speed-anchors.md    — R15/R16/R24-R26 anchor + payload verification
│   └── mod-menu-input-gating.md — gameplay ignores numpad bits 9-20 → no suppression needed
├── design/
│   └── detailed-design.md       — full self-contained design document
├── implementation/
│   └── plan.md                  — 15-step plan with checklist and demos
└── summary.md                   — this document
```

## Final Feature Scope

**Four new mods + one infra change + one tooling consolidation.**

| Component | Type | Source |
|---|---|---|
| Mod-menu scene-gate removal | infra | New (REQ-1) |
| `PremiumFreeMod` | global mod | Port of §3 |
| `QuickRestartOrFailMod` | global mod, two gestures | Port of §16 + new RE |
| `SongSelectionImprovementsMod` | global mod, 2 sub-features (was 3) | Ports of §4, §15 |
| `PowerUserStatisticsMod` | per-player mod, 3 sub-features | Ports of §2, §7, §12 |
| `gen_custom_option_labels.py` | tooling | New script consolidating two existing ones |

**Dropped from scope** during research: the Updated Speed Toggle
(formerly the third sub-feature of `SongSelectionImprovementsMod`).
Live observation confirmed Konami implemented the ±0.05/±0.50
fine/coarse semantics natively in 20260421 — the mod was redundant.

## Key Design Decisions

### Architecture
- **No new services.** Existing `judge_hook`, `scene_manager`,
  `input_manager`, `widget_renderer`, and `custom_options` are
  sufficient. The only service-layer change is a small accessor on
  `scene_manager` (`current_transition_sequence()`) for
  QuickRestartOrFailMod.
- **Mod organization.** Two single-file mods (`premium_free.rs`,
  `quick_restart_or_fail.rs`); two multi-file mods
  (`song_selection_improvements/`, `power_user_statistics/`) following
  the established pattern of `note_types_expansion/`.
- **Hook strategy mix.** `retour::GenericDetour` for function-level
  hooks (R13/R14, Flare→Lamps, FUN_1800603a0); `core::memory::write_bytes`
  + VirtualAlloc'd stubs for byte-level patches (Real Speed R15/R16/R24-R26,
  PremiumFree R9 mid-function patch).

### Cross-version portability
Every patch site uses an AOB anchor verified on both 20250805 stock
and 20260421. Critical findings during research:
- **Quick Fail R19 anchor role-flips** between versions due to
  compiler register scheduling. Recommended port uses a more portable
  flag-write at `[transition_seq + 0xE8] = 0` instead of patching the
  cmov site.
- **Real Speed R16 rel32** is the only byte payload that varies between
  versions; computed at runtime from the stock R16 site BEFORE patching.
- **Per-step ms-error data feed**: hook `FUN_1800603a0` directly,
  since `judge_hook::register_post` fires too late for the in-flight
  pacemaker render (which is a tail-call inside `judgeNotes`).

### Per-player options shape
PowerUserStatisticsMod adds four custom options to the Mods tab
(Page6) with the `pus_` prefix:
```
TIMING STATISTICS         [OFF / ON]
PACEMAKER -> MS ERROR     [OFF / ON]
  WHITE THRESHOLD          [10]      (visible only when above is ON)
EXPORT STEP DATA (CSV)    [OFF / ON]
```
All four persist via the existing custom_options
network/JSON layer.

### Open questions deferred to implementation
- **Premium Free score-save** behavior — observe live; if scores don't
  save across multiple songs, escalate to RE on the save path.
- **Quick Restart accumulator pollution** — observe live; may need a
  per-stage block-zero before the transition.
- **Quick Fail mid-song timing** — diagnostic-build phase determines
  the exact `scene_id` for the secondary `FUN_18002de40` call.
- **Flare→Lamps remap table** version-stability — verify on first
  deploy.

## Status (2026-05-25)

**Implementation complete.** All feature steps (1-14) are implemented
and verified working on 20260421. Step 15 (cross-version smoke test
on 20250805 stock) is deferred.

Key changes from the original plan:
- **Flare→Lamps (Step 9) dropped** — caused false positives on
  unplayed songs and was low value.
- **SongSelectionImprovementsMod renamed** to `RealSpeedFixMod`
  (module `real_speed_fix/`) since it's now a single-purpose mod.
- **ShowWhen runtime filtering** was implemented in the custom_options
  framework to support the pacemaker threshold option's conditional
  visibility.
- **Pre-existing Play Graph bug** discovered: the results-screen
  detail graph is empty on the second song of a session regardless
  of our mods. Not caused by our code; tracked separately.

## Areas That May Need Further Refinement

- **Premium Free hook strategy** (manual stub vs retour detour on
  enclosing function). The plan adopts the manual-stub approach
  matching the original mod, but if implementation reveals
  unexpected complexity (e.g., the enclosing function turns out to
  be cleaner to detour), revisit.
- **Quick Fail mid-song behavior** is the highest-uncertainty item
  in the plan. If the flag-write + forced-transition approach
  doesn't deliver "immediate" fail-out, the right answer might be
  a more invasive state-machine entry hook.
- **Timing Stats widget layout** is positions-in-source for v1.
  Post-deploy observation may motivate making positions
  configurable via JSON — defer until the first deploy reveals what
  positions actually work for live play.
