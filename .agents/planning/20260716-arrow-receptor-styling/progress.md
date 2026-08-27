# Progress — Playfield Styling (arrow/receptor scale + opacity)

Updated: 2026-07-19
Status: Steps 1–7 + lane background/cover addition implemented — awaiting cabinet validation
NEXT ACTION: Deploy `ddr_world_hook.dll` + the two new label PNGs, run the
acceptance checklist (see "Cabinet test plan" below), record results.

Resume protocol: read `implementation/plan.md` (step checklist),
`design/detailed-design.md` (authoritative mechanisms), and
`research/arrow-render-re.md` (verified addresses) in this feature dir.
Cabinet deploys/tests are done by the MAINTAINER — the agent builds
(`./build.sh` / `cargo check --target x86_64-pc-windows-msvc`) and pushes
via `./scripts/deploy.sh` when asked (DLL only — the label PNGs need a
separate copy into the cabinet's `data_mods/`), then requests
logs/screenshots for validation. Per maintainer direction the whole feature
was implemented in ONE pass (no per-step cabinet checkpoints) to compress
7 deploys into 1–2.

## Done

- Ghidra re-verification pass (2026-07-16, builds 20260616 + 20260324):
  collector cull insn (`F3 44 0F 10 3D` → 720.0f) @ collector+0xA6 both
  builds; guideline cull (`F3 44 0F 10 0D`) inside the guideline draw;
  emitter has exactly 1 xref (byte-identical body both builds); 3 renderer
  COLs valid (offset 0) + unique vtable meta-pointers + ctors store those
  vtables at object+0; freeze-body wrapper ends in a REAL `CALL` to the
  fill (flows through the detour); guideline draw prologue AOB = 3 matches
  on BOTH builds, uniquely classified by content.
- **Step 1** — `derive_playfield_styling` in `src/core/signatures.rs`
  (publishes `note_collector`, `collector_cull_site`, `guideline_draw`,
  `guideline_cull_site`, `guideline_bulk_emitter`, 3 renderer vtables) +
  mod skeleton `src/mods/playfield_styling/mod.rs` registered in
  `mods/mod.rs` + `lib.rs`. All derivations hard-fail (leave names
  unresolved) on any ambiguity.
- **Step 2** — `arrow_scale` / `arrow_opacity` scalar rows (25–150 / 0–100,
  step 5/25, default 100, `PersistMode::Full`), per-side atomics,
  enable-time reseed, GAMEPLAY-entry latch + exit clear via scene_manager.
  Label PNGs generated (`seop_item_arrow_scale.png`,
  `seop_item_arrow_opacity.png` — new files only; pre-existing PNGs
  reverted after the regeneration touched their encoding).
- **Step 3** — `fill_hook.rs`: `render_sprite_final` detour
  (`install_enabled`), 16-slot registry, vtable classification, presence /
  posX<640 / doubles→side-0 binding, JudgeEffect width inheritance with
  deferred bind, `x' = cx + s(x−cx)`, `y' = s·y`, `w/h × s`, color copied
  to a stack local with alpha×op. `catch_unwind` + one-shot panic warn;
  `IN_GAMEPLAY` + `MOD_ENABLED` fast-path gates; no hot-path logging.
- **Step 4** — `cull_patch.rs`: mod-owned f32 slot (int3-cave search
  ±0x20000 around the collector site, `alloc_near` fallback), byte-verified
  disp32 redirect of BOTH cull sites (collector + guideline — folded
  together since both are pre-verified and share the slot; plan had the
  guideline site in Step 5), pre-verification of both sites before any
  write, patch-once-per-process, disable = write 720.0.
- **Step 5** — `guideline_hook.rs`: capture detour on the guideline draw
  (side via mode@+0x78 / presence / posX@+0x80 split; `Ybase@+0x84 → Y/s`
  pre-scale with restore; thread-local PASS_STATE) + transform detour on
  the bulk emitter (0x14-records: x about `x+w/2`, y/w/h × s, alpha MSB ×
  op; forwards untouched without pass state).
- **Step 6** — mine integration: `mine_render`'s `render_height` now reads
  `playfield_styling::cull_bound()`. **Deviation from design §4.5 (for the
  better):** no `style_for_renderer` transform in mine_render — its quads
  are emitted through the (now detoured) `render_sprite_final` entry and
  inherit the scale/opacity transform automatically; adding the design's
  explicit transform would have double-transformed. Only the cull-window
  widen was needed. Bottom margin intentionally stays raw (mirrors the
  collector's own unscaled bottom cull → mines pop out exactly when arrows
  do).
- **Step 7 (code + docs)** — rows behind the COMPLETE all-or-nothing gate
  (fill → cull patches → guideline hooks → only then `register_rows`);
  rollback on every partial-failure path; mid-song disable = MOD_ENABLED
  gate + identity latch + 720.0 slot write; re-enable reseeds from the
  registry (`Duplicate` = success). JudgeEffect doubles-bind fallback
  (P2-carded doubles inherits the side-0 DOUBLE-width renderer). Docs:
  README mod-table row, AGENTS.md Key Entry Points row,
  `docs/playfield_styling_research.md` (distilled RE note incl. the
  first-CALL correction). Cross-build derivation check done in Ghidra
  against 20260324 (collector helper correctly rejected, collector /
  guideline / emitter all resolve, byte-identical emitter body).
- `cargo check` clean throughout; `./build.sh` release build clean.

## LANE ADDITION (post-smoke-test, maintainer request)

Smoke test (versus P1 s=50 op=90 / P2 s=150 op=50) confirmed arrows,
receptors, freeze, hit flash all scale/fade correctly. Maintainer requested
also scaling the **lane background** AND **lane cover**, horizontal-only +
scale-only (no opacity).

RE finding: both are `LaneFilterActor`-owned AFP-layer CMovieClips, NOT
`render_sprite_final` quads (verified the fill's complete caller set excludes
them). Neither is `Create`'d by its lane name:
- Lane background = find-child of `dance_root` (`1p_lane_usr`/`2p_lane_usr`/
  `double_lane_usr`) → hook `cmovieclip_find_child`.
- Lane cover = pool-create wrapper around CMovieClip::Create
  (`hidden_cover_*`/`sudden_cover_*`) → hook `cmovieclip_pool_create`.
Both capture points are collision-free (overlay-element-styling owns
`CMovieClip::Create` itself, which these bypass) — **no shared dispatcher /
overlay refactor was needed** (the earlier plan to refactor overlay was
dropped once RE showed the lane clips don't go through Create by name).

Implemented in `src/mods/playfield_styling/lane_hook.rs`: two detours,
name-filter, `layer_set_scale_raw(layer_id, s, 1.0)` (horizontal-only,
translation 0 — game repositions after, preserving scale). Best-effort /
NON-load-bearing (missing lane signatures → warn + skip, core styling
unaffected). Per-song reset via the scene callback. Background side from
name; cover side from presence + versus package-order heuristic. Two new
AOBs in `signatures.rs` (`cmovieclip_find_child`, `cmovieclip_pool_create`),
unique on both 2026 builds. RE note §4b in `docs/playfield_styling_research.md`.

Two ASSUMPTIONS that need cabinet confirmation (documented in the RE note):
1. Lane art is authored centered on its layer origin (so pure `sx=s` narrows
   in place). If a lane narrows toward an edge → add translation comp.
2. The lane clip transform is not re-driven per frame by its AFP animation
   (static skins). If a scaled lane snaps back → needs a per-frame re-apply.

`cargo check` + `./build.sh` clean with the lane addition.

## In flight

- Cabinet validation (maintainer): single consolidated acceptance pass.

## Cabinet test plan (single pass, A8 checklist)

Deploy needs TWO artifacts:
1. `./scripts/deploy.sh` — pushes the DLL.
2. The two new label PNGs → cabinet
   `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`:
   `seop_item_arrow_scale.png`, `seop_item_arrow_opacity.png`.
   (Delete `data_mods/_cache/` on the cabinet if the label atlas was
   previously cached without them — the framework hash-guards, but a stale
   atlas means blank labels, and this is the cheap fix.)

Then (log lines are `[DDR-Hook]`-prefixed):

1. **Boot log** — expect: `[+] note_collector (derived) @ +0x24B40;
   collector_cull_site @ +0x24BE6`, `[+] guideline_draw (derived) @
   +0x26210; cull_site @ +0x26448; bulk_emitter @ +0xC7B0 (1 caller)`,
   `[+] arrow/spot/judge_effect_renderer_vtable (RTTI)` (values for the
   20260616 build; different-but-resolved offsets on other builds are
   fine), then the mod's own inventory lines (cull sites must show bytes
   `F3 44 0F 10 3D …` / `F3 44 0F 10 0D …`), `cull float slot @ … = 720.0`,
   both `patched @ … → slot …` lines, `guideline hooks installed`,
   `playfield-styling: enabled (fill hook + cull patches + guideline hooks
   + options)`.
2. **Rows** — Options → Mods tab shows ARROW SCALE / ARROW OPACITY with
   correct labels, ranges (25–150 / 0–100), steps (5 fine / 25 coarse
   with Start held), default 100.
3. **Identity regression** — leave both at 100, play a song: visuals
   byte-identical to stock; log shows `latch p1 s=1.00 op=1.00 | p2 … |
   cull=720` and bind lines (`bind class=arrow/spot/judge_effect side=…`).
4. **Scale 50 %** — receptors shrink in place centered on the lane; arrows
   converge toward the lane center as they scroll; freeze head/body/tail
   coherent; shock arrow + electric overlay coherent; hit flash scaled;
   guideline lines match the shrunken lane width/positions; no pop-in at
   the screen bottom. Log: `latch p1 s=0.50 … cull=1440`.
5. **Scale 25 % on a fast/dense chart** — no pop-in at the bottom edge
   (`cull=2880`); no stutter (worst-case density = stock 0.25× speed mod).
6. **Scale 150 %** — grows about the same anchor, no cull artifacts.
7. **Opacity 50 / 0** — everything fades uniformly; at 0 the playfield is
   invisible but judging still works; game's own alpha effects (freeze
   fade, shock flash) still compose.
8. **Reverse scroll** at 50 % — arrows AND guideline correct across the
   whole scroll range (no early cut-off at the top).
9. **Versus** — P1=50 %, P2=100 % simultaneously: two coherent independent
   playfields; then swap sides.
10. **Doubles** — P1 values apply across the 8-panel lane, anchor at the
    8-panel center.
11. **Mines** (chart with MINE_DATA, note_types_expansion on) — mines track
    the scaled columns, fade with opacity, no mine pop-in at 25 %.
12. **HIDDEN/SUDDEN** at 50 % — fade zones keep stock screen distances
    (ACCEPTED characteristic, just confirm nothing crashes/looks broken).
13. **Judging parity** — same song/steps at 50 % vs 100 %: same score.
14. **Persistence** — set 50/75, card out, card in: values return (network
    and/or JSON path); `mod-config.json` gains `custom_options.p1.arrow_*`.
15. **Mid-session toggles** — mod OFF in overlay menu mid-song → stock by
    next frame (and fully stock next song); re-enable → values return, next
    song styled. Toggle OFF before a song → `latch` log absent, stock play.
16. **Coexistence** — center_arrows_single ON + 50 % scale (single play):
    the shrunken lane is centered (anchor follows the shifted lane).
17. **Lane background** — at 50 %/150 % the per-lane skin strip narrows/widens
    horizontally about the lane center, full height preserved, staying
    centered (log: `lane background scaled side=N s=… layer=…`). Confirm it
    tracks the arrow columns. If it narrows toward an edge instead of
    centered → assumption #1 (translation comp) needs tuning. If it snaps
    back to full width mid-song → assumption #2 (per-frame re-apply) needed.
18. **Lane cover** — with SUDDEN or HIDDEN on, the cover filter narrows/widens
    horizontally with scale (log: `lane cover scaled …`). Single/doubles side
    is exact; check versus cover side attribution (package-order heuristic).
19. **Overlay regression sanity** — combo / judgement / pacemaker styling
    (the `overlay-element-styling` mod) still works (it was NOT modified, but
    both mods now capture movie clips during setup — confirm no interference).

## Deploy & test log

- 2026-07-16: smoke test (versus P1 s=50 op=90 / P2 s=150 op=50) — arrows,
  receptors, freeze, hit flash scale/fade correctly. Lane background NOT
  scaled (expected — lane styling added after this test). → drove the lane
  addition below.
- 2026-07-16 (lane v1): deployed. Arrows etc. still correct. **Lane
  background NOT scaled** — log shows the lane hooks INSTALLED
  (`cmovieclip_find_child @ +0x257BB0`, `cmovieclip_pool_create @ +0x2575A0`,
  both "hook installed") but ZERO `lane ... scaled` lines fired. Root cause:
  the clip names captured (`1p_lane_usr`, `hidden_cover_*`) came from the
  ONLINE-MATCHING HUD builder (`FUN_18006bbb0`, refs `matching_*`/
  `dance_matching`) + reactive setups — NOT the path used for a local 2P
  session. The band's real clip name/acquisition path is unknown.
- 2026-07-16 (lane DIAG): deployed; user played local 2P (no SUDDEN, stock
  lane skin). **Diag = gold**: (1) `find-child name='1p_lane_usr'` DID fire
  in local 2P — names were right, but every find-child line showed
  `layer=0x00000000` → the find-child handle stores the child's **MC id at
  +0x100** (via `afp_layer_mc_refer`; Ghidra-verified in the find worker),
  not a layer id at +0x08 — v1 read 0 and silently bailed. (2) `pool-create
  name='dance_filter_single'` = the translucent FILTER band (the band in the
  screenshot) — previously unmatched. (3) Covers didn't appear (SUDDEN off —
  expected).
- 2026-07-16 (lane v2): rebuilt on the corrected mechanism:
  - libafp RE: `afp_mc_set_param`/`afp_mc_get_param` param tables mapped —
    0x1000 = position (2f, obj+0xD0/D4), **0x1003 = scale (2f,
    obj+0x124/128, 1.0-normalized, COMPONENT-based — position untouched)**,
    0x1004/5 = color/acolor, get-0x1003 = pw_get_scale (parallel tables
    verified).
  - Background: find-child capture → MC id (+0x100) → **deferred** apply
    (first fill-hook call of the song — deferral is load-bearing: the HUD
    builder reads `…/arrow_usr`/`…/judge_usr` marker positions out of the
    lane clip right after find-child; scaling at capture would shift the
    receptor/judge layout) → `mc_set_scale(id, sx·s, sy)` (read-modify-
    write).
  - Filter + covers: pool-create capture (names `dance_filter_*`,
    `hidden_cover_*`, `sudden_cover_*`) → layer id (+0x08) → deferred →
    matrix RMW `{s·a, b, s·c, d, tx, ty}` (`afp_layer_get_matrix` +
    `set_matrix` — preserves the game's own scale (unit-quad case) and
    translation).
  - Side: bg from name; `*_double` → side 0 (A7); `*_single` in versus via
    matrix `tx < 640` split; presence read otherwise.
  - New bm2d_api: `mc_set_scale`, `mc_get_vec2`, `layer_get_matrix_raw`
    (independent optional cell), `layer_set_matrix_raw`.
  - LANE-DIAG logging kept for this round (remove at final cleanup).
  - `cargo check` + `./build.sh` clean. NOT yet deployed.

- 2026-07-17 (lane v2 test): **lane background + filter scale correctly**
  (2P versus; receptor/judge layout unmoved — deferral guard works). Two
  findings: (1) one asset missed — the **red low-life lane flash**
  (pool-create `danger_single`, visible in the earlier diag log; was not
  matched). (2) UNRELATED pre-existing bug spotted: doubles-as-single-player
  + center-arrows-1P breaks arrow positioning (see Follow-up tasks).
- 2026-07-17 (lane v3): added the danger flash to `lane_hook::classify` —
  EXACT match `danger_single` (side via presence/tx-split) /
  `danger_double` (side 0), same layer-matrix RMW path as the filter.
  Exact-match on purpose: the HUD builder also find-childs
  `danger_gauge_%dp_usr` (a gauge readout, not the lane overlay), which
  must not match. `cargo check` + `./build.sh` clean. NOT yet deployed.

## NEXT ACTION (lane v3 validation — expected final round)

Deploy; play at ≠100 % scale and let the lifebar drop low (or fail out):
the red lane flash should narrow/widen with the lane (log:
`lane danger layer=0x… m=[…]→[…]`). Then run/finish the remaining A8
acceptance items. After validation: remove the LANE-DIAG logging block
(marked temporary in `lane_hook.rs`) in the final cleanup pass.

- 2026-07-17 (danger flash confirmed): user validated the red low-life
  lane flash scales correctly.
- 2026-07-17 (lane v4 — receptor hit flash): friend-tested build showed the
  per-panel receptor hit flash (successful-hit sparkle on the step zone) is
  still stock-sized/positioned at ≠100 %. RE: it's the BM2D `dance_effect`
  clip set — created by `NoteResultActor` via
  `afp_layer_create_with_property` DIRECTLY (bypasses both Create and
  find-child, so neither existing hook saw it) and stored in the actor's
  `vector<CMovieClip*>` @ +0xE8..+0xF0 (layer id at clip+0x08, mode at
  +0x90). This was **explicitly out of scope in the original design** (same
  exclusion as overlay-element-styling's `dance_effect`), now added by
  maintainer request. New hook `note_result_setup` (AOB unique on both
  builds) walks the vector after setup; unlike the lane bands the flash
  must **reposition** (converge toward lane center) as well as shrink, so
  the apply is `{s·a, b, s·c, s·d, Cx + s·(tx−Cx), ty}` with Cx = centroid
  of the panel flashes' tx. `cargo check` + `./build.sh` clean. NOT yet
  deployed.
  - ASSUMPTION to confirm on cabinet: the flash's PLAY (on each hit) does
    NOT re-drive its root layer matrix (i.e. the one-shot scale holds like
    the overlay clips' do). If a scaled flash snaps back to full size when
    it fires → needs a per-frame re-apply. Log line to watch:
    `lane receptor_flash layer=0x… s=… m=[…]→[…]`.

## NEXT ACTION (lane v4 validation)

Deploy; play at ≠100 % scale and hit arrows: the receptor hit-flash sparkle
should shrink AND sit centered over each scaled receptor (log:
`lane receptor_flash …`, one per panel + `LANE-DIAG note-result …`). Watch
that it holds across repeated hits (assumption above). Then finish the A8
pass and strip the LANE-DIAG logging in final cleanup.

- 2026-07-18 (lane v4 CRASH + root cause + fix): first lane-v4 build crashed
  on cabinet (`EXCEPTION_ACCESS_VIOLATION` during GAMEPLAY setup, top frame
  in `ddr_world_hook`). Root-caused **locally, without a diagnostic deploy**:
  - The crashing build's DLL+PDB were still in `target/…/release/`. The
    stack address `0x7FFCA8001D15` resolves against DLL base
    `0x7FFCA7FE0000` (NOT `…A8000000` — that would overlap gamemdx at
    `…A87F0000` given SizeOfImage 0x803000), i.e. crash RVA **0x21D15**.
  - `llvm-objdump` at that RVA = the `note_result_setup_cb` vector walk:
    `movq (%r15,%r13),%rax; incq %r13; … movl 0x8(%rax),%ebx` ← faulting
    insn. **Scale-1 addressing**: the walk advanced 1 BYTE per element.
  - Bug: `begin` was typed `*const u8` (the `read_unaligned` of a
    `*const *const u8` yields `*const u8`), so `begin.add(i)` strode 1 byte
    — overlapping misaligned reads → garbage non-null "clip" pointers →
    AV on the clip+0x08 layer-id read. The Ghidra offsets (+0xE8/+0xF0,
    +0x90, clip+0x08) were all CORRECT (re-verified vs `FUN_18007a230`:
    vector at `param_1[0x1d]/[0x1e]`, id at `plVar15[1]`, mode `[0x12]`).
  - Fix: `begin`/`end` typed `*const *const *const u8` (8-byte stride),
    plus defensive 8-alignment checks on begin/end and each element.
    Verified in the rebuilt binary: `movq (%r15,%r13,8),%rax` + `testb
    $0x7,%al` alignment guard. `cargo check` + `./build.sh` clean.
    NOT yet deployed.

## NEXT ACTION (lane v4b validation)

Deploy the fixed build; same test as lane v4: play at ≠100 % scale and hit
arrows — expect `LANE-DIAG note-result …` + `lane receptor_flash layer=0x…
s=… m=[…]→[…]` per panel, flash shrunk + centered over the scaled
receptors, holding across repeated hits (per-frame re-apply assumption).
Then finish the A8 pass and strip the LANE-DIAG logging in final cleanup.

- 2026-07-18 (lane v4b test — crash gone, 3 new findings): P1 single play
  (s=0.40) + center-arrows-1P, P2 slot at s=1.25. No crash; `note-result
  single n=4` fired. But:
  1. **Queue overflow**: 2 backgrounds + filter + 2 danger + 4 flashes = 9
     captures > PENDING_CAP 8 → 4th flash dropped (`lane pending queue
     full`).
  2. **Wrong side on the flashes**: `side=1 s=1.25` applied to a P1 lane.
     Root cause: with center-arrows-1P the panels (tx 496…784) center
     EXACTLY at 640.0 and the centroid split `cx < 640` broke the tie to
     side 1 → P2's values. (User read this as "not scaled" — it was
     transformed, by the wrong near-1 factor.)
  3. **Lane background apply FAILED this run** (`lane background apply
     failed` ×2, first time seen — previous versus runs worked): the MC
     get-scale returned failure at apply time. Cause unknown (stale id from
     the `matching_usr`-adjacent find-child pass?).
- 2026-07-18 (lane v5): fixes: PENDING_CAP 8→24, APPLIED_CAP 8→32; flash
  side now `Some(0)` for doubles / `None` for single → `resolve_side`
  presence read (per-clip tx split only in true versus) — centroid is used
  ONLY as the reposition anchor, never for side; background apply-fail warn
  now says whether the GET failed (stale-MC discriminator for finding #3).
  `cargo check` + `./build.sh` clean. NOT yet deployed.
- Maintainer observations queued: (a) freeze-arrow hit flash is YELLOW vs
  white for normals — believed to be the same `dance_effect` pool clips
  (different play label), so the v5 side fix should cover both; confirm
  visually, RE separately if the yellow one stays stock. (b) **Aliasing on
  scaled arrow/receptor edges** (both up- and downscale) — to dig into
  AFTER the flash work lands (likely point-sampling in the sprite path;
  needs RE of the sampler state used by `render_sprite_final`).

## NEXT ACTION (lane v5 validation)

Deploy; P1 single + center-arrows-1P at ≠100 %: expect 4× `lane
receptor_flash … side=0` with P1's `s`, converging toward lane center
(anchor 640), no `queue full` warn, and the white AND yellow (freeze) hit
flashes both scaled. Also note whether `lane background` applies or the new
`get-scale failed` warn fires (finding #3 discrimination).

- 2026-07-18 (lane v5 test): flashes now scale correctly (side fix + queue
  fix confirmed) but sit BELOW the receptors (`~/Desktop/position.png`):
  the fill's vertical map is `y' = s·y` (top-anchored), so the receptor row
  moves up at s<1 while the flash kept stock `ty`.
- 2026-07-18 (lane v6): flash `ty` now follows the fill's map — apply is
  `{s·a, b, s·c, s·d, cx + s·(tx−cx), s·ty}`. `cargo check` + `./build.sh`
  clean. NOT yet deployed.

## NEXT ACTION (lane v6 validation)

Deploy; same test: flashes should now sit ON the scaled receptors (both
white and yellow/freeze variants), hold across repeated hits, and converge
with the lane. Then: strip LANE-DIAG logging, finish the A8 pass. Still
watching: background `get-scale failed` warn (finding #3), aliasing
follow-up.

- 2026-07-18 (lane v6 test): flashes scaled + converged horizontally but
  rendered ABOVE the receptors (near the top of the playfield). Combined
  with v5 (stock ty → BELOW receptors), this brackets the real geometry:
  the fill's Y map is LANE-RELATIVE (y=0 at the receptor row — receptors
  don't move in screen space; my screen-space `s·ty` model was wrong), and
  scaling the layer matrix `a/d` also scales the art's internal offset from
  the layer origin, displacing the visual. Matrix-based scaling can't hit
  the mark without knowing that internal offset.
- 2026-07-18 (lane v7): mechanism change for the flash — matrix RMW is now
  translation-ONLY (`tx' = cx + s·(tx−cx)`, stock `ty`, a/d untouched);
  the uniform shrink goes through the clip's ROOT MC (`Ordinal_103(layer,
  "/")` in the setup decompile → `layer_find_child(layer_id, "/")`) via the
  component-based MC scale param 0x1003 — same primitive that scales the
  lane background in place about its registration point. New log line
  carries `root_mc=…` + before/after scale. `cargo check` + `./build.sh`
  clean. NOT yet deployed.

## NEXT ACTION (lane v7 validation)

Deploy; same test (≠100 % scale, hit arrows incl. freeze): flashes should
sit ON the scaled receptors. Log: `lane receptor_flash layer=… root_mc=…
tx=…→… scale=(1.00,1.00)→(s,s)`. If the art STILL displaces, the MC
registration point isn't the art center → next step is reading the MC's
position component (param 0x1000) before/after to solve for the true
offset. Then LANE-DIAG cleanup + A8 pass.

- 2026-07-18 (lane v7 test): root-MC scaling fixed the shrink (no more art
  displacement from a/d) but the flash sat slightly BELOW the receptors —
  keeping stock `ty` is also wrong. Maintainer's full-screen versus capture
  (`capture/capture_20260717_012459.jpg`, P1 down / P2 up) settled the
  geometry: the fill's vertical FIXED POINT is the TOP of the receptor row
  (the renderer's `posY` — quad `y` args are top-edge offsets from it), so
  the receptor CENTER moves to `posY + s·(centerY − posY)` and any fixed
  ty is off by `(1−s)·(ty − posY)`. All three rounds' offsets (below /
  above / slightly-below) are consistent with this model.
- 2026-07-18 (lane v8): flash `ty` now maps through the true fixed point —
  `ty' = posY + s·(ty − posY)` with `posY` read per side from the fill
  registry (new `TrackedRenderer.pos_y` from renderer+0x34, exposed as
  `fill_hook::side_anchor_y(side)`, Spot preferred). Apply supports
  `ApplyOutcome::Retry` (flash stays queued until its side's renderer
  binds). Bind log now prints `posY`. Flash log prints `ty=…→…
  (anchor_y=…)`. `cargo check` + `./build.sh` clean. NOT yet deployed.

## NEXT ACTION (lane v8 validation)

Deploy; same test: flash should sit exactly ON the scaled receptors at
both s<1 and s>1 (versus P1-down/P2-up is the sharpest test). Log lines:
`bind class=spot … posY=…` and `lane receptor_flash … ty=…→… (anchor_y=…)`
— anchor_y should equal the spot renderer's posY. Then LANE-DIAG cleanup +
A8 pass. Still open: background `get-scale failed` (finding #3), aliasing
follow-up.

- 2026-07-19 (lane v8 test): **RECEPTOR FLASH DONE** — maintainer confirmed
  the flashes anchor correctly on the receptors at both under- and
  over-scale; log confirms singles AND versus (both sides), `anchor_y=69`
  = the spot renderer's posY, Retry path working.
- 2026-07-19 (lane v9 — background dead end + REFRAME): attempted an
  apply-time re-refer for the failing background (capture parent + name,
  fresh `afp_layer_mc_refer` at apply). It ALSO failed — the captured
  parent is the PRE-GAMEPLAY dance movie, torn down before first fill (the
  live `dance_root` is pool-created after the stale finds). The decisive
  observation came from the maintainer: **lane backgrounds and covers look
  correctly scaled WITHOUT any background apply ever succeeding.**
  Conclusion: `1p_lane_usr`/`2p_lane_usr` are the HUD LAYOUT CONTAINERS
  (children = `judge_usr`/`arrow_usr`/… markers), NOT the visible lane art
  — the visible background scales through the fill quads + filter band
  alone. "Fixing" the apply would have scaled the container and shifted
  the HUD. Maintainer reverted the in-flight dance_root work.
- 2026-07-19 (lane v10 — cleanup, at maintainer request): removed the
  entire background path as dead code: `LaneKind::Background`, the
  `IdKind` enum (only Layer remained), the find-child detour + callback,
  the `cmovieclip_find_child` AOB, the `find_child` target field, and the
  temporary LANE-DIAG logging block (slated for final-cleanup anyway — all
  lane elements are cabinet-confirmed). Docs updated to match: research
  §4b (background correction + final flash mechanism + dead ends),
  AGENTS.md key-entry row. `cargo check` + `./build.sh` clean. NOT yet
  deployed.

## Remaining work

1. **Aliasing investigation — CLOSED (fix impossible at the sampler level;
   all texture-filter work REVERTED).** Chronology: (a) flipped the base
   sheet POINT→LINEAR → banding on arrows/freeze/receptors
   (`capture/capture_20260719_1054*.jpg`); (b) added half-texel UV inset +
   MIP experiments → still banded (`…_1410*.jpg`); (c) live CE session
   proved the base-sheet desc drives ALL lane art sampling and bisected
   POINT↔LINEAR bidirectionally; (d) root cause: the lane art is
   **palette-indexed** — the `gs_screencommand_arrow` pixel shader reads
   the atlas RED channel as an index into a 256×16 point-only palette on
   stage 1 (how note colors animate). LINEAR blends INDICES → unrelated
   palette colors → banding. POINT is load-bearing; the pixelation of
   scaled arrows is inherent (real fix = shader replacement or hi-res art,
   out of scope). Reverted: `texture_filter.rs`, `derive_texture_registry`,
   the UV inset. RE record: research doc §7. ACCEPTED CHARACTERISTIC:
   scaled playfield keeps nearest-texel aliasing.
2. **Regression-check deploy** of the v10-cleanup + texture-filter build
   (expect: no LANE-DIAG lines, no background warns,
   filter/cover/danger/flash still applying, smoothed arrow/receptor edges
   at ≠100 %).
3. **A8 acceptance pass** — run the checklist above, record results.
4. **Follow-up (separate task):** center-arrows-1P + doubles positioning
   bug (below).

## Follow-up tasks (out of scope for this feature)

- **Doubles + center-arrows-1P positioning breakage** (maintainer,
  2026-07-17, `capture/capture_20260717_013031.jpg`): playing DOUBLES as a
  single player with the `center-arrows-single` mod enabled badly breaks
  the arrow/receptor positioning — the 8-panel lane is shifted right
  (roughly the single-lane centering delta applied to a doubles layout:
  receptors/arrows render ~half off the playfield). Pre-existing
  `center_arrows_single` bug, NOT caused by playfield-styling. Likely fix
  direction: the layout-setter detour should skip its X shift when the
  play mode is doubles (mode==1 — e.g. read the renderer mode the way
  `playfield_styling::fill_hook` does, or gate on the lane-relative keys'
  doubles variants). To be picked up as its own task after playfield
  styling wraps.
  - **PICKED UP + FIXED 2026-07-19, cabinet-validated same day**: gated
    the setter-hook shift on the LayoutActor's per-side play STYLE
    (`builder_root+0x84+side*4`, `0=single/1=double` — the same field the
    game's lane-name selector branches on; note doubles reads `[1,1]`,
    the `2`/absent value was not observed live), read at builder entry.
    Doubles (style 1) now skips the shift entirely. Validated across
    multiple songs alternating singles/doubles: doubles stays centered,
    singles still centers. See
    `.agents/planning/20260612-center-arrows-single/implementation/plan.md`
    Step 10.

## Deviations & open questions

- **Research correction (important):** `render_notes`'s first CALL (byte
  order) is NOT the collector — stray 0xE8 displacement bytes come first,
  and the true first CALL targets a per-pass helper (0x180028780 on
  20260616). Derivation identifies the collector by content (callee whose
  first 0x100 bytes contain the XMM15 720.0 load) with a uniqueness
  requirement. Same result on 20260324 (helper 0x180027CA0 rejected,
  collector 0x1800240C0 verified).
- Guideline draw derived by prologue-AOB candidates × content check
  (XMM9 720.0 load + get_offset_y CALL within 0x800 bytes, exactly one
  winner) instead of the design's raw callee-set walk — equivalent
  classification, same hard-fail-on-ambiguity.
- Guideline cull site is patched together with the collector site in
  `cull_patch::install` (plan sequenced it under Step 5) — both are
  pre-verified before either write, sharing the one float slot.
- Mine integration needs NO explicit transform (see Step 6 note above) —
  `style_for_renderer` from design §4.5 was not needed and is not exposed.
- Doubles uses side-0 values per A7 even if P2 carded in (design-locked).
- HIDDEN/SUDDEN fade zones keep stock screen distances at scale≠100 —
  ACCEPTED (A5), documented in README + RE note.

## Key facts for a cold resume

- Feature dir: `.agents/planning/20260716-arrow-receptor-styling/`.
- Mod id `playfield-styling`; files: `src/mods/playfield_styling/{mod.rs,
  fill_hook.rs, cull_patch.rs, guideline_hook.rs}`; derivations:
  `src/core/signatures.rs::derive_playfield_styling` (+
  `derive_note_collector`, `derive_guideline_targets`,
  `find_rip_f32_loads`); mine touch-point: one line in
  `note_types_expansion/mine_render.rs::emit_mine_pass` (`render_height =
  playfield_styling::cull_bound()`).
- Never patch the shared 720.0 constant (14 readers) — disp32 redirect
  only, byte-verified, patch-once, disable = write 720.0 into the slot.
- One-detour-per-target: `render_notes` belongs to mine_render — this
  feature installs NO hook there (and none was needed).
- Rows register ONLY after the full gate (fill detour + both cull patches
  + both guideline detours); any failure → rollback + inert mod
  (`is_active()` false).
- Cabinet validation is maintainer-driven; deploy.sh pushes the DLL only —
  label PNGs are a separate copy.
