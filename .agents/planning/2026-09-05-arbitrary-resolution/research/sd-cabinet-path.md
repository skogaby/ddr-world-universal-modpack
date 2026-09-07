# SD (4:3) cabinet path in DDR World — what "discontinued" means in the binary

Ghidra, build 20260825 (`gamemdx_20260825.dll`; MD5-identical to the local
install's live binary), cross-checked with `arkmdxbio2_20260721.dll`. Names are
that build's `FUN_`/`DAT_`. Complements `docs/arbitrary_resolution_research.md`
§2/§5 (20260616 names).

## 1. Machine type → HD flag (onBoot)

`Application::onBoot` = `FUN_180002060`. The ark import slot is `DAT_1806f2330`
(`arkMDXGetMachineType`, resolved by the 0x144-entry GetProcAddress table in
`FUN_1800042c0`); `DAT_1806f2338` = `arkMDXGetPCType`.

```c
(*DAT_1806f2330)(&mt);  fps = 0x3c; if (mt == 1) fps = 0x4b;          // +0x1C
(*DAT_1806f2330)(&mt);  hd  = (mt < 0 || mt > 1);                      // +0x12  ← the HD flag
(*DAT_1806f2338)(&pc);  if (pc && 1 < pc && pc < 5 && hd) aa = 3;      // +0x18
FUN_1801f2c30(&display_struct);                                        // graphics init
```

Machine codes: `mt ∈ {0,1}` = SD-class cabinet (75 Hz CRT when `mt == 1`),
`2` = HD white, `3` = HD refurb, `4` = GOLD (bio2). The HD-flag store is
`TEST EDX,EDX; JS; CMP EDX,1; JG; XOR AL,AL / MOV AL,1; MOV [RSP+0x62],AL`
(struct at `RSP+0x50` ⇒ `+0x12`); AOB
`85 D2 78 ?? 83 FA 01 7F 04 32 C0 EB 02 B0 01 88 44 24 ??` is unique on all
four builds (`prototypes/aob_sweep/REPORT.md`).

## 2. Every gamemdx consumer of machine type (20260825)

Direct `DAT_1806f2330` readers (15):

| site | role | SD behaviour |
|---|---|---|
| `FUN_180002060` ×4 | onBoot (above) | HD flag 0 → 640×480 back-buffer; 75 Hz for type 1 |
| `FUN_180013910` | `isHD()` helper | 2 callers, both in `FUN_18007deb0` |
| `FUN_180013690` | cabinet-class enum `{0,1}×pc → 0/3; 2 → 1/4; 3 → 5; 4 → 6/7/8` | **69 call sites** — lights path selection (`FUN_18000fcf0` dispatcher, see `docs/smx_hardware_research.md`), boot-time IO class (`FUN_1800013b0`, class 6/7/8 ⇒ GOLD init), entry-flow, satellite/tape output |
| `FUN_18007deb0` | attract advertise loader | SD ⇒ prefers `demo_advertise_%02d_sd%s.arc` / `demo_advertise_%02d_sd%s` layer, falls back to the HD arc when the SD one is missing |
| `FUN_1800aecb0`, `FUN_180082f40`, `FUN_180007250` | UI text objects | SD ⇒ `vfunc+0xC8(scale_const)` + `vfunc+0x38(x, y)` — three SD-specific text scale/position tweaks |
| `FUN_1801ae260` | one more text object | same shape |
| `FUN_1800066d0` ×3, `FUN_1800092d0` | TEST-menu / debug drawing | percentage-based, resolution-agnostic |

**No boot gate.** There is no "unsupported cabinet" string or branch; World on
an SD machine type boots and renders. What was discontinued is the **data**:
the local install has only six `demo_advertise_*_sd_*.arc` files left and no
other `_sd` asset; every other SD-variant lookup falls back to HD art. So an SD
cabinet today sees the 16:9 canvas through the engine's present-path crop.

## 3. The present path's SD behaviour (per game state, not an operator setting)

`FUN_1801f5010(this, mode)` (= research `FUN_1801f3f60`): short-circuits to a
1:1 POINT copy when `screen_w == 1280`; otherwise `mode 1` = 960-px centre
crop (`src = {0xA0, 0, 0x460, 720}`, LINEAR) — the crop rect is hard-coded for
a **1280-wide source**; `mode 0` = width-fit letterbox (`scaled_h =
screen_w/1280·720`, centred). Callers:

- `FUN_18002e3b0` ×13 — the entry-flow / scene switcher: **mode 1 (crop)** on
  every game-state transition, one site with a dynamic `mode = EDI+1`.
- `FUN_18002da60` — **mode 0 (letterbox)**: TEST-menu entry (the full canvas is
  needed to read the menu).
- `FUN_1801f4e90` — the present-chain ctor (default).

So a stock SD cabinet plays cropped (outer 160 canvas px per side lost) and
shows the TEST menu letterboxed. The `center_arrows_single` situation: in
single play the P1 lane (canvas x ≈ 128..512) loses its left 32 px under the
crop; versus P2 (768..1152) loses its right 32 px; doubles (256..1024) fits.

## 4. The ark side (`arkmdxbio2`)

- `arkMDXGetMachineType` (export @ `1800d2fe0`) → `MdxHWIO` vtable `+0x438` →
  `FUN_1800c9320`: returns **1 when `MdxHWIO+0x5EE` is set** ("force SD"),
  else the backend getter. `+0x5EE` is written by the vtable setter
  `FUN_1800c9d50(this, bytes)` (`+0x5EC=1, +0x5ED..+0x5F6 ← bytes[1..5]`) — a
  device/config message handler, no direct caller (vtable-dispatched).
- `arkmdxp3.dll` / `arkmdxp4.dll` ship alongside `arkmdxbio2.dll` in World's
  `modules/` — P3IO cabinets report cabinet type through P3IO cmd `0x27`
  (spice2x `-o` answers `0x00` = SD there; it does nothing for bio2).
- The modpack already detours the two exports (`smx_hardware/cabinet_force.rs`)
  to force GOLD — a second detour on the same export would violate the
  one-detour rule; any machine-type override must go through that module.

## 5. Consequence for the design (D16)

Two viable SD mechanisms:

| | (a) mod-owned 4:3 output via the present path | (b) spoof machine type ∈ {0,1} |
|---|---|---|
| what changes | back-buffer/display = 640×480 (any 4:3), render stays 1280×720, present crop/letterbox chosen by config | everything in §2: HD byte, 75 Hz (type 1), `_sd` advertise arcs, 4 text tweaks, **lights/IO class → SD satellite path** |
| hardware | independent of the real cabinet | wrong on non-SD hardware (lights); redundant on real SD hardware (already reports SD) |
| collisions | none | `cabinet_force` detour, spice2x `-o` |
| residual SD-ness lost | the 4 text tweaks and the 6 SD advertise arcs | — |

(a) also falls out of D2 (render ≠ output) for free, and leaves the HD flag
alone — the mod forces the HD branch and writes the configured output dims
into it, so a real SD cabinet (HD flag 0) and a spoofed one behave
identically. The 960 crop rect must be scaled by `render_w/1280` if render is
ever ≠ 1280 wide — simplest rule: **4:3 output requires render = 1280×720**
(the canvas is 16:9-logical; there is nothing to gain from a bigger render on
a 640×480 CRT). Recommendation: **(a)**; (b) only as an optional
`sd_spoof_machine_type` dev switch if the SD text tweaks turn out to matter,
and only through `cabinet_force`.

## 6. Open (cabinet) questions

- Does the maintainer's SD cabinet run `arkmdxp3` (P3IO) or `bio2`? Decides
  whether the real hardware already reports SD (and thus whether anything
  beyond (a) is needed for it).
- What does stock World look like on it today — the §3 crop as predicted?
