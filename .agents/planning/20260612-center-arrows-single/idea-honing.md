# Idea Honing — Center Arrows for Single Player

Requirements clarification Q&A. One question at a time.

---

## Q1: What exactly should "centered" cover — just the playfield, or the surrounding HUD too?

The 32-bit hack moved the **lane/arrow receptors** (`arrow_raw`, `arrow`, `freeze_judge`)
to the screen center and forced the centered ("double") lane geometry/skin. It did **not**
touch the score/gauge/combo/judge/bpm/option HUD elements — those stayed where the 1P
layout put them (to one side).

For this mod, what's the intended visual result?

- **(A) Lane only (match the original hack):** center the arrow receptors + freeze judge +
  lane skin. Leave score/gauge/combo/judge/etc. in their stock 1P positions. Simplest,
  proven, matches what players expect from "center arrows".
- **(B) Lane + the lane-attached readouts:** also center `judge`, `combo`, `fast_slow`,
  `filter`, `score_compare` (the elements the builder positions relative to the lane via
  the same `%s/..._usr` keys), so the under/over-lane readouts track the centered lane.
- **(C) Full HUD center:** reposition everything the builder lays out to a centered
  arrangement. Largest scope; risks overlapping elements that were authored for a
  side-offset layout.

(We can also make this configurable, but I want the default/primary intent first.)

**Answer:** (B) — center the lane **and** the lane-attached readouts. Specifically the
elements positioned relative to the lane: `arrow_raw`, `arrow`, `freeze_judge`, plus
`judge`, `combo`, `fast_slow`, `filter`, `score_compare`. Do **not** move `score` or
`gauge` (they stay in stock 1P positions). (`bpm`/`option` referenced other builder keys —
`bpm_%dp_usr`, `option_%dp_usr` — and are out of scope; only the lane-relative `%s/*_usr`
group moves.)

---

## Q2: How should the mod be enabled — global toggle, or a per-player in-game option?

The codebase has two enable patterns:
- **Global mod toggle** (like `timer_freeze`, `real_speed_fix`): on/off in `mod-config.json`
  + the in-game mod menu. Affects the whole cabinet.
- **Per-player custom option** (like `premium_free`, the WebUI/power-user toggles): a row on
  the Mods tab that each side sets independently, persisted per-player.

For "center arrows single player", which fits your intent?

- **(A) Global toggle only:** one on/off for the mod. Simple. When on, single-player play is
  centered for whoever is playing.
- **(B) Per-player option:** each player can choose centered or not, from the in-game
  options. More flexible but more plumbing (custom_options service, persistence).
- **(C) Global toggle now, per-player later:** ship the simple global toggle first; leave
  per-player as a future enhancement.

Note: this mod only affects **single-player** sessions by nature (the centering only makes
sense when one side is playing). The question is about the enable *mechanism*, not the
single-vs-double detection (which we'll cover separately).

**Answer:** (B) — per-player in-game option (via the `custom_options` service, persisted
per-player like the other cosmetic toggles). Important explicit gate: the centering must
**only** apply when single-player mode is active — it must NOT trigger in two-player /
versus mode even if a logged-in player has the option enabled. So the effective condition
is `option_enabled[side] AND single_player_session`. (The exact single-vs-double signal is
covered in a later question.)

---

## Q3: When BOTH the option is on and it's a single-player session, which side's lane gets centered?

In a single-player session the active player can be on the P1 side or the P2 side (the
game still lays out two "sides" in the builder loop, but only one is actually playing).
The builder's centered geometry (`double_lane_usr` / `lane_..._double`) is a single fixed
center position regardless of which side is playing.

How should the mod decide *what* to center and to where?

- **(A) Center the active playing side to the screen center.** Detect which side is the
  lone active player and move that side's lane-relative elements to the centered X; leave
  the inactive side untouched (it isn't rendering a playfield anyway). This matches the
  original hack's intent (one centered playfield).
- **(B) Always center the P1 side specifically** (assume single-player = P1). Simpler, but
  wrong if the lone player is on the P2 side (e.g. P2-side single play).
- **(C) Center whichever side(s) have the option enabled**, independent of which is the
  active player — but gated on single-player mode.

The cleanest is usually (A): in single-player there's exactly one active side, center that
one. Does that match your expectation, or do you specifically want P2-side single play to
behave a certain way?

**Answer:** (A) — center the lone active playing side to the screen center, whether that
side is P1 or P2. Detect which single side is active and move that side's lane-relative
elements to the centered X; the inactive side is untouched (no playfield rendering there).

---

## Q4: The lane *skin/geometry* — force the centered "double" lane, or only move the elements?

The original hack did two distinct things: (1) forced the **lane skin + geometry** to the
centered "double" variant (`double_lane_usr` lane key + `lane_%s_%s` → `"double"`
selector), and (2) repositioned the arrow/freeze elements to X=center.

These are somewhat independent. Just moving the receptor X without switching the lane art
could leave the **lane background/frame graphic** still drawn in the side-offset position,
so the arrows would float over a misaligned lane skin. Forcing the "double" lane art makes
the whole playfield (lane backdrop + arrows) consistently centered.

For this mod:

- **(A) Do both (match the original):** force the centered "double" lane skin/geometry AND
  reposition the lane-relative elements. Produces a visually consistent centered playfield.
  This is what the original hack did and what looks "right".
- **(B) Only reposition elements:** move the arrow/judge/combo/etc. X but leave the 1P lane
  skin where it is. Likely looks wrong (arrows off the lane art) — generally not desired.

I expect (A), since (B) tends to look broken. Confirm, or tell me if the lane art should be
handled differently (e.g. you've seen the centered double-lane art look acceptable/not).

**Answer:** Preferred = **(C) keep the SINGLE lane skin but manually reposition it to
center**, rather than switching to the double-lane skin and relying on the game's default
double-centering. The double-lane skin is a different graphic (full-width / two-lane
backdrop); the player wants the *single* lane art, just shifted to the center X. **Fallback
= (A)** (force the centered "double" lane skin + reposition elements) if repositioning the
single lane skin proves infeasible.

**RESEARCH ITEM (feasibility):** Determine whether the single lane skin's position can be
moved to center independently. In the 64-bit builder `FUN_18006c230`, the lane is resolved
via `FUN_18021bae0(parent, "%dp_lane_usr")` and bound through `FUN_18021c170(..., "lane_%s_%s")`
(AFP layer position), which is a *different* path than the `FUN_18006f5d0(name, &coord)`
setter used for arrow/judge/etc. Need to confirm: (a) does the single lane skin's X come
from an AFP layer position we can override, or is it baked into the `%dp_lane_usr` template;
(b) can we set it to the same centered X the elements use; (c) if not feasible, fall back to
forcing `double_lane_usr` + `lane_..._double` (option A). This is the first research task.

---

## Q5: What's the centered X coordinate, and how should the mod obtain it?

The 32-bit hack hardcoded X=495. Options: (A) derive at runtime from the centered/double
lane path or screen-center math (version/resolution-agnostic), (B) hardcode 495 / a
re-confirmed constant, (C) config-driven with a default.

**Answer:** **(B) hardcode the centered X = 495**, same as the original hack, for now. The
game runs at a fixed resolution (1280×720) and is expected to keep doing so, so a constant
is acceptable. Define it as a single named constant in the mod (not scattered), trivially
tunable later if needed.

**Background captured (why 495 isn't an obvious pixel-center):**
- 495 is **not framebuffer pixel space** — `FUN_18006f5d0` stores coords in the engine's
  authored layout/virtual-canvas space, mapped to pixels by a downstream scale+offset, so
  495 ≠ 640 px and isn't expected to be the pixel midpoint.
- 495 is an **anchor/reference X, not the visible midpoint.** The builder computes
  `arrow.x = arrow_raw.x − laneWidth/2`; the hack sets the `arrow_raw` reference to 495 and
  the engine's own `−width/2` shift yields the final centered draw origin.
- The hack never touches **Y** (only `dword[0]`/X is written), so vertical position is
  unchanged by design.
- Likely hand-tuned by the original author, not computed.

**Decision:** hardcode 495 as the centered X-reference, applied the same way the original
applied it (to the `arrow_raw`/`freeze_judge` reference, letting the engine's `−width/2`
math produce the final position). Revisit deriving-from-double-lane only if 495 proves
wrong on the 64-bit build during testing.

---

## Q6: Failure / safety behavior — what should happen if a signature or hook can't be resolved?

Per CLAUDE.md the project favors **graceful degradation**: if a required AOB signature
doesn't resolve or a hook can't install, the mod should log a warning and disable itself
cleanly rather than crash the game.

For this mod the critical resolution targets are: the layout setter `FUN_18006f5d0` (the
hook point), single-vs-double / active-side detection, and (depending on Q4 research) the
lane-skin reposition path.

Confirming the intended posture:

- **(A) Graceful degradation (project default):** if any required signature/hook fails to
  resolve, log a warning, leave the option non-functional (or hide it), and let the rest of
  the game/mods run normally. Never panic across the FFI boundary.
- **(B) Hard requirement:** declare the signatures in `required_signatures()` so
  `ModRegistry` skips the whole mod if they're missing (still no crash, but the mod won't
  enable at all if any piece is unresolved).

Is (A) the right posture, and are there any pieces you'd want treated as hard requirements
(B) — i.e. "if we can't do X, don't half-apply the centering"?

**Answer:** (A) — graceful degradation. If the layout-setter hook or required detection
signatures don't resolve, log a warning and leave the mod inert (option does nothing /
isn't offered); never panic across FFI, never crash the game, and let other mods run. No
piece is escalated to a hard "skip the whole mod" requirement. (Implementation detail for
design: prefer to no-op the centering cleanly if a sub-part is missing rather than
half-apply in a way that looks broken, but a partially-applied state is not considered a
crash risk — worst case is a cosmetic glitch that the player can toggle off.)

---

## Q7: Does the centering need to react live to mid-session changes, or is per-frame-at-build enough?

The layout builder `FUN_18006c230` runs to (re)lay out the HUD at certain points (e.g.
entering gameplay, possibly on style/option changes). Our hook on `FUN_18006f5d0` fires
whenever the builder positions an element, so the centering is naturally applied **whenever
the game rebuilds the layout**.

Question is whether anything needs to force a re-layout or react live:

- **(A) Apply at layout-build time only (passive hook).** We only adjust coordinates as the
  builder emits them. The option value is read at build time. If a player toggles the option
  mid-song, it takes effect the next time the game rebuilds the layout (next song / re-entry)
  — not instantly. Simplest; matches how a cosmetic layout option normally behaves.
- **(B) React live / force re-layout on toggle.** If the option changes mid-gameplay, force
  the playfield to re-center immediately. More complex (need to find/trigger a re-layout or
  move live element transforms), and arguably unnecessary for a setting you'd choose before
  playing.

For a layout option like this, (A) is normal (you set it in options, it applies when play
starts). Is "applies on next layout build" acceptable, or do you specifically need live
mid-song re-centering?

**Answer:** (A) — apply at layout-build time only via the passive `FUN_18006f5d0` hook. The
option value is read when the builder runs; toggling mid-song takes effect on the next
layout rebuild (next song / re-entry), which is acceptable for a pre-play cosmetic setting.
No live re-layout machinery needed.

---

## Q8: The per-player option row — label, placement, default, and persistence?

The mod exposes a per-player option (Q2) via the `custom_options` service, on the Mods tab,
persisted like the other cosmetic toggles. Confirming the specifics:

- **Label / wording:** what should the option row read? e.g. "CENTER PLAY (1P)",
  "CENTER ARROWS", "CENTER SINGLE PLAY". (It only takes effect in single-player, so the
  label might hint at that.)
- **Type:** a simple **ON/OFF** toggle (matching the binary nature of the hack)?
- **Default:** OFF (opt-in) — consistent with other cosmetic mods?
- **Persistence:** the standard per-player persistence the framework already provides
  (network + JSON cache, gated by the existing `persist_network` / `persist_json` flags) —
  same as every other custom option, nothing special?

My assumption: an **ON/OFF toggle, default OFF, standard per-player persistence**, with a
label like **"CENTER PLAY"**. Confirm the label you want and whether any of those defaults
should differ.

**Answer:**
- **Label:** **"CENTER ARROWS (1P ONLY)"** — generated as an option-row texture by the
  existing Python script under `scripts/` that produces option textures (same pipeline the
  other custom-option rows use). _Research/impl item: identify the exact script + invocation
  and the generated texture name/location so the design references it precisely._
- **Type:** boolean ON/OFF toggle.
- **Default:** OFF (opt-in).
- **Persistence:** standard per-player persistence (network + JSON cache, gated by the
  existing `persist_network` / `persist_json` flags) — nothing special.

---

## Q9: Anything else to gate on besides single-player mode — game mode / scene restrictions?

We've established the centering applies only in single-player sessions (Q2/Q3). Are there
other contexts where it should be suppressed, or is "single-player + option on, whenever the
HUD layout builder runs" the complete gate?

Consider:
- **Special game modes / scenes** that also use the gameplay HUD builder but where centering
  would be unwanted or look wrong — e.g. course/nonstop mode, replay/auto-demo, battle/versus
  variants, the network-matching "dance_matching" layout the builder also handles. Should any
  of these be excluded?
- **Battle/BPL or event modes**, if those are present in this build.

Options:
- **(A) Gate only on single-player** (+ option on). Apply in every single-player context the
  builder lays out, including courses. Simplest; assume centering is always fine when one
  player is playing.
- **(B) Gate on single-player AND restrict to normal gameplay**, explicitly excluding any
  special modes you call out (e.g. matching/versus already excluded by the single-player
  gate, but also exclude X/Y/Z).

Is single-player the only gate you care about, or are there specific modes/scenes to exclude?

**Answer:** (A) — gate only on single-player (+ the per-player option being on). Apply in
every single-player context the builder lays out (including courses/nonstop, etc.); no
additional mode/scene exclusions. The single-player condition is the complete gate.

---

## Requirements clarification — provisional summary

1. **Scope (Q1):** Center the lane **and** the lane-relative readouts — `arrow_raw`, `arrow`,
   `freeze_judge`, `judge`, `combo`, `fast_slow`, `filter`, `score_compare`. Do **not** move
   `score` or `gauge`. `bpm`/`option` out of scope.
2. **Enable mechanism (Q2):** Per-player in-game option via `custom_options` (Mods tab),
   standard per-player persistence. Hard-gated so it applies **only** in single-player —
   never in 2P/versus, regardless of the per-player setting.
3. **Which side (Q3):** Center the lone **active** playing side (P1 or P2) to screen center;
   inactive side untouched.
4. **Lane skin (Q4):** Preferred — keep the **single** lane skin but reposition it to center.
   Fallback — force the centered "double" lane skin + selector (original behavior) if
   single-skin reposition is infeasible. **Research item.**
5. **Centered X (Q5):** Hardcode **495** (layout-space X reference, same as original), as a
   single named constant. Applied to the `arrow_raw`/`freeze_judge` reference; engine's own
   `−laneWidth/2` math yields the final draw position. No Y change.
6. **Safety (Q6):** Graceful degradation — missing signatures/hooks → log + inert mod, no
   panic, no crash, other mods unaffected. No hard "skip whole mod" requirement.
7. **Liveness (Q7):** Apply at layout-build time via the passive `FUN_18006f5d0` hook; option
   read at build time; mid-song toggle applies on next layout rebuild. No live re-layout.
8. **Option row (Q8):** Label "CENTER ARROWS (1P ONLY)" (generated via the existing `scripts/`
   option-texture tool), boolean ON/OFF, default OFF, standard persistence.
9. **Gating (Q9):** Single-player is the **only** gate; no special-mode exclusions.

**Requirements clarification: COMPLETE** (user-confirmed). Proceeding to research R1–R4,
then detailed design.

### Open research items (to resolve before/inside design)
- **R1 — Single lane-skin reposition feasibility (Q4):** Can the single `%dp_lane_usr` lane
  skin's X be moved to center independently (AFP layer position via the `lane_%s_%s` bind /
  `FUN_18021c170` path), or is it baked into the template? Determines preferred vs. fallback.
- **R2 — Single-player + active-side detection:** What signal does the builder/game expose for
  "single-player session" and "which side is the active player"? (The builder reads per-side
  state at `param+0x84+side*4`; need the authoritative single-vs-double + active-side source.)
- **R3 — Hook point confirmation:** Confirm `FUN_18006f5d0` signature/AOB on both 64-bit
  builds, the coord payload layout (dword[0]=X, dword[1]=Y, 6 dwords total), and that the
  `name` argument is a readable C-string at the hook for matching the target keys.
- **R4 — Option-texture script:** Identify the exact `scripts/` generator, invocation, and
  output texture name/path for the "CENTER ARROWS (1P ONLY)" row.
