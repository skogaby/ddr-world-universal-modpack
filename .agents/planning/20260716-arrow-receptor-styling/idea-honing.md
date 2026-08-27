# Idea Honing — Arrow / Receptor Styling

Q&A log for requirements clarification.

## Q1: Scale anchor — what point does the playfield shrink toward?

When scale < 100%, the lane must converge toward some anchor point. Candidates:
- **(a) Lane center X + receptor Y** — receptor row shrinks in place, staying
  horizontally centered where the stock lane is; arrows converge toward the
  center column line as they approach. (Matches the reference screenshots.)
- (b) Lane top-left origin — simplest math (pure multiply), but the whole
  playfield drifts left as it shrinks.
- (c) Configurable / follow the judgement-text anchor.

**A1: (a) — scale about lane center X + receptor row Y.** The receptor row
shrinks in place, staying horizontally centered on the stock lane center; the
arrow columns converge toward the center line. No configurability needed.

## Q2: What exactly participates in the scale/opacity pair?

The shared quad fill covers, per side: normal arrows, freeze heads/bodies/tails,
shock arrows + their electric overlay, the receptor row, and the sprite-based
receptor hit flash (`JudgeEffectRenderer`). Also in scope-decision territory:
mines (drawn by our own `mine_render`, must be scaled in our code), and the
measure guideline (does NOT go through the fill — would need extra RE to scale).

**A2: ALL in scope — including the measure guideline.** (1) scrolling notes
(normal/freeze/shock + electric overlay), (2) receptor row, (3) sprite-based
receptor hit flash, (4) mines (via `mine_render` integration), and (5) the
measure guideline — which does NOT flow through the shared quad fill, so it
requires additional RE on `GuidelineRenderer`'s draw path as part of this
feature. One scale value and one opacity value govern all five uniformly.

## Q3: Option ranges, steps, defaults — and naming

Mirror the overlay pair (scale 25–150% step 5/25 default 100; opacity 0–100%
step 5/25 default 100; `PersistMode::Full`)? Ids/labels: `arrow_scale` /
`arrow_opacity` with "ARROW SCALE" / "ARROW OPACITY", vs "PLAYFIELD ...".

**A3: Mirror the overlay pair exactly.** Scale 25–150% (step 5 fine / 25
coarse, default 100), opacity 0–100% (step 5/25, default 100),
`PersistMode::Full`. Option ids `arrow_scale` / `arrow_opacity`, row labels
"ARROW SCALE" / "ARROW OPACITY".

## Q4: When do value changes take effect?

Latch at gameplay entry (overlay-styling precedent, stable cull window per
song) vs live atomic reads. Edits can only happen at song select either way.

**A4: (a) Latch at gameplay entry.** Both sides' scale/opacity are snapshotted
when the song starts; the fill hook and the cull-window extension use the
snapshot for the whole song. Matches the overlay mod's "takes effect at the
next song's start" semantics.

## Q5: Interaction with judge/appearance mechanics at reduced scale

Three confirmations: (1) purely visual — no timing/judge impact; (2)
HIDDEN/SUDDEN fade zones keep stock screen distances from the receptor (the
free behavior) instead of compressing with the lane; (3) reverse scroll works
for free (fill mirrors after our transform, and receptor-Y anchoring commutes
with negation).

**A5: Accepted on all three.** (1) hard requirement, automatic with the
fill-site design; (2) take the free behavior, document it as a known
characteristic; (3) reverse scroll in scope for cabinet validation.

## Q6: Graceful degradation and coexistence gates

Proposed: core (fill detour + cull byte patch) all-or-nothing with
self-disable and no inert option rows; guideline scaling non-fatal;
mine integration same-crate; center-arrows coexistence free via live posX
anchor; per-song latch + shared cull float avoids the taken `render_notes`
detour entirely.

**A6: Accepted, with one change — guideline scaling is ALSO load-bearing.**
The full gate set is all-or-nothing: fill detour + cull-window byte patch
(with instruction-bytes verification) + the GuidelineRenderer scaling path
must ALL install, or the whole mod self-disables and registers no option
rows. Mine integration is same-crate (active when both mods enabled).
Coexistence with center_arrows_single is automatic (anchor from live posX);
no render_notes dispatcher needed (per-song latch + one shared cull float =
720 / min(s_P1, s_P2)).

## Q7: Doubles play + mod identity

Doubles: one renderer set spans 8 panels as side 0 → P1's option pair
applies, anchor = the 8-panel lane center. Mod id/name proposal.

**A7: Doubles behavior confirmed** (P1's values, 8-panel-lane center anchor).
**Mod identity: id `playfield-styling`, display name "Playfield Styling"**
(option ids stay `arrow_scale` / `arrow_opacity` with labels "ARROW SCALE" /
"ARROW OPACITY" per A3).

## Q8: Success criteria for cabinet validation

**A8: Accepted as proposed:**
1. Scale 25/50/150% and opacity 50/0% render correctly for: normal arrows,
   freeze (head/body/tail), shock + electric overlay, receptors + press
   animation, hit flash, guideline, mines (with note_types_expansion active).
2. No pop-in at screen bottom at 25% scale (cull extension works); no
   regression at 100%.
3. Versus: independent per-side values simultaneously. Doubles: P1 values
   across the 8-panel lane. Reverse scroll correct.
4. Judging/scoring unaffected (spot-check a known song's score at 50% scale).
5. Stress: lowest speed mod + 25% scale on a dense chart — no stutter or
   visual corruption (CommandList arena headroom).
6. Values persist through card-out/card-in; two rows on the Mods tab;
   toggling the mod off reverts everything to stock next song.

**Requirements clarification declared COMPLETE by the maintainer.**
