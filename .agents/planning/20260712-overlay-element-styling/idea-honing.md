# Idea Honing — Overlay Element Styling

Q&A record for requirements clarification. Each question was asked and answered
in conversation; the final decision is recorded here.

---

## Q1: Per-player options or cabinet-wide?

Should scale/opacity be **per-player settings** (each carded-in player adjusts
their own side's elements via rows on the game's native Options screen → Mods
tab, persisted with their profile like Premium Free / PUS), or **cabinet-wide
settings** (one set of values for both sides, adjusted in the DLL's overlay mod
menu and stored in `mod-config.json`, like Timing Offsets / FPS Unlock)?

Implications:
- **Per-player**: needs side attribution of captured clips (single/double =
  trivial via the one active side; versus = bind on first SetPosition x, per RE
  doc §5.3). Values ride the custom_options persistence (network + JSON).
  Feels like a player cosmetic preference — matches WebUI Options / PUS
  precedent.
- **Cabinet-wide**: no side attribution needed at all (apply to every captured
  clip), simpler capture path, but players share one setting.

**Answer:** **Per-player.** Rows on the game's native Options screen (Mods
tab) via the `custom_options` framework, persisted with the player profile like
Premium Free / PUS. Requires side attribution of captured clips (single/double
via the active side; versus via first-SetPosition x-binding, RE doc §5.3).

---

## Q2: Option granularity — how many knobs per player?

The scoped elements fall into three natural groups (matching ownership and how
players think about them):

1. **Combo counter** (the 3 `dance_combo_root` clips)
2. **Judgement** (`dance_judge` + freeze O.K./N.G. + FAST/SLOW)
3. **Pacemaker** (`dance_score_compare`)

With scale + opacity each, the options-per-player count is:

- **(a) Per-group knobs**: 3 groups × 2 = **6 rows** — full control, but the
  Mods tab is shared with Premium Free / Autoplay / PUS rows (scroll driver
  exists, so row count is workable).
- **(b) One shared pair**: 1 × 2 = **2 rows** ("overlay scale" / "overlay
  opacity" applied to all three groups) — minimal UI, no independent tuning.
- **(c) Split judgement further** (judgement text vs FAST/SLOW vs freeze):
  up to 10 rows — probably over-fitted.

**Answer:** **(b) One shared pair** — 2 rows per player: one **scale** row and
one **opacity** row, applied uniformly to all scoped elements (combo counter,
judgement text incl. freeze O.K./N.G. and FAST/SLOW, pacemaker). Minimal UI
footprint on the Mods tab.

---

## Q3: Value ranges, steps, and defaults?

The custom_options UI kinds are `Enum` (fixed labelled choices, Left/Right
cycles) or `Scalar` (numeric min/max with fine/coarse steps). For two rows that
players adjust on a timer-pressured options screen, sensible candidates:

- **(a) Enum presets (recommended)**:
  - Scale: `50% / 75% / 100% / 125% / 150%` (default 100%)
  - Opacity: `0% / 25% / 50% / 75% / 100%` (default 100%)
  - Predictable wire values for persistence, quick to set, no timer pressure.
  - Note 0% opacity = elements fully hidden (a "hide combo/judgement" feature
    for free).
- **(b) Scalars**: e.g. scale 25–200 step 5, opacity 0–100 step 5 — finer
  control but slower to adjust and more wire-value surface.

**Answer:** **(b) Scalars**, with scale allowed up to **150%**:
- **Scale**: 25–150 (%), default 100. Steps: fine 5, coarse 25.
- **Opacity**: 0–100 (%), default 100. Steps: fine 5, coarse 25.
(0% opacity = fully hidden is intentional and allowed. Step values to be
sanity-checked during design against the options screen's hold-to-repeat
behavior.)

---

## Q4: When do changes take effect?

The elements are created at gameplay-scene build (per song). The options
screen is only reachable *before* a song (song select / options modal), so the
natural flow is: player sets values → next song's creation captures the clips
→ one-shots apply at capture. Two sub-questions:

1. Is **apply-at-song-start** sufficient (no live mid-song changes needed)?
   The options screen isn't reachable mid-song anyway, so the only "live"
   concern would be a future overlay-menu integration — out of scope?
2. The 0%-opacity case for combo/pacemaker relies on the SetColor compose
   detour multiplying game writes; judge/freeze/fast-slow rely on the one-shot.
   Any change made at the options screen is therefore fully in effect from the
   first note of the next song. Confirm this matches expectations.

**Answer:** Confirmed — **apply-at-song-start** is sufficient. Values set on
the options screen are fully in effect from the first note of the next song.
Live mid-song adjustment and DLL-overlay-menu integration are **out of scope**.

---

## Q5: Persistence mode for the two options?

The custom_options framework offers `PersistMode`:
- **`Full`** (default; what Premium Free / PUS use): value is saved to the
  server on card-out (`mod_<id>` kbin fields), loaded on card-in, AND cached
  offline in `mod-config.json` under `custom_options.{p1,p2}` — so settings
  survive with or without a mod-aware server.
- `SaveOnly` / `None` — for special cases (WebUI cosmetics use SaveOnly
  because the game natively loads them; not applicable here).

**Answer:** Confirmed — **`PersistMode::Full`** for both options (network
save/load on card-out/card-in + offline `mod-config.json` cache).

---

## Q6: Row visibility — always show the two rows, or gate behind a toggle?

Existing Mods-tab patterns:
- **PUS style**: a parent toggle row ("POWER USER STATS") with child rows that
  appear only while the parent is ON (`ShowWhen` predicate).
- **Premium Free style**: a single always-visible row.

For this mod:
- **(a) Two always-visible rows** — "OVERLAY SCALE" + "OVERLAY OPACITY"
  directly on the Mods tab. Simplest; default 100/100 means inert until
  touched. The mod's enable/disable master switch remains the DLL mod registry
  (mod-config `mods` map + overlay menu), like every other mod.
- **(b) Parent toggle + two child rows** — an explicit ON/OFF row; children
  hidden when OFF. Adds a third option row and a redundant on/off state (the
  defaults already mean "off").

**Answer:** **(a) Two always-visible rows** ("OVERLAY SCALE", "OVERLAY
OPACITY") directly on the Mods tab. Defaults 100/100 are identity; the mod's
master enable/disable remains the DLL mod registry like every other mod.

---

## Q7: Versus side-attribution — required for v1, or acceptable fallback?

Per-player application needs to know which captured clip belongs to which side.
From the RE doc (§5.3):

- **Single / double** (one active player): trivial — all captures belong to the
  active side. Zero risk.
- **Versus** (two players): each side gets its own judge/combo/pacemaker
  clips; the reliable discriminator is the first wrapper `SetPosition` after
  capture (x < / > screen middle — exact threshold to validate on cabinet).
  This needs one extra small detour (wrapper SetPosition vfunc) or an
  equivalent.

Options for v1 scope:
**Answer:** **(a) Full versus support in v1** — implement the SetPosition
side-binding (small cold-path detour, x-threshold validated on cabinet).

---

## Q8: Should the int-percent SetColor variant (+0xB0) be hooked in v1?

The RE doc identifies four wrapper color methods; the compose detour on the
float form (+0x90) covers every color write we *observed* on the scoped
elements (combo handler, digit tint, pacemaker dim — the array form +0x98
dispatches into +0x90 virtually). The int-percent variant (+0xB0,
`FUN_180259180`/`0x18021D140`) also writes mult color, but its callers are
unknown (virtual dispatch) — we found no evidence it fires on our elements.

**Answer:** **(a) Hook both in v1** — the float form (+0x90) and the
int-percent form (+0xB0), same filter/compose logic.

---

## Q9: Graceful-degradation policy when signatures/hooks fail?

Codebase convention is two-tier degradation. Proposed policy for this mod:

- **Load-bearing (mod self-disables if missing)**: the `cmovieclip_create`
  AOB + detour (capture is the foundation), the libafp named exports
  (`afp_layer_set_matrix`, `afp_layer_set_color`), and the color-twin AOB +
  IAT disambiguation for +0x90 (opacity compose). Without any of these, the
  feature can't deliver its contract — don't register the option rows at all
  (rows that do nothing are worse than no rows).
- **Non-fatal (log + degrade)**:
  - +0xB0 int-variant hook fails → continue (observed coverage is +0x90).
  - SetPosition side-binding detour fails → versus falls back to stock
    rendering (single/double still works via active-side attribution); log a
    warning.
**Answer:** Confirmed as proposed — load-bearing set (create AOB/detour,
libafp exports, +0x90 color detour w/ IAT disambiguation) self-disables the
mod with no rows registered; +0xB0 hook and versus side-binding are non-fatal
(log + degrade).

---

## Q10: Mod identity — id, display name, row labels?

Registry conventions: kebab-case id, human display name, SCREAMING row labels
(rendered via the custom_options label atlas). Proposal:

- Mod id: `overlay-element-styling`
- Mod display name: `Overlay Element Styling`
- Description: "Per-player scale and opacity for combo, judgement, and
  pacemaker displays during gameplay"
- Option ids / row labels:
  - `overlay_scale` → label `OVERLAY SCALE` (hint: "COMBO/JUDGE/PACEMAKER SIZE")
  - `overlay_opacity` → label `OVERLAY OPACITY` (hint: "COMBO/JUDGE/PACEMAKER FADE")
**Answer:** Approved as proposed — id `overlay-element-styling`, name
"Overlay Element Styling", rows `overlay_scale`/`OVERLAY SCALE` and
`overlay_opacity`/`OVERLAY OPACITY`, source at
`src/mods/overlay_element_styling/`.

---

## Requirements clarification status

Ten questions answered (Q1–Q10). Remaining known-open items are
*implementation-detail* level and deliberately deferred to design/cabinet
validation rather than requirements:

- Exact versus x-threshold for side binding (cabinet-validate; likely 640 on
  the 1280-wide playfield space — confirm units at runtime).
- Scalar step ergonomics on the options screen (fine 5 / coarse 25 initial
  values, tune during cabinet testing).
- Whether scale visually composes acceptably at 150% over the lane (pure
  aesthetics; the cap is a requirements decision already made).

