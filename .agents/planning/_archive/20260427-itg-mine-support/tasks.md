# Tasks: 20260427-itg-mine-support

Tasks are sized to be CR-ready: one shippable unit that builds independently, roughly ~1 day of focused work.

Ordering follows the PE's deployment sequence: shared foundation first (judge_hook service + autoplay refactor), then NoteTypesExpansion scaffolding, then layered Analyze → inject → render → judge → sprite tasks. Two open questions from the design (empirical penalty defaults, signature choice) are called out as explicit subtasks so they don't get lost.

## Workspace Info
**Primary Package**: ddr-world-hook
**All Packages**: ddr-world-hook

---

## Task 1: Shared `judge_hook` service + autoplay migration
**Package(s)**: ddr-world-hook
**Goal**: A reusable `services/judge_hook.rs` service owns the `judgeNotes` hook and dispatches to registered subscribers. The existing Autoplay mod is refactored to be one subscriber, with no user-visible behavior change. Foundation ships standalone to validate the new service before layering mine work on top.
**Scope**: Create `services/judge_hook.rs` with a subscriber registry. Move the `judgeNotes` retour hook out of `mods/autoplay.rs` and into the service. Autoplay becomes a subscriber that installs its panel-state override via the service API. No changes to Autoplay's external behavior (same pre-press override semantics, same panel-forcing logic).
**Tests**: Build succeeds. Autoplay still hits every arrow perfectly in a test chart, with no combo breaks. Logs show the shared judge_hook service initialized before Autoplay registers its subscriber. Disabling Autoplay via the mod menu still works, and the judge_hook service remains installed (idle) so other subscribers can still register.
**Dependencies**: None (foundation task)

- [x] 1.1 Design the subscriber API shape: what hook points does a subscriber need? (pre-judge panel-state override for Autoplay; post-judge mine-result injection for later tasks.) Define the trait / callback signatures in `services/judge_hook.rs`.
- [x] 1.2 Implement `services/judge_hook.rs`: install the `judgeNotes` hook, maintain a subscriber list, iterate and dispatch in the correct order at each hook point.
- [x] 1.3 Add `judge_hook::init()` to `lib.rs` init sequence before mod registration.
- [x] 1.4 Refactor `mods/autoplay.rs`: remove the `judgeNotes` retour detour, register as a subscriber on enable, unregister on disable.
- [x] 1.5 Deploy and manually verify Autoplay still hits every arrow with zero combo breaks on a test chart. Confirm log.txt shows no regressions.

---

## Task 2: NoteTypesExpansion mod skeleton + config + mod menu integration
**Package(s)**: ddr-world-hook
**Goal**: The `NoteTypesExpansion` mod is registered, appears in the mod menu, reads an empty `"note_types_expansion"` section from `mod-config.json`, and currently does nothing. This ships the shell so subsequent tasks can plug features into a live mod.
**Scope**: Create `mods/note_types_expansion.rs` implementing the `Mod` trait (mod ID `"note-types-expansion"`). Add `NoteTypesExpansionConfig` struct to `config.rs` with placeholder fields for score penalty and gauge damage (defaults per design). Register the mod in `lib.rs`. No parse, render, or judge logic yet — enabling/disabling the mod has no gameplay effect.
**Tests**: Build succeeds. Mod appears in the mod menu with a working on/off toggle. Toggle state persists in `mod-config.json` under `mods.note-types-expansion`. Missing `"note_types_expansion"` section falls back to defaults gracefully.
**Dependencies**: Task 1 (shared judge_hook service — this mod will eventually register as a subscriber; even though no subscribe happens in this task, later tasks plug into it)

- [x] 2.1 Add `NoteTypesExpansionConfig` struct to `config.rs` with fields for score penalty and gauge damage (values placeholder — Task 6 picks real defaults).
- [x] 2.2 Create `mods/note_types_expansion.rs` implementing `Mod` trait with empty `init`/`enable`/`disable` bodies and INFO-level lifecycle logging.
- [x] 2.3 Register `NoteTypesExpansionMod` in `lib.rs` mod registration block.
- [x] 2.4 Update `.spec/steering/structure.md` to include the new module under `src/mods/`.
- [x] 2.5 Deploy and verify the mod appears in the mod menu, toggle persists, no behavior changes.

---

## Task 3: SSQ mine-chunk format specification document
**Package(s)**: ddr-world-hook (doc only)
**Goal**: A self-contained spec document that `ddr-chart-tools` can implement SSC→SSQ mine emission against without follow-up questions. Ships early so authoring-side work can proceed in parallel with DLL work from Task 4 onward.
**Scope**: Write `docs/ssq_mine_chunk_format.md` covering: final chunk kind (the PE's choice from design Decision N; justify collision-avoidance against `docs/ssq_format.md` including legacy type 17), chunk header field values, byte-by-byte body layout, panel bitmask convention, tick space, sorting/validity rules, StepMania `M` → this chunk semantic mapping, forward-compatibility guarantees (vanilla DDR ignores; modded-disabled ignores), a worked annotated byte-level example, and the validation checklist a writer must enforce.
**Tests**: Spec is self-contained — anyone with `docs/ssq_format.md` can read it and implement read/write support without asking questions. User can take this document directly into `ddr-chart-tools` as the basis for a feature spec there.
**Dependencies**: None (independent from code tasks; can ship at any time after design is approved)

- [x] 3.1 Draft the document following the structure defined in requirements US-10. Reuse content from design.md's format section where appropriate.
- [x] 3.2 Include an annotated byte-level worked example with at least 2 mine entries (single-bit and multi-bit) and at least one regular-note co-location edge case.
- [x] 3.3 Cross-reference `docs/ssq_format.md` for chunk header conventions and collision-avoidance justification.
- [x] 3.4 User review — confirm doc is sufficient to base a `ddr-chart-tools` feature spec on.

---

## Task 4: Hook `IStepReader::Analyze` and inject synthetic mine notes
**Package(s)**: ddr-world-hook
**Goal**: When a mine-enabled SSQ is analyzed, a new MineParser reads the mine chunk, converts tick positions to `musicCount` using the SSQ's tempo data, and injects synthetic `Note` entries with a new `kind = MINE` value into the Notes vector at the correct positions. Render and judge paths downstream see them as normal notes (with a special kind). No rendering or gameplay impact yet — validation is log-only: "N mines parsed and injected at ticks [...]".
**Scope**: Signature resolution for the Analyze hook (resolves design Open Question #7 as subtask 4.1). MineRegistry, MineParser, MineInjector services under `mods/note_types_expansion/`. Allocation follows design Decisions 9–10 (app-heap allocator + pre-size strategy) — reserve capacity in the notes vector before injection to avoid reallocating game-owned memory. Graceful handling of malformed chunks (warn + treat as zero mines, no crash).
**Tests**: Build succeeds. On a mine-enabled test chart from `ddr-chart-tools` (produced per Task 3 spec), log.txt shows correct parse count and tick positions. Regular notes still judge and render identically to vanilla. On a vanilla chart (no mine chunk), there is zero overhead — the injection path short-circuits. On a deliberately malformed chunk, the game logs a warning and does not crash.
**Dependencies**: Task 2 (mod shell to host the services), Task 3 (spec document — so test charts can be generated in `ddr-chart-tools`)

- [x] 4.1 **Resolve design Open Question #7**: decide whether to reuse an existing signature slot or reserve a new one for the Analyze hook target; update `core/signatures.rs` accordingly and document the choice in a comment.
- [x] 4.2 Define the new `NoteKind::MINE` value (or equivalent — whatever the design specified for representing mines in the Notes vector).
- [x] 4.3 Implement `MineParser`: scan the SSQ buffer for the mine chunk (kind per Task 3 spec), parse entries into `Vec<MineEntry>`. Graceful malformed-chunk handling.
- [x] 4.4 Implement beat-tick → `musicCount` conversion using the tempo chunk data, matching the game's linear-interpolation integer rounding.
- [x] 4.5 Implement `MineInjector`: hook `IStepReader::Analyze` via the service/trait established in design. Pre-reserve vector capacity per design Decisions 9–10, then insert synthetic Notes with `kind = MINE`.
- [x] 4.6 INFO-level logging for parse count, tick positions, injection count; WARN for malformed data.
- [x] 4.7 Deploy and verify via log.txt that mines parse and inject correctly; regular notes untouched.

---

## Task 5: Render mines using a reused (shrunk) shock arrow sprite
**Package(s)**: ddr-world-hook
**Goal**: Mines are visible on-screen during gameplay, scrolling alongside regular notes, using a single-panel-width variant of the existing `shock_effect00` lightning sprite. Stepping on them still has no gameplay effect (that's Task 6). This is the visual proof-of-concept before authoring dedicated assets.
**Scope**: Extend the ArrowRenderer path to recognize `kind = MINE` notes and render them with the shock sprite clipped/scaled to a single panel width. Respect Speed / HIDDEN / SUDDEN / Reverse scroll transforms. No new asset work — reuse existing shock textures. Mines only render when the mod is enabled; if disabled, the mine notes are either filtered from the render list or rendered invisibly.
**Tests**: Build succeeds. On a mine-enabled test chart, mines appear at the correct panels and correct scroll positions. HIDDEN/SUDDEN/Speed modifiers apply. Visuals are thematically consistent with shock arrows but sized to one panel. Disabling the mod via the mod menu mid-song removes mine visuals cleanly with no crashes.
**Dependencies**: Task 4 (synthetic mine notes must be in the Notes vector before they can be rendered)

**Delivered Milestone A (shock-sprite stopgap deferred to Task 8 per user decision — B1):** Mines render visibly using the standard arrow palette instead of a shock-sprite variant. Widening achieved via retour detour on `collect_render_notes` (`FUN_1800240C0`) — temporary `kind = MINE → ARROW` rewrite around the original call, restored after. All geometry/appearance transforms (Speed, HIDDEN, SUDDEN, Reverse, panel rotation) fall out automatically because the original collector treats mines as arrows during the widened window. Visual identity (dedicated mine sprite via LayeredFS) handled in Task 8.

- [x] 5.1 Investigate the ArrowRenderer draw path to find the hook point for per-note rendering decisions (design should identify; verify against Ghidra if uncertain).
- [x] 5.2 Implement the mine rendering variant: shock sprite scaled or UV-clipped to single panel width.
- [x] 5.3 Wire scroll transforms (Speed, HIDDEN, SUDDEN, Reverse) — should fall out automatically from reusing the ArrowRenderer per-note path; verify each.
- [x] 5.4 Gate rendering on mod-enabled state; on disable, mines become invisible (preferred) or are filtered out of the render call.
- [x] 5.5 Deploy and manually verify mine visuals at various Speed multipliers and HIDDEN/SUDDEN settings.

---

## Task 6: Mine judgment — combo break, score penalty, gauge damage
**Package(s)**: ddr-world-hook
**Goal**: Stepping on a mine within its timing window triggers a MISS/NG judgment, breaks the combo, deducts a configurable amount from the standard score, and deducts a configurable amount from the life gauge. Mines avoided have no effect. Mines are excluded from the max score / EX score calculation.
**Scope**: Add a `note_types_expansion` subscriber to the `judge_hook` service (Task 1). On each judge tick, check for mine notes in the timing window matching a pressed panel; if so, apply the mine-hit effects. Resolve design Open Question #2 in subtask 6.1 — research ITG and DDR shock arrow penalty values, pick defaults, document in README. Ensure arrow-takes-priority rule when a mine and arrow co-exist on the same panel at the same tick.
**Tests**: Build succeeds. On a mine-enabled test chart: stepping on a mine in its window shows MISS/NG, breaks combo, reduces score and gauge by the configured amounts; avoiding mines leaves score/combo/gauge untouched. Score ceiling on a perfect-play mine-enabled chart matches vanilla 1,000,000 (mines excluded from denominator). Gauge can reach 0 from repeated mine-hits and trigger the normal GAME OVER path. Config overrides for penalty amounts take effect after a restart.
**Dependencies**: Task 1 (shared judge_hook service), Task 2 (mod + config struct), Task 4 (mine notes present in Notes vector). Does NOT depend on Task 5 — judgment works on invisible mines too.

- [x] 6.1 **Resolve design Open Question #2**: ~~research StepMania mine penalty, DDR shock arrow penalty, pick default score-penalty and gauge-damage values; document the rationale in README.md and in `config.rs` doc comments.~~ Resolved differently: the user chose shock-arrow parity over a separate configurable penalty. Mines are counted as shock arrows in the engine's score/combo formula, so the penalty falls out of the engine's existing math (no config, no documented rationale for separate values). `config.rs` was removed.
- [x] 6.2 Confirm shock arrow judgment-display behavior in DDR World (MISS vs NG) — match it for mines.
- [x] 6.3 Identify DDR's shock-arrow timing window and use the same for mines (not configurable, per US-8).
- [x] 6.4 Implement the mine-hit judge logic as a `judge_hook` subscriber: per-panel mine-in-window detection, effect dispatch (combo break, score deduct, gauge deduct, judgment display).
- [x] 6.5 Implement the arrow-takes-priority rule for same-panel-same-tick co-location.
- [x] 6.6 Ensure max-score denominator is unaffected by mines (spot-check: perfect play on a mine-enabled chart hits the same 1,000,000 as the same chart with mines stripped).
- [x] 6.7 Deploy and manually verify all effects; confirm log.txt has no silent errors.
- [x] 6.8 **Shock-lane visual effect on hit** (follow-up). Root cause: the engine's shock-effect listener gates its full-lane ("big flash") animation on `param->note->state[NumPanels * sideIndex]` being in the triggered state — a check that real shock arrows satisfy trivially (all four per-side panels set) but that single-panel mines only satisfy when they sit on the leftmost panel. Fix shipped in `mines.rs`: at the shock-NG dispatch site, the scratch struct's `note_ptr` is redirected from the real mine note to a stack-allocated synthetic `GameNote` whose per-side state bits are all TRG (and `beat_count` is copied through because the talent-measurement listener reads it). Safe because the engine's message dispatch is synchronous — the synthetic lives the entire dispatch window — and the real note is untouched for rendering, judgment, and every other code path. User verified in-game: all mine hits now trigger the full-lane flash consistently across all panels.

---

## Task 7: Autoplay avoids mines
**Package(s)**: ddr-world-hook
**Goal**: When the Autoplay mod is enabled on a mine-enabled chart, Autoplay skips mine panels at the current tick while continuing to press regular arrow panels.
**Scope**: Extend the Autoplay subscriber (installed in Task 1) with a mine-awareness hook that queries the MineRegistry (from Task 4) for mines at the current tick and masks those panels out of the auto-press bitmask. This is the clean case — both Autoplay and NoteTypesExpansion are subscribers to the same shared `judge_hook` service, so the interaction is contained.
**Tests**: Build succeeds. With Autoplay + NoteTypesExpansion both enabled on a mine-enabled chart, Autoplay achieves a full-combo with zero mine-hits. Disabling NoteTypesExpansion reverts to full-combo vanilla behavior (mines don't exist at runtime). Disabling Autoplay leaves mine judgment and rendering untouched.
**Dependencies**: Task 1 (shared service architecture), Task 4 (MineRegistry), Task 6 (judgment exists — so a missed mine-avoid during Autoplay is visible as a penalty). Declared non-P0 in US-6 with a fallback; if this task proves to require substantial Autoplay rework, it can be split off as a follow-up and the fallback (avoid autoplay with mine charts) applied instead.

- [x] 7.1 ~~Query MineRegistry from Autoplay's panel-press decision path; mask mine panels out of the press bitmask.~~ Not needed: Task 6's pre-mark (writing `judgeTimestamp = note.music_count` to every mine Result entry on first-frame encounter) satisfies the engine's `AutoFootPanel::Update`'s `IsJudged` skip test. The engine's own autoplay naturally does not press mine panels — no separate MineRegistry query or bitmask mask needed.
- [x] 7.2 ~~Handle the edge cases: mine + arrow on same panel same tick (arrow wins, so press), mines-only tick (don't press at all on that tick if no arrows).~~ Falls out automatically: mines are pre-marked as judged so AutoFootPanel ignores them; arrows at the same tick/panel are processed normally by AutoFootPanel (not pre-marked) and get pressed. Mines-only ticks: AutoFootPanel has no arrow to press, so no panel press — correct.
- [x] 7.3 Deploy and manually verify with multi-mine test chart that Autoplay achieves full-combo. Verified by user on the 74-mine Xuxa chart — autoplay achieved 743/743 full combo with zero mine-hits.

---

## Task 8: Dedicated mine sprite
**Package(s)**: ddr-world-hook
**Goal**: Replace the reused shock sprite (Task 5) with a dedicated mine sprite that is visually distinct from shock arrows while remaining thematically consistent (lightning / hazard visual language). Ships polish on the visual front; no gameplay change.
**Scope**: Author PNG assets for the dedicated mine sprite (or reuse existing shock frames, visually modified). Integrate via LayeredFS or a custom ARC (PE decision in design determined which — follow it). Swap the render path in Task 5 to use the new sprite when available; fall back to the reused shock sprite if assets are missing.
**Tests**: Build succeeds. Mine visuals are distinct enough from shock arrows that a blind playtest user can tell them apart without prior explanation. Missing assets gracefully fall back to the Task 5 reused-sprite behavior. Speed / HIDDEN / SUDDEN / Reverse transforms still apply identically.
**Dependencies**: Task 5 (rendering path exists and is the thing being upgraded)

- [x] 8.1 Author or modify mine sprite assets (PNG, matching the single-panel dimensions and DDR's hazard visual style). **Delivered**: 3 variants (s/m/l) at `data_mods/note_types_expansion/tex/note_types_mine00_{s,m,l}.png`, cropped from the shock-effect texture's second section.
- [x] 8.2 Integrate via the delivery path chosen in design (LayeredFS vs custom ARC). **Delivered**: PNGs loaded via the engine's own file pipeline (`agcs::FileManager` + `PngFileCallback`) — no custom ARC, no LayeredFS hook. Textures register by filename stem in the engine's resource system.
- [x] 8.3 Update the Task 5 render path to prefer dedicated sprite, fall back to reused shock sprite if missing. **Delivered differently**: Task 5's kind-swap approach was replaced entirely by a dedicated mine render pass (`mine_render.rs`) hooking the outer `render_notes` function. The mine pass emits two layers per mine: silver shock-arrow glyph (Layer 1) and additive lightning overlay (Layer 2). Missing lightning texture gracefully skips Layer 2 (Layer 1 still renders). Task 5's `render_hook.rs` kind-swap was deleted.
- [x] 8.4 Playtest to confirm visual distinctness and thematic consistency. **Verified**: mines render as silver shimmering shock-arrow glyphs with animated lightning overlay, distinct from both regular colored arrows and full-lane shock arrows. Per-panel rotation produces 4 distinct orientations using the engine's 2-variant shock atlas.
- [x] 8.5 **Arrow-shape s/m/l variant selection** (follow-up). **Delivered**: `MineTextureLoader` already loaded all three size variants; what was missing was a runtime read of the player's selected "Arrow Design" option. New `player_work_table_anchor` signature + `derive_player_work_table` derive the per-side Player Work global at `gamemdx.dll + 0x6EBE50`. `mine_render::resolve_arrow_shape` walks `actor[+0x84] → table[playSide*8] → *wrapper → +0xE0 → Option → +0x60` to read the design value (0..=6 in DDR World; shape 7 is unreachable but the shock-size table covers it for safety). Pre-judge callback primes the cache once per chart via `mine_render::prime_arrow_shape`; the render hook reads the cache. Scene-exit and mod-disable call `reset_cache` so the next chart re-resolves. User in-game verified: Medium design → medium mine texture; Small and Dot designs → small mine texture. Large designs → large mine texture.

---

## QA Section
**Status**: Approved
**Test Results**: User-verified in-game across multiple charts and arrow-design settings. Mine visuals match the selected design size (large / medium / small). Log diagnostics confirm the pointer chain resolves correctly per chart and resets cleanly on exit.
**Feedback**: N/A

## Acceptance Section
**PM**: N/A (solo-maintainer)
**Status**: Approved
**Notes**: Feature fully complete as of 2026-05-04. All 8 tasks plus the Task 8 arrow-shape follow-up have shipped and been user-verified in-game.
