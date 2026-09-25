# Step 9 RE — legacy combo (2026-09-24)

Addresses are 20260825 (`0x180000000` base) unless stated; A3 =
`gamemdx_20240402`. Starting point: `hud-actors.md` §2 (this file corrects and
completes it).

## 1. Actors and functions

| | World `ComboActor` | A3 `ComboActor` |
|---|---|---|
| vtable (RTTI `.?AVComboActor@dance@sequence@@`) | `0x180361448` | `0x180269488` |
| init (slot 4) | `FUN_180066250` | `FUN_180046a60` |
| finalize (slot 5) | `FUN_1800666b0` | `FUN_180046f00` |
| update (slot 6) | `FUN_180066630` | `FUN_180046e40` |
| msg (slot 8) | `FUN_180066770` | `FUN_180046f70` |
| digit / texture writes | `FUN_180066930` (`combo_digit_refresh`, callers: init + msg only) | `FUN_1800470e0` |
| layout | — | `FUN_180047460` (position + centre message), `FUN_180047570` (number scale) |
| growth | — | `FUN_180046970` (+ digit count `FUN_180046a20`) |

Created by `GamePlayActor::onInitialize` (`FUN_18005be20`) — a child of the
side's GamePlayActor, like the NoteResultActor.

World fields (identical offsets on 20250805 / 20260224 / 20260721 / 20260825 /
20260915 — 20250805 init `0x180062b40`, refresh `0x180063220`; 20260224
`0x180061b60` / `0x180062240`; 20260721 `0x180066270` / `0x180066950`; 20260915
`0x180066920` / `0x180067000`): side holder `+0x58` (`**` = side; the
LayoutActor per-side struct the record / marker lookups take), ctor byte `+0x60`,
combo `+0x68`, worst grade `+0x6C` (ctor `0xFF`), roots 1..3 `+0x70/+0x78/+0x80`,
package `+0x88`, `+0x90` f32 1.0 (ctor, unread — A3's `+0x88` growth), game-over
byte `+0x94`.

A3 fields: combo `+0x64`, worst `+0x68`, clip `+0x70`, marker x/y
`+0x78/+0x7C`, cell w/h `+0x80/+0x84`, growth f32 `+0x88`, game-over `+0x8C`,
skin `+0x90`, per-grade flag `+0x94` (0 for skins `{1,2,3}` = `DAT_180265038`).

## 2. World vs A3

- **init.** World: option `vt+0x288 == 1` ⇒ layer side+2 else 0; loop
  `r15 = 2 .. 0` / `r13 = 3 .. 1` / `rbp = +0x80 .. +0x70` over
  `dance_combo_root%d` (format assigned with fixed length 0x12, then
  `sprintf_s`) — record lookup `FUN_18006ece0(holder, "dance_combo")` →
  package → pool-slot create `FUN_180257af0(slot, pkg, name, 0, 1)` →
  `vt+0xE8` layer, `vt+0xE0` priority `r13` (layer 0) / `r13 + 9` (side layer),
  `vt+0x90 SetColor(a = 0, 1, 1, 1)`, play 1.0, attribute 1 = 1, marker
  `FUN_18006f100(holder, "combo")`, `vt+0x48(3, 3)`, `vt+0x38 SetPosition`; if
  combo > 0: `SetFrameLabel(root, "loop")` + refresh. A3: the same create for
  ONE clip (export `dance_combo`), option `vt+0x100 == 0` ⇒ side layer
  priority 10 else layer 0 priority 1 — i.e. World's root1 priorities; play
  0 + attribute 1 = 0 (hidden), SetPosition(marker), marker kept, cell
  `number_usr/0001_usr` `0x1015` (skin 1: `/ 2`) × `0x1016`, layout for the
  current count; combo > 0: play 1, show, `SetFrameLabel(root, "loop")`,
  texture writes. (The combo-priority option: World's own mapping is kept —
  World's init runs.)
- **msg.** Both: combo message World `0x1033` / A3 `0x1036`
  `{side, combo, max, grade, flag}` — side check against `**(+0x58)`; combo
  stored; `combo < 1` ⇒ worst = `0xFF`, else `g = grade == 6 ? 0 : grade`,
  worst = `max(worst, g)` (`0xFF` = none). World then: `combo > 3` ⇒ every root
  `SetColor(1,1,1,1)` + `combo_usr` goto-play label + refresh, else
  `SetColor(0,…)` on all three roots (unconditional — a NULL root2 crashes).
  A3: `combo < 4` ⇒ layout for `combo`, play 0, hidden; else play 1, show,
  `GotoAndPlay(frame 0)` on the root (op `0xF08`), texture writes. Game over
  World `0x103C` / A3 `0x103F` ⇒ byte. A3 also re-laid out on `0x1046` = World
  `0x1043` (DPS pre-start broadcast). World's message ids are A3's − 3
  throughout.
- **update.** World: game over ⇒ play 0 on the three roots. A3: every frame
  `number_usr` scale (`0x1003`, sibling chain) = growth × `combo_usr` scale
  (get `0x100D`), only while the layer is visible (`vt+0x140` = info bit 0);
  game over ∧ playing (info rate ≠ 0) ∧ frame("loop") ≤ current ⇒ play 0 +
  hidden.
- **finalize.** Both release the clip(s) (`vt+0x18`) and write the worst grade
  into PlayerWork (World `+0x6C` → `PW + stage·0x2B8 + 0x5AC` and `PW+0x23C`)
  — World's finalize is kept, so the worst grade must stay in `+0x6C`.
- **A3 layout** `FUN_180047460(digits, growth)`:
  `x = (int)(mx − (4·cw − digits·cw·growth)·0.5)` (f32, `0.5` =
  `DAT_180265198`); SD cabinets (machine type 0/1), 2P, ≥ 3 digits, `+0x60 == 0`:
  `x −= 30` (not ported — World always draws the HD clip); `SetPosition(x, my)`;
  centre = `layer_w / 2 + x` (`vt+0x120` = `afp_layer_get_info` size) sent as
  `0x1038 {centre}` to the parent's subtree — World's NoteResultActor still
  handles it as `0x1035` (FAST/SLOW x = centre, all five builds), World's combo
  never sends it.
- **A3 growth** `FUN_180046970(c, skin)` (constant bits): skin 1: `c ≤ 9` →
  1.0; `c < 100` → `(c / 10.0f) · 0.1f + 1.0f` (`0x41200000`, `0x3DCCCCCD`);
  `c < 10000` → 2.1 (`0x40066666`); else 1.0. Other skins: `c < 100` →
  `(float)((c / 10) · 160) · (1/4096) + 1` (`0x39800000`); `c < 1000` → 1.5
  (`0x3FC00000`); `c < 10000` → 1.25 (`0x3FA00000`); else 1.0.
- **A3 texture writes** `FUN_1800470e0`: prefix `dance_combo%04d` (skins 1–3)
  or `dance_combo%04d_<marvelous|perfect|great|good>` by worst grade (table
  indexed unchecked); `combo_usr` siblings: bitmap `<prefix>_combo`, `0x1007 =
  1`, `0x101E = 1`; `n = min(combo, 9999)`; growth → `FUN_180047570(growth)`
  (store + scale) → layout; places `number_usr/{0001,0010,0100,1000}_usr`:
  bitmap `<prefix>_<digit>`, visible = ones always, higher places only when the
  count reaches them.

## 3. Legacy data

`dance_combo0001..4_v0` (World) and A3's `dance_combo0005_v0`: exports
`dance_combo` (root labels `in` 0 / `loop` 36, children `combo_usr`,
`number_usr`, `number_usr/0001..1000_usr`), `number`, (1–3, 5)
`dance_combo_old`. Textures: skins 1–3 `dance_combo000N_{0..9,combo}`; 4
`dance_combo0004_{marvelous,perfect,great,good}_{0..9,combo}`; 5 the same +
`gray` (unused by A3's code). Digits 74 × 77, word 149 × 47.

**World's `dance_combo0005_v0.arc` is blanked**: a valid arc header whose one
member (518016 bytes) decompresses to zeros (the other 54 World copies of
A3-era arcs are intact; the file's mtime differs). A3's copy has the IFS magic
`6C AD 8F 89`. Detection: read the arc the game's probe would open (LayeredFS
mod file first, then `data/`; `_v3`, `_v0`, bare), decompress the member, test
the magic — once per skin per boot.

## 4. Interactions

- **S-Marvelous** post-original detour on the refresh — promoted to
  `services/combo_hooks` (refresh POST subscriber, behaviour identical). The
  refresh is only called by the init and the msg case, both overridden for
  legacy actors, so the repaint never touches a legacy clip (roots 2/3 are
  null there).
- **overlay_element_styling** classifies combo clips at `CMovieClip::Create`
  by the `dance_combo_root` prefix — the legacy clip is created under the
  export name `dance_combo` (exact match added). Its opacity composes through
  the SetColor detour (World's init sets alpha 0; the port sets alpha 1 once,
  A3 hides by attribute), its scale / position through the SetPosition
  detour (the A3 layout repositions every combo step).
- **song_reset** in-place restart broadcasts `0x1033 {side, 0, 0, 0}` and
  `0x1043` — the port's msg case hides the clip / resets the worst grade, and
  re-lays it out, exactly like a fresh song.
- **markers** (Step 7): the `combo` key moves with `dance_combo`
  (`Gate::Package`); A3's nested `combo_set_usr/combo_usr`.

## 5. Mechanism as built (Step 9)

- **Derivations** (no AOB beyond the existing `combo_digit_refresh`):
  `derive_combo_actor` — RTTI vtable slots 4/5/6/8 + fields from the msg / update
  instructions (`SUB EDX,0x1033`, `MOV EAX,[R8+4]; MOV [RDI+combo],EAX`,
  `MOV ECX,[RDI+worst]; CMP ECX,0xFF`, `MOV byte [RCX+go],1`, update `CMP byte
  [RCX+go],0`); `derive_ddr_sel_combo` — inside the init: the loop head
  `MOV R15D,2; LEA R13D,[R15+1]; LEA RBP,[R14+root3]`, the `MOV R8D,0x12; LEA
  RDX,["dance_combo_root%d"]` format LEA, the record / marker CALLs after
  `LEA RDX,["dance_combo"]` / `["combo"]`, the side holder `MOV RCX,[R14+d8]`,
  the SetPosition vslot (`MOV R8D,[RBX+4]; MOV EDX,[RBX]; CALL [R9+d8]`), the
  SetColor vslot (`MOVAPS XMM1,XMM7; CALL [RAX+d32]`), the root MovieClip id
  load, and the loop tail `MOV RCX,R14; CALL refresh; DEC R13D; SUB RBP,8; DEC
  R15; JNS` (its call must be `combo_digit_refresh`). All five builds
  identical; sweep ALL GREEN; `shape_diff`: old builds diverge only outside
  the read bytes.
- **`services/combo_hooks.rs`**: the one owner of the refresh + init /
  finalize / update / msg detours (refresh POST, refresh / update / msg
  OVERRIDE, init PRE (may skip) / POST, finalize POST).
- **`combo.rs`**: init PRE reads the actor's `dance_combo` record skin; legacy ⇒
  three checked patches for this one call (count `02 → 00`, first root
  `+0x80 → +0x70`, format LEA → near `"dance_combo"` + NUL padding to 0x12), so
  World's own init creates A3's one clip in root1 with A3's priorities; POST
  restores them and runs the rest of A3's init. Msg `0x1033` / update / refresh
  are A3's for legacy actors (World's counters `+0x68` / `+0x6C` kept for the
  finalize); `0x1043` re-lays out then falls through to World; the centre goes
  to the side's NoteResultActor (`update_broadcast(nra, 0x1035, &x, 0)`).
  A patch failure skips World's init (no clip, no combo this song) — World's
  init would NULL-deref on a legacy package. HD only.
- **`combo_math.rs`** (pure, host-tested): growth, digits, cell, layout,
  centre, worst grade, sheet / texture names, place visibility, format bytes,
  IFS magic, arc candidates — A3's constant bits checked.
- **Skin 5**: `combo::package_usable` refuses a damaged `dance_combo000N`
  (package stays stock, one WARN naming the A3 import).
