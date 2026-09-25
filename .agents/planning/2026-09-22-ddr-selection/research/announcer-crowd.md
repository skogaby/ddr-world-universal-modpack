# Step 11 RE — A3 announcer and crowd on the legacy skins (2026-09-25)

Addresses 20260825 (`0x180000000` base) unless stated; A3 = `gamemdx_20240402`.
Starting point: `sounds-options-folder.md` §A.1 / §A.4 (this file re-verifies
them from the decompiles and adds what the implementation needed).

## 1. The actor

| | World | A3 |
|---|---|---|
| RTTI | `.?AVCallVoiceActor@dance@sequence@@` | same |
| ctor | `FUN_180055260(this, diff0, diff1)` | `FUN_180036890` |
| onUpdate (slot 6) | `FUN_1800553b0` (= the `announcer_dispatcher` AOB announcer_mute already detoured) | `FUN_1800369e0` |
| onMessage (slot 8) | `FUN_180055ad0` | `FUN_180037220` |
| voice guard | `FUN_1800559f0` | `FUN_180037140` |

onUpdate on the five sweep builds: 20250805 `+0x51F70`, 20260224 `+0x51100`,
20260721 `+0x55430`, 20260825 `+0x553B0`, 20260915 `+0x55CF0`.

Field layout — identical in both games (ctor + both onMessages + all
onUpdates): step-state array `+0x58 + i·8` indexed by the word `+0x82` (count
`+0x80` = 5; 0 wait, 1/2 running, 3 disabled), last voice handle `+0x84` (−1),
time / second time word `+0x88` / `+0x8C` (World msg `0x1045` = A3 `0x1048`),
next state voice / next crowd `+0x90` / `+0x94` (seeded `t + 0xBE70` /
`t + 0xFC7C` by World `0x1048` = A3 `0x104B`), next combo milestone `+0x98`,
voice-off / SE-off `+0x9C` / `+0x9D` (World `0x104F` from the music entry
`+0x17F` = A3 `0x1052` / `+0xE7`, musicdb `<voice>` 1 / 2 / 3), was-low
`+0x9E`, per side `+0xA0 + s·0xC` = gauge f32 (World `0x103E/F` = A3
`0x1041/2`), combo (`0x1033` = A3 `0x1036`), difficulty (ctor args — the DPS
passes `*(side info + 4)`, the same expression in both games), players left
`+0xB8`. World's onUpdate still carries A3's logic shape but one table
(`vo_ingame_combo_%04d` / `_state_0N_*`, `se_kansei_*`) and no skin branch.

## 2. A3's rules (skins 1–5), from `FUN_1800369e0`

Per frame with the step state ∈ {1, 2}; `c` = max combo, the "side" = the one
with the higher gauge (tie ⇒ side 1: `g0 <= g1`), `g` / `d` its gauge /
difficulty; "quiet" = `c % 100 ≥ 91`.

1. Combo: `m = +0x98`; when `0 < m ≤ c`: `m % 100 == 0` ⇒ table
   `[m/100 − 1]` (skin 1 none, 2–3 `sn2_dgm25..34`, 4–5
   `vo_ingame_combo_100..1000`) played **unguarded** (handle stored), past the
   table skins 4–5 `vo_ingame_combo_over` (guarded, not quiet); odd 50s skins
   4–5 `vo_ingame_combo_gen` (guarded, not quiet). Then `m = (c/50 + 1)·50`
   every frame.
2. State voice when `!voice_off ∧ next_state < t` (`next_state += 0x8000`):
   `g > 0.8` ⇒ `ACT6` / `sn2_dgm_high` / `vo_ingame_high`; `0.2 ≤ g ≤ 0.8` ⇒
   `was_low ∧ t2 > 20000` ? (4–5 `vo_ingame_regain`, else nothing) : (1
   nothing, 2–3 `sn2_dgm_middle`, 4–5 `vo_ingame_gen`), was-low cleared;
   `g < 0.2` ⇒ 4–5 only: `vo_ingame_low_hard` (d ∈ {3, 4}) / `_low_easy`,
   was-low set (skins 1–3 never set it). All guarded, not quiet.
3. Crowd when `!se_off ∧ next_crowd < t` (`next_crowd += 0x10000`): unless
   `g ≤ 0.4 ∧ c < 13`: 1 `2nd_BIG2`, 2 `2nd_KANSEI_B`, 3 `STG_APP03`, 4–5
   `STG_APP02` (slot-2 SE, no voice — the cheer / boo voices are skin 0 only).

Every play passes the mute filter (`(*filter)(5) != 6`, `-1` stored when
muted). Note the `<voice>2` quirk: voice-off + SE-off but the step state stays
running, so combo callouts still play. Thresholds: 0.2 `DAT_180265034`, 0.4
`DAT_1802645f8`, 0.8 `DAT_180288c60` (bit-exact 0x3E4CCCCD / 0x3ECCCCCD /
0x3F4CCCCD).

## 3. Playing into the era bank

- World's guard: `h = +0x84; lock; playing = FUN_1801ab360(*mgr, h); unlock;
  if !playing ∧ cue: mute filter; lock; h = FUN_1801ab240(3, cue, 0);
  unlock; +0x84 = h`. The lock = libavs ordinals 16 / 17 (`XCnbrep700000f` /
  `…0010`, `(i32 id)`), called only while the id global `DAT_1806f38a8 > 0`,
  with ECX = that id.
- The public `se_play` façade (`FUN_1801aa180`) is exactly "mute filter for
  slots ∉ {1, 5}; lock; `FUN_1801ab240`; unlock" — so `se_play(dsel slot, cue,
  0)` = World's guarded play for our slot; its handle lives in the cue table
  `FUN_1801ab360` reads (`mgr + (h + 5)·0x20`).
- Every rule cue is in the Step 3 `dsel` bank (`sound/cues.rs`).

## 4. Interactions

- announcer_mute already detoured onUpdate (one-detour rule) ⇒ promoted to
  `services/call_voice_hooks.rs`: mute predicate first (silences World's and
  A3's announcer alike), then the override, else the original.
- HERE WE GO (skin 1 `ACT3_1` / `ACT4_2`) is Step 4's `intro.rs`; stage calls
  are Step 5's panel — not this actor.

## 5. Mechanism as built

- `derive_call_voice` (RTTI, no AOB): slot 6 (must equal
  `announcer_dispatcher` when that resolved), A3's layout checked by eleven
  exact instructions in onUpdate (each once, all five builds), the guard =
  the CALL after `LEA reg,["vo_ingame_state_01_highest"]`, inside it the one
  lock / `MOV RCX,[mgr]; CALL is_playing` / unlock block (both counter loads
  equal; `mgr` == `audio_manager_global`). Publishes `call_voice_update /
  _guard / _is_playing / _audio_manager / _lock_count / _lock_iat /
  _unlock_iat`. Sweep ALL GREEN.
- `services/call_voice_hooks.rs`: the detour; `set_mute`, `set_override`,
  `cue_is_playing(h)` (lock + the game's is-playing, verbatim).
- `sound/rules.rs` (pure, 13 tests): §2 as `step(fields, skin) -> Step`
  (fixed 3-play array, no allocation).
- `sound/call_voice.rs`: the override — armed skin 1..=5 ∧ bank registered ∧
  every rule cue in it ⇒ read the fields, `rules::step`, write back
  milestone / due times / was-low, play in order through `se_play` into the
  bank slot (Voice: store handle; Guarded: only when `cue_is_playing(+0x84)`
  is false, store; Se: fire). Otherwise World's onUpdate. Installed at enable
  once the era bank build starts.
