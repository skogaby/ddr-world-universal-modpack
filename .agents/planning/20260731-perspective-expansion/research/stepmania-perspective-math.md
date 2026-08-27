# Reference: StepMania/ITGMania perspective math (Distant, Incoming, Space)

Status: research complete (from the ITGMania and Simply Love SM5 checkouts, 2026-07-31)

Sources are external checkouts, cited as `<repo> src/File.cpp:line`:
- ITGMania: https://github.com/itgmania/itgmania
- Simply Love SM5: https://github.com/Simply-Love/Simply-Love-SM5

## 1. The presets are (tilt, skew) pairs — nothing more

`ITGMania src/PlayerOptions.cpp:1404-1423` (`FromOneModString`), sign convention at
`src/PlayerOptions.h:433-436` (*tilt: −1 = near, 0 = overhead, +1 = space; skew: 0 =
vanish at player center, 1 = vanish at screen center*):

| Perspective | tilt | skew |
|---|---|---|
| Overhead | 0 | 0 |
| Hallway  | −1 | 0 |
| Distant  | +1 | 0 |
| Incoming | −1 | +1 |
| Space    | +1 | +1 |

So the whole family is **two axes**: tilt sign (which end of the lane recedes) and
skew (does the receding end converge on the lane's own center or on screen center).
**Incoming = Hallway with screen-center convergence; Space = Distant with
screen-center convergence.**

Simply Love adds no math of its own — `metrics.ini:533-540` offers exactly these five
stock mod strings at implicit level 1.0 (no fractional/percentage variants exposed).

## 2. How SM turns (tilt, skew) into a camera

Per-player, at draw time (`ITGMania src/Player.cpp:1764-1878`):

1. **Skew → vanish point** (`PushPlayerMatrix`, `Player.cpp:1826-1832`):
   `LoadMenuPerspective(FOV=45°, W, H, Vx, Vy)` with
   `Vx = SCALE(skew, 0.1, 1.0, playerX, SCREEN_CENTER_X)`, `Vy = center_y`
   (the receptor midline — skew never moves the vertical convergence).
   `LoadMenuPerspective` (`RageDisplay.cpp:498-532`) places the camera at
   `(Vx, Vy, d)` with `d = (W/2)/tan(22.5°) ≈ 1.207·W`, looking at `(Vx, Vy, 0)`,
   with an off-axis frustum chosen so **anything at z = 0 renders pixel-identical
   to the ortho pass** — skew is invisible until tilt tips the field out of plane.
2. **Tilt → actor rotation + empirical compensation** (`PlayerNoteFieldPositioner`,
   `Player.cpp:1845-1878`, verbatim decode):
   - `rotX = −30° · tilt · (reverse ? −1 : +1)` about the notefield pivot
     (the receptor midline, receptors ±144 px from it)
   - `zoom *= SCALE(|tilt|, 0, 1, 1, 0.9)` — **0.9× shrink at full tilt, both signs**
   - `y_offset = (tilt > 0 ? −45·tilt : +20·tilt) · reverse_mult` px — asymmetric,
     acknowledged in-source as legacy magic ("simply preserving old behavior")
   - both compensations exist to keep the receptor row near its flat-screen position
3. **Draw distance** (`NoteField.cpp:825-833`): pixel window × `(1 + 0.5·|tilt|)`;
   `ArrowEffects.cpp:145-147` grows the effective field height by 200 px at full tilt.
4. **Reverse scroll**: sign-flips rotation and y_offset (not zoom).

### Measured consequence of the pivot choice (computed from the SM numbers)

Rotating about the mid-field pivot with the camera ~1.2·W away means the **s=1 fixed
point is mid-field**, scale runs ≈0.9–1.2 across the visible lane before the 0.9 zoom,
≈0.82–1.07 after. i.e. under Distant/Space the *near* (entrance) end of the SM field
genuinely renders slightly LARGER than stock and the receptors ~0.82×, displaced ~30 px
toward the horizon. SM tolerates the slight overflow because its lane dressing rotates
with the field; DDR's dressing cannot (affine-only AFP).

## 3. Translation to the DDR screen-space map

The existing hyperbolic map `s = k/(k+d)` with anchor/convergence constants is a
faithful screen-space equivalent of the SM camera for a planar field (both are
projective maps of a plane; the horizon sits at `anchor + k·direction`). The four
presets translate to constant choices:

| Preset | anchor Y (s=1 point) | d grows toward | convergence X | notes |
|---|---|---|---|---|
| Hallway  | receptor row (as today) | entrance edge (approach) | lane center | shipped |
| Incoming | receptor row | entrance edge | **screen center (640)** | = Hallway + cx change |
| Distant  | **mid-field-ish** (design choice) | past the receptors | lane center | needs base-zoom compensation to respect the stock-rectangle constraint |
| Space    | same as Distant | past the receptors | **screen center (640)** | = Distant + cx change |

Everything the DLL already reads per pass (receptor `posY`, lane center, reverse flag)
is sufficient to compute all four constant sets; the effective direction sign folds
into the existing `c48.w` slot, the anchor into `c48.x`, cx into `c48.y`. The only
candidate VS change is a base-zoom multiplier (`s *= c49.y`, defaulting 1.0) mirroring
SM's 0.9 compensation, to keep the positive-tilt field inside the stock lane rectangle.

Doubles note: lane center for doubles = 640 = screen center, so Incoming≡Hallway and
Space≡Distant automatically — same degenerate behavior as SM (skew is a no-op when the
player is centered).

Versus + skew caveat: with cx = 640, the far end of each player's field shifts toward
screen center (~125 px at s≈0.6 for a side lane) — it exits the stock filter band
horizontally (band edge ~±192 about the lane center). SM avoids this only because its
dressing rotates with the field. This is the one genuinely new visual-mismatch surface
the skewed presets introduce.
