# Porting `patches.js` Hex Edits to 64-bit DDR World

Research notes for porting five hex-edit hacks — originally authored for the
**32-bit** DDR World build `gamemdx_x86_20250610_02.dll` (the binary `patches.js`
targets) — to the **64-bit** builds the current hook DLL runs against.

## Source format (`patches.js`)

Each patch is `{ offset, off, on }` where `offset` is a **file offset** into
`gamemdx.dll`, `off` is the stock bytes at that offset, and `on` is the bytes to
write instead. Numeric/union options give a single `offset` + `size` and a value
to poke. **All offsets are raw file offsets, not virtual addresses.**

## Binaries in play

| Build | Bits | Image base | Role |
|---|---|---|---|
| `gamemdx_x86_20250610_02.dll` | 32 | `0x10000000` | The build `patches.js` targets (source of truth for the hacks) |
| `gamemdx_20260324` (`gamemdx.dll`) | 64 | `0x180000000` | Current primary target build |
| `gamemdx_20260526` | 64 | `0x180000000` | Second 64-bit build, for cross-version verification |

> Note: the task brief said "20250526" — the actually-loaded second 64-bit build
> is **20260526**. Both 64-bit builds are 2026-era.

## File-offset ↔ virtual-address mapping

Derived from each PE's section table (verified against known bytes).

### 32-bit `gamemdx_x86_20250610_02.dll` (base `0x10000000`)

| Section | File range | `VA = file +` |
|---|---|---|
| `.text`  | `0x000400`–`0x259800` | `0x10000C00` |
| `.rdata` | `0x259800`–`0x34AC00` | `0x10001800` |
| `.data`  | `0x34AC00`–`0x39A800` | `0x10002400` |

### 64-bit `gamemdx_20260324` (base `0x180000000`)

| Section | `VA = file +` |
|---|---|
| `.text`  | `0x180000C00` |
| `.rdata` | `0x180001600` |
| `.data`  | `0x180001800` |

### 64-bit `gamemdx_20260526` (base `0x180000000`)

| Section | `VA = file +` |
|---|---|
| `.text`  | `0x180000C00` |
| `.rdata` | `0x180000C00` |
| `.data`  | `0x180000C00` |

(For this build all three sections happen to share the `0xC00` delta. None of the
five patches land in `.data`; only `.text` and `.rdata` deltas matter in practice.)

To convert a 64-bit Ghidra VA back to a `patches.js`-style file offset, subtract
the per-section delta above.

---

## Hack 1 — Mute Announcer

**Tooltip:** "Mutes the announcer and cheering voices."

### 32-bit patches

| # | File off | VA (32-bit) | off → on | Meaning |
|---|---|---|---|---|
| 1 | `0x46EF3` | `0x10047AF3` | `0F 84` → `90 E9` | `JZ rel32` → `NOP` + `JMP rel32` at function entry |
| 2 | `0x2DB0AF` | `0x102DC8AF` | `76` (`'v'`) → `62` (`'b'`) | corrupts `"voice.xwb"` → `"boice.xwb"` |

### Mechanism

`FUN_10047ab0` is the **in-game announcer / voice dispatcher**. It plays combo
callouts (`vo_ingame_combo_%04d`, `vo_ingame_combo_other`), score-state cues
(`vo_ingame_state_NN_*`), and stage-clear cheer SFX (`se_kansei_big/middle/small`),
driven by combo count and life/score thresholds.

- **Patch 1** rewrites the function's entry guard. The prologue does:
  ```
  10047af1: TEST EAX,EAX
  10047af3: JZ 0x10047ea7      ; -> function epilogue
  ```
  Patch 1 turns `JZ 0x10047ea7` (`0F 84 AE 03 00 00`) into `NOP` + `JMP 0x10047ea7`
  (`90 E9 AE 03 00 00`). The `E9 rel32` reuses the original `JZ`'s 4-byte
  displacement; because `E9`'s operand is measured from the byte after the opcode
  (one byte later than the `0F 84` operand) the math still lands on the epilogue.
  Net effect: the whole announcer body is skipped on every call.
- **Patch 2** corrupts the wave-bank filename `"voice.xwb"` (the announcer/voice
  XWB sound bank) so it fails to load — belt-and-suspenders muting even if some
  other path reaches a play call.

### 64-bit port (status: CONFIRMED — both builds)

**Option A — `voice.xwb` corruption (portable data patch, recommended).** In 64-bit
the string is the full path `"data/sound/win/voice.xwb"`, referenced by the
sound-bank loader `FUN_18002be00`. Poke the `v` of `voice` (`0x76`→`0x62`) to make
the bank fail to load → `"...win/boice.xwb"`.

| Build | path string VA | `v`-byte VA | `v`-byte **file offset** |
|---|---|---|---|
| 20260324 | `0x18035A2A0` | `0x18035A2AF` | **`0x358CAF`** |
| 20260526 | `0x18035C2E0` | `0x18035C2EF` | **`0x35B6EF`** |

(`v` is at path offset +15; `.rdata` file off = VA − `0x1600` for 20260324, − `0xC00`
for 20260526.) Anchor via the `"data/sound/win/voice.xwb"` string directly.

> ⚠️ This bank also backs `bgm_menu.xwb`? No — only `voice.xwb` is corrupted; the
> other three banks (`se_system.arc`, `se_normal.arc`, `bgm_menu.xwb`) load from
> separate strings in the same loader and are unaffected. But note the voice bank
> may hold more than announcer VO; verify nothing else essential rides it before
> shipping (in practice it's the announcer/voice bank).

**Option B — entry-guard early-return (version-fragile).** The 64-bit announcer
dispatcher is `FUN_180055a50` (32-bit analog `FUN_10047ab0`; anchored by
`vo_ingame_combo_%04d` @ `0x18035D4D8`). Its entry guard:

```
180055a8c: MOVZX EAX, word ptr [RCX+0x82]
180055a93: MOV   ECX, [RCX+RAX*8+0x58]
180055a97: TEST  ECX,ECX
180055a99: JZ    0x180056058      ; -> epilogue
180055a9f: CMP   ECX,0x3
180055aa2: JZ    0x180056058
```

NOP+JMP the `JZ` at `0x180055A99` to force the whole body to skip — same idea as the
32-bit patch 1, but the `0F 84` rel32 encoding/displacement differs per build, so
this must be re-derived each version (don't hardcode).

> **Recommendation for the hook DLL:** prefer hooking `FUN_180055a50` (AOB-resolved)
> and early-returning, OR the portable `voice.xwb` data patch. Avoid baking the
> entry-guard byte patch as a fixed offset.

---

## Hack 2 — Center arrows for single player

**Tooltip:** "Centers the arrows for single player."

### 32-bit patches

| # | File off | VA (32-bit) | off → on | Role |
|---|---|---|---|---|
| 1 | `0x5996D` | `0x1005A56D` | `75` → `EB` | `JNZ` → `JMP`: force `"double_lane_usr"` (centered) lane layout |
| 2 | `0x59A44` | `0x1005A644` | `75 05` → `90 90` | NOP a `JNZ`: force `lane_%s_%s` selector to `"double"` |
| 3 | `0x59C5D` | `0x1005A85D` | `83 C4 0C 8D 4C 24 20` → `E9 …` | replace `ADD ESP,0xC; LEA ECX,[ESP+0x20]` with `JMP cave A` |
| 4 | `0x59BF2` | `0x1005A7F2` | `83 C4 0C 8D 44 24 20` → `E9 …` | replace `ADD ESP,0xC; LEA EAX,[ESP+0x20]` with `JMP cave B` |
| cave A | `0x3353` | `0x10003F53` | `CC…` → code | re-run displaced insns, `MOV [ECX],0x1EF`, jump back |
| cave B | `0xB0D4` | `0x1000BCD4` | `CC…` → code | re-run displaced insns, `MOV [EAX],0x1EF`, jump back |

(Both cave regions confirmed stock `0xCC` alignment padding — free space.)

### Mechanism

`FUN_1005a180` is the **gameplay HUD / lane layout builder**. It loops over both
player sides and positions every HUD element by name via
`FUN_1005bcd0(key, coordStruct)`, where `coordStruct` is a 6-dword payload and its
**first dword is the element's X coordinate** (`local_c0`, stack `[ESP+0x20]`).
Element keys include `score_%dp_usr`, `bpm_%dp_usr`, `gauge_%dp_usr`,
`%dp_lane_usr` / `double_lane_usr`, `%s/arrow_usr`, `%s/freeze_judge_usr`, etc.

The hack centers single-player play by:

1. **Patches 1 & 2** force single-player to use the **centered ("double") lane
   geometry** instead of the side-offset 1P geometry:
   - Patch 1 flips the `if (*local_a8 == 0)` branch so the lane key is always
     `"double_lane_usr"`.
   - Patch 2 NOPs the `JNZ` that selects `"single"` vs `"double"` in the
     `lane_%s_%s` format, forcing `"double"`.
2. **Patches 3/4 + caves** hard-set the X coordinate of two elements to **`0x1EF`
   = 495** right after their coord struct is built but before `FUN_1005bcd0`
   stores it:
   - Patch 4 + cave B → the **`arrow_raw`** receptor block (`%s/arrow_usr`).
   - Patch 3 + cave A → the **`freeze_judge`** block (`%s/freeze_judge_usr`).
   - Each cave re-executes the two displaced instructions (`ADD ESP,0xC` +
     `LEA reg,[ESP+0x20]`), writes `MOV dword ptr [reg], 0x1EF` to overwrite the X,
     then `JMP`s back to the instruction after the patched site.

### 64-bit port (status: CONFIRMED — both builds; recommend hook, not byte patch)

The 64-bit HUD layout builder is **`FUN_18006c230`** (32-bit analog `FUN_1005a180`;
anchored by `double_lane_usr` / `arrow_raw`). The named-layout setter is
**`FUN_18006f5d0(parent, name, &coord6)`** (32-bit analog `FUN_1005bcd0`) — it stores
a 6-dword payload keyed by `name`, where dword[0] = **X**, dword[1] = **Y**,
dword[4]/[5] = scale. Present and structurally identical in both 64-bit builds.

The decompile maps the 32-bit hack 1:1, and the 64-bit code is actually *cleaner*
to intercept than the 32-bit code-cave approach:

1. **Lane geometry branch** (force centered "double" lane for 1P):
   ```c
   if (*(int *)(this + 0x84 + side*4) == 0)
       sprintf(name, "%dp_lane_usr", playerNo);   // 1P side-offset lane
   else
       sprintf(name, "double_lane_usr");          // centered lane
   ```
   Forcing the `== 0` branch to take the `else` is the analog of 32-bit patch 1.
2. **Lane-skin single/double selector** (analog of 32-bit patch 2):
   ```c
   pcVar11 = "double";
   if (local_28b != '\0') pcVar11 = "single";   // local_28b = (side input flag == 0)
   ...
   sprintf(&local_1a8, "lane_%s_%s", pcVar11, normal/reverse);
   ```
   Forcing `pcVar11 = "double"` centers the lane skin.
3. **Arrow / freeze_judge X override** (analog of patches 3/4 + caves). The native
   code already computes a centered "arrow" position from "arrow_raw":
   ```c
   FUN_18006f5d0(parent, "arrow_raw", &coord);        // raw receptor pos
   coord.x = rawX - width/2;                            // engine's own centering math
   coord.y = rawY + yAdj;
   FUN_18006f5d0(parent, "arrow", &coord);            // adjusted
   ...
   FUN_18006f5d0(parent, "freeze_judge", &coord);     // freeze arrows
   ```
   The 32-bit hack hard-set X=`0x1EF` (495) for the `arrow_raw` and `freeze_judge`
   structs. In 64-bit the same effect is achieved by overriding dword[0] (X) of the
   coord passed to `FUN_18006f5d0` when `name ∈ {"arrow_raw","arrow","freeze_judge"}`
   for the single-player side.

**Recommended hook-DLL implementation (no byte patch / no code cave):**
post-hook `FUN_18006f5d0` (AOB-resolved). In the callback, when the current scene is
single-player and `name` is one of the lane-relative keys
(`arrow_raw`, `arrow`, `freeze_judge`, and optionally `judge`/`combo`/etc. if you
want the whole HUD centered), rewrite `coord[0]` (X) to the centered value before
the original stores it. This is exactly the pattern CLAUDE.md/AGENTS.md prescribe
(shared dispatcher, render-thread-safe, version-agnostic via AOB).

> The `0x1EF` (495) X constant is in the 32-bit playfield's coordinate space. DDR
> World's playfield logical width is consistent across the 32→64 transition (the
> layout coords are authored in the same virtual canvas), so **495 is very likely
> still correct**, but confirm against `double_lane_usr`'s own X (read what the
> centered lane resolves to and match it) rather than hardcoding blindly.

> If a pure byte patch is still wanted (no hook), the branch flips are small
> (force the lane-name `if`/selector), but the X-override has no spare register to
> reuse the 32-bit cave trick cleanly — the hook is strictly better here.

---

## Hack 3 — Hide all bottom text

**Tooltip:** hides bottom-corner text — `CREDITS`, `TOKEN`, `PASELI`, `FREE PLAY`, etc.

### 32-bit patch

| File off | VA (32-bit) | off → on |
|---|---|---|
| `0x26097C` | `0x1026217C` | a ~360-byte `.rdata` string block → all `0x00` |

### Mechanism

`FUN_10007950` is the **bottom-corner HUD text renderer** (credits / coin / token /
PASELI balance / online-status line). It builds each line from `printf`-style
format literals that live in one contiguous `.rdata` block starting at
`0x1026217C`, then draws them via a text-draw virtual call `(...+8)(str, 1)`.

The block contains (in order): `"EVENT MODE"`, `"FREE PLAY"`, `"S"`, `" "`,
`"TOKEN"`, `"COIN"`, `"%s%s:%2d/%2d"`, `"CREDIT%s:%2d"`, `"00000"`, `"000000"`,
`"******"`, `"PASELI: %s + %s"`, `"PASELI: %s"`, `"EXTRA PASELI: %s"`,
`"PASELI: NOT AVAILABLE"`, `"LOCAL MODE"`, `"OFFLINE MODE"`, `"MAINTENANCE"`,
`"CHECKING"`, `"CHECKING."`, `"CHECKING.."`, `"CHECKING..."`, `"ONLINE"`,
`"ERROR"`, `"NOT AVAILABLE"`.

Zeroing the whole block turns every format string into `""`, so the renderer still
runs but emits empty text — nothing visible. **Side note:** this also blanks the
online/maintenance status strings (`ONLINE`, `CHECKING...`, etc.) that share the
block, not just the credit/paseli text.

### 64-bit port (status: CONFIRMED — both builds)

- 64-bit renderer is `FUN_180009680` (xref'd from `"EVENT MODE"`).
- The `.rdata` block is **byte-for-byte identical** to the 32-bit one (same strings,
  same order, same padding) — verified on both 64-bit builds. It runs from
  `"EVENT MODE"` through the standalone `"NOT AVAILABLE\0"`, immediately before the
  `"SOFTWARE ID:"` / `"SYSTEM  ID:"` / `"HARDWARE ID:"` strings (which must stay
  intact). Block length = **333 bytes** (same `on` payload of 333 zero bytes as the
  32-bit patch).

| Build | block VA | block **file offset** | length |
|---|---|---|---|
| 20260324 | `0x1802DE318` | **`0x2DCD18`** | 333 |
| 20260526 | `0x1802E0318` | **`0x2DF718`** | 333 |

(`.rdata` file off = VA − `0x1600` for 20260324, − `0xC00` for 20260526.)

- **Caveat (unchanged from 32-bit):** this also blanks the online/maintenance
  status strings (`ONLINE`, `CHECKING…`, `OFFLINE MODE`, `ERROR`, etc.) that share
  the block. If you only want the credit/coin/paseli line hidden, zero individual
  format strings (`"CREDIT%s:%2d"`, `"%s%s:%2d/%2d"`, `"FREE PLAY"`, `"PASELI: …"`)
  rather than the whole block, OR hook the text-draw vcall in `FUN_180009680` and
  suppress the corner-text emission. The draw-suppression hook is the
  version-stable choice for the Rust DLL.

### Detour-and-no-op is safe (recommended hook-DLL approach)

`FUN_180009680` was fully decompiled and does **only** corner-text work: it composes
each string (credit/coin/token/paseli/free-play/event-mode + the network-status line)
and emits each via a text-object draw vcall `(*(obj+0x18)+0x10)(obj, str, 1)`. It calls
no other rendering and has no side effects beyond those draws. **Detouring it to an
immediate `return` cleanly hides all bottom text** — simplest and most version-stable
option (no offsets, survives updates as long as the function prologue is AOB-findable).

It draws to several distinct text objects:
- `DAT_1806ebc08` → the credit/coin/free-play/event line
- `DAT_1806ebc00` → (same composed string, second anchor)
- `DAT_1806ebc10` / `DAT_1806ebc18` → P1 / P2 PASELI lines
- `DAT_1806ebc38` → the **online/network-status** line (ONLINE / CHECKING… / MAINTENANCE)

So a blanket no-op also hides the network-status line. If you want to keep that one,
don't no-op the whole function — instead detour the **text-draw vcall** and drop only
the calls whose target object is in `{ebc08, ebc00, ebc10, ebc18}` (let `ebc38`
through), or zero only the credit/coin/paseli format strings in `.rdata`.

---

## Hack 4 — Timing Offsets (Sound / Input / Render / Bomb-frame) + High-Precision Input

**Original `patches.js` scope:** just the sound offset.

> **Tooltip (sound only):** "Larger numbers make audio later (Default: 87)."
> `size: 4`, range 0–1000.

The 32-bit `patches.js` exposes only `SOUND_OFFSET`, but the game builds a whole
**timing-offset record** with five tunable fields. The 64-bit binary even names
them in cleartext (used as config-map keys). All five are documented here because
the hook DLL wants to manipulate the lot.

### The timing record (5 fields, 0x14 bytes)

A "timing init" routine builds a **table of ten 0x14-byte records** (one per
preset / cabinet-configuration) and copies the selected record into a per-run
struct, then publishes each field into a global config map keyed by name:

| Record off | Engine config key | Type | Record-0 default | Meaning |
|---|---|---|---|---|
| `+0x00` | `SOUND_OFFSET` | i32 | `0x57` = **87** | audio sync; larger = audio later |
| `+0x04` | `INPUT_OFFSET` | i32 | `0x1C` = **28** | input/judge timing offset (the "SSQ/judge" offset) |
| `+0x08` | `RENDER_OFFSET` | i32 | `0x11` = **17** (rec 0) / `0x24`=36 most presets | render/display latency compensation |
| `+0x0C` | `BOMB_FRAME_OFFSET` | i32 | `0x00` = **0** (rec 0) / `1` or `2` other presets | shock-arrow ("bomb") frame timing |
| `+0x10` | `HIGH_PRECISION_INPUT` | bool | `0x01` = **on** | sub-frame input timestamping (see below) |

> **Naming note:** the engine's `INPUT_OFFSET` is what's colloquially called the
> "SSQ offset" / judge offset. There is no separate SSQ field — `INPUT_OFFSET` is
> it.

### 64-bit anatomy (build 20260324, `gamemdx.dll`)

- **Timing init** (publishes the fields): `FUN_18002bbd0` (32-bit analog
  `FUN_10025dd0`). Anchored by the strings `"ConfigBank.csv"` (`0x18035A1D0`) and
  `"Timing Init: %d"` (`0x18035A260`). It calls the record builder, then:
  ```c
  rec = FUN_180012f50(&out, presetIndex);
  set_int("SOUND_OFFSET",      rec.f0);   // FUN_1801acbf0(key, value)
  set_int("INPUT_OFFSET",      rec.f1);
  set_int("RENDER_OFFSET",     rec.f2);
  set_int("BOMB_FRAME_OFFSET", rec.f3);
  set_bool(rec.f4);                        // FUN_1801acb50 -> "HIGH_PRECISION_INPUT"
  ```
- **Record builder**: `FUN_180012f50` (32-bit analog `FUN_1000f610`). Builds all
  ten records on the stack — record 0's first 16 bytes come from a `.rdata`
  constant table via `MOVDQA`; records 1–9 and all the `+0x10` bool bytes are
  inline `MOV` immediates. `MOVSXD`/bounds-check selects record `presetIndex`
  (clamped 0..9) and copies 0x14 bytes to the caller.
- **`.rdata` default table**: `0x180358960`. First 16 bytes =
  `57 00 00 00 | 1C 00 00 00 | 11 00 00 00 | 00 00 00 00`
  (= SOUND 87, INPUT 28, RENDER 17, BOMB 0 for record 0).
- **Config setters/getters** (FNV-1a–hashed string keys into the map at
  `DAT_1806ebcf0`): `FUN_1801acbf0` (set int), `FUN_1801acb50` (set the
  `HIGH_PRECISION_INPUT` bool specifically), `FUN_1801acd50` (get bool).

### Patch sites (defaults) — both 64-bit builds

The cleanest single-value pokes mirror `patches.js` (overwrite the default
constant). Two equivalent locations exist per field: the **`.rdata` record-0
constant** and the **inline imm32** for the other records. Patching `.rdata`
record-0 changes the most-common preset's default.

| Build | `SOUND_OFFSET` imm32 (`.rdata` rec0) | file off | inline `0x57` imm32 (rec1) | file off |
|---|---|---|---|---|
| 20260324 | VA `0x180358960` | `0x357360` | VA `0x180012F8B` (op `0x180012F88`) | `0x1238B` |
| 20260526 | VA `0x18035A950` | `0x359D50` | VA `0x180012E6C` (op `0x180012E68`) | `0x1226C` |

(`.rdata` file off = VA − `0x1600` for 20260324, − `0xC00` for 20260526; `.text`
file off = VA − `0xC00` both.) `INPUT_OFFSET` / `RENDER_OFFSET` / `BOMB_FRAME_OFFSET`
defaults sit at `+4` / `+8` / `+0xC` from the `.rdata` rec0 base.

### `HIGH_PRECISION_INPUT` — the interesting boolean

**It is enabled by default** (the record builder writes `1` to the `+0x10` byte of
**every** one of the ten presets, and the `.rdata` table is irrelevant for this
byte since it's always set inline). It is read once at sound/input subsystem init
(`FUN_180022850`, reached from the bank loader `FUN_18002be00`) into the input
manager state at `DAT_1806ebc70 + 0x1261`.

What it controls — in the per-button event recorder `FUN_1800229e0`:

```c
if (*(char *)(input_state + 0x1261) == '\0') {   // HIGH_PRECISION_INPUT == OFF
    param_5 = now_frame_clock();                   // XCnbrep700002c(): per-frame "now"
}
*event_timestamp = param_5;                        // else keep the I/O layer's timestamp
```

- **ON (default):** input events keep the **sub-frame / device timestamp** passed
  up from the I/O layer → judge timing measured finer than the 60 Hz frame clock.
- **OFF:** the event timestamp is **snapped to the per-frame clock**
  (`XCnbrep700002c`, cached at `state+0x1268`) → legacy frame-quantized behavior;
  every input in a frame collapses to the same instant.

So there's no "enable for better timing" — Konami ships it on. The available lever
is forcing it **off** to A/B test feel or match older-cabinet behavior.

### Recommended hook-DLL approach (all five)

Because these flow through a runtime config map and a per-run state struct, the
hook DLL has two clean levers:

1. **Patch the defaults** (AOB-scan the record builder; rewrite the `.rdata` rec0
   ints and/or the inline imm32s). Survives as a static value, matches `patches.js`
   semantics.
2. **Write the live state** after the subsystem inits: the per-tick reader picks up
   `DAT_1806ebc70 + 0x1261` (HIGH_PRECISION_INPUT) live, and the published int
   offsets can be re-set via the same config-map setter the game uses
   (`FUN_1801acbf0(key, value)` analog) — keyed by `"SOUND_OFFSET"` etc. This is
   the more flexible path for a config-driven, runtime-toggle mod.

AOB anchor for the record builder (both builds):
`C7 45 ?? 57 00 00 00 C7 45 ?? 1C 00 00 00` (record-1 SOUND/INPUT inline pair),
or anchor via the `"Timing Init: %d"` / `"ConfigBank.csv"` strings → the init
function → the builder call.

---

## Hack 5 — Fullscreen FPS Target (union / numeric)

**Tooltip:** "Experimental: fast animations and menu scrolling." Options: 60 / 120 /
144 / 165 / 240 / 360 FPS (`size: 4`).

### 32-bit patch

| File off | VA (32-bit) | Stock | Instruction |
|---|---|---|---|
| `0x1896` | `0x10002496` | `60` (`0x3C`) | imm32 of `MOV dword ptr [ESP+0x88], 0x3C` at `0x1000248F` |

### Mechanism

In the main app-init function `FUN_10001e90`, after reading a display-mode flag:

```
1000248a: MOV EAX,[ESP+0x28]
1000248e: DEC EAX
1000248f: MOV dword ptr [ESP+0x88], 0x3C   ; default target = 60
1000249a: JNZ 0x100024a7
1000249c: MOV dword ptr [ESP+0x88], 0x4B   ; alt branch = 75
```

`0x3C` (60) is the default fullscreen frame-rate target. The union patch rewrites
this imm32 to 60/120/144/165/240/360 (`0x3C`/`0x78`/`0x90`/`0xA5`/`0xF0`/`0x168`).

### 64-bit port (status: CONFIRMED — both builds)

The structure is **identical** to 32-bit. In the 64-bit app-init function
`FUN_1800020f0` (20260324):

```
180002637: MOV EDX,[RSP+0x40]
18000263b: DEC EDX
18000263d: MOV dword ptr [RSP+0x6c], 0x3C   ; op C7 44 24 6C 3C 00 00 00; default 60
180002645: JNZ 0x18000264f
180002647: MOV dword ptr [RSP+0x6c], 0x4B   ; alt = 75
```

| Build | opcode VA | imm32 VA | imm32 **file offset** |
|---|---|---|---|
| 20260324 | `0x18000263D` | `0x180002641` | **`0x1A41`** |
| 20260526 | `0x1800025BD` | `0x1800025C1` | **`0x9C1`** |

(`.text` file off = VA − `0xC00`.) Poke the imm32 with the desired value:
60/120/144/165/240/360 = `0x3C`/`0x78`/`0x90`/`0xA5`/`0xF0`/`0x168`.

**AOB signature (both builds):**
`C7 44 24 ?? 3C 00 00 00 75 08 C7 44 24 ?? 4B 00 00 00` — the FPS value is the
imm32 at offset +4 (after the first `C7 44 24 ??`). Stable across both 64-bit
builds; resolve the immediate's address from the match and write the chosen value.

### What the value actually is, and where the side effects come from

The `0x3C` is **not** a logic-tick rate — it's the **display/refresh target** for the
screen-graph. The init sequence in `FUN_1800020f0` (= `Application::onBoot`):

```c
(*arkMDXGetMachineType)(&machineType);    // DAT_1806eb2a8
displayTarget = 0x3c;                      // 60   (struct field +0x1C — see correction below)
if (machineType == 1) displayTarget = 0x4b; // 75
... struct field +0x1C = displayTarget ...
FUN_1801f0030(&struct);                    // forwards struct to FUN_1801eda10 (see correction)
```

So the stock value is itself **cabinet-selected** (`MachineType == 1` ⇒ 75 Hz,
else 60). The patch overrides that.

> **⚠️ RE corrections (2026-06-28, fresh re-verification during implementation).** The
> original notes above had two structural errors, fixed here:
> 1. **The display target lands at struct `+0x1C`, not `+0x14`.** `[RSP+0x6c]` with the
>    struct base at `[RSP+0x50]` ⇒ +0x1C. (`+0x14`/`+0x18` are *adjacent* fields in the
>    same display block.)
> 2. **`FUN_1801f0030` does NOT read the struct.** It forwards `RCX` to **`FUN_1801eda10`**,
>    which copies `struct+0x1C` into the global **`DAT_1806ea488`**. That global's **sole
>    reader** is **`FUN_1801edd20` ("Renderer:initGs")**, at boot, feeding **D3D device
>    creation**. So the target is **consumed exactly once at boot and never re-read per
>    frame** — there is no per-frame display-target read to hook for a live change.

**Why animations speed up.** The engine *is* fundamentally delta-time based. Every
frame, the tick function `FUN_18020e230` computes:

```c
now      = arkGetTickTime();
dt_meas  = (now - prev) * tick_scale;          // real elapsed seconds
dt       = min(dt_meas, CAP);                   // CAP = DAT_18045f114 / 59.94 ≈ 33.4 ms
DAT_1806ea714 = dt;                             // global delta-time (read by ~120 fns)
```

`DAT_1806ea714` is the global frame delta. Anything that scales motion by it stays
correct at any FPS (this is why **gameplay arrow scroll is smooth and correct** when
you raise the target — exactly the useful case). The "menus/animations run too fast"
symptom comes from code paths that advance **per-tick by a fixed step** (frame-counted)
instead of multiplying by `DAT_1806ea714` — menu scrollers, some AFP timeline advances,
etc. Because the **logic tick is driven 1:1 by the render loop** here, raising the
display rate makes those frame-counted animations step more often ⇒ faster.

The dt **clamp** `CAP = DAT_18045f114/59.94` (`DAT_18045f114 = 2`, set in init) also
matters: it caps the per-frame delta at ~2 vanilla-frames, so lowering FPS doesn't let
dt-based motion teleport — but it does nothing for the frame-counted paths.

### Expanding the patch — static config option (IMPLEMENTED as the `fps-unlock` mod)

> **⚠️ Conclusion revised after fresh RE (2026-06-28).** The original recommendation below
> (a per-scene auto-switch) was **dropped**, for two reasons the re-verification established:
>
> 1. **Per-scene live switching is infeasible via this lever.** The display target is
>    consumed **once at boot** to create the D3D device (see the RE correction above) — it is
>    *not* re-read each frame, so there is no "live re-write" to do on scene transition.
>    Changing it at runtime would require tearing down and recreating the device.
> 2. **DDR World does not appear to exhibit the menu-animation speedup.** The engine is
>    overwhelmingly delta-time based (`DAT_1806ea714`, ~100 readers, per-frame clamp); a
>    sampled menu/effect animation path scales by dt. A live test of the World FPS-unlock hex
>    edit showed **no** menu speedup. The speedup is real on **older** DDR versions (which the
>    original notes generalized from), but does not carry to World.
>
> **What shipped:** a static, cabinet-wide FPS value, AOB-byte-patched into the imm32 during
> the DLL's `early_apply` boot phase (before `onBoot` reads it), with a config-defined preset
> list selectable from an `Enum` row in the mod overlay. Applies on next launch. See
> `.agents/planning/20260627-fps-unlock/` for the full design/research.

---

The original (superseded) analysis, kept for reference:

A bigger byte patch can't fix the speedup *on engines that have it* (the value is global and
the offending animations ignore dt). The proposed design was a **hook-DLL mod that varies the
display target by scene** via `scene_manager` (high in gameplay, 60 in menus). This rested on
the display target being **re-read each frame** — which the fresh RE disproved for World (it's
latched once at boot), and which is moot anyway since World shows no speedup.

---

## Hack 6 — Timing preset selection (new mod idea, not in `patches.js`)

Not a `patches.js` entry — this is the mod idea raised during research: let the user
pick which **timing preset** the game uses, instead of having it dictated by detected
hardware. Documented here because it's tightly coupled to Hack 4 (the timing-offset
record) and confirmed reversible.

### How the game picks a preset

The timing-offset record builder `FUN_180012f50` (Hack 4) takes a **preset index 0–9**.
That index is chosen by the selector **`FUN_180012e50`** (32-bit analog `FUN_1000f520`,
called from timing-init `FUN_18002bbd0`), which queries two hardware properties and maps
the pair to an index:

```c
arkMDXGetMachineType(&machineType);   // arkmdxbio2.dll export, via DAT_1806eb2a8
arkMDXGetPCType(&pcType);             // arkmdxbio2.dll export, via DAT_1806eb2b0
// returns preset index:
```

| MachineType | PCType | preset index |
|---|---|---|
| 0 or 1 | 0 or 1 | **0** |
| 0 or 1 | 2 | 3 |
| 2 | 0 or 1 | 1 |
| 2 | 2 | 4 |
| 3 | 2 | 5 |
| 4 | 2 | 6 |
| 4 | 3 | 7 |
| 4 | 4 | 8 |
| anything else | | **2** (fallback) |

So presets **are** hardware/cabinet-keyed, confirming the premise: different machine +
PC-board generations get different SOUND/INPUT/RENDER/BOMB offset tunings (and all keep
`HIGH_PRECISION_INPUT = 1`). The per-preset offset values are in the inline table in
`FUN_180012f50` (Hack 4 documents record 0; records 1–9 are the other inline rows).

### Where MachineType / PCType come from (arkmdxbio2.dll)

Both are thin exported shims that dispatch through the I/O singleton
`SingletonArkMDXIO::mdxIO` (`FUN_1800d2860`) to vtable slots:

| Export | arkmdxbio2 addr (20260324) | vtable slot |
|---|---|---|
| `arkMDXGetPCType` | `0x1800d2fb0` | `(*mdxIO + 0x440)` |
| `arkMDXGetMachineType` | `0x1800d2fe0` | `(*mdxIO + 0x438)` |

They write the result through an out-pointer (`param_1`), like the other ark getters.
The concrete value is whatever the live I/O backend reports (real hardware probe; under
spice2x / unofficial hardware it's whatever the driver emulates). **This is exactly why
a "pick your preset" mod is useful** — home/unofficial setups report a fixed pair and
get one preset, but players often prefer a different cabinet generation's timing.

### Recommended mod design — intercept the selector, not the hardware

Don't spoof `arkMDXGetMachineType` / `arkMDXGetPCType` (touches arkmdxbio2, varies by
driver, and the mapping is indirect). Instead **hook the game-side selector
`FUN_180012e50` and force its return value** to the user-chosen index `0..9`:

- Resolve `FUN_180012e50` by AOB (or via the `arkMDXGetMachineType`/`arkMDXGetPCType`
  call pair + the constant compare ladder `0,1,2,3,4` as an anchor).
- Detour it; if the mod is enabled, `return chosen_index;` (skip the original entirely).
  If disabled, call through.
- Expose a per-cabinet dropdown in the mod menu. Suggested labels (map index → the
  hardware pair that natively yields it): 0 = "Gen-0/1 board", 1 = "Type-2 / old PC",
  4 = "Type-2 / new PC", 5 = "Type-3", 6/7/8 = "Type-4 (white/gold/…)", 2 = "fallback".
  Refine the labels by cross-referencing each preset's offset values (record N in
  `FUN_180012f50`) against known cabinet feel.
- This is hardware-agnostic, version-robust (AOB), and one detour with no allocator or
  render-thread concerns (it runs once at timing-init).

> Caveat: the selector runs **once** at boot/timing-init. Changing the choice at runtime
> requires re-running the publish path (`FUN_18002bbd0`) or writing the four published
> offset values + `HIGH_PRECISION_INPUT` directly via the config-map setter (Hack 4).
> For a boot-time preference (set in config, applied at startup) the simple selector
> hook is sufficient.

---

## Cross-version compatibility summary

All five `patches.js` hacks (plus the timing-offset expansion and the preset-selector
idea) were verified against **both** 64-bit builds (20260324 and 20260526). Every anchor
(string, function structure, record layout) is present and structurally identical in
both; only absolute addresses differ.

| Hack | 64-bit anchor (20260324) | Portable as data patch? | Recommended hook-DLL approach |
|---|---|---|---|
| 1 Mute announcer | dispatcher `FUN_180055a50`; path str `"data/sound/win/voice.xwb"` | `voice.xwb` byte: **yes**. Entry-guard: fragile (re-derive per build) | Hook `FUN_180055a50` early-return, or the portable `voice.xwb` byte poke |
| 2 Center arrows | builder `FUN_18006c230`; setter `FUN_18006f5d0` | No (was code caves; ABI-specific) | Post-hook `FUN_18006f5d0`; for 1P rewrite X of `arrow_raw`/`arrow`/`freeze_judge`; force `double` lane branch + selector |
| 3 Hide bottom text | renderer `FUN_180009680`; block @ rdata | **Yes** (333-byte block, identical layout) | **Detour `FUN_180009680` → `return` (does only corner-text; safe to no-op)**; or suppress per-object draws to keep the network-status line |
| 4 Timing offsets (×4 + bool) | init `FUN_18002bbd0`; builder `FUN_180012f50`; rdata table | **Yes** (imm32 / rdata constants) | AOB-scan builder; patch defaults, OR re-set via config-map setter / write live state (`DAT_1806ebc70+0x1261` for HIGH_PRECISION_INPUT) |
| 5 FPS target | app-init `FUN_1800020f0` = `Application::onBoot` (display target imm32 at struct+0x1C → global `DAT_1806ea488`, read once by `Renderer:initGs`) | **Yes** (single imm32) for a static value | **IMPLEMENTED (`fps-unlock` mod):** AOB-scan `C7 44 24 ?? 3C 00 00 00 75 08 C7 44 24 ?? 4B 00 00 00`, byte-patch the imm32 (match+4) in `early_apply`. Consumed once at boot → static value, applies next launch. Per-scene switching dropped (infeasible + no World speedup). |
| 6 Timing preset select (new) | selector `FUN_180012e50`; `arkMDXGetMachineType`/`PCType` (arkmdxbio2) | n/a (logic, not data) | Detour `FUN_180012e50`, force return index `0..9` from config — hardware-agnostic, runs once at init |

### Confirmed 64-bit patch offsets (quick reference)

| Hack / field | 20260324 file off | 20260526 file off |
|---|---|---|
| 1 `voice.xwb` `v`-byte (`0x76`→`0x62`) | `0x358CAF` | `0x35B6EF` |
| 3 bottom-text block (333 bytes → `0x00`) | `0x2DCD18` | `0x2DF718` |
| 4 `SOUND_OFFSET` default (rdata rec0 i32) | `0x357360` | `0x359D50` |
| 4 `INPUT_OFFSET` default (+4) | `0x357364` | `0x359D54` |
| 4 `RENDER_OFFSET` default (+8) | `0x357368` | `0x359D58` |
| 4 `BOMB_FRAME_OFFSET` default (+0xC) | `0x35736C` | `0x359D5C` |
| 5 FPS target imm32 (`0x3C`) | `0x1A41` | `0x9C1` |

Hacks 1 (entry-guard) and 2 are intentionally omitted from the byte-offset table —
they are recommended as AOB-resolved Rust hooks rather than fixed-offset pokes.

> Verified: 2026-06-12. Re-derive all addresses via the named anchors on any future
> build; do not treat these absolute offsets as version-stable.
