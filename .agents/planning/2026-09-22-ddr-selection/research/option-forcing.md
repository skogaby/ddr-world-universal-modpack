# 1st-5th option forcing — A3's classic options on skin 1 (Step 12, 2026-09-25)

Re-verification of `sounds-options-folder.md` §B against A3 and all five
World builds, plus the interaction audit. Addresses 20260825 unless stated;
A3 = `gamemdx_20240402`.

## 1. A3: getter-level forcing, live while the skin is 1

A3 has two Option classes: `ddr::player::Option` (vtable `0x180280538`,
plain `MOV EAX,[RCX+off]` getters) and `ddr::player::CourseOption` (vtable
`0x1802806e8`, the PlayerWork one). Nine `CourseOption` getters return a
constant while `*(GameWork + 0xB0) == 1` (A3's skin field), the stored
value otherwise (course-fixed block `+0x90` aside):

| vslot | field | forced | meaning |
|---|---|---|---|
| `+0x20` | speed `+0x0C` | 3 | ×1.00 (`(v+1)·0.25`) |
| `+0x30` | boost `+0x10` | 0 | normal |
| `+0x40` | appearance `+0x14` | 0 | visible |
| `+0x60` | dark `+0x1C` | 0 | step zone shown |
| `+0x70` | scroll `+0x20` | 0 | normal |
| `+0x80` | colour `+0x24` | 2 | FLAT (A3 `{vivid, note, flat, rainbow}`) |
| `+0xC0` | arrow `+0x34` | 2 | classic (`2d_arrow02`) |
| `+0xD0` | filter `+0x38` | 0 | off |
| `+0xE0` | guideline `+0x3C` | 0 | off |

So A3 never wrote the profile's options: every read through the getters
between the skin write (song-select commit) and the skin reset (end of the
stage results) saw the forced value, the stored value never changed.

**In-song speed (A3 answers the open question).** A3's
`sequence::dance::ControlSpeedActor` (vtable `0x180268aa8`, ctor
`FUN_1800379b0`) is created by the GamePlayActor init (`FUN_18003b490`)
whenever the option allows it — no skin gate — and embeds a *plain*
`ddr::player::Option` at `+0x90`, filled by `FUN_180125dc0`, which copies
**through the source's virtual getters**. The copy therefore starts at the
forced ×1.00; the actor's update then steps its own copy (`FUN_180126380`)
and broadcasts `0x1045` with the new speed. On A3 skin 1 the player could
change speed in-song, starting from ×1.00. Parity = allow it (World's
`ControlSpeedActor` copies the player's Option the same way — see §3 — so
forcing the fields gives exactly this).

**Versus / bot.** The getters test the cabinet-global skin: every player of
the song was forced. The multiplayer bot's side is a second player ⇒ forced
too.

**Profile.** A3's skin stayed 1 through the stage results, so A3's
per-stage save marshal (which reads the getters) would have written the
forced values into the profile block — an A3 quirk we deliberately do not
port (the profile must come back unchanged; see §4).

## 2. World: fields and enums (all five builds identical)

Getter stubs `8B 41 off C3` at the RTTI `ddr::player::Option` vtable, byte-
identical on 20250805 / 20260224 / 20260721 / 20260825 / 20260915:

| A3 option | World field | getter | World enum (name table `0x1804b4288`) | forced |
|---|---|---|---|---|
| speed | `+0x08` speed type | `0x208` | {real_speed, speed_rate} | 1 |
| | `+0x0C` hispeed ×100 | `0x220` | 25..800 (setter snaps 5) | 100 |
| boost | `+0x54` scroll moving | `0x2A8` | {normal, boost, brake, wave} | 0 |
| appearance | `+0x28` visibility | `0x250` | {normal, constant, stealth} | 0 |
| | `+0x34` lane cover | `0x268` | {off, hidden, sudden, hidden_sudden} | 0 |
| dark | `+0x40` step zone | `0x280` | {on, off} | 0 |
| scroll | `+0x1C` direction | `0x238` | {normal, reverse} | 0 |
| colour | `+0x5C` arrow colour | `0x2B8` | {note, rainbow, vivid, flat} | 3 |
| arrow | `+0x60` arrow design | `0x2C0` | {normal, x, classic, cyber, medium, small, dot} | 2 |
| filter | `+0x30` lane transparency | `0x260` | 0..100 | 100 |
| guideline | `+0x3C` guideline | `0x278` | {center, border, off} | 2 |

- The effective-speed getter (`0x218`, `FUN_1801e27d0`; `FUN_1801ca440` on
  20250805) is `type == 1 ? +0x0C : type == 0 ? +0x10 : …` on every build —
  type 1 + 100 = ×1.00, and `song_rate::real_speed` (type-0 sides only)
  leaves it alone.
- Lane transparency: the options menu writes `100 − darkness`
  (`0x180186bed`: `MOV EDX,0x64; SUB EDX,[rax+4]; CALL [r8+0x108]`), the
  filter actor (`FUN_18006a710`, per frame, both sides) draws alpha
  `min((100 − v)/100, cap)`, the setter `SetLaneTransparency`
  (`FUN_1801e1f80`, vslot `0x108`) keeps 100. 100 = no filter (World's
  PlayerWork default is 70, darkness 30).
- Guideline renderer (`FUN_180025db0`): mode 2 ⇒ returns (latched at init).
- The name-table groups are identical on all five builds.
- PlayerWork offset of the Option: `+0xE0`, `+0xF0` on 20250805 /
  20260224 (`stage_records::player_option_offset()`). The resolver
  `FUN_1801ea0e0(table[side], 0)` returns it except for mcode `0x9733` in
  one special mode (a static default Option) — not a DDR SELECTION song.

## 3. Readers and timing (World)

- `GamePlayActor` init (`FUN_18005be20`) latches speed / lane cover / arrow
  design; its update (`FUN_18005cc70`) visibility / guideline / step zone /
  boost / colour at its first steps; the filter actor re-reads the lane
  transparency every frame (live) — the forced values must stay until
  gameplay has ended.
- `ControlSpeedActor` (in-song speed mod) embeds its own Option copy
  (`+0x90`) made from the player's — starts at ×1.00 and steps its copy
  only (`option-icons.md` §5), exactly A3's behaviour.
- Result commit (`GamePlayActor` slot 5 `FUN_18005d900`): `record+0xF8 ←
  FUN_1801e2850(player Option)` — a getter copy into the stage record. The
  per-stage save's `<note>`-level option block is built from that record
  copy ⇒ it carries the forced options the song was actually played with
  (accurate; nothing to restore).
- Save marshal `FUN_180018ee0` (`ReflectSavePlayerData`): copies the
  player's Option getters into ess's staging buffer when the save is built
  (savekind 2 in the results scene, 3 at logout). ess.dll builds
  `/data/option` from it: `speed_type hispeed scroll_moving visibility lane
  stepzone scroll_direction arrow_color arrow_design lane_filter guideline`
  are all **s32 (kbin type 6)** children (ess `property_node_create(…, 6,
  name, …)` at every site) ⇒ `custom_options_persistence::replace_option_s32`
  can rewrite them unchanged in type.
- The multiplayer bot's impersonation (`impersonation.rs`, scene edge
  25 → {26,27,28}) copies the human's `Option+0x08..=0x6C` to the bot side.
  Scene callbacks run in registration order, which a runtime mod toggle can
  change; both orders are safe: bot first (the boot order — `multiplayer_bot`
  registers before `ddr_selection`) ⇒ both sides are entered when we force
  and both are snapshotted; bot after ⇒ it copies the human's already-forced
  fields, and the bot side (now entered) is snapshotted at the next window
  scene — or, on a direct 25 → 28 flip, never: it then simply keeps the
  forced copy (the bot's own copy had already replaced that side's values,
  and the bot side never saves). `PlayerWork+0x4`
  (`stage_records::side_entered`) covers the bot.

Repo interactions: `per_song_judgement_offsets` (`+0x24`) disjoint;
`song_rate::real_speed` skips type 1; `anytime_speedmod` patches the actor's
window gate, not the Option; `mine_render` / `training_mode` read `+0x60`
(consistent with the forced classic design); `song_reset` reads `+0x7C`
(flare level, not forced); `option_icons` shows nothing on skin 1 (World's
own skin-1 gate skips the actor). Quick restart (`finish` → 27 → 28, fresh
DPS) re-reads the still-forced fields; the in-place reset never leaves 28.

## 4. Mechanism

Per-side field writes with a snapshot (design §4.8 / research §B.3), zero
detours, zero code patches:

- **Window** = scenes {26, 27, 28} while the armed skin is 1. On every scene
  change: in the window, every entered side without a snapshot is
  snapshotted (all 11 values range-checked against the World enums — any
  implausible value ⇒ the chain is distrusted, that side is left alone,
  one WARN) and forced; sides already snapshotted are re-asserted
  (idempotent — covers quick restart's fresh DPS and anything that re-wrote
  a field). Outside the window, every snapshotted side is restored and the
  snapshot dropped — the first scene after gameplay (29) precedes the
  savekind-2 marshal by a whole loader scene, and a restart via 29 → 28
  re-forces from a fresh snapshot.
- **Save-tree fallback**: if a save is built while a side is still forced
  (unreachable by design), the save trampoline rewrites the 11 `/data/option`
  nodes from the snapshot (the `<timing_music>` precedent).
- **Disarm / disable** restore immediately.
- Offsets come from a new all-or-nothing derivation
  (`derive_ddr_sel_option_force`: RTTI Option vtable, the 11 getter stubs
  and the effective-speed getter shape), independent of the option-icon
  group; missing ⇒ skin 1 plays with the player's own options + one WARN.

No genuine choices remain for the maintainer: A3 settles in-song speed
(allowed, from ×1.00), versus (both forced) and the bot (forced).
