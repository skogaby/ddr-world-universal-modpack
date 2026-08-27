# Idea Honing — Timing Offsets

Requirements clarification Q&A. One question at a time.

---

## Q1: Which of the five timing fields should the mod expose?

The game's timing-offset record has five tunable fields (per the RE doc; to be
re-verified):

| Field | Type | Default | What it does |
|---|---|---|---|
| `SOUND_OFFSET` | i32 | 87 | audio sync; larger = audio plays later |
| `INPUT_OFFSET` | i32 | 28 | input/judge timing offset (the "SSQ"/judge offset) |
| `RENDER_OFFSET` | i32 | 17 | render/display latency compensation |
| `BOMB_FRAME_OFFSET` | i32 | 0 | shock-arrow ("bomb") frame timing |
| `HIGH_PRECISION_INPUT` | bool | on | sub-frame input timestamping (shipped on; lever = force off) |

The original `patches.js` only exposed `SOUND_OFFSET`. The RE doc recommends exposing
the whole record.

Which fields do you want this mod to expose?

- **(A) All five** — the four i32 offsets + the HIGH_PRECISION_INPUT bool. Maximum
  control; matches the doc's "manipulate the lot" recommendation.
- **(B) The four i32 offsets only** — SOUND/INPUT/RENDER/BOMB_FRAME; skip the bool.
- **(C) Just the player-facing pair** — SOUND_OFFSET + INPUT_OFFSET (audio sync +
  judge offset), the two a player most commonly wants to tune.
- **(D) Just SOUND_OFFSET** — literal port of `patches.js`.

(We can also expose all five but only surface some in the in-game UI — that's a later
UX question. This is about which fields the mod controls at all.)

**Answer:** (B) — the **four i32 offsets only**: `SOUND_OFFSET`, `INPUT_OFFSET`,
`RENDER_OFFSET`, `BOMB_FRAME_OFFSET`. The `HIGH_PRECISION_INPUT` bool is **out of
scope** for this mod (it's shipped on and the only lever is forcing it off — not
exposed here). RE work this session can still confirm its location for documentation
provenance, but the mod itself controls only the four integer offsets.

---

## Q2: How should the four offset values be set — config file, in-game options, or both?

Unlike center-arrows (a per-player cosmetic), the timing record is built **once per
run** and published to a single global config map — these are **cabinet-wide** values,
not per-player. So the per-player option pattern doesn't naturally fit; the value is one
global number per field.

Where should an operator/player set these values?

- **(A) `mod-config.json` only** — a `timing_offsets` config section with the four
  integers (and an enable flag). Values applied at boot. Simplest; no UI/texture work.
  Matches how `series_expansion` / `folder_expansion` are configured. Changing a value
  means editing JSON + restarting.
- **(B) In-game option rows only** — Scalar option rows on the Mods tab (the
  `custom_options` Scalar UiKind, like the power-user white-zone threshold), adjustable
  live in the options screen, persisted via the framework. More plumbing (option
  registration, label textures, value ranges, persistence), and since the values are
  global the per-player framework would need a global/shared treatment.
- **(C) Both** — config file provides defaults/headless control; in-game rows let you
  tune live. Most flexible, most work.

(If in-game rows are wanted, a follow-up question will cover whether they're truly
global vs. shown per-side, value ranges, and live-vs-boot application.)

**Answer:** A combination of **`mod-config.json`** + the **DLL-managed global mod
overlay menu** (the `mod_menu`, triggered by triple-pressing `0` on the numpad).
**NOT** the game's own in-game options Mods tab.

**Governing design principle (stated by the maintainer):** options that make sense
**per-player** belong on the **Mods tab** in the game's own in-game options UI (the
`custom_options` framework). Options that only make sense as **global / cabinet-wide**
config (like these timing offsets) belong **only** in the DLL-managed overlay menu,
which exists at the global context. Therefore the timing offsets are adjustable **only**
in the mod overlay menu (plus persisted/seeded via `mod-config.json`) — never via the
game's Mods tab.

**New UX requirements this introduces for the `mod_menu` overlay** (it currently only
renders boolean on/off toggles — no concept of scalar/numeric input rows):

1. The overlay needs a **scalar input row** type (numeric value with increment/decrement),
   in addition to the existing boolean toggles.
2. A **top-level boolean** "is the offset-adjustment mod active?" toggle. When **on**,
   **four additional scalar rows** are revealed for the four offsets (SOUND / INPUT /
   RENDER / BOMB_FRAME). When off, those scalar rows are hidden (conditional visibility,
   analogous to the `ShowWhen` parent-gating in `custom_options`).
3. Adjusting any scalar row edits the **global value in memory in real-time** (live
   write to the published config-map value / live state), not just at boot.

**Scope note:** this makes the mod a two-part effort — (a) extend `mod_menu` with
scalar rows + conditional visibility, and (b) the timing-offset mod itself that
registers those rows and performs the live writes. Follow-up requirements questions
will cover the scalar-row UX details (ranges, step sizes, display, persistence) and the
live-write mechanism.

---

## Q3: How should the menu structure the top-level boolean + the 4 scalar rows?

Today the overlay is a **flat list** where each row = one registered mod, rendered as a
boolean `[ON]/[OFF]` (toggled left/right), sourced from the registry. There's no
abstraction for "rows that aren't a whole mod" or for scalars.

Your "top-level boolean to activate, then 4 scalars appear" can be modeled a few ways.
Which matches your intent?

- **(A) The mod's own enable toggle IS the top-level boolean; the 4 scalars are child
  rows nested directly under it in the same list.** The timing-offsets mod already gets
  a boolean row in the menu (like every registered mod). When it's ON, four indented
  scalar rows appear immediately below it (SOUND / INPUT / RENDER / BOMB_FRAME);
  navigation flows through them inline. When OFF, they're hidden. This is the
  `ShowWhen`-style parent-gating analog. Requires the menu to support per-mod child
  rows + scalar rows in the one flat list.
- **(B) The mod's enable toggle opens a dedicated sub-screen.** The menu row stays a
  simple boolean; "entering" the timing-offsets row (or pressing a select key) opens a
  separate detail page that hosts the 4 scalar rows. Keeps the main list uniform; adds a
  navigation layer (a sub-menu concept the overlay doesn't currently have).
- **(C) A separate master-boolean row distinct from registry enable, plus 4 scalars,
  all as free-standing config rows.** The menu gains a general notion of "config rows"
  (not tied to a registered mod) — a boolean master + 4 scalars — independent of whether
  the mod is registry-enabled. More general, but decouples "mod enabled" from "offsets
  active," creating two separate on/off concepts.

My read of your description is **(A)** — the mod's enable toggle is the master boolean,
and the 4 scalars are children revealed beneath it when on. Confirm (A), or tell me if
you prefer a sub-screen (B) or a decoupled master row (C).

**Answer:** (A) — the mod's own enable toggle is the master boolean; the four scalar
rows are child rows nested directly under it in the same flat list, revealed when on,
hidden when off, navigated inline. **Take architectural cues from the existing
`custom_options` framework** (the construct used to inject mod option rows into the
game's *own* options menu): reuse the *shape* of its `UiKind::Scalar` (min/max/step)
and its `ShowWhen` parent-gating (child rows excluded from the list when the parent
predicate fails). The overlay's row model should be generalized in the same spirit —
not literally the same code (that framework targets the game's native UI; this is the
DLL overlay), but the same conceptual API (typed rows: Bool / Scalar; parent→child
visibility gating; per-row on-change).

> Reference shape (`custom_options::api`): `UiKind::Scalar { min, max, step_fine,
> step_coarse, format: ScalarFormat::Integer | FixedPoint{decimals} }`; left/right
> adjusts by `step_fine`, Start-held adjusts by `step_coarse`; `ShowWhen::Equals{
> parent_id, value }` gates child visibility; `OnChangeFn` fires on change. The overlay
> rows should adopt the same conceptual fields.

---

## Q4: Value ranges, step sizes, and display for the four scalar rows?

Each offset scalar row needs a `[min, max]`, a step (and optionally a coarse step for
Start-held / fast adjust), and a display format. Stock defaults: SOUND=87, INPUT=28,
RENDER=17, BOMB_FRAME=0. The original `patches.js` sound-offset range was 0–1000.

Two sub-decisions:

**(a) Ranges/steps.** Proposed starting point (tunable):

| Field | Default | Proposed min | Proposed max | Fine step | Coarse step |
|---|---|---|---|---|---|
| SOUND_OFFSET | 87 | 0 | 1000 | 1 | 10 |
| INPUT_OFFSET | 28 | -100? | 200? | 1 | 10 |
| RENDER_OFFSET | 17 | -100? | 200? | 1 | 10 |
| BOMB_FRAME_OFFSET | 0 | 0 | 10? | 1 | 1 |

Open questions: should INPUT/RENDER allow **negative** values (the fields are i32; a
negative judge/render offset may be meaningful)? What upper bounds make sense? Is
BOMB_FRAME a small frame count (0–~10) or wider? (We can also re-confirm sane bounds
during RE by reading the 10 presets' actual values, since those bracket the
"game-legitimate" range.)

**(b) Display format.** Plain integers (`ScalarFormat::Integer`)? Or anything that
should render with units/sign (e.g. show `+28` / `-12`)?

What ranges/steps do you want, should negatives be allowed for INPUT/RENDER, and is
plain-integer display fine? (If you'd rather I derive sensible bounds from the 10
presets during the RE phase and bring them back, say so and I'll treat the table above
as provisional.)

**Answer:** Negative values are valid in offset contexts, so **all four fields use a
uniform range of `[-1000, 1000]`, fine step `1`, coarse step `20`** (coarse = Start-held
/ fast adjust). Plain integer display (signed — a negative value renders with its `-`).
No per-field bound tuning; the same range applies to SOUND / INPUT / RENDER /
BOMB_FRAME. (The mod clamps writes to this range.) The defaults remain the game's stock
values (SOUND=87, INPUT=28, RENDER=17, BOMB=0) unless overridden by config / menu.

---

## Q5: Overlay input model — switch navigation to cabinet menu buttons + close the game-suppression gap

**Maintainer's realization:** the overlay shouldn't rely on numpad input for navigation.
Numpad keys don't support hold semantics, so coarse (Start-held) scalar adjustment isn't
expressible. The whole overlay should be navigable with the **cabinet menu buttons**
(Start / Up / Down / Left / Right), enabling "hold Start + Left/Right = coarse step." And
while the overlay is open, those cabinet-button inputs should be **prevented from reaching
the game** (we already do this for the numpad).

**Findings from reading the current input code** (`input_manager.rs`, `mod_menu.rs`,
`hello_world.rs`):

1. **The cabinet menu buttons are already half-wired for nav.** `handle_exclusive_input`
   already accepts `MENU_UP/DOWN/LEFT/RIGHT` *alongside* the `NUM_8/2/4/6` numpad
   substitutes. So switching to cabinet-button navigation is mostly a matter of making
   them primary (and adding the coarse-step gesture), not building nav from scratch.
2. **Hold state is already tracked — no new infra needed for coarse.** The `arkMDXGet*`
   exports return `(trigger, hold)` out-params; `input_manager` ORs them into the
   per-player held bitmask (`player_state`), exposed via `get_button_state(player)`. So on
   a `MENU_LEFT`/`MENU_RIGHT` Pressed event we can check `get_button_state(player) &
   START` to pick coarse vs. fine. (Start is reported as a held bit like any other.)
3. **Suppression gap (the new work item).** Game-side input suppression today is
   **numpad-only**: the `arkMDXGet10Key` detour zeros the buffers for game-side callers
   when `IS_INPUT_SUPPRESSED` is set (modpack reads bypass via the `IN_MODPACK_POLL`
   re-entry flag). There is **no** detour on the five menu-button exports
   (`arkMDXGetStart/Up/Down/Left/Right`), so cabinet-button presses currently reach the
   game even with the overlay open. Driving the overlay with cabinet buttons therefore
   requires adding analogous suppression for those five exports (detour each, zero the
   trigger/hold out-params for game-side callers while the overlay is open, modpack poll
   bypasses via the same re-entry flag).

**Decision (maintainer):** Yes — make the overlay navigable via cabinet menu buttons,
support Start-held coarse adjust on scalar rows, and suppress those cabinet-button inputs
from the game while the overlay is open (close the gap that currently exists only for the
numpad). This is overlay-infrastructure work that benefits the whole menu, not just the
timing mod.

**Open sub-decision (need confirmation):**
- **Open/close gesture.** Opening the overlay is currently a **triple-press of numpad 0**;
  closing is also triple-0. If navigation moves to cabinet buttons, should the open/close
  gesture **stay on numpad-0 triple-press** (numpad still used solely for the open/close
  toggle, everything else cabinet buttons), or move to a cabinet-button gesture too?
- **Numpad nav retained as alias?** Should the existing `2/4/6/8` numpad nav be **kept as
  a secondary alias** (harmless, already coded), or **removed** so the overlay is strictly
  cabinet-button driven? (Keeping the numpad-0 open gesture implies numpad isn't fully
  abandoned regardless.)

**Answer (sub-decision):**
1. **Open/close gesture unchanged** — keep the **triple-press of numpad 0** to open and to
   close. Numpad 0 remains the toggle; navigation/adjustment moves to cabinet buttons.
2. **Cabinet menu buttons are the primary navigation** (Up/Down navigate, Left/Right
   adjust, Start-held = coarse adjust on scalars), and the existing **`2/4/6/8` numpad
   substitutes are retained as a secondary alias** (no removal). Both drive the same
   actions. Note the numpad alias can only ever do fine steps (no hold semantics); coarse
   adjust is a cabinet-button-only capability (Start-held). The game-side suppression must
   cover the cabinet menu buttons too (the new detour work), so cabinet-button nav doesn't
   bleed into the game while the overlay is open.

---

## Q6: How "live" must the offset writes be, and what's the apply mechanism?

You said adjusting a scalar should "edit the global value in memory in real-time." The RE
doc describes two levers: (1) **patch the defaults** in the record builder (`.rdata` rec0
ints / inline imm32s) — a static value applied at boot; (2) **re-set the published value
live** via the config-map setter the game uses (`FUN_1801acbf0(key, value)` analog), keyed
by `"SOUND_OFFSET"` etc.

The catch: whether a live write actually changes behavior **this song** depends on how each
subsystem consumes its offset — some read the published config value fresh when they need
it (live-honoring), others snapshot it once at subsystem init or at gameplay-start
(latched). That's an RE fact to verify per field, not assume.

What's the expectation you want the design to target?

- **(A) Best-effort live, honest about latching.** Write the live config-map value (and/or
  the live state) immediately on every scalar change, so any field the game reads fresh
  updates in real-time. For any field that turns out to be latched (read once), it applies
  on the **next** natural reload (next song / re-entry) — and we document which fields are
  truly live vs. next-song. Also write at boot from config so a fresh boot starts at your
  configured values. (Pragmatic; matches "edit in real-time" where the game allows it.)
- **(B) Live-only, no boot patch.** Only re-set values via the setter at/after subsystem
  init; don't patch the builder defaults. Simpler surface, but a field read before our
  first write uses stock until we set it.
- **(C) Boot-patch only (defaults), no mid-session writes.** Patch the record builder
  defaults from config at boot; the menu edits take effect only after a restart. Contradicts
  the "real-time" ask — listed for completeness.

I read your intent as **(A)**: write live on every change for immediate effect where the
game honors it, seed from config at boot, and be explicit (in logs/docs) about any field
that only takes effect on the next song because the game latched it. The RE phase will
determine, per field, which are live vs. latched. Confirm (A) or adjust.

**Answer:** (A). Real-time adjustment is a **tuning convenience, not a competitive-play
feature** — there's no realistic scenario where a player steps away mid-session, changes
timing, and resumes expecting valid scores. The realistic use is *dialing in* the values
without caring about score that run, so live adjustment is worth supporting **if
feasible**. Design target: write the live value on every change (immediate effect wherever
the game honors a fresh read), seed all four from config at boot. **If real-time isn't
actually feasible for a given field (the game latches it), that's acceptable — just
document it** (which fields are live vs. next-song/next-boot). No requirement to force a
re-latch or re-run the publish path for live effect; document the limitation instead.

---

## Q7: Config schema + persistence for the offsets and the master toggle?

The values need to persist across reboots (set them once, they stick). `mod-config.json`
already has: a `mods` map (per-mod enable booleans, saved via `config::save_mod_states`),
typed per-feature sections (`series_expansion`, `folder_expansion`, `custom_options`,
`diagnostics`), and a generic `config::save_json_key(key, value)` that preserves all other
keys. The master enable/disable boolean is naturally the mod's entry in the `mods` map
(every registered mod gets one). The four offset values are new persisted state.

Proposed schema — a typed `timing_offsets` section holding just the four values:

```jsonc
{
  "mods": {
    "timing-offsets": true            // master toggle (the mod's registry enable)
  },
  "timing_offsets": {
    "sound_offset": 87,
    "input_offset": 28,
    "render_offset": 17,
    "bomb_frame_offset": 0
  }
}
```

Behavior:
- **Master toggle** = the mod's `mods["timing-offsets"]` entry, toggled by its top-level
  boolean row in the overlay (existing `save_mod_states` path). When off, the mod is
  inert and the offsets revert to the game's own stock values (we stop overriding).
- **Four values** persist under a typed `timing_offsets` section. Written whenever a
  scalar changes in the overlay (read-modify-write via `save_json_key`, preserving other
  keys). Seeded into memory at boot. Absent keys → game's stock defaults (87/28/17/0).

Questions:
1. Is a **typed `timing_offsets` section** (four named integer keys) the right shape, or
   would you prefer they live somewhere else (e.g. under a shared namespace)?
2. When the **master toggle is OFF**, should the offsets revert to **stock game values**
   (we stop writing), or stay at the **last-set values minus the override** (i.e. off
   simply means "don't apply")? My assumption: OFF = revert to stock (don't override at
   all); ON = apply the four configured values.
3. Should a scalar change **persist immediately** on each adjustment (like the mod-toggle
   booleans do), or only on overlay close? My assumption: immediately (simplest, matches
   `save_mod_states` behavior), with the understanding these are infrequent writes.

**Answer:** All three recommendations/interpretations confirmed:
1. **Typed `timing_offsets` section** with four named integer keys (`sound_offset`,
   `input_offset`, `render_offset`, `bomb_frame_offset`) — the shape shown above. Added to
   the typed `ConfigFile` like the other per-feature sections.
2. **Master toggle OFF = revert to the game's stock values** (stop overriding entirely);
   ON = apply the four configured values. (Live revert is best-effort per Q6: where the
   game honors a fresh read it reverts immediately; where it latched our value, revert
   takes effect next song/boot — documented. Implies capturing/knowing the stock values so
   we can restore them; the stock defaults are 87/28/17/0, re-confirmed during RE.)
3. **Persist immediately on each scalar change** (read-modify-write via `save_json_key`,
   preserving other keys) — matches the `save_mod_states` behavior; infrequent writes.

---

## Q8: Failure / safety posture if RE targets don't resolve?

Per CLAUDE.md the project favors **graceful degradation**: missing signatures/hooks → log
a warning and disable cleanly, never panic across FFI, never crash, let other mods run.
This mod has a few distinct resolution targets that could independently fail:

- the **config-map setter** (`FUN_1801acbf0` analog) used for live writes / boot seeding,
- the **config-map keys / record-builder anchor** needed to know what to set,
- (if the live-setter path proves infeasible) the **builder default patch sites**,
- the **overlay scalar-row + cabinet-button-suppression infra** (separate from the offset
  writes — this is `mod_menu` work).

Confirming the posture and partial-failure handling:

- **(A) Graceful degradation (project default).** If the offset-apply mechanism (setter /
  anchors) can't resolve, the mod logs a warning and goes inert — its master toggle either
  isn't offered or is shown disabled, and no scalar rows appear. If the overlay scalar-row
  infra fails, fall back cleanly (no scalar rows; the rest of the menu unaffected). Never
  panic, never crash. No field is a hard "skip the whole DLL" requirement.
- **(B) Hard requirement.** Declare the setter/anchors in `required_signatures()` so the
  mod is skipped entirely if any are missing.

My assumption is **(A)**, consistent with center-arrows and the rest of the project. The
one nuance worth your call: if the offset-write mechanism resolves but **persistence or the
overlay rows** don't, should the mod still apply **config-file-seeded** offsets at boot
(headless value control still works even with no in-game UI)? I'd say **yes** — the
boot-seed path and the in-overlay-tuning path degrade independently, so a player with a
hand-edited `timing_offsets` section still gets their offsets even if the scalar-row UI
couldn't initialize. Confirm (A) + independent degradation, or adjust.

**Answer:** (A) — graceful degradation, with a clear two-tier split between what's
**load-bearing for the mod** vs. what's a **non-fatal UI layer**:

- **Load-bearing (failure ⇒ the whole mod gracefully disables):** the **offset-apply
  mechanism** — i.e. resolving *where/how to inject the four offsets* (the config-map
  setter + the keys/anchors, or whatever the RE phase settles on as the apply path). If we
  cannot resolve how to apply the offsets at all, the mod is useless, so it disables
  itself cleanly (logs a warning, no master toggle effect, no rows). It never panics or
  crashes the game or other mods.
- **Non-fatal UI layer (failure ⇒ mod still works via config file):** the **overlay
  scalar-row rendering / cabinet-button infra**. If for some reason the overlay menu can't
  render the rows, the user can **still apply and configure the mod purely through
  `mod-config.json`** (boot-seed path). The in-overlay-tuning path and the config-file
  path degrade independently; losing the UI does not disable the mod's actual function.

So: offset-apply path = effectively required (graceful self-disable if absent); overlay UI
= best-effort enhancement on top. Both via the project's standard graceful pattern (no
hard `required_signatures()` that would skip registration — the mod self-disables in
`enable()` instead, matching center-arrows).

---

## Q9: Overlay scalar-row presentation — labels, value display, layout?

The overlay renders everything as plain `TextWidget` text (white text, a `>` cursor, a
right-aligned `[ON]/[OFF]` status column at x≈1100) — it does **not** use the game's native
digit-sprite compositor that `custom_options` scalar rows use. So a scalar row here is just
text we format ourselves.

Current per-row layout: name (left), description (smaller, below the name), status
(right). For the four child scalar rows, proposing:

- **Child rows are visually indented** under the master "Timing Offsets" row to signal the
  parent/child relationship (e.g. name prefixed with a couple spaces or a `└`/`-` marker).
- **Row label** = the field name, human-readable: e.g. `Sound Offset`, `Input Offset`,
  `Render Offset`, `Bomb Frame Offset`.
- **Value display** in the status column = the current integer, signed, e.g. `12`, `-40`,
  `87`. When the selected scalar row is highlighted, Left/Right adjust it (Start-held =
  coarse). Could optionally show `< 87 >` to hint adjustability on the selected row.
- **Description line** = a short hint, e.g. for Sound Offset: "Audio sync; higher = audio
  later". (Optional — could omit to keep rows compact.)

Questions:
1. **Labels** — are `Sound Offset` / `Input Offset` / `Render Offset` / `Bomb Frame Offset`
   the wording you want? (vs. the engine key style `SOUND_OFFSET`, or something more
   descriptive like `Judge Offset` for INPUT_OFFSET since the doc calls it the judge offset.)
2. **Value affordance** — plain number in the status column, or the `< value >` style on the
   selected row to signal Left/Right adjusts it?
3. **Per-row description hints** — include short hints, or keep rows label+value only?

My defaults if you have no strong preference: human-readable Title Case labels, `< value >`
on the selected row (plain number when not selected), and include short one-line hints.
This is purely overlay cosmetics, so it's easy to revise during implementation.

**Answer:**
1. **Labels confirmed:** `Sound Offset`, `Input Offset`, `Render Offset`, `Bomb Frame
   Offset`.
2. **Value display:** plain signed integer (e.g. `+12`, `-40`, `87`) in the value column.
   No `< >` affordance for now — iterate later if needed.
3. **Hint/flavor text — ALL-OR-NOTHING, backed by RE findings (maintainer's revised
   call).** Preference is to include hints for **all four** rows, but every hint must be
   backed by confirmed RE findings about the field's actual gameplay effect — not guessed.
   This is an explicit, **bounded** research task:
   - The RE phase makes a **rough attempt** to determine what each of the four values
     actually does with respect to gameplay (which subsystem consumes it, sign/polarity,
     units — ms vs. frames, effect direction).
   - **If all four can be confidently characterized**, ship hints for all four (e.g.
     `Sound Offset (global audio offset, higher = audio is later)`).
   - **If confirming the semantics turns into a deep rabbit hole** for any of them, **omit
     all four hints** (rows ship label + value only). No partial/guessed set — it's all or
     nothing.

   Starting evidence: **Sound Offset is already effectively confirmed** — the 32-bit
   `patches.js` tooltip states *"Larger numbers make audio later (Default: 87)"* and the RE
   doc agrees. The research effort focuses on bringing Input / Render / Bomb Frame up to
   the same confidence bar (or concluding it's not worth the depth, in which case all
   hints are dropped). The decision (all vs. none) is made at the end of the research phase
   based on what's confidently established within a reasonable effort.

---

## Q10: Mod identity, and scope boundary vs. the preset-selector idea (Hack 6)?

Two loose ends before closing requirements:

**(a) Mod identity.** Proposed: id `timing-offsets` (kebab-case, the `mods`-map key and
registry id), display name **"Timing Offsets"**, description something like *"Adjust the
game's global timing offsets (sound/input/render/bomb)."* Good, or different wording?

**(b) Scope boundary vs. Hack 6 (timing-preset selector).** The RE doc's Hack 6 is a
*separate* idea: force which of the 10 hardware-keyed presets the game uses (detour the
selector `FUN_180012e50`, return a chosen index). That's tightly coupled to the same
timing record this mod touches — but it's a **different lever** (pick a whole preset row
vs. override individual published values). I'm treating Hack 6 as **out of scope** for this
mod (a future, separate mod), and this mod overrides the four individual offset values
regardless of which preset the game selected. Confirm Hack 6 is out of scope here, or do
you want preset-selection folded into this mod?

**(c) Anything else?** Any other requirement, constraint, or interaction (e.g. ordering
vs. other mods, behavior when another timing-related mod is active, attract-mode display)
I should capture before moving to the RE/research phase?

**Answer:**
(a) **Mod identity confirmed:** id `timing-offsets`, display name **"Timing Offsets"**,
description *"Adjust the game's global timing offsets (sound/input/render/bomb)."*
(b) **Hack 6 (preset selector) is OUT OF SCOPE** — a potential future, separate mod the
maintainer may or may not implement; if they do, they'll handle expanding this feature at
that time. This mod overrides the four individual published offset values regardless of
which preset the game selected.
(c) **Nothing else** — requirements are complete.

---

## Requirements clarification — provisional summary

1. **Fields (Q1):** Expose the **four i32 offsets only** — `SOUND_OFFSET`, `INPUT_OFFSET`,
   `RENDER_OFFSET`, `BOMB_FRAME_OFFSET`. `HIGH_PRECISION_INPUT` bool is **out of scope**
   (location may still be documented for provenance).
2. **Config surface (Q2):** `mod-config.json` + the **DLL-managed overlay menu** (triple-0)
   only — **not** the game's native Mods tab. Governing principle: per-player options →
   game's Mods tab (`custom_options`); **global/cabinet-wide** options → DLL overlay only.
   Timing offsets are global, so overlay-only.
3. **Menu structure (Q3):** The mod's **own enable toggle is the master boolean**; the four
   scalar rows are **child rows** nested under it in the same flat list, revealed when on,
   hidden when off — `ShowWhen`-style parent gating. Architectural cues from
   `custom_options` (typed rows, parent→child visibility, per-row on-change), reimplemented
   for the overlay (not the same code).
4. **Ranges (Q4):** All four uniform **`[-1000, 1000]`**, fine step **1**, coarse step
   **20** (Start-held). Negatives allowed. Plain signed-integer display. Clamped to range.
   Stock defaults 87/28/17/0 unless overridden.
5. **Overlay input (Q5):** **Cabinet menu buttons become primary nav** (Up/Down navigate,
   Left/Right adjust, **Start-held = coarse**), with `2/4/6/8` numpad retained as a
   secondary alias (fine-only). **Open/close stays triple-0.** New infra: **suppress the
   five cabinet menu-button exports from the game while the overlay is open** (mirror the
   existing numpad `arkMDXGet10Key` suppression; the gap is real today). Hold detection
   already available via `get_button_state` (the `arkMDXGet*` `hold` out-param).
6. **Liveness (Q6/A):** **Best-effort live** — write the live value on every change for
   immediate effect wherever the game honors a fresh read; **seed from config at boot**.
   Real-time is a *tuning convenience, not competitive*; if a field is latched and can't
   update live, that's acceptable — **document which fields are live vs. next-song/boot**.
   No forced re-latch.
7. **Persistence/schema (Q7):** Typed **`timing_offsets`** section (`sound_offset`,
   `input_offset`, `render_offset`, `bomb_frame_offset`) in `ConfigFile`. **Master toggle =
   the mod's `mods["timing-offsets"]`**; **OFF = revert to stock** (stop overriding; live
   revert best-effort). Scalar changes **persist immediately** (read-modify-write via
   `save_json_key`).
8. **Safety (Q8):** Graceful degradation, two tiers — **offset-apply mechanism is
   load-bearing** (can't resolve how to inject offsets ⇒ whole mod self-disables cleanly);
   **overlay UI is non-fatal** (rows fail to render ⇒ mod still works via `mod-config.json`
   boot-seed). Never panic across FFI; never crash. No hard `required_signatures()` — mod
   self-disables in `enable()` like center-arrows.
9. **Overlay presentation (Q9):** Labels `Sound Offset` / `Input Offset` / `Render Offset` /
   `Bomb Frame Offset`; child rows indented under the master row; plain signed-integer value
   in the value column (no `< >` for now).
10. **Hints (Q9 revised):** **All-or-nothing**, RE-backed. Bounded research task to
    characterize each field's gameplay effect (consumer, sign/polarity, units). All four
    confidently characterized → ship all four hints; deep rabbit hole → omit all. Sound
    Offset already effectively confirmed (`patches.js` tooltip + RE doc: higher = audio
    later); research focuses on Input/Render/Bomb to the same bar.
11. **Identity/scope (Q10):** id `timing-offsets`, name "Timing Offsets". **Hack 6 preset
    selector is out of scope** (future separate mod).

**Two-part effort:** (I) extend the `mod_menu` overlay — typed scalar rows + parent/child
visibility gating + cabinet-button navigation with Start-held coarse + game-side
suppression of the five menu-button exports; (II) the `timing-offsets` mod — register the
master toggle + four scalar rows, apply/seed/persist the four offset values (live where the
game honors it), graceful self-disable if the apply path is unresolved.

**Requirements clarification: COMPLETE** (pending user confirmation). Proposing to proceed
to the RE/research phase next.

### Open research items (to resolve in the research phase)
- **R1 — Timing record + apply path (re-verify Hack 4 fresh).** Confirm on **both** 64-bit
  builds (20260324 + cabinet 20260526): the record builder, the init/publisher, the record
  layout (5×, 0x14 bytes, the four i32 offsets + the bool), the **config-map setter**
  signature(s) used to publish/re-set int values, and the config-map key strings
  (`"SOUND_OFFSET"` etc.). The binary is the source of truth — documented absolute
  addresses are provenance only. Author AOB signatures that resolve on both builds (and
  ideally survive the running OmniMAX-style build per the memory-note caveat).
- **R2 — Liveness per field (Q6).** For each of the four offsets, determine whether the
  consuming subsystem reads the published config value **fresh** (live-honoring) or
  **latches** it at init/gameplay-start. Establishes which fields update in real-time vs.
  next-song/boot, for documentation.
- **R3 — Field semantics for hints (Q9/Q10).** Bounded attempt to characterize each
  field's actual gameplay effect (consumer site, sign/polarity, units ms-vs-frames,
  direction). Drives the all-or-nothing hint decision. Sound Offset already confirmed.
- **R4 — Boot-seed mechanism + stock-value capture (Q6/Q7).** Determine how to seed values
  at boot (re-set via the config-map setter after subsystem init, vs. patch the builder
  defaults) and how to capture/restore the **stock values** for the master-OFF revert.
  Decide setter-re-set vs. default-patch as the primary apply lever.
- **R5 — Overlay infra (cabinet-button suppression + scalar rows).** Confirm the five
  `arkMDXGetStart/Up/Down/Left/Right` exports can be detoured for game-side suppression the
  same way `arkMDXGet10Key` is (out-param shape `(i32, *mut u32, *mut u32)`), and the
  render-thread/widget approach for scalar rows + parent/child visibility in `mod_menu`.
  (Largely codebase research, not Ghidra.)
