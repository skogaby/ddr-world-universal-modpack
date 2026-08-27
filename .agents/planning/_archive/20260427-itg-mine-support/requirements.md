# Requirements: NoteTypesExpansion — Mines (ITG/StepMania)

## Overview

Add a new mod, `NoteTypesExpansion`, that extends DDR World to recognize and support note types lifted from StepMania / In The Groove. The mod is designed as an extensible framework — future additions (lifts, rolls, other ITG concepts) will live in this same mod rather than as separate mods per note type.

**v1 scope: mines only.** Mines are single-panel penalty notes — semantically similar to DDR's existing shock arrows, except they apply to a single panel instead of all four on one side. Mine-enabled charts are authored in a separate repo (`ddr-chart-tools`); this feature covers the runtime DLL side only, plus a format-spec document that `ddr-chart-tools` will implement against.

This mod depends on a new SSQ chunk type carrying mine data. The chunk is silently ignored by vanilla DDR (forward-compatible) and by this mod when disabled. When enabled, the mod parses the chunk, renders mines on-screen, and judges player steps against them.

## User Stories

### US-1: Mines are parsed from SSQ and associated with charts
**As a** player loading a mine-enabled chart
**I want** the mod to read mine data from the SSQ file and associate it with the active chart
**So that** mines can be rendered and judged during gameplay

**Acceptance Criteria:**
- [ ] On SSQ load, the mod scans for the `NoteTypesExpansion` mine chunk (chunk kind TBD during design; chosen to avoid collision with the full chunk-kind range observed in `docs/ssq_format.md`, including the legacy type 17)
- [ ] **Mines are keyed per-difficulty**, mirroring how regular step chunks work: each difficulty (Single Basic, Single Difficult, ..., Double Challenge) has its own independent set of mines. A song with unique mines on Challenge only, and no mines on lower difficulties, is a first-class supported authoring pattern
- [ ] The mine chunk uses the same `(type, param2)` lookup convention as vanilla step chunks, where `param2` encodes the difficulty+style code per `docs/ssq_format.md §5.1`. The DLL's Analyze hook runs per-difficulty and looks up the mine chunk with the matching `param2` for the difficulty being parsed
- [ ] If no mine chunk is present for the current difficulty, the mod takes no gameplay action for that chart+difficulty (zero overhead, zero side-effects). This is legal and common (e.g. mines only on Challenge)
- [ ] If a mine chunk is present, the mod parses all mine entries into an in-memory table keyed by (chart identity, difficulty, tick / music count) and retains it for the duration of the play session
- [ ] Mine tick positions are converted to the same `musicCount` timeline used by regular notes, using the SSQ's tempo (TIMING) chunk data — the conversion matches what the game itself uses for regular notes to within integer rounding
- [ ] If the mine chunk is malformed (bad chunk size, entries past chunk end, etc.), the mod logs a warning and treats the chart as having zero mines. The game must not crash.

### US-2: Mines render on-screen during gameplay, scrolling with the chart
**As a** player
**I want** mines to appear visually during gameplay at their correct positions
**So that** I can see them and avoid them

**Acceptance Criteria:**
- [ ] Each mine renders at its correct panel and scroll position (driven by `musicCount` relative to current play position, same as a regular note)
- [ ] Mine visuals are thematically consistent with shock arrows — matching the lightning / hazard visual language used by `data/2d/shock_effect00/shock_effect00_[s/m/l].dds`, but sized to a single panel width rather than spanning all 4 panels
- [ ] Mines respect the scroll transforms that regular notes respect: Speed, Appearance (HIDDEN/SUDDEN alpha curve), Reverse, and any other per-chart scroll option (PE to enumerate during design)
- [ ] Mines are visually distinct enough from regular arrows that players can identify them without prior knowledge (the shock-arrow-style hazard visual is assumed sufficient; flag during QA if testing proves otherwise)
- [ ] When the mod is disabled, no mine visuals appear even if the chart contains mine data

### US-3: Stepping on a mine applies a penalty
**As a** player who steps on a mine panel within its judgment window
**I want** the game to register a penalty
**So that** the mine has the intended gameplay impact

**Acceptance Criteria:**
- [ ] Stepping on a panel that has a mine at the current playhead (within the mine's timing window) triggers a mine-hit event
- [ ] Mine-hit events break the player's combo (combo → 0)
- [ ] Mine-hit events deduct from the player's standard score by a configurable amount (see US-8)
- [ ] Mine-hit events deduct from the player's life gauge by a configurable amount (see US-8). If the gauge reaches 0, the normal GAME OVER path applies
- [ ] Mine-hit events display a `MISS` judgment on-screen (same as shock arrow behavior — PE to confirm shock behavior during design; if shock arrows display something else, match that)
- [ ] The mine-hit timing window matches the shock arrow judgment window (not configurable — inherits whatever shock arrow uses in the current build of DDR World)
- [ ] A single mine triggers at most one penalty event. Simultaneous mines on different panels at the same tick (same chunk entry with multiple bits set) fire independently — N panel bits = N potential penalty events
- [ ] If a mine and a regular arrow coexist on the same panel at the same tick (edge case), the regular arrow judgment takes priority; the mine is not additionally penalized on that press
- [ ] Pressing any panel where no mine exists at that tick has no mine-related effect (normal note judgment proceeds)

### US-4: Ignoring a mine is neutral — no reward, no penalty
**As a** player who correctly avoids a mine
**I want** avoidance to be the default, no-op behavior
**So that** I am not rewarded for standing idle and not punished for natural play

**Acceptance Criteria:**
- [ ] A mine whose timing window elapses without the corresponding panel being pressed produces zero effect (no combo break, no score change, no gauge change, no judgment display)
- [ ] Avoiding all mines on a chart is equivalent to the chart having no mines at all — it does not add to score, does not contribute to combo, does not affect max score calculation
- [ ] A chart with mines played perfectly (all arrows MARVELOUS, all mines avoided) achieves the same score as the same chart with mines stripped out

### US-5: Mines do not raise the max possible score above 1,000,000
**As a** player chasing a high score on a mine-enabled chart
**I want** the score/rank ceiling unchanged from a non-mine chart
**So that** mine presence does not inflate or deflate the score scale

**Acceptance Criteria:**
- [ ] A perfect-play score on a mine-enabled chart (all arrows MARVELOUS, all mines avoided) equals 1,000,000 — the same 1,000,000 cap as any standard chart
- [ ] Mines participate in scoring exactly like shock arrows: they contribute to the score denominator, and avoiding them contributes to the numerator with OK-weight — so avoided mines are score-neutral while hit mines reduce score by the shock-NG delta
- [ ] Mines do not affect EX score. Mine-hits do not subtract from EX score, and avoided mines do not add to it
- [ ] Rank (AAA, AA, etc.) naturally reflects mine-hits via the standard score reduction — no special rank handling required

### US-6: Autoplay avoids mines
**As a** user running the Autoplay mod on a mine-enabled chart
**I want** Autoplay to avoid mine panels
**So that** demo/attract playback does not accumulate penalties

**Acceptance Criteria:**
- [ ] When Autoplay is enabled and the current chart contains mines, Autoplay must not press any panel that holds a mine at the current tick
- [ ] Autoplay continues to press panels with regular arrows normally
- [ ] This behavior is always on — there is no user-facing knob to disable it

**Priority:** Desired for v1, not strictly P0. If implementing autoplay-avoidance proves to require substantial rework of the Autoplay mod, that work may be split off into a follow-up task. In that case, mine-enabled charts must not be selected for attract-mode demo play (or autoplay must be disabled when a mine-enabled chart is active) so the game does not visibly thrash.

### US-7: The mod has a global on/off toggle via the in-game mod menu
**As a** user who wants to disable the mod without editing files
**I want** `NoteTypesExpansion` to appear in the mod menu like other mods
**So that** I can toggle it at runtime and revert to vanilla behavior

**Acceptance Criteria:**
- [ ] `NoteTypesExpansion` is registered with the mod registry and appears in the mod menu
- [ ] Enabling the mod activates all mine handling (parse + render + judge + autoplay-avoid)
- [ ] Disabling the mod suppresses all mine handling — a chart with mine data still loads, but the mine chunk is silently ignored: no mines render, no mine judgment fires, no penalties apply. Gameplay is indistinguishable from the chart having no mines
- [ ] Toggling is persisted via the standard `mod-config.json` `mods` section
- [ ] On disable, any runtime resources allocated by the mod (side tables, hooks, sprite bindings) are released cleanly without crashing

### US-8: Configurable mine-hit cost knobs
**As a** user who wants to tune mine difficulty
**I want** to configure the score penalty and gauge damage for a mine-hit via `mod-config.json`
**So that** I can dial the punishment to match my play preferences without recompiling

**Acceptance Criteria:**
- [ ] A `"note_types_expansion"` section in `mod-config.json` exposes:
  - A score-penalty-per-mine-hit field (integer; points deducted per mine-hit)
  - A gauge-damage-per-mine-hit field (using whatever unit DDR uses internally — raw or percentage; PE decides during design based on DDR's gauge conventions)
- [ ] Both fields have sensible defaults if the section is missing or a field is absent. Defaults are research-driven — PE picks defaults during design by consulting the StepMania source (checked out locally) and DDR's shock-arrow penalty path for comparable values
- [ ] Default values are documented in README.md and include a short justification for each value
- [ ] Invalid or negative values fall back to the default and emit a warning log
- [ ] Timing window is NOT configurable and tracks whatever DDR uses for shock arrows
- [ ] There is no per-player enablement field
- [ ] If the entire `"note_types_expansion"` key is missing, the mod uses all defaults and functions normally (matches how other mods treat missing config sections)

### US-9: Framework designed for future note types
**As a** modpack maintainer planning to add lifts, rolls, and other ITG note types later
**I want** the mod internally structured so new note types plug in cleanly
**So that** follow-on work does not require re-architecting the mod

**Acceptance Criteria:**
- [ ] The mod is named `NoteTypesExpansion` and registered with a single mod ID (e.g., `"note-types-expansion"`) — not `MineSupportMod` or similar mine-specific naming
- [ ] Mines are implemented as one concrete note type inside the mod, using whatever abstraction (trait, registry, dispatch table — PE's choice) makes sense for clean future extension
- [ ] Adding a second note type (e.g., a lift or roll) in a follow-up feature does not require rewriting the parse, render, or judge scaffolding — the abstraction should accommodate at least parse-side (separate SSQ chunk per type) and judge-side (different trigger semantics: press, release, repeated-tap) variations
- [ ] PE's design document explicitly identifies the extension points with 1–2 sentences per point describing how a future note type would use them
- [ ] No concrete implementation of note types other than mines is required for this feature. The framework is validated by the mine implementation alone

### US-10: SSQ mine-chunk format specification for `ddr-chart-tools` handoff
**As a** developer about to implement the authoring side in `ddr-chart-tools`
**I want** a self-contained spec document describing the SSQ mine chunk
**So that** I can implement SSC↔SSQ round-trip in a separate repo without needing to reverse-engineer the DLL

**Acceptance Criteria:**
- [ ] A spec document is produced as part of this feature (filename and location PE's choice, under `docs/` or the feature workflow directory). It lives in this repo
- [ ] The document specifies:
  - Chunk kind / ID (final value, justified — explicitly calling out that it avoids collision with all chunk types observed in `docs/ssq_format.md` including the legacy type 17)
  - Chunk header field values: type, `param2` as the per-difficulty key (using the same difficulty+style codes as vanilla step chunks per `docs/ssq_format.md §5.1`), `param3`, `param4`. Makes explicit that a song with N difficulties gets up to N mine chunks, each keyed by its corresponding difficulty code
  - Body layout byte-by-byte (per-entry size, field offsets, types, endianness)
  - Panel bitmask layout (matching the existing step-byte bit convention)
  - Tick space (same 4096-per-measure space as FOOTSTEP), sorting rules, co-existence rules with regular notes in the same difficulty
  - Forward-compatibility guarantees (unmodded DDR ignores the chunk, modded DDR with the mod off ignores the chunk)
  - A worked example: a small mine-enabled SSQ with a few mine entries, annotated byte-by-byte, tagged as a specific difficulty
  - Validation rules a writer must enforce (e.g. `param2` must be a valid difficulty code and must match an existing step chunk in the file, tick ordering, panel bit validity, co-location rules, no duplicate `(type, param2)` pairs)
  - Semantic mapping from StepMania `M` note type → this chunk format, accounting for per-difficulty chart separation in SSC files
- [ ] The document is written at a level of detail where a reader with `docs/ssq_format.md` in hand can implement read + write support in `ddr-chart-tools` without asking follow-up questions
- [ ] The document is produced before or alongside DLL implementation, not after, so that test-chart generation in `ddr-chart-tools` can proceed in parallel with DLL work

### US-11: Vanilla compatibility — mine-enabled SSQs play cleanly on unmodded DDR
**As a** player on a vanilla (unmodded) DDR World cabinet
**I want** to load a mine-enabled SSQ without crashes or visual artifacts
**So that** mine-enabled charts can ship alongside vanilla charts without breaking vanilla installs

**Acceptance Criteria:**
- [ ] A mine-enabled SSQ loaded on unmodded DDR World plays as if it had no mines — no crashes, no visible mines, no spurious judgments, no error logs
- [ ] The chunk kind and layout are chosen so the vanilla parser skips the mine chunk cleanly (standard unknown-chunk path)
- [ ] This property is verified at least once during acceptance by running a mine-enabled test chart in a known vanilla (hook DLL disabled or removed) configuration

### US-12: Logging and diagnosability
**As a** developer debugging a mine-enabled chart
**I want** the mod to emit informative logs
**So that** problems are diagnosable from log.txt / DebugView output without live debugging

**Acceptance Criteria:**
- [ ] On SSQ load, the mod logs whether a mine chunk was found and how many mine entries were parsed (at INFO level)
- [ ] On each mine-hit, the mod logs the hit (musicCount, panel, resulting score/gauge delta) at INFO or DEBUG level
- [ ] Parse-time errors (malformed chunk, out-of-range values) log at WARN or ERROR level with enough context to diagnose
- [ ] When the mod is disabled, no mine-related log spam appears beyond initialization lifecycle messages
- [ ] log.txt output during a typical play session of a mine-enabled chart contains no silent errors, unhandled exceptions, or unexplained warnings related to mines

## Out of Scope

- **Authoring pipeline (`ddr-chart-tools` changes).** SSC↔SSQ round-trip with mines, `M` note parsing, test-chart generation — handled by a separate feature in the `ddr-chart-tools` repo. This feature only delivers the SSQ spec document that `ddr-chart-tools` will implement against.
- **Other ITG note types (lifts, rolls, etc.).** Framework must accommodate them but no implementation is required. Each future note type will be added via a follow-on feature in this mod.
- **Mine-stats display on the result screen** (e.g., "Mines Avoided: X / Y"). Requires dedicated RE of DDR's result screen UI, which is useful for many future mods and is better done as a standalone effort.
- **Mine-hit polish: dedicated sound effect, explosion/flash VFX on the panel.** Can be added in a follow-up feature.
- **Ghost / replay data integration.** Mine-hit events are not recorded in ghost data. Out of scope for v1.
- **Network score suppression on mine-enabled charts.** Normal score upload applies; the user's custom-server setup handles score integrity separately.
- **Per-player enablement or other player-scoped mine config.** Mod is global on/off only.
- **Configurable mine-hit timing window.** Tracks shock arrow timing window implicitly.
- **Migration / legacy chart detection.** Charts without a mine chunk are indistinguishable from vanilla charts; no migration required.
- **Custom chunk IDs or schema version negotiation with other modding communities.** The chunk kind is a fixed value picked by PE during design; a community-registry scheme is deferred.

## Open Questions

- **Chunk kind final value.** Deferred to PE. Must avoid collision with all values observed in `docs/ssq_format.md` (including legacy type 17) and should leave headroom for Konami to add new official types.
- **Default score penalty and gauge damage values.** PE to pick during design based on (a) StepMania source (checked out locally), (b) DDR shock arrow penalty values, (c) ITG community conventions. Configurable by the user regardless.
- **Shock arrow MISS-vs-NG judgment display.** User expectation is that shock arrows display `MISS` when stepped on, and that mines should match. PE confirms actual shock arrow behavior in DDR World during design and matches it. If DDR uses `NG`, mines use `NG` too.
- **Visual starting point vs. dedicated sprite.** PE decides whether v1 ships with a shrunken reused shock sprite (quick, lower fidelity) or goes straight to a dedicated mine sprite (more asset/LayeredFS work). Either is acceptable as long as the result meets US-2's thematic-consistency and distinctness criteria.
- **Autoplay-avoidance rework depth.** PE estimates during design whether the existing Autoplay mod can be extended inline or needs a refactor. If refactor cost is high, US-6 can be demoted to a follow-up feature per that user story's priority clause.

## Dependencies

- **SSQ format reference:** `docs/ssq_format.md` (complete byte-level map of the format, validated against 1523 shipped charts).
- **Ghidra (loaded with gamemdx.dll from DDR World)** — the authoritative target for runtime addresses, exact function behavior, struct layouts, and reverse-engineering observations this feature relies on.
- **StepMania source (checked out locally by user)** — prior-art reference for mine semantics, penalty values, timing windows.
- **Existing modpack infrastructure:** mod-registry (`mod_trait.rs`), config system (`config.rs`), signature scanner (`core/signatures.rs`), render hooks (`widget_renderer`, `afp_patcher`), autoplay hook (`mods/autoplay.rs`).
- **LayeredFS** — likely required if a dedicated mine sprite is served via file replacement; PE confirms during design.
- **Follow-on feature in `ddr-chart-tools`** — not a blocker for this feature's DLL work, but required before mine-enabled charts can be authored and tested end-to-end. The spec document in US-10 is the handoff.

## Assumptions

- The existing `docs/ssq_format.md` is accurate. Its finding that chunk types 1, 2, 3, 4, 5, 9, 17 are in-use in DDR World (refining the feasibility doc's "highest legacy type is 9" claim) is taken as fact.
- DDR's shock arrow system gives the user a MISS-equivalent judgment and reacts similarly to what mines should do (combo break, score deduction). If shock-arrow behavior differs significantly from expectations, PE surfaces the divergence during design.
- DDR's gauge damage math uses a consistent internal representation that can be driven from a config value (raw or percent); PE picks the form.
- The user has access to a DDR World cabinet or instance for manual testing, and access to log.txt for diagnostics.
- The user runs a private DDR World server backend that can accept score uploads from mine-enabled charts; network score behavior is explicitly not this mod's concern.
- The user will implement the authoring side in `ddr-chart-tools` in parallel once the spec document from US-10 is available, and will provide mine-enabled test charts back to this feature for integration testing before acceptance.
