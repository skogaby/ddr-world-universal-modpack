# Rough Idea — Arrow / Receptor Styling (Scale + Opacity)

Replicate the recently added **Overlay Element Styling** feature (per-player scale
and opacity for combo counter, judgement text, pacemaker), but this time targeting
the **actual gameplay arrows and the arrow receptacles (receptor row)**.

- Driven by a **new pair of options** (scale + opacity) that apply specifically to
  the arrows/receptors — separate from the existing overlay-element pair.
- Per-player, like the existing feature (versus support).
- Reference screenshots: a "small" playfield (arrows + receptors scaled down) vs a
  "large" (stock-ish) playfield.

## Known consideration (from the maintainer)

If we shrink the playfield, more arrows become visible on the Y axis at once. The
game likely **culls off-screen arrows** — the culling window may need to be
extended so that arrows spawn/draw earlier when the playfield is scaled down,
otherwise arrows would pop into existence mid-screen.
