# Option icons — A3's in-gameplay option row on the legacy skins (2026-09-25)

Maintainer request after Step 11 (not in the original phases): the icons that
show the chosen options during gameplay. Addresses 20260825 unless stated; A3
= `gamemdx_20240402`.

## 1. A3

- `sequence::dance::OptionIconActor` (vtable `0x18026aa38`, init
  `FUN_180055230`): record `dance_option_icon` (skin at record `+0x28`),
  marker `option` (`{x, y, w}`), then creates a
  `sequence::common::OptionIconActor` (ctor `FUN_18002b490`, vtable
  `0x1802674b8`): `+0x58` side, `+0x60` a copy of the side's
  `ddr::player::Option`, `+0xE0/+0xE4/+0xE8` marker x / y / w, `+0xEC`
  priority **8**, `+0xF0` record skin, `+0xF8` sprite vector.
- Its init `FUN_18002b630`: prefix `daopic%04d_%dp` (record skin, side + 1)
  and eleven `BM2D::CSprite`s from the sprite pool (`0x180386630`, 0x1000 ×
  0x238), each: `afp_sprite_layer_create(texture)` (the pool's
  `CreateSprite()`), priority 8, group side + 2, anchor (3, 3) = centre,
  position `(x + i·(w − 2), y)` — **the slot advances whether the icon is
  shown or not** —, visible per option, scale `w / sprite width`. Order and
  rules (A3 enums):

  | # | kind | values | shown when |
  |---|---|---|---|
  | 0 | speed | `x%03d` = (index + 1)·25 | always |
  | 1 | boost | normal, boost, brake, wave | ≠ 0 |
  | 2 | appear | visible, hidden, sudden, hidden+, sudden+, hidden+_sudden+, stealth | ≠ 0 |
  | 3 | turn | off, mirror, left, right, shuffle | ≠ 0 |
  | 4 | dark | off, on | ≠ 0 |
  | 5 | scroll | normal, reverse | ≠ 0 |
  | 6 | arrow | vivid, note, flat, rainbow | ≠ 3 (rainbow = default) |
  | 7 | cut | off, on1, on2 | ≠ 0 |
  | 8 | freeze | on, off | ≠ 0 |
  | 9 | jump | on1, off | ≠ 0 |
  | 10 | gauge | —, risky, life4, —, —, —, fl_1..fl_9, fl_ex | created only when named |

- msg `0x1045` (speed change) re-textures the speed sprite
  (`afp_layer_change_sprite`, libafp ordinal 36).
- Data: only `dance_option_icon0000_v0` exists (A3 and World, md5-identical):
  a **texture-only** IFS (no afplist), 2 × 78 textures `daopic0000_{1,2}p_*`,
  36 × 28. A3's probe fell back to `0000` on every skin (record skin 0), so
  every skin used this set. Skin 1: no icons (both games skip the actor when
  `GameWork+0xA8 == 1`).
- Markers: `option_icon_%dp[_reverse]_usr` in the legacy roots (skin 2
  (345.5, 655) / 2P (610.5, 678.5); skins 3–5 (17.5, ~604–611) / 2P (943, …),
  reverse at y ≈ 86–88), placeholder 34 × 26 ⇒ scale 34/36, pitch 32 px.

## 2. World

- `OptionIconActor@dance` (vtable 20260825 `0x180362c08`, init
  `FUN_180076c80`, update `FUN_180077370`, msg = the generic no-op): record
  `dance_option`, clip `dance_option_root` (priority 7, group side + 2) at
  marker `option_icon`, eight `icon%02d_usr` slots with World's `daop_icon_*`
  art (gauge, turn, boost, scroll, stealth, cut, freeze, jump — no speed /
  dark / arrow icons). The update re-textures the gauge icon for FLOATING
  FLARE from the flare level. A texture-only package would NULL-deref the
  init (no `dance_option_root`) and the update reads the clip at `+0x60`.
- The side's option: `resolver(table[side], 0)` (`FUN_1801ea0e0`; old builds
  one argument) = `PlayerWork + 0xE0` (a static default for one special
  mcode). `ddr::player::Option` fields (getter stubs `8B 41 off C3`):
  speed type `+0x08`, hispeed `+0x0C`, derived real-speed multiplier ×100
  `+0x10`, gauge `+0x18`, scroll `+0x1C`, visibility `+0x28`, lane cover
  `+0x34`, step zone `+0x40`, boost `+0x54`, turn `+0x58`, colour `+0x5C`,
  cut `+0x64`, freeze `+0x68`, jump `+0x6C`, flare level `+0x7C`. Enums:
  `sounds-options-folder.md` §B.2 + World's own name tables (turn / boost /
  cut / freeze / jump orders equal A3's; gauge {normal, floating, flare
  I..IX, EX, life4, risky}).
- World still has A3's `BM2D::CSprite` (RTTI) and its pool (20260825
  `0x180789b20`, 0x1000 × 0x238), used by `sequence::SpriteLayer`
  (`FUN_1801d3570`); `CSprite::Create(this, texture, priority)` =
  `FUN_18025c260` (`afp_sprite_layer_create`, World still imports ordinal
  34). CSprite vtable slots identical on all five builds: `+0x18` destroy,
  `+0x20` visible, `+0x38` position (int, int), `+0x48` anchor, `+0xC8`
  uniform scale, `+0xE0` priority, `+0xE8` group, `+0x120` size, `+0x138`
  valid.

## 3. World → A3 value mapping

speed = effective ×100 (type 1 hispeed, type 0 the derived multiplier)
rounded to the nearest ×0.25, clamped ×0.25..×8.00; boost / turn / scroll /
cut / freeze / jump by index; dark = step zone index (1 = zone off ⇒
`dark_on`); arrow colour {note, rainbow, vivid, flat} → note / (rainbow,
hidden) / vivid / flat; appearance = stealth (visibility 2) else lane cover
{hidden+, sudden+, hidden+_sudden+}, CONSTANT has no A3 icon (hidden);
gauge flare I..IX / EX / life4 / risky by name, FLOATING FLARE = the current
level with A3's `fl_*` art (World's update rule; A3 had no floating flare),
NORMAL none.

## 4. Mechanism as built

- Policy row `dance_option` (skins 2–5) → `fixed_arc`
  `dance_option_icon0000_v0`, adapter `OptionIcons` (detours ∧ marker
  post-pass). Marker key `option` ← `option_icon_{n}p{r}_usr` (per side,
  gated on the `dance_option` package; World's setter inserts the key).
- `derive_ddr_sel_option_icons` (RTTI ×3 + one AOB for the pool create;
  optional): actor init / update, holder `+0x58`, record / marker getters,
  option resolver + table, Option vtable + the 15 field offsets (stubs),
  CSprite vtable, pool / count / stride / create. All five builds.
- `option_icons.rs`: init detour — record skin 2..=5 ⇒ World's init skipped,
  the A3 row built (pure plan `option_icons_logic`); update detour — ours ⇒
  World's skipped, speed / gauge icons re-created on change. Sprites
  destroyed at GAMEPLAY exit, on a new actor for the side, at disarm /
  disable. Missing marker / option ⇒ no icons (WARN), never World's init.

## 5. In-song speed changes (cabinet 2026-09-25)

World's `ControlSpeedActor` (anytime speed mod) steps its OWN embedded Option
copy (`+0x90`) and broadcasts `0x1042` (= A3's `0x1045`, the message A3's
icon actor re-textured the speed sprite on); only the `GamePlayActor`
consumes it (speed cluster: current / target f32, int ×100 target at
`gameplay_actor_layout().speed_int`). The player's Option never changes, and
World's OptionIconActor `onMessage` is the shared no-op (not hookable). Fix:
the update reads the side's GamePlayActor (`song_reset::gameplay_actors`,
side `+0x84`, cached per icon actor) int target — the lanes' own speed, real
speed included — and falls back to the Option before the actor exists.
