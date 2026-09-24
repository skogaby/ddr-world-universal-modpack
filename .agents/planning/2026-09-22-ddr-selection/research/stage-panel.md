# Research: the legacy stage panel, era cut-in and stage voices (Step 5, 2026-09-23; rev 2 after cabinet run #1)

Builds: A3 `gamemdx_20240402` (spec), World 20260825 (addresses), sweep over
20250805 / 20260224 / 20260721 / 20260825 / 20260915. File-relative to
`0x180000000`. Data: the stock World install (every A3 panel arc ships
byte-identical), unpacked with `scripts/unpack_arc.py` + `ifstools`.

## 1. World's ShutterActor (20260825)

RTTI `.?AVShutterActor@shutter@common@sequence@@`, vtable `0x18035ebc8`:
slot 6 = onUpdate `FUN_180033f60`, slot 8 = onMessage `FUN_180035170`
(slot 6 on every build: 20250805 `0x180033d00`, 20260224 `0x180033570`,
20260721 `0x180033a50`, 20260915 `0x180034540`). Singleton `DAT_1806f2d48`.
Nothing in the repo detours either.

### 1.1 Kind art (`FUN_180035420(this, kind)`, called from update state 0 with EDX = pending kind)

Table rows (`0x18035e040`, 0x40 stride = 8 pointers): `{pkg, root, SE in,
SE out, voice in, voice out, map3 in, map3 out}`. Mode `GameWork+0x1c` 10 /
9 selects the dan / galaxy-brave tables instead (`0x1804800b0`,
`0x18035e280`). Row 3 = `{NULL, "shutter_play", "se_start_game", "", …}`.

* `pkg == NULL` → `FUN_180035890(closure, *(*DAT_1806f2d70+0xb0) /*the
  scene's common_shutter*/, row)`: `FUN_180257920` creates the CMovieClip,
  `SetView(5)`, `SetPriority(3)`, SD scaling `FUN_1801ae260`, play rate 0,
  invisible, stored in the per-kind `shared_ptr` at `+0x88 + kind*0x10`;
  the row's six sound strings go into per-kind label maps `+0x3d8` (SE,
  slot 2), `+0x3f8` (voice, slot 3), `+0x418` (slot 0) that
  `FUN_180034d10(kind, "in"/"out")` plays.
* `pkg != NULL` (World uses it for `event_shutter_brave`) → the async
  named-package loader at `+0x118`: dir lambda4 `"bm2d"`, name lambda5
  `"%s"` (the name verbatim), done-callback lambda6 → looks the package up
  in the bm2d registry (asserts if missing) → the same `FUN_180035890`.
  Resolution is the arc probe `_v3, _v0, _lite, bare`, so
  `"common_choice_v2"` loads `common_choice_v2.arc` on the bare rung.
  State 1 waits `FUN_180038fd0(+0x118)` = the bm2d `is_ready` of that name.

Only reader of the table: this function (disp32 scan). The table is in
`.rdata`.

### 1.2 Update states (kind 3 = stage)

| st | World |
|---|---|
| 0 | pending ≥ 0 → kind art → 1 |
| 1 | package ready → kind 3: **fill `FUN_180035f00`** (pending layer: goto+play `in` at the MC, layer rate 0, invisible; `+0x340` = song basename (music-info vslot 1), `+0x390` = jacket name (vslot 2 + 1); `info_%dp_usr` tips / difficulty + name SpriteLayers (anchor-less ⇒ hidden) / best + target score; `stage_usr` texture) → `data/arc/jacket/<+0x390>.arc` into `+0x3b8` → 2 |
| 2 | jacket ready → swap active/pending, `+0x318 = +0x340`, `+0x368 = +0x390`; `FUN_180034d10(kind,"in")` (row SE in = `se_start_game`); `FUN_180034bf0(layer,"in")` = goto+play `in`, layer rate 1, visible; kind 3: `jacket_usr` textures (null-checked on 20260721+), `jacket_usr` visible, **`choice_stage_usr` Pause(false)** (an A3 vestige — exactly A3's legacy-branch step), **stage voice `FUN_180033760`** → 3 |
| 3 | wait label `loop` ∧ timer > `+0x3d0` (always 0) → 4 |
| 4 | covered (loader gate A, DPS 1/5 gates); a request → 5 |
| 5 | kind 3: deep `SetFrameLabel("stage_out")` (`FUN_1801ae160`) + code `vo_ingame_ready` → 6; else `out` → 8 |
| 6 | parked (mid-song) |
| 7 | (`0x100c`) replay `stage_out` + voice iff the clip is before it; deep `out` → 8 |
| 8 | wait `max(out_end, end)` → release layer + async package, active −1 → 0 |

`FUN_1801ae160(clip+0x10, label)` = `CMovieClip::SetFrameLabel`: op
`0xF03` on the root MC and on every direct child
(`afp_mc_traversal(…,1)` / `(…,3)`). **The "afp_mc_goto_play_label no
label[stage_out]" lines on the CrossOver install come from that deep goto
reaching children** that lack the label — the root itself has `stage_out`
(325), `ready_loop` (408) and `end` (481), which is why `mc_frame_by_label`
returns real frames. What `shutter_play` genuinely lacks is `out` / `out_end`
(labels: in_stage 0, loop 14, loop_stage 198, stage_out 325, ready_loop 408,
ready_out 458, end 481): state 7's deep `out` finds nothing, so an early
drain (clip still looping in `ready_loop`) never reaches `end` — the real
cause of QR §14.1's stall (timing-dependent, not install-dependent); its
drain unblock stays correct. A3's legacy root has `out` (500) and `end` (531),
so its drain completes on its own.

### 1.3 Stage voice `FUN_180033760()` (only caller: update state 2)

World rule `vo_stage_extra` / `_final` / `_%02d` (+ savior / brave
variants), played `se_play_inner(3, …)` behind the voice mute filter:
`MOV [rsp+x],5; CALL [filter]; CMP [rsp+x],6; JZ skip` — the `JZ` jumps
over lock / play / unlock (balanced), `74 3F` on 20260721 / 20260825 /
20260915 (`+0x22F` into the function).

### 1.4 Who requests kind 3, and when (CORRECTED 2026-09-23, cabinet run #1)

The first cut assumed only `SelectMusicTerminateSequence` requests the stage
panel and armed at the 25 → 26 edge. **Wrong**: on every song of run #1 the
AVS line `afp_mc_deep_goto_play_label no label[in] in stream[shutter_play]`
(World's kind-3 art load / swap) precedes `Scene change hook: prev=25 …
next=26` — World loads its stage panel DURING song select, so the row patch
came after the only read of the row and the legacy root never appeared.

* Request = `FUN_180033a00(kind)` (msg `0x1007` → pending kind) or an inlined
  copy. Direct callers (20260825): `FUN_180112480` Terminate (state 3, ONLY
  when the shutter is idle — it never is after a confirm), `FUN_1800bc120` =
  **`ResultSequence::onUpdate`** (kinds 2 / 7 / `e8 ? 3 : 4`, results),
  `FUN_180154940` + selectmusic `lambda8` (the dan `GradePreparePanel`,
  `se_dan_start_board_out`), `FUN_180051660` (`BreakTimeSequence`),
  `FUN_180057e10` / `FUN_180061cc0` (DPS / MatchingDPS: 3 or 8, 4 / 5),
  `FUN_1800c9ed0` (TotalResult), demo / caution sequences (kinds 0 / 1).
* Inlined: the versus confirm `FUN_180114a10` commits (`FUN_1800fdc90`) and
  then requests kind 3 inline (`MOV [RSP+0x50],3; … MOV EDX,0x1007; CALL
  [RAX+0x18]`). Only two sites in the binary load the shutter global and send
  `0x1007`: `FUN_180033a00` itself and this one. The solo confirm
  `FUN_18010d480` commits (`FUN_1800fd970` writes `PlayerWork+0x54`, then
  `FUN_1800fdc90`) and schedules event 0 at +0.2 s; `SelectMusicSequence`'s
  event-0 handler `FUN_1800fc170` then WAITS for the shutter (named loader
  ready ∧ state 4) before finishing to scene 26. (The solo requester itself
  was not pinned; the cabinet log proves it runs in scene 25.)
* Whatever the exact requester, the request follows the confirm, which
  commits the song synchronously, and the ShutterActor's own update (where
  the row is read) runs after it — so at that update the committed mcode and
  the option rows are final: **the resolution can be made there.**

Fix (rev 2): the ShutterActor detour runs PRE-original on the update whose
state 0 has `pending == stage kind` (that update calls the kind-art loader,
the only reader of the table): during song select it resolves the song
(`ddr_selection::stage_panel_request_skin`, the same inputs as the play-edge
arm) and creates the session; then, for this one update, it patches the row
and restores it right after the original (the loader copies the row into its
closure). A request after the edge (the Terminate path) uses the session the
edge arm created. The edge re-resolves: same skin ⇒ nothing; stock ⇒ drop
the session unless World already read the row (then A3's panel stays for
that song, WARN).

### 1.4a World's named-package path (the one the row patch uses)

`FUN_180035420` with `pkg != NULL` builds lambda6 (row copy + `this`) and
stores into the loader at `+0x118` (ctor `FUN_180038c50`, vtable
`0x18035ec18`): `+0x120` requested name, `+0x190` dir lambda (`"bm2d"`),
`+0x1b0` name lambda (`"%s"`), the done callback via `FUN_180038f50`. The
loader's slot 0 (`FUN_1800392e0`, called at the top of every update) ticks
two sub-loaders and invokes the callback; `FUN_180038fd0` (state 1's wait) =
"no request pending (`+8 == +0x50`) or the named package is ready". lambda6
(`FUN_180035d60`) looks the name up in the bm2d registry (`*DAT_1806f2d70` —
the registry `services::bm2d_package` uses, keyed by the requested name) and
**int3s if it is missing** — `panel::arm` probes `common_choice_v2` first.
State 8 releases via `FUN_180039120` (every loaded name
`FUN_1801acd00`-released, `+8 = +0x50`). Galaxy brave (mode 9) and the dan
tables (mode 10, new builds) use the same path in stock World; in those modes
the default row is not read at all (our patch is harmless; the root that
appears is World's and the adoption refuses it).

### 1.4b World's kind-3 fill on A3's root

Harmless on every build: child writes go through `afp_layer_mc_refer`
(Ordinal 103, −1 on a miss) and the SpriteLayers (`FUN_1801bd2c0` stores
parent + path; the layout vfunc `find`s the anchor and hides its sprites when
it is null — 20250805 `FUN_1801bd6c0`). The one exception is §1.5.

### 1.5 Old builds (20250805 / 20260224) — hosted since rev 2

Stage kind 1, kind fields `+0x2E0/+0x2E4`, stage layer slot `+0x98`,
basename `+0x310` / jacket name `+0x338`, loader at `+0xE8`, 0x30-stride
default table (6 pointers `{pkg, root, SE in, SE out, voice in, voice out}`;
20250805 `0x18033ecc0`, getter `0x1800351e0` — a mode-9 table only, no dan
table; stage row = row 1 `{NULL, "shutter_play", "se_start_game", "", "",
""}`). The kind-art loader `FUN_180034fc0` (20250805) has the same
named-package branch (lambda4 / 5 / 6, loader `+0xE8`); the update
`FUN_180033d00` has the same nine states (state 8 waits `end` only — A3's
root has it); the msg handler `FUN_180034d10` the same `0x1007` / `0x1008` /
`0x100c` (stage kind 1) / `0x1048` (`ready_out`, absent on A3's root ⇒
no-op).

The one blocker: state 2 calls `SetVisible(find(stage_clip, "jacket_usr"),
1)` **without a null check** (20250805 `0x1800342ae`: `LEA RDX,"jacket_usr";
MOV RCX,[RSI+0x98]; CALL find 0x1802569e0; MOV DL,1; MOV RCX,RAX; CALL
0x180257120` — `SetVisible` reads `[RCX+0x100]`). A3's root has no direct
`jacket_usr` (`afp_layer_mc_refer` only descends unnamed children; A3's is
`choice_jacket_usr/jacket_root_usr/jacket_usr`), so `find` returns 0. Fix:
the 5-byte `CALL SetVisible` (return value unused) is replaced with
`0F 1F 44 00 00` pre-original on every update about to run state 2 for the
stage kind, exactly when the pending stage clip has no `jacket_usr` (World's
own `shutter_play` has one ⇒ restored) — decided from the clip itself, not
from the session, so a root that outlives a disarm / disable is covered too
(`memory::apply_checked_patch`, game thread). Derived as `ddr_sel_shutter_jacket_vis_call` (`tail − 5`,
shape-checked from `tail − 29`); that pre-tail shape is unique and identical
on both old builds (`0x1800342c6` / `0x180033b36`).

## 2. A3's legacy panel (20240402)

`FUN_1800328c0(skin)` (msg `0x100B` at song commit): names
`common_choice%04d` `+0x1a0`, `common_shutter%04d` `+0x1c8`,
`common_choice_cutin%04d` `+0x1f0`; loads both + cut-in + `common_choice_cutinbg`
(texture-only). `FUN_1800306c0` kind 1: root `shutter_choice_%s_root`
(`hd`, `sd` on machine types 0/1) from `common_choice` (v2), `SetView(5)`,
**`SetPriority(6)`** (other kinds 3), SD scale, rate 0, invisible.

Fill `FUN_180030d10` (legacy branch = both names set): goto+play `in`,
rate 0; `+0x118` basename; jacket name = skin 3 `banner_sn2_<basename>`
else vslot 2 + 1; per side `p%d_score_set_mc` visible iff entered ∧
(not course ∨ stage 0) with high score / rank / FC mark / dancer name /
area (`FUN_180032240`) and target; `choice_background_usr/movie_usr` →
`render_back`; `caution_usr` hidden; **`choice_stage_usr2` ←
`afp_mc_load_movie("choice_stage" of common_choice000N)`** + texture
`choice_stage_usr2/scene_choice_stage_usr` = `scene_choice_stage%04d_`
`{extra, final, 2nd (stage 1), 1st (else)}`; **`choice_background_usr` ←
`choice_background` of common_shutter000N**; skins 3–5 **`choice_jacket_usr`
← `choice_jacket` of common_shutter000N**; `fullcombo_challenge_usr`
hidden; `savior_special_usr/rinon_usr` (not in the hd root).

Update `FUN_18002f5f0`:

| st | A3 |
|---|---|
| 0 | readiness `FUN_1800327b0` (all names resident) → art + fill → 1 (cut-in loaded) / 3 |
| 1 | `choice_cutin` of `common_choice_cutin000N`, group 5, priority 7, rate 1, visible, SD scale; code SE `sele_1st/ext/sn2/x2/2013` (slot 2) → 2 |
| 2 | after frame 0x3B, before `out`, a player's decide button → goto `out` + stop the SE; wait `close` → 3 |
| 3 | jacket arc: skin 3 `data/arc/banner/%s.arc`, else `data/arc/jacket/%s.arc` → 4 |
| 4 | swap, `in` on the root; `choice_jacket_usr/jacket_root_usr/jacket_usr` texture; skins 1–2: `choice_jacket_usr` hidden + deep Pause; else visible; legacy: `choice_stage_usr` Pause(false), voice gate → 5 |
| 5 | voice gate; wait `data_release` (kind 1) → destroy the cut-in, release the cut-in packages → 6 |
| 6 | voice gate; wait `loop` (∧ the flare-caution hold `+0x280`, 0 otherwise) → 7 (covered) |
| 8 | (`0x1009`, DPS 5) deep `frame_out` → 9 (parked; `ready_loop` loops) |
| 10 | (`0x100D`, ReadyGo at READY) deep `out` → 11 → wait `end` → release |

Voice gate `FUN_180032a50`: once, when `choice_stage_usr2`'s frame ≥ its
`voice` label (absent ⇒ 0 ⇒ at once): `FUN_18002e210(skin)` — skipped
when skin 1, or when not course and (`stage == override` or `stage >
max+1`); skins 2–3 `sn2_etc{73 extra, a7 final, a<stage+2> for stage ≤ 3,
else none}`; skins 4–5 `vo_stage_{extra, final, %02d}` (slot 3).

## 3. Labels (legacy data, identical in World)

| clip | labels |
|---|---|
| `shutter_choice_hd_root` (common_choice_v2) | in 0, data_release 70, loop 240, loop_end 253 (→ loop), frame_out 299, ready_loop 357, ready_loop_end 392 (→ ready_loop), out 500, end 531 |
| `choice_cutin` (cutin0001–5) | in 0, out 271 (270 on 0002), close 360, end 413 |
| `choice_stage` common_choice0004 | in 0, voice 115, end 251 (`Plate_spin4_st` f100); 0001: in 0, end 225 (no voice label) |
| `choice_background` common_shutter0004 | in 0, loop 99, loop_end 203, frame_out 575, end 642 (`STG_APP02` **f1**, `STG_CLOSE01` f15) |
| `choice_jacket` common_shutter0004 | in 0, loop 161, out 500, end 527 (`banner_in` f39); child `jacket_root_usr/jacket_usr` |

Root direct children: `caution_usr`, `p1/p2_score_set_mc`,
`fullcombo_challenge_usr`, `choice_stage_usr`, `choice_stage_usr2`
(placeholder), `choice_jacket_usr`, `choice_background_usr` — no
`jacket_usr`, no `info_%dp_usr`, no `stage_usr`. The placeholders' default
content is common_choice_v2's modern clips (`choice_background` plays
`STG_APP02` / `se_shutter_in` / `se_shutter_out`, `choice_jacket` plays
`banner_in` f116, `choice_stage` plays `Plate_spin3_st` f16). Banner arcs:
`data/arc/banner/banner_sn2_<basename>.arc` → PNG stem
`banner_sn2_<basename>` (the 12 SN2 songs). Skins 1–2 shutter packages have
no `choice_jacket`.

## 4. Mapping onto World (the Step 5 design, rev 2)

Host = World's kind 3 (design D28). Mechanism:

1. **Session** (`panel::arm`): created at the song-select stage-panel request
   (pre-original, §1.4) or, if none came yet, at the 25 → 26 arm; probes
   `common_choice_v2`, requests our four packages. Nothing is patched yet.
2. **Row patch for one update**: in the update whose state 0 has `pending ==
   stage kind` (pre-original) the stage row gets `pkg = "common_choice_v2"`,
   `root = "shutter_choice_hd_root"`, `SE in = ""` (the row's own `""`);
   restored post-original. Only the session's first request uses it; a later
   request in the same window gets World's own panel.
3. **One detour on ShutterActor::onUpdate** (RTTI slot 6), post-original —
   the only point that sees each state change inside the same update:
   * state 1→2 (World's fill ran on the new clip): **adopt** — the pending
     stage clip must have `choice_stage_usr2` (else abandon: World's panel);
     only then is the root live (World's stage voice, READY dismissal and
     dwell stand down), priority 6
     (raw 94), the score sets / caution / `choice_stage_usr` / FC-challenge
     hidden;
   * **fill** as soon as the era packages are ready (at the latest at World's
     swap — a partial fill keeps the placeholders' default content; a drain
     before it skips the fill): `afp_mc_load_movie` of `choice_stage` /
     `choice_background` / (3–5) `choice_jacket`, the stage band texture,
     skins 1–2 `choice_jacket_usr` hidden + deep-paused, skin 3 banner arc
     (`asset_loader`), the cut-in (group 5 / priority 7) + `sele_*`;
   * state 2→3 (World's swap played `in`): while the cut-in is before
     `close`, the root layer is paused + hidden in the same update (World's
     state 3 then waits `loop` = our hold); at `close` goto+play `in`,
     visible. START skips the cut-in to `out` after frame 0x3B.
   * the voice at `choice_stage_usr2`'s `voice` label; the cut-in destroyed
     at the root's `data_release`; the jacket texture once loaded.
   * state 6 first seen (World's state 5 found no `stage_out`): deep
     `SetFrameLabel("frame_out")` — A3's state 8.
   * CMA READY (the ReadyGo `0x100D`): `0x100c` → World's state 7 plays the
     legacy `out`, state 8 waits `end`, release (no drain unblock — the
     labels resolve on this art; fallback unblock only if it stalls).
4. **World stage voice** silenced by a `code_se` site (the voice function's
   mute `JZ` → `JMP`) while A3's root is live.
5. **DPS READY? dwell** skipped (`panel.rs`, seeded from the shutter update)
   only while A3's root is live.
6. **AFP sound route**: also active while A3's root is live before the play
   edge arms the skin (the panel's embedded sounds play in scene 25).
7. Our packages (`common_choice000N`, `common_shutter000N`,
   `common_choice_cutin000N`, `common_choice_cutinbg`) are released only once
   World destroyed the legacy root layer (`afp_id_is_valid` false) and our
   cut-in layer, plus a grace period.

Because the panel is hosted from the confirm, the A3 sequence plays where A3
played it: the cut-in over the closing song select, then the panel's `in`;
World's `SelectMusicSequence` waits for the covered state (state 4 = the
root's `loop`) before it finishes to scene 26, exactly as it waits for
`shutter_play`.

Not ported yet (follow-up): the score sets' contents (high score digits,
rank, FC mark, dancer name, area, target) — hidden for now; the SD root
(`_sd_`) on machine types 0/1 and A3's SD scaling of the cut-in.

## 5. Display priority (`CLayer::SetPriority`)

`CLayer::SetPriority(p)` (A3 `0x1801b8790`, World `0x180259710` on
20260825 — identical bytes on all six builds) calls
`afp_layer_set_priority(layer, p <= 100 ? 100 - p : p)`, and the display
draws ascending raw priorities. So a HIGHER `CLayer` priority draws
EARLIER: World's shutter kinds (`SetPriority(3)`) = raw 97 (on top), A3's
ReadyGo clips (5) = 95, the stage-choice root (6) = 94, the cut-in (7) = 93.
**Step 4 set the legacy READY / HERE clips to raw 5** (below the panel);
Step 5 corrects them to raw 95 (A3: READY draws over the panel's `out`).
`SetView(p)` (`0x180259730`) = `afp_layer_set_group(layer, p)` for p ≤ 15.

## 6. Signatures (all swept, ALL GREEN; `shape_diff` identical on 20260721+, old builds diverge in the checked field displacements only)

| name | shape | hits | yields |
|---|---|---|---|
| `ddr_sel_shutter_swap` | state-2 kind swap + the basename / jacket copies | 1 on all five | `ddr_sel_shutter_basename_off` (0x340 new / 0x310 old), `ddr_sel_shutter_jacket_off` (0x368 / 0x338); kind offsets cross-checked |
| `ddr_sel_shutter_stage_tail` | `choice_stage_usr` find + Pause + CALL stage voice | 1 on all five | `ddr_sel_shutter_stage_voice`; layer slot 0xB8 / 0x98 = `0x88 + stage_kind*0x10`; jacket null-check at −15 (new builds only) |
| (in the voice fn) | `MOV [rsp+x],5; CALL [filter]; CMP [rsp+x],6; JZ` | 1 per build | `ddr_sel_stage_voice_jz` (`74 3F`, skipped block holds `MOV ECX,3; CALL`) |
| `ddr_sel_shutter_kind_table` / `_v1` | 0x40-row movups copy (20260721+) / 0x30-row getter (old) | 1 per layout | `ddr_sel_shutter_stage_row` (0x35e100 on 20260825), `ddr_sel_shutter_row_stride` |
| RTTI `ShutterActor` slot 6 | — | all five | `ddr_sel_shutter_update` (swap and tail must lie inside it) |
| (before the stage tail) | new: `TEST RAX,RAX; JZ` at −15; old: `LEA "jacket_usr"; MOV RCX,[RSI+slot]; CALL find; MOV DL,1; MOV RCX,RAX; CALL` from −29 | one per build | `ddr_sel_shutter_jacket_vis_guard` (new, informational) / `ddr_sel_shutter_jacket_vis_call` (old: the CALL at −5, NOPed while hosted) — a `report.py` ALT_GROUPS pair |
| — | — | — | `ddr_sel_panel_host_ok` = 1 on all five (rev 2) |
