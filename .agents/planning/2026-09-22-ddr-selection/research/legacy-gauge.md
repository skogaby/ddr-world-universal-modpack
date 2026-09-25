# Step 8 RE — legacy life gauge (2026-09-24)

Addresses are 20260825 (`0x180000000` base) unless stated; A3 =
`gamemdx_20240402`. Starting point: `hud-actors.md` §1 (this file corrects and
completes it).

## 1. Actors and shared functions

| | Percent family (Normal / Grade / Flare / Immortal, + base GaugeActor) | LifeGaugeActor (LIFE4 / RISKY) |
|---|---|---|
| init (slot 4) | `FUN_180073cf0` — shared by all five vtables | `FUN_1800706e0` |
| update (slot 6) | `FUN_1800743d0` — shared | `FUN_180070de0` |
| fill | `FUN_180074e10`, called once at the end of every update | (none — lives via `damage_%d_usr`) |
| state → label (slot 11, vt+0x58) | Normal `mov eax,edx`; Flare `FUN_180075b10`; Immortal 3 → 3 else 5 | — |
| fields | side parent `+0x88` (`**` = side), value (displayed, f32) `+0x94`, state `+0x9c`, clip `CMovieClip*` `+0xb0`, record skin `+0xd4` | parent `+0x88`, clip `+0xa8`, skin `+0xb4` |

Identical offsets on all five builds (20250805 init `0x180070550` / update
`0x180070c30` / fill `0x180071650` / Life init `0x18006cf20`; 20260224
`0x18006f6c0` / `0x18006fda0` / `0x1800707c0` / `0x18006c060`; 20260721
`0x1800738b0` / `0x180073f90` / `0x1800749d0` / `0x1800702a0`; 20260915
`0x180073cb0` / `0x180074390` / `0x180074dd0` / `0x1800706a0`). The label
getters and every other overridden slot (9/10) are pure / name-free.

## 2. World vs A3 (percent family)

- **init** (`FUN_180073cf0` vs A3 `FUN_180052ec0`): identical except
  - export `dance_gauge` (A3 `00_dance_gauge`, `00_sd_dance_gauge` on machine
    types 0/1) — `LEA R8,["dance_gauge"]; MOV R9D,4; … CALL
    CMovieClip::Create` at init+0xA8 (the init's other two `"dance_gauge"`
    LEAs are the record key and the error log);
  - scale: World `MOVSS XMM2,[1.0]; MOVSS XMM1,XMM2; CALL [RAX+0xC0]`
    (`SetScale(1,1)` for both sides); A3 `SetScale(side ? -1 : 1, 1)`
    (`DAT_1802626d4` = −1.0). **World dropped the 2P mirror.**
  - both keep the skin-3 root goto `0xF03` `1p_in` / `2p_in` on
    `*(clip+0x110)` (the root MovieClip id).
- **update** (`FUN_1800743d0` vs `FUN_180053600`): same smoothing (no easing on
  skin 1: `if skin != 1`), same label table, danger messages renumbered;
  World splits the state into vt+0x50 (value → state) / vt+0x58 (state →
  label). World always ends with its continuous fill; A3 ends with
  `skin − 2 < 3 || 6 ≤ label(state) < 17 ? continuous : segmented`.
- **World fill** `FUN_180074e10`: `g = clamp(+0x94, 0, 1)`; `g ≥ 1` ⇒ hide
  `fill _usr` and `fill _2_usr`; else show `fill _usr`, read its 0x1008 pos
  (+0.5), 0x1015 / 0x1016 size (truncated), write MovieClip param **0x1023 =
  `{i32 x, y, w, h}`** (the scissor) with `x = (cx − w/2) + w·g`,
  `y = cy − h/2`, hide `fill _2_usr`. No 2P mirror.
- **A3 continuous** `FUN_1800544b0`: the same, plus the 2P scissor
  `x = (w/2 + cx) − w·g − w` (the gauge is drawn mirrored).
- **A3 segmented** `FUN_180054050`: `N = skin 1 ? 63 : 26` cells (SD 60 / 26),
  cell width `skin 1 ? 6.984127 (0x40DF7DF8) : 17.0` (SD 5.0 / 11.5);
  `t = N·g`, `cells = trunc(t)`, `frac = t − cells`; scissor x from
  `cells·cell_w` (mirrored like the continuous one); when `frac > 0` and skin
  ≠ 1: `fill _2_usr` visible and cropped by 0x1023 to
  `{x (2P: x + w − cw), y, cw = (int)(cell_w + 0.5), (int)((1 − frac)·h)}`,
  and the main scissor moves by `cw` (2P: −cw). SetVisible = World's
  `FUN_180258670` (`0x1007 = v`, `0x101e = 1` over the `traversal(6)` chain).

## 3. LifeGaugeActor

World `FUN_1800706e0` = A3 `FUN_18004f4a0` minus the SD export and **minus
the skin-3 root goto** (`1p_in` / `2p_in`, A3 did it on every legacy skin-3
LIFE gauge). Frames `loop_%dlife`, full-lives `skin != 2` rule, `fill _2_usr`
and `damage_1..8_usr` hidden — kept. No mirror in A3 either. The update reads
`damage_%d_usr`, `fill _usr`, `gauge_usr` + `loop_{normal,rainbow,danger}` —
all present in the legacy exports.

## 4. Legacy data

`dance_gauge0001..5_v0` export `00_dance_gauge` (and SD, `gauge_frame`,
`gauge_damage8`, …) with `gauge_usr`, `fill _usr`, `fill _2_usr`,
`damage_1..8_usr`, `gauge_frame_usr` — every child World's actors name.
`gauge_usr` has only `loop_{danger,normal,rainbow}` (FLARE / GRADE / check
labels miss, as in A3 — design D19; FLOATING FLARE is World's FlareGaugeActor
and follows the FLARE rule automatically).

## 5. Mechanism as built (Step 8)

- **Derivation** `SignatureStore::derive_ddr_sel_gauge` — no AOB, all from
  the RTTI vtables (`find_gauge_vtables`): percent vtables must share slots 4
  / 6; the export LEA = the one `4C 8D 05 … 41 B9 ?? 00 00 00` → `"dance_gauge"`
  in each init; the fill = the one CALL target of the update that loads
  `"fill _usr"`; offsets from the instructions using them (skin store after
  `MOV r,[RAX+0x28]`, clip store after the create, `LEA RAX,[RCX+0x94]` in the
  fill, `CALL [RAX+0x50]; MOV ESI,EAX; CMP [RDI+0x9C],EAX` + `CALL
  [R8+0x58]`, `MOVSS XMM2,[1.0]; …; CALL [RAX+0xC0]`, the skin-3 block's
  `MOV EBX,[R8+0x110]`). Resolves identically on all five builds.
- **`gauge.rs`**: export-LEA patches (both inits → near `"00_dance_gauge"`)
  applied by the package helper before it registers `dance_gauge000N`
  (failure ⇒ stock), restored on a stock `dance_gauge` request, disarm,
  disable; post-original percent init detour → `SetScale(-1, 1)` on 2P;
  post-original LifeGauge init detour → skin-3 root goto; full-replacement
  fill detour for legacy actors (`gauge_math.rs`, pure, host-tested), World's
  fill otherwise. A legacy actor = armed ∧ record skin 1..=5 (only the
  helper's legacy registration sets it).
- HD constants only (World always creates the HD export).
- The `gauge` marker moves with the package (Step 7 gate).
