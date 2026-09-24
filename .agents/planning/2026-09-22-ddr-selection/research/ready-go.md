# Research: legacy READY / HERE WE GO and World's intro (Step 4, 2026-09-23)

Builds: A3 `gamemdx_20240402` (spec), World 20260825 (addresses), sweep over
20250805 / 20260224 / 20260721 / 20260825 / 20260915. File-relative to
`0x180000000`.

## 1. A3 ReadyGoActor (deleted in World)

ctor `FUN_180042000` (0xA8 bytes, `+0x58` = LayoutActor desc, `+0xA0` =
no-HERE flag), init `FUN_1800420d0`, update `FUN_1800423d0`, finalize
`FUN_180042520`, msg `FUN_180042570`. Created by DPS step 0
(`FUN_180039650`) once the LayoutActor is ready, when `!DPS+0x12C` (demo
flag — attract, never armed by DDR SELECTION) or the lesson flag.

- init: record `dance_message` → skin N ⇒ package `dance_message000N`; clips
  `00_ready` (`00_howtoplay` in lesson mode) and `00_here` (unless no-HERE),
  CMovieClip `SetView(5)` (group, vt+0xE8) / `SetPriority(5)` (vt+0xE0),
  `FUN_180100410` (SD-cabinet scale/position only — machine type 0/1), play
  rate 0, invisible. Identity transform on HD.
- msg `0x104A` READY (state 0): play `00_ready` rate 1 + visible; send
  `0x100D` to the ShutterActor subtree (= World `0x100c`); state 1.
- msg `0x104B` HERE (state 1): goto-and-play `out` on `00_ready` (op
  `0xF09`), play `00_here`; **skin 1** plays `ACT3_1` (final stage
  `ACT4_2`, `FUN_180123a20`) from the voice bank, pan 0; state 2.
- msg `0x104C` OUT (state 2): goto-and-play `out` on `00_here` (or
  `00_ready`); state 3.
- update, state 3: both clips at their `end` label frame ⇒ the actor
  flags itself dead (finalize releases both clips).

Final-stage test (`FUN_180123a20`, non-course): `stage+1 == override` or
(`stage+1 != override` ∧ `stage == max_stage`); event modes 1/2 ⇒ false.

## 2. dance_message000N clips (World ships them byte-identical)

| clip | labels | embedded sounds |
|---|---|---|
| `00_ready` | in 0, loop 10, loop_end 63 (→ gotoAndPlay loop), out 64, end 80 (stop) | skin 1 `ACT2_1` (f1) + `2nd_BIG2` (f2); 2 `sn2_mst09` + `2nd_KANSEI_B`; 3 `sn2_mst09`; 4–5 `vo_ingame_ready` |
| `00_here` | in 0, loop 33 (f63 → loop), out 64, end 80 | none |
| `00_howtoplay` | in 0, loop 33, loop_end 63, out 64, end 80 | none |

## 3. World's intro (20260825)

- ControlMessageActor (`FUN_180056010`, send `FUN_1800561f0`, broadcast
  from the root): on each tick `0x1045` fires `0x1047` READY → StackStep 1,
  `0x1048` HERE → 2, `0x1049` OUT → 3 (then `0x104A`/`0x104B` = 4/5, the end
  cascade). Several in one tick when thresholds already passed. Nothing
  resets the step on an in-place `song_reset` (World does not replay its
  intro either).
- ShutterActor update `FUN_180033f60`: state 4 covered → DPS step 5 sends
  `0x1008` → state 5 goto `stage_out` + **code `vo_ingame_ready`** (slot-3
  bank, inlined, `@0x1800346f8`) → state 6. State 7 (the `0x100c` drain)
  plays `vo_ingame_ready` again (`@0x1800348ba`) only when the reveal never
  ran (current frame < `stage_out`), then label `out` → 8 (wait
  `max(out_end, end)`) → release.
- `shutter_play` timeline (2026-09-23 dump): the `stage_out` reveal runs
  325–344 (wipe shapes 324–344; `jacket_usr`, `info_1p/2p_usr`, shadows,
  logo removed at 344), World's own `ready` sprite (READY?) is placed at
  **350** and removed at 477, flash shapes 338–363. A legacy dismiss must
  land in 344..349 (`jacket_usr` absence is the runtime signal).
- `shutter_play` (common_shutter_v3) root labels: in_stage 0, loop 14,
  loop_stage 198, stage_out 325, **ready_loop 408**, ready_out 458, end 481
  (sub-sprites also carry ready_here / here_out). No `out` / `out_end`
  label: every drain relies on the clip reaching `end` (hence
  `shutter::unblock_drain`). On the maintainer's CrossOver install the
  runtime goto calls report every label missing (known, QR §14.1).
- Shutter msg `0x1047`: vestigial frame reads; `0x1048`: goto `ready_out`
  if the clip is before it (World's own HERE WE GO); `0x100c`: state 7 iff
  active kind == stage kind.
- DPS step 5 dwell: `MOVSS XMM0,[RSI+0x130]; COMISS XMM0,[rip 5.0f]; JB`
  (`@0x1800588a4`), timer at DPS+0x130 on all five builds.

## 4. Implementation choices (Step 4)

- Triggers: poll the highest CMA StackStep per frame (no detour, ≤ 1 frame
  late) — `song_reset::intro_cascade_step`.
- Clips: raw AFP layers from the LayoutActor-owned `dance_message000N`
  (`bm2d_package::lookup_unowned`, re-validated every frame), group 5 /
  priority 5, destroyed at the GAMEPLAY exit scene change (before
  `createNextSequence` / `installSequence` tear the DPS down), on a package
  change, or once both clips ended.
- World panel: dismissed (`0x100c` + drain unblock) once `jacket_usr` left
  (rev 2; rev 1 waited for `ready_loop` and showed World's READY? ~1 s).
  Backstops: World's `ready` sprite placed, `ready_loop` reached, READY.
- `vo_ingame_ready`: both inlined plays' null-bank `JZ` → `JMP`
  (`code_se`, derived `ddr_sel_vo_ready_jz_0/1`) while the legacy clips
  exist.
- Dwell: DPS+0x130 seeding (`ddr_sel_dps_ready_dwell` →
  `ddr_sel_dps_ready_timer_off`) implemented but OFF until Step 5 — the
  dwell is where World shows its panel.
- Not done: lesson / `00_howtoplay` (attract only), SD-cabinet clip
  scaling (A3 `FUN_180100410`), World's stage voice and the panel itself
  (Step 5).
