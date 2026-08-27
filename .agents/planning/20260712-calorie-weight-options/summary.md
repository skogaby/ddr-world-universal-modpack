# Summary — In-Game Weight & "Display Burned Calories" Options

## What this is

A two-repo feature adding two web-UI-only player-profile settings to the in-game
options menu:

- **DISPLAY BURNED CALORIES** (`is_disp_weight`) — OFF/ON toggle (parent row).
- **WEIGHT** — body weight in **kg**, child row shown only when calories are ON,
  fed into the game's calorie calculation.

It mirrors the existing cosmetic-customize design: the game's own profile load stays
the source of truth; the DLL adds the *save* direction the stock game lacks, and the
**bemani-buddy** backend persists the values into its already-existing native
columns. Both repos ship together.

## Artifacts

```
.agents/planning/20260712-calorie-weight-options/
├── rough-idea.md                     # the seed concept
├── idea-honing.md                    # Q1–Q9 requirements Q&A (all resolved)
├── research/
│   ├── existing-re-findings.md       # pointer to docs/calorie_weight_profile_research.md
│   └── backend-bemani-buddy.md       # backend change surface (Rust)
├── design/
│   └── detailed-design.md            # standalone design (2 mermaid diagrams, data models)
├── implementation/
│   └── plan.md                       # 4-step checklist + TDD steps
└── summary.md                        # this file
```

Supporting RE doc (in the main tree, written earlier this session):
`docs/calorie_weight_profile_research.md` — offsets, wire format, reflect evidence,
full calorie formula, cross-version notes, signature basis.

## Key design decisions

- **Placement:** new `src/mods/webui_options/profile_fields.rs` submodule under the
  existing `webui-options` toggle.
- **Memory:** `weight` = `PlayerWork+0x24` (s32, kg), `is_disp_weight` =
  `PlayerWork+0x28` (u8) — hardcoded offsets (verified stable), reached via the mod's
  existing `player_work_table` chain.
- **UI:** `bool_toggle` parent + `scalar(30..=200, fine 1 / coarse 10, default 60)`
  child, gated by `ShowWhen::Equals` — no framework work.
- **Unit:** kg end-to-end, no conversion. `weight==0` seeds to 60.
- **Persistence:** `PersistMode::SaveOnly` → framework auto-emits
  `<mod_weight>` / `<mod_is_disp_weight>` on `playerdata_save`.
- **Backend:** 2-line save-handler detection + 2 schema entries writing native
  `weight` / `is_disp_weight` columns — **no migration** (columns, load emit, and
  persistence already exist).

## Implementation shape (4 steps)

1. **Backend** — save-path detection + schema entry + test (independently testable).
2. **DLL** — `profile_fields` submodule: register rows + apply-on-change; wire into
   `enable()`.
3. **DLL** — seed rows from `PlayerWork` at SONG_SELECT (read-only, `0→60`).
4. **End-to-end** — live round-trip validation (cabinet + backend) + README/AGENTS
   docs.

Each step is demoable and ends with integration; validation is live deploy + log
observation (DLL) and `cargo test` (backend).

## Next steps

1. Hand off `implementation/plan.md` to the implementing agent (both repos).
2. Maintain `progress.md` in this directory during implementation (per AGENTS.md).
3. Perform the one-off Cheat-Engine **unit calibration** (confirm `PlayerWork+0x24`
   is plain kg) before shipping — non-blocking; localized to `profile_fields.rs` if
   it turns out scaled.

## Areas that may need refinement

- **Weight unit anomaly** (RE §3.1): the calc's unset branch (`F=60.0`) vs
  `weight/100` suggests either plain kg + an inflated default or a scaled unit. The
  round-trip is unit-agnostic; only the display range constants depend on it. Settle
  via the calibration check.
- **Menu row count / ordering** on the Mods tab (cosmetics + power-user + these two)
  — overflow is handled by `options_scroll`, but confirm ordering reads well.
