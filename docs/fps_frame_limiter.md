# FPS Unlock — sub-60 targets and the frame limiter

Mod: `src/mods/fps_unlock/` (`mod.rs` policy, `pacing.rs` pure logic +
`scripts/validate_fps_unlock.sh`, `limiter.rs` the detour). Status
(2026-09-27): implemented; host tests, `cargo check`, the five-build
signature sweep and the post-match shape diff pass. **Cabinet validation
still required** (checklist at the end).

Addresses are file-relative to `gamemdx.dll` base `0x180000000`, build
**20260915** unless noted. Nothing in shipped code hard-codes them.

## 1. Why a sub-60 value in the refresh imm does nothing (or worse)

The `fps_target_imm32` value (onBoot `MOV [RSP+0x6c],0x3C`) travels
`onBoot` → display init `FUN_1801efc80` (`DAT_1806f1508 = struct+0x1C`) →
`Renderer:initGs` `FUN_1801eff90` (display descriptor `+0x28`) → the gd
device layer `FUN_180235a40`, which fills `D3DPRESENT_PARAMETERS`:

```
Windowed                   = (DAT_1806f31c0 == 0)
FullScreen_RefreshRateInHz = windowed ? 0 : desc+0x28      ; 180235aaa..180235ace
PresentationInterval       = D3DPRESENT_INTERVAL_ONE
```

So the value is a **fullscreen display-mode request**, never a frame cap. The
frame rate is whatever vsync paces `Present` at:

- **Windowed** (spice2x `-w`, the CrossOver dev loop): the game forces the
  refresh to 0, and so does spice2x's D3D9 hook. The game vsyncs to the
  desktop. On a 120 Hz ProMotion panel every preset renders at 120 fps,
  including 60 and 144. Confirmed in spice2x's `log.txt` on a 30-selected
  boot: `Windowed: 1 … FullScreen_RefreshRateInHz: 0, PresentationInterval:
  D3DPRESENT_INTERVAL_ONE`, while the mod's own line showed the patch landed
  (`graphics_init display struct … fps=30`).
- **Fullscreen:** D3D9 requires the rate to be an enumerated display mode.
  No monitor offers 20 or 30 Hz, so all three `CreateDevice` attempts in
  `FUN_180238c60` (HW VP, SW VP, REF) fail. It returns 4 and no renderer
  comes up. The selection is persisted, so the operator could not get back
  into the menu to undo it.

Conclusion: a target **below 60 must never be written to the imm**. It keeps
the stock 60 Hz request and needs a real frame limiter. Targets ≥ 60 keep the
original refresh-request behaviour unchanged (`pacing::mode_for`).

## 2. The frame loop and the limiter seam

Per-frame application tick `FUN_180003070` (20260915):

```
18000308d  CMP byte [DAT_180cf3329],0     ; first frame already done?
           JZ  +6
           CALL FUN_1801f2a80             ; frame_begin  (frames >= 2)
           NOP
           CALL FUN_180210b80             ; frame_dt: measure + clamp dt
           NOP
           INC dword [DAT_1806f2044]      ; frame counter
           ...  input, actor update 0x102, layer dispatch, 0x103 ...
           (first frame only: CALL frame_begin here instead, then set the flag)
```

`frame_begin` (`FUN_1801f2a80`; `FUN_1801f3410` on 20260825, `FUN_1801f2c00`
on 20260721 — the "submit+reset" of `overlay_draw_research.md`) drains the GPU
executor (wait until the previous stream was executed and presented), resets
the per-frame lists and submits the previous frame's command stream. Its
single caller is the app tick.

**The limiter is a post-original detour on `frame_begin`:**

1. The original runs first, so the previous frame is already handed to the
   executor and heading to `Present` while we wait. The GPU is never held
   back.
2. Then `pacing::Pacer` waits until the next deadline on a fixed grid of
   `1/target`: coarse `thread::sleep` to 2 ms before it, then `yield_now`.
   `timeBeginPeriod(1)` is requested at install.
3. Input, `frame_dt` and the whole update run right after the wait, so the
   limiter adds no input-to-present latency beyond the lower rate itself.

The grid advances exactly one period per frame, so the average rate is exact
under jitter. Up to one period late keeps the grid; a longer stall re-anchors
it, so there is no burst after a loading hitch.

With vsync still on (`INTERVAL_ONE`), each frame is presented at the first
vblank after submission. At 60 Hz, 30 and 20 fps are every 2nd and 3rd
vblank; at 120 Hz, every 4th and 6th. On a refresh that the target doesn't
divide evenly (144 Hz), frame intervals alternate, which is inherent. The QPC
grid is not phase-locked to vblank: a display running at 59.94 Hz slips one
vblank every ~30 s at 30 fps. That is the known limitation of this design. A
vblank-locked pacer is future work.

## 3. The per-frame dt clamp

`frame_dt` (`FUN_180210b80`):

```
dt_raw = (now - prev) * tick_scale
MOVSS XMM1,[DAT_1806f178c]         ; the clamp   (disp32 at fn+0x44)
COMISS XMM0,XMM1
MOVSS [DAT_1806f1798],XMM0         ; raw dt
JBE   +…
MOVSS [DAT_1806f1794],XMM1         ; dt = clamp  (when raw > clamp)
```

`DAT_1806f178c` is written only in `onBoot` (and `FUN_180210a70`, called only
from `onBoot`) as `2.0 / 59.94` ≈ **33.37 ms**: two stock frames. A 50 ms
(20 fps) frame would be clipped every frame, so every dt-driven animation
(~100 readers of `DAT_1806f1794`) would run at 2/3 speed. A 33.33 ms (30 fps)
frame sits right at the edge.

While pacing, the limiter keeps the clamp at
`pacing::limiter_dt_clamp(target) = max(stock, 2 / target)`. That is the same
two-frame headroom at the limiter's own rate. It captures the stock value on
the first paced frame and restores it when the mod is toggled off.

## 4. Anchors

| Name | Kind | Pattern / derivation |
|---|---|---|
| `app_tick_frame_begin_site` | AOB | `80 3D ?? ?? ?? ?? 00 74 06 E8 ?? ?? ?? ?? 90 E8 ?? ?? ?? ?? 90 FF 05 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? E8` |
| `frame_begin` | derived | CALL target at match+9; prologue gate `40 53 48 83 EC 20 E8 … E8 … 80 3D … 00 75 05 E8` |
| `frame_dt_clamp` | derived | CALL target at match+15 must match the full dt-measure + clamp shape; global = RIP disp32 at +0x44 |

`SignatureStore::frame_limiter_anchors` computes both from the linear hit
(usable in `early_apply`). `derive_frame_limiter` publishes them for the boot
log / sweep (read-only).

| Build | site | frame_begin | frame_dt_clamp |
|---|---:|---:|---:|
| 20250805 | +0x308D | +0x1DAF10 | +0x6B44BC |
| 20260224 | +0x306D | +0x1DF0E0 | +0x6C82BC |
| 20260721 | +0x303D | +0x1F2C00 | +0x6F178C |
| 20260825 | +0x301D | +0x1F3410 | +0x6F178C |
| 20260915 | +0x308D | +0x1F2A80 | +0x6F178C |

The AOB is unique on 20250805/20260721/20260825/20260915 (Ghidra) and
resolves on all five in `validate_signatures.sh`. `shape_diff.py`:
`app_tick_frame_begin_site` and `frame_begin` are byte-shape-identical
through 0x60 bytes on every build.

## 5. What runs for which target

| Target | Refresh imm | Limiter detour | dt-clamp write |
|---|---|---|---|
| 60 | untouched | not installed | none |
| > 60 | patched (unchanged behaviour) | not installed | none |
| < 60 | untouched (stock 60 Hz request) | installed in `early_apply` | while pacing |

The only thing the new code adds for a ≥ 60 boot is the read-only anchor
lookup in `resolve_derived`, the same as any other signature. The sub-60
selection is read at boot; changes apply on the next launch, like the
refresh value. A menu toggle OFF suspends pacing live and restores the
clamp. `custom_resolution`'s fullscreen display-mode fail-safe now matches
the refresh actually requested (`fps_unlock::requested_refresh_hz`: stock 60
for sub-60 targets) instead of the raw selection.

Normalization floor raised from 1 to 10 fps (`pacing::FPS_MIN`). Values below
10 were never usable, and they would put the raised clamp at > 0.2 s.

## 6. Cabinet checklist

- Windowed (`-w`) and fullscreen, select 30 and then 20. Boot log should show
  `FpsUnlock: early_apply armed the Nfps frame limiter`, `frame limiter
  installed`, and `first paced frame -- game dt clamp 33.37 ms -> …`.
  spice2x's D3D9 line must still show the stock 60 Hz request in fullscreen.
- Measure the actual frame rate (spice2x overlay or a capture), and look for
  evenly spaced frames.
- dt-driven animation (menus, attract, background dancers, gameplay scroll)
  runs at normal wall-clock speed at 20 fps. Watch for any frame-counted
  animation that now runs slow; R3 of the original research found World to
  be dt-based, but that was only sampled.
- **Gameplay judgement at 20 and 30 on real pads (highest-risk item).**
  Judgement uses IO-layer press timestamps (`input_polling_research.md`
  §6.3), so `T` cancels — but the ark's per-frame press derivation walks only
  the newest **7** ring samples (§4.2), ≈15–35 ms of history depending on the
  poll cadence. That covers a 60 fps frame (16.7 ms) but not reliably a
  30 fps (33.3 ms) one, and never a 20 fps (50 ms) one. A press edge older
  than the window gets the oldest visible sample's time (stamped late, so
  judged late), and a tap entirely inside the uncovered part of the frame is
  never seen. spice2x emulated IO pads its ring at 125 Hz below 120 fps
  (7 × 8 ms = 56 ms), so the CrossOver dev loop will NOT show this; test on a
  stock-IO (bootstrap-mode) cabinet. If confirmed, sub-60 targets are for
  attract/menus/low-power use, not play.
- 60 / 120 / 144: behaviour identical to before. The only new log lines are
  the anchor lookups (`[+] app_tick_frame_begin_site`, `[+] frame_begin`,
  `[+] frame_dt_clamp`); no `FpsUnlock` limiter lines.
- Toggle the mod OFF in the menu while limited: the frame rate returns to
  vsync-paced immediately.
