# A3 Runtime Rules — Choreography, Tempo, Camera, Placement, Animation Mechanics

Source: Ghidra RE of `gamemdx_20240402_A3_Final.dll` (image base `0x180000000`, addresses file-relative),
2026-09-16, plus read-only checks against the stock A3 install (`startup.arc` → `ConfigBank.csv`,
the rlists; `stage_camera.arc`; `camera_music_*.arc`; `mc_*.arc`). Nothing was renamed in Ghidra.
These findings SUPERSEDE `docs/background_dancers_feasibility.md` §3 steps 4–6 where they differ.

## 0. Message vocabulary (A3 actor tree)

Delivered by `FUN_18018d5d0(actor, msg, a, b)` — actor vtable slot 3, then recursion into children.

| msg | sender | meaning |
|---|---|---|
| `0x1001` | DPS step 3 | readiness poll (SceneManageActor answers not-ready while its step ≠ 3) |
| `0x1046` | DPS step 5 (`FUN_180039650` case 5) | **song start / GO** — same frame the scene graph update bit is set (`*(*DAT_1802eee40+8) \|= 1`) |
| `0x1047` | DPS step 6 | music-time origin tick |
| `0x1048` | GPA per-frame chart-time update `FUN_18003de80` | `{side, count, ms}` every frame; count in 1/1024-beat units |
| `0x104f` | SceneManageActor step 3 | a dancer clip has **< 2.0 s** left |
| `0x1050` | SceneManageActor step 3 / CharaActor fallback | a dancer clip has **< 1.5 s** left → "advance choreography" |
| `0x1051` | CameraActor `FUN_18005b070` | camera clip started (payload = remaining seconds) |
| `0x1052` | DPS step 2 | `{basename, stage}` to the fresh SceneManageActor |

## 1. Choreography sequencing (Q1) — CONFIDENCE HIGH

A3 uses a **pre-shuffled playlist with a look-ahead cut**. No loop, no idle return.

- At set-up (`FUN_180060460` step 2) each dancer's model handle gets the ENTIRE generic pool registered
  as animation sets, **Fisher–Yates shuffled with `rand()`** (`FUN_180061b00`, `FUN_180061c10`), and
  `CharaActor+0xEC = 1` ("cycle forever"). Pool literals `0x18026b5d8..0x18026b848`:
  male `br01 br02 hh01 hh02 ht01 ht02 ht03 ht04 ja01 ja02 sf01 sf02 sf03` + `br03` always;
  female `br01 br02 hh01 hh02 hh03 ht01 ht02 ht03 ja01 ja02 sf02 sf03` + `sf01` always.
  (`mc_male_tu01_exec.anm` exists on disk but is NOT in the code's pool.) With `+0x122` set (mcode `0x9439`)
  the pool is ONLY `br03`/`sf01`.
- Song-specific songs register exactly ONE set (`mc_<sex>_<song>_<song>_exec`, `FUN_18001c750`); it plays
  once and then freezes on its last frame.
- `FUN_18001c750(handle, name)` = REGISTER a set (FNV-1 hash lookup in the `.anm`/`.vanm` registries), not play.
- Playback starts on `0x1046`: `CharaActor::onMessage` (`FUN_18005e840`) sends itself `0x1050` → plays set 0
  from t=0 via `FUN_18001c970(handle, idx % nsets, 0)`.
- **Advance rule** (`FUN_180060460` step 3, skipped when `+0x122`): every frame the SceneManageActor
  takes the most-urgent dancer (`FUN_180061c90`) and computes (`FUN_18005e960`)
  `remaining = frames(anm+4)/fps − node_time`; `remaining < 1.5 s` (`DAT_1802647b8`) ⇒ broadcast `0x1050`
  to ALL children (both dancers + camera); `< 2.0 s` (`DAT_1802624d8`) ⇒ `0x104f`. On `0x1050` each
  CharaActor does `idx++` and, if cycling or still inside the list, `FUN_18001c970(handle, idx % n, 0)`.
  **Hard cut, no blend; both dancers cut on the same frame; the last 1.5 s of every clip are never shown.**
- Fallback (`FUN_18005d5d0` step 2): if every AnimationNode of the handle reports finished (`+0x42 & 1`),
  the CharaActor sends itself `0x1050` (covers sub-1.5 s clips / `+0x122`).

Evidence (trimmed): `FUN_18005e840`:
```c
if (msg == 0x1046) { send(self, 0x1050); step = 2; }
else if (msg == 0x1050) { i = +0xE8++; if (+0xEC || i < nsets) FUN_18001c970(handle, i % nsets, 0); }
```
`FUN_18005e960`: `remaining = frames/fps − t; if (remaining < 1.5f) return 2; if (remaining < 2.0f) return 1; return 0;`

## 2. Tempo / time base (Q2) — CONFIDENCE HIGH

**Animation time is wall-clock frame delta × a graph-wide rate; NOT the music count.**

- `FUN_18001d9d0` → `FUN_180159a90(graph, DAT_1802ed8d8 (1.0) · frameDelta · *(SceneGraphManager+0x38))`;
  `frameDelta` = clamped QPC delta (`FUN_1801550f0`). Each AnimationNode does `t += dt` (`FUN_18013a5e0`).
- Rate setter = DancePlaySequence vtable slot 7 `FUN_18003a1b0` (+ matching twin `FUN_180041cf0`), per frame:
  ```
  bpm = GPA+0x16c per live side; min/max over sides
  if (minBpm < 10.0 && ConfigBank["MOTION_STOP_SLOW"])       rate = 1/12   (0x3daaaaab)
  else if (ConfigBank["MOTION_BPM_DEPENDENCY"])              rate = maxBpm / 120.0
  else                                                       rate = 1.0
  ```
  `GPA+0x16c` = current chart BPM at the render-offset time (`FUN_18003de80` → `FUN_18011c520`,
  `bpm = Δcount/Δs · 150/2560`, 1024 counts per beat).
- **Stock A3 `ConfigBank.csv`: `MOTION_BPM_DEPENDENCY = FALSE`, `MOTION_STOP_SLOW = TRUE`.** So retail
  dancers run at real time regardless of song BPM (the "clips authored at 120 BPM and scaled" hypothesis is
  a DISABLED debug option), and slow to 1/12 speed while the chart BPM is < 10 (STOPs).
- `mc_bpm120.arc`, `start01`/`between01`, `ne01_loop`: **no string in the binary references them** — dead
  authoring data (the only clip-name patterns are `mc_%s_%s_%s_exec`, the 27 pool literals, `%s_play_loop`).

## 3. Idle / pre-song / post-song (Q3) — CONFIDENCE HIGH

- **There is no idle animation.** Before `0x1046` the scene graph update bit is CLEARED (DPS `onInitialize`,
  `AND dword [graph+8], 0xFE` at `0x18003958c`) and set at DPS step 5. Nodes exist with no clip bound; a
  ModelNode without animation carries its bind pose (`FUN_18015afb0` copies `model+0x48` bind table into
  `node+0x80`). Since the update never runs, the visible/item lists are never rebuilt → **nothing 3D is
  drawn until song start; then stage loops, dancer playlist and camera all start on the same edge.**
- Clip time 0 ≈ music time 0 (no beat wait).
- After the chart ends nothing changes: generic dancers keep cycling until the DPS is finalised
  (`FUN_18003a310` destroys the actors); song-specific dancers hold their last frame.

## 4. Camera set selection and switching (Q4) — CONFIDENCE HIGH

Chosen at `CameraActor::onInitialize` (`FUN_180059d60`): song has a `music_camera_resources.rlist` row ⇒
**music mode**; else **stage mode**. Both → step 2; `0x1046` → step 3 (sequencing).

**Music row `key [f0 f1 f2 cue…]`** (`FUN_18005a230` step 0):
- `f0` = camera clip **start-frame offset at 60 fps** (`+0x138 = atoi(f0)·1000/60` ms; applied to every cue start).
  Data check: `music_dace.camanm` 6177 frames − `mc_*_dace_dace_exec.anm` 5646 = **531** = f0.
- `f1` = **near-plane multiplier** (`CameraNode+0x88`; `cam+0x2a8 *= f1`). Stock 1.0 / 0.1 / 0.01.
- `f2` = **aspect multiplier** into `camera_set_perspective_fov_aspect` (`CameraNode+0x8c`; `mawa` 0.75).
- cues = fields ≥ 3, `name` or `name:time`; `<name>.camanm` must be registered; `time` = absolute deadline in
  seconds since `0x1046` at which the NEXT cue starts; 0/absent = advance when the camanm finishes. Plays once,
  in order; after the last cue the camera holds. Empty list ⇒ fall back to stage mode.

**Stage mode** (`FUN_18005a230` step 1 + 3): walk `stage_camera_resources.rlist` row `bgstage`; every
registered `<name>.camanm` goes to the MAIN list unless its name contains `"_non"` → the **`_non` list**
(`+0xf8`). Both lists Fisher–Yates shuffled (`FUN_18005b830`, RNG seeded from `timeGetSystemTime`). Step 3
cycles the main list forever (`idx % count`), switching when the current camanm's finished bit is set
(450 f = 7.5 s typical). Dancer interlocks (`FUN_18005aed0`):
- `0x104f` (< 2.0 s of dance clip left) → `+0x135 = 1`: **stage-mode switching frozen**.
- `0x1050` (< 1.5 s, dance cut imminent) → if the `_non` list is non-empty: `FUN_18005b070(actor, non[0],
  hold = stepTimer + 1.0 + U[0,1) s, waitBeat = 1)`, then `std::rotate` the `_non` list by one.
  The next switch requires `stepTimer ≥ hold` AND a **beat boundary this frame** (`+0x140`, set by `0x1048`
  when `count >> 10` changes).
So **`_nonNN` sets are the cut-away shots that hide the dance-clip hard cut** (1–2 s, resuming a dancer shot
on a beat). `chara_in01..03` / `chara_out01..03` are referenced by no code and no stage row — used only by
the `mawa` inline cue list.

`FUN_18005b070(actor, name, hold, waitBeat)`: destroy old clip, new clip from the registered camanm,
`FUN_180158160(cameraNode, clip)`, `cameraNode+0x30 = +0x138/1000` (start offset), `+0x130 = hold`,
`+0x134 = waitBeat`, `+0x135 = 0`, broadcast `0x1051`.

## 5. Dancer / stage / shadow placement (Q5) — CONFIDENCE HIGH (shadow formula MEDIUM-HIGH)

- **Dancer X** (generic branch): `x = (i − (n−1)·0.5) · 1.6` (`DAT_180265198` = 0.5, `DAT_180294188` = 1.6),
  `y = z = 0`, written to `ModelNode+0x40/+0x44/+0x48` with `node+0x28 |= 1`. n=1 → 0; n=2 → ∓0.8. No rotation.
  Song-specific branch: always exactly two dancers (slot 0 kind −2 random male A, slot 1 kind −1 random
  female A) at the origin; the choreography's root motion places them.
- **Stage**: every `gm_<stage>_<part>` node at the origin, identity, pass mask 4 (dancers/shadow 2; `:N`
  parts `0x10` + priority `node+0xE8`).
- **Colour pair** (closes the docs' "consumer untraced"): colour 0 (`StageActor+0xF8`) is pushed every frame
  into every dancer material's private copy at `+0x48..+0x54` with `+0x18 = 3` (dirty), recursively into
  parts (`FUN_18005e9e0`); colour 1 (`StageActor+0x108`) is the shadow node tint `node+0xD8..0xE4`
  (`FUN_18005eba0`). Defaults `DAT_1802dc338..344` when no stage. (All stock rows are `000000`/`000000`.)
- **`pl_shadow00`** (per frame in `FUN_18005d5d0`): world positions of `{Hips, Spine2, Head, LeftToeBase,
  RightToeBase}` from the body's bone matrices (`node+0x80`, row 3) with **y := 0.02** (`DAT_180290138`);
  centre = mean; spread = max distance from centre; `size = clamp(1 + 1.5·spread, 1, 2) × h` where `h` from
  the Hips Y delta vs bind (`h = d ≤ 1 ? d² : (d−1)²+1`, clamped `[0,2]`), `× rlist shadow_scale`, low-pass
  `prev += 0.1·(target − prev)` (`DAT_1802888b8`); quad position = body world transform applied to centre.

## 6. Animation-node mechanics (Q6) — CONFIDENCE HIGH

- Model handle (`FUN_18001c300`) = ModelNode (`+0x28`) + **8 AnimationNodes** (`+0x78..+0xB0`, vtable
  `0x180281e10`), one per set slot. Player: `+0x30` t (s), `+0x38` clip, `+0x40` flags — **bit0 loop, read from
  the ANM header byte `+6` bit 0** (corrects the format doc's "+6 not read"; `*_loop` = 1, `*_exec` = 0),
  bit16 `0x10000` finished, bit17 `0x20000` wrapped.
- Time advance (`FUN_18013a5e0`), `dur = frames(anm+4) / fps (clip+8, default 60.0)`:
  ```
  t' = t + dt
  0 ≤ t' < dur  → t = t'
  t' ≥ dur      → loop ? (t = fmod(t', dur); flags |= 0x20000) : (t = dur; flags |= 0x10000)
  t' < 0        → loop ? (t = fmod(t', dur) + dur; …)        : (t = 0;   flags |= 0x10000)
  ```
- Evaluation (`FUN_18013a680` → `FUN_18013ab80`): `f = t · fps`; explicit key times → binary search + linear u;
  uniform → `i = floor(f), u = frac(f)`; clamped at last key; decoders from `DAT_180262520`. Bones without
  tracks keep the **bind-derived local TRS** (`FUN_18013ba50`) — a port must seed from bind, not identity
  (differs from `anm_dump.evaluate_pose`'s identity default; irrelevant for the stock 33-bone dance clips
  which carry all tracks, relevant for stage `_play_loop` clips and custom clips).
- Binding a clip is a hard reset (`FUN_180158160` → `FUN_180158920`); **no cross-fade anywhere**.
- Camera clips use the same player embedded in the `CameraNode` (0x98 bytes, `FUN_18001ca00`).

## 7. Consequences for the port (decision inputs)

1. **Clock.** A3 = wall dt × rate (rate 1.0 in retail; 1/12 during STOPs). Our design may use the
   content-domain music count instead (rate/seek/loop free; identical to A3 at 100 %); the STOP slow-motion
   and the BPM-dependency debug option are optional extras, not fidelity requirements.
2. **Playlist.** Shuffle the fixed pool per sex (code list, not directory listing — `tu01` is excluded by
   A3), start at song start, cut to the next clip when `remaining < 1.5 s`, both dancers together, cycle
   forever. Schedule is deterministic from (seed, t): clip k starts at `Σ_{j<k} (dur_j − 1.5 s)`.
3. **Pre-song.** Nothing visible before song start; everything appears on the start edge in bind pose → clip 0.
4. **Camera (stage mode).** Main/`_non` lists shuffled; cycle main on finish; freeze at "< 2 s"; `_non` shot for
   `1 + U[0,1)` s at "< 1.5 s"; resume on a beat boundary (needs a beat clock — optional for v1).
5. **Placement.** X pitch 1.6 m about the origin, stage at origin; shadow rule above; stage colour pair to
   dancer material param + shadow tint.
6. **Two doc corrections**: ANM header `+6 & 1` IS read (loop flag); the `map_resources` colour pair consumers
   are `FUN_18005e9e0` / `FUN_18005eba0`.

## 8. A3 addresses visited

| Address | Role |
|---|---|
| `FUN_180039650` | `DancePlaySequence::onUpdate` — step 2 SceneManageActor (+`0x1052`), step 3 `0x1001`, step 5 `0x1046` + graph enable, step 6 `0x1047` |
| `FUN_18003a1b0` / `FUN_180041cf0` | graph playback-rate setter (STOP_SLOW 1/12, BPM_DEPENDENCY maxBPM/120, else 1.0) |
| `FUN_18003a310` | DPS onFinalize |
| `FUN_18003de80` / `FUN_18011c520` | GPA chart-time update → current BPM `GPA+0x16c`, msg `0x1048` |
| `FUN_18005fe00` / `FUN_180060090` / `FUN_180060460` / `FUN_180061ad0` | SceneManageActor ctor / onInitialize / onUpdate / onMessage |
| `FUN_180061c10` / `FUN_180061b00` / `FUN_180061c90` / `FUN_18005e960` / `FUN_180061dd0` | register pool / shuffle / most-urgent dancer / remaining-time state / colour push |
| `FUN_18005c720` / `FUN_18005d070` / `FUN_18005d5d0` / `FUN_18005e840` / `FUN_18005e6e0` | CharaActor ctor / onInitialize / onUpdate / onMessage / onFinalize |
| `FUN_18005e9e0` / `FUN_18005eba0` / `FUN_18005ec60` / `FUN_18005ee10` | material colour / shadow tint / ground-bone floor positions / centroid distances |
| `FUN_180059a60` / `FUN_180059d60` / `FUN_18005a230` / `FUN_18005aed0` / `FUN_18005b070` | CameraActor ctor / onInitialize / onUpdate / onMessage / start clip |
| `FUN_18005b830` / `FUN_18005b930` / `FUN_18005b490` | shuffle / rotate / substring find |
| `FUN_18001ca00` / `FUN_18001cb50` | `scene::CameraNode` ctor / camanm record apply |
| `FUN_180061f30` / `FUN_1800622a0` / `FUN_180062450` / `FUN_180062a70` / `FUN_180062a40` / `FUN_180062c10` | StageActor ctor / arc request / onUpdate / part handle / onMessage / start loops |
| `FUN_18001c300` / `FUN_18001c750` / `FUN_18001c970` / `FUN_18001c6a0` / `FUN_18001c510` | model handle create / register set / play set / attach / destroy |
| `FUN_180158a70` / `FUN_180158ad0` / `FUN_18013a5e0` / `FUN_18013a680` / `FUN_18013ab80` / `FUN_180158160` / `FUN_180158920` / `FUN_18013a7a0` / `FUN_18013a800` | AnimationNode visit / apply frame / time advance / eval dispatch / bone evaluator / bind clip / stop / clip ctor / chunk map |
| `FUN_18015afb0` | ModelNode setModel (bones ← bind) |
| `FUN_180159a90` / `FUN_18001d9d0` / `FUN_18001cd80` | SceneGraph update / dt driver / SceneGraphManager ctor (+0x38 rate) |
| `FUN_1801550f0` | per-frame wall delta |
| `FUN_180024ff0` / `FUN_180100d30` / `FUN_1800248b0` / `FUN_180100f60` / `FUN_180101430` | ConfigBank registry / ctor / CSV load / parser / bool lookup |
| Constants | `DAT_180265198`=0.5, `DAT_180294188`=1.6, `DAT_1802647b8`=1.5, `DAT_1802624d8`=2.0, `DAT_180265258`=120.0, `DAT_180264a58`=10.0, `DAT_1802dc26c`=60.0, `DAT_180290138`=0.02, `DAT_18028885c`=2⁻²⁴, `DAT_1802888b8`=0.1, `0x3daaaaab`=1/12 |
| Strings | `0x18026b5a0` `mc_%s_%s_%s_exec`, `0x18026b5d8..848` pool, `0x18026b930` `play_loop`, `0x180268c30/c48` `MOTION_STOP_SLOW`/`MOTION_BPM_DEPENDENCY`, `0x18026b118` `camera_music_%s.arc` |
