# Random Song Entry Research

RE record for a proposed "Random Song" entry at the head of the song-select wheel
(scene 25): a card with its own texture that, when decided, picks a random chart from
the *currently filtered* song set and proceeds into the normal difficulty-select flow.

All addresses are file-relative to `gamemdx.dll`'s `0x180000000` base. Primary build:
**20260721**. Cross-checked where noted on 20260616 and 20260825 (plus an older
unlabelled `gamemdx.dll` in the Ghidra project). Status: **static RE only — nothing
below has been verified live yet.** Open items are collected in §8.

## 1. Summary

The game already contains a dormant "Random" `FunctionCard`. The wheel builder
(`sequence::selectmusic::MusicListGenerator`, build vfunc `FUN_180199a10`) creates it
under a UI-mode gate (`GameWork+0x1c == 7`), **disabled** (`enabled=0`), behind a
"Menu" toggle card. Its decide lambda (`FUN_18019b5c0`) does precisely the job we want:

```
if (model+0x1b0 == null) {                 // no chart currently selected
    chart = FUN_1800ffbe0(model);          // random pick from the FILTERED+SORTED list
    FUN_1800ffb40(model, chart);           // select it: model+0x1b0 := chart, GameWork+0x18 := mcode
}
FUN_1800fc5d0(model+0xc0, 0xc);            // event 0xc → "invoke_difficulty" (normal decide flow)
```

So the mod does not need its own picker. It needs to (a) construct one always-enabled
`FunctionCard` wired to that lambda, (b) insert it at grid index 1 (right after the
`FolderHeaderCard`), keeping the 3-row column alignment, and (c) supply a texture named
`muca_button_<key>_text` for its label. Everything is reachable through the game's own
constructors and the plain pointer array the GridPanel uses for its children.

The stock card is invisible on a normal cabinet for two independent reasons (§5), which
is why it has never been seen in ALL MUSIC.

## 2. Song-select model (`DAT_1806f2d50`)

`SelectMusicModel`, 0x400 bytes, allocated in `FUN_1800fc100` (SelectMusicSequence
setup; model at seq `+0xb0`, view at seq `+0xb8`). Ctor `FUN_1800fcfc0`, dtor
`FUN_1800fd930`. The global holds a **pointer** to the model.

| Offset | Meaning |
|---|---|
| `+0x4 + side*4` | difficulty per side |
| `+0xc0` | event dispatcher (`FUN_1800fc5d0(model+0xc0, id)` fires; `FUN_180051930(model+0xc0, id, fn)` registers) |
| `+0x118..+0x124` | copied from seq `+0x78..+0x84`; byte `+0x118 != 0` ⇒ `+0x128 = 2` |
| `+0x128` | int state: 0 normal, 1 "mode fell back to 7", 2 special (see §6) |
| `+0x130/+0x138` | shared_ptr; `FUN_1800fdbb0(model,out)` copies it, `FUN_1800fdc10` writes `GameWork+0x1c = **(+0x130)` |
| `+0x150` | current side |
| `+0x1b0/+0x1b8` | `shared_ptr<ChartMetadata>` **selected chart** (null while a FunctionCard is focused) |
| `+0x1c0` | byte: base lists built |
| `+0x1c1/+0x1c2` | bytes: filter dirty / sort dirty (`FUN_180100f00` rebuilds when either set, then zeroes both) |
| `+0x1c8[2]`, `+0x208[2]`, `+0x248[2]` | per-side `vector<shared_ptr<ChartMetadata>>`, stride 0x20 per side. `+0x1c8` = full DB list (`FUN_180100100(model, side)` from music DB `DAT_1806f2d78`); `FUN_180100770(model, side, anyFilterActive)` returns `+0x208` or `+0x248` |
| `+0x288/+0x290/+0x298` | **the filtered + sorted wheel list** (`vector<shared_ptr<ChartMetadata>>`, stride 0x10) — what the wheel and the random picker read |
| `+0x2c8` | ptr; when 0, `FUN_1801007e0` builds the filter list from the `+0x358` tree × `+0x378` map |
| `+0x358/+0x360` | rb-tree (node `+0x18` key, `+0x20` sp, `+0xd1` nil) |
| `+0x378` | map (lookup `FUN_1801d5b80`/`FUN_1801d5a40`); iterated against PlayerWork `+0x1768` tree (per-side folder/filter state) in `FUN_1800fdc80` |
| `+0x3c0/+0x3c8` | shared_ptr focused |
| `+0x3d8..+0x3e8` | `vector<shared_ptr<panel>>` stride 0x10 |

`ChartMetadata` (`sequence::selectmusic::ChartMetadata`, 0x90 body after 0x10 refcount
header; ctor `FUN_1801a7530(chart, sp<music::Info>)`): `+0x10` `shared_ptr<music::Info>`,
`+0x20` five vectors (stride 0x10), `+0x70` int, `+0x74..+0x84` per-side difficulty,
`+0x88` sort key. `FUN_1801a7930(chart)` = mcode accessor (vt slot 0).

## 3. The filtered list — `FUN_180100f00(model) → model+0x288`

Returns `model+0x288`. Rebuilds only when `+0x1c1 || +0x1c2`:

1. `FUN_1801007e0(model, &filters)` — vector of **filter entries, stride 0xb0**; entry
   byte `+0` = active, entry `+0x20` = predicate object, `(pred->vt+8)(pred, &sp_chart)`
   → bool.
2. `FUN_180100910(model, &sorters)` — vector of **sorter entries, stride 0x68**; entry
   `+0x40` = sorter object, `(sorter->vt+8)(sorter, &sp_chart)` → int key.
3. Source list = `FUN_180100770(model, GameWork+4 (side), anyFilterActive)` copied into
   `+0x288` via `FUN_1801022f0`. Profiler tag `"Music Data init"`.
4. Filter pass: for each chart, all active predicates must return true; surviving
   indices collected. Per-sorter key maps built alongside. Tag `"Music filter"`.
5. Sort pass: `FUN_1801075e0` (merge sort over the index vector with the sorter keys),
   then the index vector is materialised back into `+0x288`. Tag `"Music sort"`.

`FUN_1801006c0(model, side)` = hard rebuild: `FUN_180100100` (base lists from the music
DB) then **resets `+0x1b0` to empty** (`FUN_180105130`).

The Filter/Sorter class family lives in anon namespace `5cc38b7a` (RTTI strings
`0x18048e160–0x18048e8c0`: `Linq::<lambda12/13>(Filter const&)`, `<lambda7>(bool,
sp<ChartMetadata> const&)`, `<lambda4>(bool, Sorter&, Sorter&)` …) — not decompiled;
not needed for this feature since we consume the already-built `+0x288` list.

## 4. Wheel construction — `MusicListGenerator::build` (`FUN_180199a10`)

RTTI `.?AVMusicListGenerator@selectmusic@sequence@@`, vftable `0x180373d08`:
`[FUN_180141a90, FUN_1801998d0 (→GridPanel), FUN_180199a10 (build cards),
FUN_18019af70 (restore focus by GameWork+0x18 mcode)]`. Factory `FUN_180145350`
(0x48 alloc), instantiated 8× from the wiring function `FUN_180141b00`. Entry
(param_2 = the GridPanel, `sequence::GridPanel`, RTTI `0x180481ed8`).

Every card goes through the same recipe:

```
card = operator new(size)   (FUN_180279714 — CRT heap)
ctor (FUN_1800451b0 = sequence::Button base, or FUN_18003cce0 = Component base)
set vftable at +0 and secondary at +0x28
name string  → card+0x38   (std::string, FUN_1800038a0 / FUN_180003440)
w = FUN_180049370(grid, rows)  → card+0xa0 (double);  card+0xa8 = w / K * ratio
FUN_180045c10(grid+0x68, &card)   // vector<Component*>::push_back
card+0x60 = grid                  // parent
```

GridPanel children = plain `Component*` array at `grid+0x68/+0x70/+0x78`
(begin/end/cap). `grid+0x168` = focused index, `+0x16c` previous index, `+0x12c` wrap
mode. Layout (`FUN_180049da0`, run every frame from `FUN_18004ab60`) walks the array in
order and **skips cards whose enabled byte `card+0xb8` is 0** (`FUN_180045820(card,
bool)` is the setter — it also propagates to the card's own children). Because
positions are recomputed from array order each frame, inserting a pointer anywhere in
the array is honoured immediately.

Build order on 20260721 (`rows = 3`, `local_100`):

| # | Card | Size | vft | Notes |
|---|---|---|---|---|
| 0 | `FolderHeaderCard` "Back" | 0x120 | `0x18036fbf8` | `+0x118 = -1`; decide = lambda2 |
| (mode 7) | `SpaceCard`, `FilterCard` "Filter" (`FUN_180119e70`, 0x2a8, lambda3), `SortCard` "Filter" (0xe0, vft `0x180370888`) | | | three cards = one full column, so alignment is preserved |
| 1..N | `MusicCard` × N | 0x158 | `0x180376848` / `0x180376890` | via lambda4 → `FUN_18019b420` → `FUN_18019add0`; `card+0x148` = `sp<ChartMetadata>`; ctor `FUN_180159e80` |
| | `EmptyContentCard` "Empty" (0x118, lambda5) if N == 0, else `BlankCard` × `(3 - N%3) % 3` (0x100, vft `0x1803768a8`) | | | pads the music block to a multiple of 3 |
| | `CenterBackground` (0xc0, vft `0x180376968`), `SpaceCard` × 2 (0xc8, vft `0x180376908`, w 402.0 h 7.0) | | | |
| (mode 7) | `FunctionMenuCard` "Menu" (0x1a0, ctor `FUN_180113670`, vft `0x18036fc68`) | | | toggle for the hidden function cards |
| (mode 7) | **`FunctionCard` "Random"** (0x140, vft `0x18036fcd8` / `0x18036fd30`) | | | decide = **lambda6**; texture key `+0x118 = "random"`; **created disabled** |
| (mode 7) | `FunctionCard` "Skip"/"skip" (lambda7) if `FUN_1801dd690()` | | | created disabled |
| (mode 7) | padding `FunctionCard`s (no name/lambda) to a multiple of 3, then `SpaceCard` | | | created disabled |

Strings: table at `0x18037f1b8` = `Back, Filter, Music Data Prepare, Empty, Menu,
Random (0x18037f1ec), random (0x18037f1f4), Skip, skip, Music UI Prepare`.

### FunctionCard layout (0x140)

Initialised at `0x18019a754..0x18019a79e`:

| Offset | Value |
|---|---|
| `+0x00` / `+0x28` | vft `0x18036fcd8` / secondary `0x18036fd30` |
| `+0x38` | std::string display name ("Random") |
| `+0x60` | parent GridPanel |
| `+0xa0/+0xa8` | width/height doubles |
| `+0xb8` | enabled byte (`FUN_180045820`) |
| `+0xc0` | AFP clip ptr (set lazily by `FUN_180113f90`) |
| `+0xc8` | 0 |
| `+0xd0` | byte 0 |
| `+0xe0/+0xe8` | std::string (len 0 / cap 0xf) — `FUN_180113f20` returns `+0xe8 != 0` |
| `+0xf8..+0x110` | `std::tr1::function<void()>` (small-buffer at `+0xf8`, impl ptr at `+0x110`); set with `FUN_1800d30c0(card+0xf8, &impl)` |
| `+0x118` | std::string texture key ("random") — `+0x128` len 0, `+0x130` cap 0xf |

Vftable `0x18036fcd8` slots: `[0] FUN_18023
4b20`-family dtor thunks, `[1] FUN_180113e90` (dtor), `[3] FUN_180044fb0`, `[4]
FUN_180113f30` = **decide** (invokes the std::function via `FUN_180044ff0`), `[5]
FUN_180045af0`, `[7] FUN_180044de0` (render), `[8] FUN_180113f90` (clip setup) …
Secondary `0x18036fd30`: `[0] FUN_180113f20`, `[1] FUN_180114350` = onFocus(bool):
`SetFrameLabel loop_button_on / loop_button_off / loop_blank`, posts event 6, and calls
`FUN_1800ffb40(model, empty)` — i.e. focusing any FunctionCard clears the selected
chart (`GameWork+0x18 = -1`), which is exactly the precondition lambda6 tests.

Clip setup `FUN_180113f90`: pool `DAT_1806fa600` (0x400 slots × 0x48·8), template
`"music_card_button"` (`0x18036fb90`) created via `FUN_18026ecb0(clip,
*DAT_1806f2d68+0x5f0, name, 0)`, label bitmap `"muca_button_%s_text"` (`0x18036fba8`)
formatted with `card+0x118`. So the stock Random label texture would be
`muca_button_random_text`; the known siblings are `muca_button_menu_open_text` /
`muca_button_menu_close_text` (`0x18036fb50` / `0x18036fb30`). Whether
`muca_button_random_text` actually ships in the `select_music_card` IFS is **unverified**
(§8).

The lambda6 impl is a `std::tr1::_Impl_no_alloc0<_Callable_obj<<lambda6>,0>,void>` —
a **stateless** functor: the on-stack holder is a single qword (the impl vftable
`0x18037f348`). Its `operator()` body is `FUN_18019b5c0`, copy-ctor `FUN_18019b550`.

### FunctionMenuCard (`FUN_180113670`, decide `FUN_1801137f0`)

Decide plays `se_common_decide_a`, flips the global `DAT_1806f224d`, then walks the
parent grid and `__RTDynamicCast`s every child to `FunctionCard`, writing the new bool
into `+0xb8` (with child propagation). That is the only stock path that ever enables the
Random / Skip cards.

## 5. Why the stock Random card is never seen

1. **Mode gate.** The whole Menu/Random/Skip group (and the Filter/Sort prefix) exists
   only when `*(int*)(*DAT_1806f14f8 + 0x1c) == 7` (`0x180199a93: CMP [RCX+0x1c],7 ;
   SETZ R15B`). See §6 for how that value is derived.
2. **Created disabled.** `0x18019a932: XOR EDX,EDX ; CALL FUN_180045820` right after
   the texture key is set. Disabled cards are skipped by layout, so the card has no
   slot until the Menu card is toggled.

Byte-identical gate on all four builds (`41 BD 03 00 00 00 44 89 6C 24 38 48 8B 05 ??
?? ?? ?? 48 8B 08 83 79 1C 07 41 0F 94 C7 44 88 7C 24 3C B9 20 01 00 00 E8`):

| Build | Match | `FUN_180199a10` equivalent (match − 0x6e) |
|---|---|---|
| 20260616 | `0x18019899e` | `0x180198930` |
| 20260721 | `0x180199a7e` | `0x180199a10` |
| 20260825 | `0x180199a8e` | `0x180199a20` |
| (older `gamemdx.dll`) | `0x180197f5e` | `0x180197ef0` |

## 6. UI mode (`GameWork+0x1c`) derivation — `FUN_18010b580` (SelectMusicView ctor)

`GameWork = *DAT_1806f14f8`. `PlayerWork = (&DAT_1806f2ed0)[GameWork+8]`.

The view ctor registers the generator list at `seq+0x430` (built by `FUN_180110290`;
entries are `shared_ptr<Obj>` stride 0x10 where `*Obj = int mode`, `Obj+0x20` = enable
predicate; `FUN_180147110(list, &out, mode)` = lookup with the predicate applied). Then,
at `0x18010bcd1..0x18010c031`:

```
mode = 1
t = PlayerWork+0x4c                          // current folder type (0x63 → treated as 1)
if t != 0:
    mode = t
    if lookup(mode) == null: mode = 7        // fallback: no generator for this folder type
if t == 7 and lookup(8): mode = 8            // EXTRA SAVIOR generator enabled
if t == 7 and lookup(9): mode = 9            // GALAXY BRAVE generator enabled
if t == 10 and lookup(10): mode = 10         // DAN RANK
GameWork+0x1c = mode
model+0x128 = (fell back to 7) ? 1 : 0, or 2 if byte model+0x118 != 0
```

Folder types per `folder_system_research.md`: 1–6 genre, **7 ALL MUSIC**, 8
extrasavior/brave, 9 galaxybrave/brave, 10 Dan Ranking, 0x63 final. Known mode
semantics elsewhere: 9 → `scene_result_brave`, 10 → `scene_result_danrank`
(`event_flag_system_research.md`). So mode 7 is "ALL MUSIC with no event generator
active" **or** "any folder type with no registered generator". Whether a stock ALL
MUSIC folder lands on 7 (and therefore shows a Menu card at the end of the wheel) is
not confirmed live — the maintainer reports no Random card in ALL MUSIC, which is
consistent with either the gate failing or the card simply being hidden behind the
Menu card. `FUN_18010f240` also tests mode 7 (`0x18010f5f6`, `0x18010f63a`).

Other mode consumers: `FUN_18010c770` looks the current mode up in the generator list
and fires event 9 when it is missing; `FUN_18010ef00` (handler on `+0x128`) and
`FUN_1801445e0` re-derive it.

## 7. Selection / decide path

- Random pick `FUN_1800ffbe0(model)`: RNG object from `FUN_180231c30`; seeded with
  `(int)(float_global_product * DAT_18038fb08)` via vt`+0x10`; `list = FUN_180100f00
  (model)`; if non-empty, `idx = (vt+0x20)() % count` → `&list[idx]`; if empty, hard
  rebuild `FUN_1801006c0(model, side)` and pick from `model+0x208+side*0x20` instead;
  if that is empty too it throws (`FUN_18027cc9c`). Returns a pointer to the
  `shared_ptr<ChartMetadata>` element.
- Commit `FUN_1800ffb40(model, sp*)`: `FUN_18016f2e0(model+0x1b0, sp)` (shared_ptr
  assign) then `GameWork+0x18 = mcode` via `FUN_1801a7930`. `GameWork+0x18` is what
  `FUN_18019af70` uses to restore wheel focus after a rebuild (pred `FUN_180048800` /
  `FUN_18019b090` compare `MusicCard+0x148` mcode).
- Event `0xc` → `FUN_18010d6d0` ("invoke_difficulty"). For comparison the MusicCard
  decide is `FUN_18010d350` ("select_music" SE + `vo_choice_music`, then event `0xd`
  when `+0x1b0` is set). Other event ids seen: 0, 1, 6 (function-card focus), 7, 8, 9,
  0xa, 0xe, 0x11, 0x14, 0x16, 0x18.

The decide dispatch itself: GridPanel focused child (`grid+0x168`) → card vftable slot
4 (`FUN_180113f30` for FunctionCard) → `FUN_180044ff0` → the std::function at
`card+0xf8`.

## 8. Design options and open questions

### Recommended shape

Detour `MusicListGenerator::build` (`FUN_180199a10`; post-original) and, when the grid
now starts with a `FolderHeaderCard` (vft `0x18036fbf8`) and contains ≥1 MusicCard:

1. Build a `FunctionCard` with the game's own recipe (§4): `operator new(0x140)` →
   `FUN_18003cce0` → vftables → field init exactly as `0x18019a754..0x18019a79e` →
   name `+0x38` → `FUN_1800d30c0(card+0xf8, &{0x18037f348})` (borrow the stateless
   lambda6 impl) → texture key `+0x118` (≤15 chars, SSO rule) → size via
   `FUN_180049370(grid, 3)` with the same `w / DAT_180393b28 * DAT_1803aef58` height
   formula → **leave enabled** → `card+0x60 = grid`.
2. Insert at index 1 by shifting the `Component*` array at `grid+0x68..+0x70` (push_back
   via `FUN_180045c10`, then rotate the tail pointer to index 1). Also insert two
   `BlankCard`s (or make the Random card the first of a 3-card column) so the 3-row
   padding computed for the music block is not thrown off by one.
3. Texture: inject `muca_button_<key>_text` into the `select_music_card` atlas via
   `atlas_cloner` (donor `muca_button_menu_open_text` if present; otherwise FRESH mode as
   `music_wheel_song_length` does). Using our own key (e.g. `rnd`) avoids colliding with
   any stock `random` art.
4. No changes to the picker, commit, or event flow — all stock.

Alternative (not recommended): flip the mode-7 gate (`41 0F 94 C7` SETZ → force 1) and
skip the `FUN_180045820(card,0)`. That also drags in the Filter/Sort prefix, the Menu
card, Skip, and the padding FunctionCards — far more side effects than inserting one
card.

### Open questions (need a cabinet run)

1. What `GameWork+0x1c` actually is in ALL MUSIC on this build (log it at scene 25).
   If it is 7, a Menu card should be present at the wheel's tail and toggling it
   would show the stock Random card — a free end-to-end test of lambda6.
2. Does `muca_button_random_text` exist in `select_music_card*.ifs`? (Look for
   `get_bitmap_info[muca_button_random_text] can not find` after the above.)
3. Focus-restore after a wheel rebuild (`FUN_18019af70`) when the focused card is our
   FunctionCard (`GameWork+0x18 == -1`): confirm it lands on the header/first card and
   not out of range.
4. `FolderHeaderCard` "Back" wrap behaviour with an extra column at the head.
5. The `+0x1b0 == null` precondition in lambda6 relies on onFocus (`FUN_180114350`)
   having cleared the selection — confirm the secondary vftable slot is invoked for a
   card inserted after build (it is called from the grid's focus-change path, so it
   should be).
6. Allocator: the card is freed by the GridPanel dtor through the card's own vftable
   dtor (`FUN_180113e90` → `free`), so it must come from the CRT heap
   (`FUN_180279714` / `game_malloc`), never `VirtualAlloc`.
