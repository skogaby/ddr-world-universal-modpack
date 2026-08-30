# Progress — S-Marvelous Judgement

Updated: 2026-08-30
Status: Step 4 of 10 — implementation done; CABINET DEMO PENDING (maintainer)
NEXT ACTION: maintainer deploys (`./scripts/deploy.sh`) and runs the Step 4
demo (see Deploy & test log, deploy #2). NOTE: this deploy needs BOTH the
DLL AND the `data_mods/s_marvelous/` asset dir on the cabinet, and the
FIRST boot after deploy rebuilds the atlas — the flash may need ONE reboot
to appear (house atlas-rebuild rule).

Resume protocol: read `implementation/plan.md` (checklist = step status),
`design/detailed-design.md` (Approved 2026-08-29), task files under
`.agents/tasks/2026-08-29-s-marvelous-judgement/step<NN>/`.

## Done

- Step 6 implemented (S-MFC splash — uncommitted, maintainer commits):
  - Appendix-B dump DONE: the four main templates are SELF-CONTAINED
    (each carries the 4 marvelous-art shapes; the `which_fullcombo_*` /
    `effect_*_*` sub-templates are other scenes' surfaces — not in the
    splash path). `marbelous_in` (sic) in root + inner sprite (158/214)
    — the dual-timeline rule again. Art shapes SHARE sprite chains
    (sprite 123 places {117,120}, 148 places {120,127,130}) ⇒ per-shape
    chain clone impossible.
  - core/ap2: `clone_segment_with_new_shapes` (NEW) — multi-shape
    generalization: reachability fixpoint over the section's sprites,
    children-first topological clone order, one cumulative remap, then
    the placements-only segment clone into EVERY labeled section
    (object-id death-frame shift inherited). Host test on a shared-chain
    fixture (dedup, leaf remap completeness, shared non-art shape kept,
    unknown-id fail-closed).
  - `core/signatures.rs`: `fullcombo_actor_on_message` (prologue AOB,
    `CMP EDX,0x1034` pins uniqueness — module-unique, Ghidra-verified).
  - `assets.rs::stage_fullcombo`: per template — geo-first art-shape
    resolution (rename rule: prefix `s` onto the last `_`-token iff it
    starts with `mar`: `dafu_eff_mar`→`dafu_eff_smar`,
    `dafu_light_marvelous`→`dafu_light_smarvelous`; shipped art files
    already named accordingly), dry-run, rewritten geos, MD5 mappings,
    afplist geo extensions (multi-entry per IFS — the Step-4 mechanism
    handles 4 templates in one afplist rewrite); once per IFS —
    donor-anchored atlas clone + per-image PNGs.
  - `splash.rs` (NEW): post-original detour on the message handler;
    predicate `msg==0x1034 && type==0 && combo_is_all_smarv(side)`
    (side = first dword of `*(this+0x88)`); re-drives `*(this+0x98)`'s
    mc to `s_marbelous_in` via 0xF09 (Step-4's proven op + all sections
    carry the label). Never re-plays the SE. Registers the 4 afp_patcher
    patches at first activate (byte-gated vs staged stock, id-verified
    vs the dry run).
  - Harness Leg E: the recipe on all four REAL templates (geo-first
    resolution + label/sort checks in every section) — green:
    single 117/120/127/130→159..162, double 159/162/169/172→215..218,
    3 sprite clones each.
  - Gates: 99/78 host tests, Legs A–E, fmt/build clean.
  - Cabinet demo (2026-08-30): **S-MFC splash VALIDATED** (maintainer
    confirmed the S-MARVELOUS splash on an all-S full combo). The
    non-S-MFC path was NOT explicitly tested (maintainer accepted):
    stock-by-construction — predicate false ⇒ no re-drive ⇒ the stock
    label the original just played stands. STEP 6 DEMO DONE.

- Step 5 implemented (combo digits — uncommitted, maintainer commits):
  - `core/signatures.rs`: `combo_digit_refresh` (prologue-anchored AOB,
    tint-immediates run 0xA9FEEC/0xDFA6EF pins uniqueness; verified
    exactly-once on 20260721 in Ghidra; cookie disp wildcarded).
  - `assets.rs::stage_combo_digits`: FRESH-mode atlas entries
    (`smarv_dc` prefix, donor `daco_combo_marvelous_0` for encoding) +
    per-image PNGs at `dance_combo_v3_ifs/tex/daco_combo_smarvelous_%d.png`
    (per-image serving — the deploy-#4/#5 lesson applied from the start;
    NO geo/afplist/AP2 patch needed: `afp_mc_load_bitmap` binds by
    texturelist image name alone). Arc = unsuffixed `dance_combo_v3`
    (deploy-#2 rule). Stock digits 104×120 imgrect; our art 100×118.
  - `combo.rs` (NEW): post-original GenericDetour on the refresh;
    predicate `worst(this+0x6C)==0 && combo_is_all_smarv(side)` (side =
    `**(this+0x58)`, verified against the stock decompile which reads
    the same); replicates the stock traversal-6 walk for places
    {10,100,1000} with the display-clamped combo (min 9999), ones place
    left stock (quirk); tint pair via wrapper vfunc+0x98 float[4]
    (stock's own call shape, verified in the decompile — float conv =
    (c>>16&FF)/255 etc. matches). Violet pair 0xE9C8F8/0xB05CE0 (root2
    light / root3 deep — mirrors the stock light/deep pairing). Gated on
    `ASSETS_READY` + panic-contained; self-healing per design (no
    cleanup path).
  - `mod.rs`: combo::install at init (data_feed-gated),
    stage_combo_digits at enable, assets_ready dropped at disable.
  - state.rs: +5 host tests (the §4.5 override-predicate sequences:
    all-S keeps, loose drops, O.K. neutral, combo-break resets, grades
    1–3 degrade).
  - Gates: 98/77 host tests, Legs A–D, fmt/build clean.
  - CABINET DEMO PENDING: combo ≥ 4 all-S ⇒ violet digits + tint;
    first loose Marvelous ⇒ stock repaint; O.K. steps neutral. Tint
    SEMANTICS (multiply vs add on vfunc+0x98) are the design's flagged
    live-verify item — if the violet looks wrong, tune TINT_ROOT2/3
    in combo.rs (compiled constants).
  - Combo deploy #1 (2026-08-30): CRASH at the first combo refresh
    (EXCEPTION_ACCESS_VIOLATION, IP = float-data garbage). Phase-logged
    bisect build pinned it in ONE cycle: died at `tint root2 start`.
    Bug: `vtable.add(0x98 / 8)` — `vtable` is a BYTE pointer after
    `read_unaligned()`, so the offset advanced 19 BYTES (mid-slot read →
    wild call). Fix: `.add(0x98)`. LESSON (learnings sweep): raw vtable
    slot reads in Rust — keep the pointer as `*const u8` and offset in
    BYTES, or cast to `*const usize` and offset in SLOTS; mixing the two
    is exactly one keystroke away. flash.rs unaffected (no raw vfunc
    calls). Phase logging left in (first-override-only, ~8 lines/session).
  - Combo deploy #2 (2026-08-30): **VALIDATED ON CABINET.** Maintainer
    confirmed: violet digits + violet tint tracking the all-S combo,
    correct per-grade color adjustment on drop (marvelous/perfect/great
    repaints stock). Tint constants approved as-is (match the word art).
    STEP 5 DEMO DONE.
- PDD complete: register accepted (Readiness Confirmed 2026-08-29), design +
  plan Approved 2026-08-29.
- Research: `research/orientation.md` (infra seams), `research/afp-tooling.md`
  (bemaniutils spec + repo AP2 gaps), `research/display-side-re.md` (all five
  display surfaces RE'd on 20260721, AOB anchors verified).
- Step 1 implemented (all 3 tasks Complete, uncommitted — maintainer commits):
  - `src/mods/s_marvelous/state.rs` — pure classification core + atomics
    (10 host tests via `scripts/validate_s_marvelous.sh`).
  - `data_feed.rs` tap block (all grades 0..=6, combo from actor+0x1DC,
    disarmed cost = one relaxed load).
  - `src/mods/s_marvelous/mod.rs` lifecycle + `s_marvelous.window_ms` config
    (clamp 1..=17, default 12) + lib.rs registration (default ON).
  - Gates: cargo check ✓, validate script 10/10 ✓, cargo fmt ✓, ./build.sh ✓.
- Step 2 implemented (all 3 tasks Complete, uncommitted):
  - `src/core/ap2/` — full AP2 document model, parser, serializer
    (std-only/harness-mountable; 38 host tests; PlaceObject strategy:
    opaque payload + decoded view + from-scratch encoder; string table
    carried verbatim with append-only interning).
  - `scripts/validate_s_marvelous.sh` dev legs: Leg A real-template
    round-trip — **76/76 templates byte-identical** across
    dance_judge0000_v0 / dance_fullcombo0000_v0 / scene_result_v3; Leg B
    bemaniutils parseafp cross-check 3/3 structural match. Legs skip
    cleanly without `DDR_WORLD_INSTALL`/bemaniutils.
  - Template intel: score tab template = `body_tab_detail_result` (RE's
    "detail_result" is the creation-call arg — confirm mapping in Step 7);
    the MARVELOUS word is its own tiny `marvelous` template (clean Step 4
    clone donor); dance_fullcombo also carries per-grade `effect_*` /
    `which_fullcombo_*` template families (Step 6 fidelity decision).
- Step 3 task-01 implemented (Complete, uncommitted):
  - `src/core/ap2/edit.rs` — the five editing primitives on `Ap2Doc`:
    `add_label`, `add_shape` (id = max+1, inserted inside the target frame's
    span per docs §9 ordering), `clone_labeled_segment` (+`TagRemap` =
    `HashMap<u16,u16>`; segment = label frame → next label / section end;
    tags+frames append, label added), `add_place_object_named`
    (`NamedPlacement` struct; per-frame depth uniqueness; FrameSpan fixups),
    `adjust_placements` (doc-wide recursive, predicate-scoped).
  - Key decision: PlaceObject id-remap and translate adjustment are
    SURGICAL BYTE SPLICES at flag-determined offsets (never a
    `PlaceObject::build` rebuild — that would drop the unmodeled
    color/event/filter tail real segment animations carry). Matched tags
    without a translate field are skipped and not counted (caller WARNs).
  - Failure atomicity everywhere: validate-then-mutate, intern last-fallible;
    any `None` leaves the doc serialize-byte-identical.
  - +17 host tests (65 total in the harness lib mount); Leg A still 76/76
    byte-identical, Leg B 3/3. Gates: cargo check msvc ✓, cargo fmt ✓.
  - Working docs:
    `.agents/scratchpad/2026-08-29-s-marvelous-judgement/editing-primitives/`
    (progress.md carries the exact API signatures + caller gotchas for
    task-02 and Steps 4/6/7/9).

- Step 3 implemented (both tasks Complete, uncommitted):
  - `src/core/ap2/edit.rs` — the five editing primitives (add_label,
    add_shape, clone_labeled_segment + TagRemap, add_place_object_named,
    adjust_placements as a surgical translate splice); 65/65 host tests.
    Gotcha: inserts shift tag indices — re-resolve SpritePaths after any
    insert (append-only ops are safe).
  - `validate_s_marvelous.sh` Leg C: edit-demo on the REAL dance_judge —
    remapped variant structurally verified (new shape id referenced by the
    clone); identity variant accepted by bemaniutils parseafp
    (`in_smarvelous @ frame 600`); cloned segment (38 frames) RENDERED via
    AFPRenderer and visually verified (shows stock "Marvelous!!!" art).
    Render preview: `$TMPDIR/s_marvelous_preview/in_smarvelous_identity.gif`.
  - Facts: in_marvelous segment = 38 frames; word placement references
    character id 8; dance_judge root timeline carries stop() bytecode
    (render previews must neutralize DoActions; in-game is label-driven).

- Step 4 task-01 implemented (Complete, uncommitted):
  - `src/core/ap2/edit.rs` — definition-aware cloning:
    `clone_sprite_definition` (deep copy under a fresh id with recursive
    internal remap) + `clone_labeled_segment_placements_only` (dictionary
    never duplicated). 73 lib / 63 bin host tests; Legs A/B/C green.
  - Working docs + the exact task-02 API notes:
    `.agents/scratchpad/2026-08-29-s-marvelous-judgement/definition-aware-cloning/`.

- Step 4 task-02 implemented (Complete, uncommitted):
  - `src/core/geo.rs` (NEW, std-only, promoted from
    folder_expansion::patch_ge2d_labels): GE2D label read + rewrite with
    LENGTH-CHANGING rebuilds (in-place when the new name fits — byte-
    identical to the shipped equal/shorter behavior; append+repoint+
    filesize update when longer). folder_expansion now wraps it (its old
    code silently truncated longer keys). 8 host tests + real-shape32
    oracle run (176→203 bytes, bemaniutils Shape.parse validates).
  - `src/core/ap2/edit.rs` — `Ap2Doc::clone_word_segment_with_new_shape`
    (the §10 recipe as a game-agnostic core/ap2 fn: dynamic word-sprite
    resolution from the segment's placements, add_shape →
    clone_sprite_definition → placements-only clone, all ids from return
    values, ambiguity fails closed). 4 host tests incl. byte-identity vs
    the hand-driven primitive sequence.
  - `src/mods/s_marvelous/assets.rs` (NEW): enable-time staging — arc/IFS
    extraction + descramble, word-chain resolution (geo labels via
    core::geo), recipe DRY RUN to precompute the ids the patch will
    allocate (deterministic: max_character_id+1 on fixed bytes), geo
    rewrite → `data_mods/s_marvelous/dance_judge0000_v0_ifs/geo/
    dance_judge_shape{N}` + `register_afp_geo_mapping`, donor-anchored
    cache-guarded atlas clone (region `dance_judge0000_smarvelous` at the
    donor rect), mod-paths rescan-once.
  - `src/mods/s_marvelous/afp_patches.rs` (NEW):
    `register_patch("dance_judge")` wiring + the patch fn (v1 skin gate =
    byte-compare vs staged stock bytes; id verification; latched WARNs;
    fail-open to stock) + `patch_ready()` / `patch_applied()` for task-03
    (applied stays latched across disable — loaded templates remain
    patched; gate re-drives on applied && active).
  - `validate_s_marvelous.sh` Leg D: REAL recipe + REAL geo rewriter on
    the real template → render `in_smarvelous` with the new region mapped
    to the placeholder art → 38 non-blank frames, violet word confirmed.
    Preview: `$TMPDIR/s_marvelous_preview/in_smarvelous_patched.gif`.
  - Facts: real chain = sprite 35 → shape 32 → `dance_judge0000_marvelous`;
    allocated ids shape 54 / sprite 55 (derived, never hardcoded). Open
    question for the cabinet deploy: whether the runtime opens the new
    geo's MD5 on demand or only afplist-listed geos load (fallback plan in
    the task's scratchpad progress.md).
  - Working docs:
    `.agents/scratchpad/2026-08-29-s-marvelous-judgement/dance-judge-patch/`.
  - Gates: 85 lib + 75 bin host tests, Legs A/B/C/D green; cargo check
    msvc ✓; cargo fmt ✓; ./build.sh ✓.

- Step 4 implemented (all 3 tasks Complete, uncommitted):
  - core/ap2: definition-aware primitives (`clone_sprite_definition`,
    placements-only segment clone) + the game-agnostic
    `clone_word_segment_with_new_shape` recipe; `core/geo.rs` NEW (GE2D
    label rewrite with length-changing rebuilds; folder_expansion now
    wraps it — latent truncation bug fixed).
  - `s_marvelous/assets.rs` (enable-time staging: arc/IFS extract,
    donor-anchored atlas clone `dance_judge0000_marvelous` →
    `dance_judge0000_smarvelous`, geo synthesis + MD5 mapping, id
    precompute via recipe dry-run) + `afp_patches.rs`
    (`register_patch("dance_judge")`, patch_ready/patch_applied).
  - Flash re-drive: shared clip capture (overlay_element_styling
    `ensure_capture_installed` + `judge_clip`; tracking decoupled from
    styling enable; remove() guarded), `bm2d_api::mc_op_str` (0xF09
    goto-label), `flash.rs` re-drive wired to the classification return.
  - Offline render proof (Leg D): the FULL synthesized chain renders the
    violet word — visually verified frame dump.
  - Gates: 85/75 host tests, Legs A–D green, cargo check 0 warnings, fmt,
    ./build.sh clean.

## In flight
- Step 1 cabinet demo (maintainer gate — see Deploy & test log).

## Deploy & test log
- Deploy #2 (2026-08-30, Step 4 demo attempt): **FAIL — root-caused + fixed
  same day.** Chain worked end-to-end until the skin gate: the game loads
  the UNSUFFIXED `dance_judge_v3.arc` (log: `no mod folder for
  'dance_judge_v3_ifs'`; patch fn refused: "variant differs from the staged
  default-skin template") — the `dance_judge0000_v0`-style arcs we staged
  from are skin/revision variants the game never opens. Fail-open worked
  exactly as designed (stock visuals, one WARN, classification unaffected:
  53/53 autoplay line). FIXES: (1) all three placeholder sets regenerated
  from the LIVE `_v3` packages (v3 donors differ: `daju_marvelous` 260×90,
  combo digits 100×118 + NO per-grade caption texture, fullcombo family
  gains `dafu_rocket_*`, drops ring/rsring); (2) assets.rs repointed at
  `dance_judge_v3.arc` / `dance_judge_v3_ifs`; (3) the word chain in v3 is
  THREE sprites deep (46 → 43 → 42 → shape 41) — the recipe's word-sprite
  resolution + cloning generalized to transitive chains (bottom-up clones
  with cascading remaps, `word_chain` resolver, nested fixture test);
  (4) dev legs repointed at the `_v3` arcs — Leg D now patches + renders
  the REAL live template (violet angled v3 word art visually verified).
  Gates re-green: 86/76 host tests, Legs A–D, check/fmt/build clean.
  CABINET CLEANUP for deploy #3: delete the stale
  `data_mods/s_marvelous/dance_judge0000_v0_ifs/` dir + old cached atlas if
  present.
- Deploy #3 (2026-08-30, Step 4 retry): **FAIL — root-caused + fixed same
  day.** New failure moved EARLIER: enable-time staging WARN
  `dance_judge word chain unresolved (unknown structure)`. Root cause:
  `assets.rs::resolve_word_chain` still used the OLD single-level
  sprite-walk (candidate sprite → DIRECT shape placement) — v3's 3-deep
  nesting broke it. Leg D hadn't caught it because the harness resolved
  the shape id via a PARALLEL bemaniutils scan instead of the DLL's code.
  FIXES: (1) geo-first resolution promoted into core/ap2 as the SHARED
  `find_word_shape_by_geo(src_label, suffix, geo_labels_closure)` —
  identifies the word shape by its geo's `*_marvelous` region label,
  independent of sprite nesting; assets.rs now delegates to it;
  (2) harness `smarv-patch` resolves through the SAME shared fn (takes
  geo_dir, not a precomputed id) with the bemaniutils scan demoted to a
  cross-check assert ("oracle agrees"); (3) host test
  `edit_find_word_shape_by_geo` (happy/ambiguous/none/unknown-label).
  Gates re-green: 87/77 host tests, Legs A–D (resolver: shape 41,
  daju_marvelous → daju_smarvelous, oracle agrees), check/fmt/build clean.
  LESSON (for step 10 learnings sweep): dev legs must exercise the DLL's
  actual code path, never a parallel reimplementation.
- Deploy #4 (2026-08-30, Step 4 retry 2): **FAIL — but the deepest layer
  yet: patch + flash both LIVE.** Log showed staging OK, template patched
  (10188 → 11140 bytes), `flash live — first in_smarvelous re-drive` —
  the label jump works. Two GAME-side warnings isolated the remaining
  gaps to ASSET SERVING:
  (a) `afpu-ngp: binary file[...tex/7fdbbb5b...] open failed` —
  `7fdbbb5b… = md5("daju_smarvelous")`: this IFS family stores texture
  data PER-IMAGE (verified by unpacking dance_judge_v3 AND
  select_music_folder_v3 — no atlas blobs in either container); the game
  opens `tex/md5(image_name)` per image and composes the atlas itself.
  atlas_cloner's per-ATLAS cache blob is never requested here. The proven
  serving path (folder_expansion's shipped pattern) is a per-image PNG at
  `{ifs_mod_path}/tex/{image_name}.png` via `handle_texture` (converts +
  pads 260×90 → imgrect 262×92; known 1px offset caveat unchanged).
  (b) `afp-mip: can not find geo id [dance_judge_shape63] in stream` —
  the Appendix-B open question ANSWERED: geos load STRICTLY from the
  afplist `<geo>` id list at IFS mount; no on-demand MD5 fallback. The
  scratchpad's flagged fallback (duplicate `<afp>` node via the
  append-only merger) was rejected as designed; instead built a targeted
  afplist rewrite: NEW std-only `avs_layeredfs/afplist_ext.rs`
  (`extend_afplist_geo` — extends the EXISTING entry's id list + __count;
  6 host tests incl. exact-name/no-count/fail-open) + registry/serve in
  `ifs_textures` (`register_afplist_geo_extension`,
  `rewrite_afplist_if_extended` → cache write, `has_afplist_extensions`
  cheap gate) + both afplist branches in `file_hooks.rs` (merge and
  plain) serve the rewritten text XML (game accepts text where kbin was —
  proven by the served merged texturelists; our kbin reader's output
  shape verified against the transform).
  assets.rs staging now: registers the afplist extension (shape id) +
  stages `dance_judge_v3_ifs/tex/daju_smarvelous.png` (copy of WORD_PNG)
  + rescan check extended to the new file.
  Gates re-green: 93/77 host tests, Legs A–D, check/fmt/build clean.
- Deploy #5 (2026-08-30, Step 4 retry 3): **FAIL — but the entire asset
  chain went green.** Log: afplist extended (23 mapped, was 22), ZERO
  game-side warnings, patch applied, `flash live` re-drive INFO. Stock
  word still on screen. Root cause found in the TAP, Ghidra-validated
  before shipping the fix: `data_feed`'s judge_submit_hook called
  `flash::on_smarvelous` BEFORE `detour.call(original)` — but
  `judge_submit` (`FUN_18005fd30`) ends by dispatching the grade opcode
  0x1028+g through the actor tree SYNCHRONOUSLY (vtable+0x18 +
  `FUN_18022eaa0` recursive child walk, no queue), so the stock
  `in_marvelous` play ran inside the original AND AFTER our jump —
  clobbering it every event. Research §8 line 424 specified
  "post-original"; the tap drifted. FIX: classification stays
  pre-original (state ordering unchanged); the re-drive is deferred via
  a `smarv_side` local to after `detour.call`. Ghidra also validated:
  (a) op equivalence — stock path = label→frame 0x1012 + `mc_op 0xF08`
  (SetFrame twin wrapper `0x180270030`); ours = `0xF09` (SetFrameLabel
  twin `0x1802700e0`, identical play-flag selection) — same end state;
  (b) NO per-frame re-assertion — NoteResultActor vtable `0x180363200`
  (identity confirmed via slot-0 cleanup of +0xA0/+0xA8/+0xB0 clips) has
  a shared ret-only no-op in both update slots, so the jump persists
  until the next judgement; (c) grade-0 events take the FAST/SLOW
  clip's hidden branch — no interference.
  Gates: 93/77 host tests, Legs A–D, check/fmt/build clean.
- Deploy #6 (2026-08-30, Step 4 retry 4): **PARTIAL — the re-drive now
  WINS (stock word gone) but the in_smarvelous segment renders BLANK**
  (autoplay: no judgement texture at all). Server-side audit: served
  per-image texture byte-perfect (14361 B avslz, 96416 B decompressed =
  262×92 BGRA, 10094 opaque px), staged geo correct (normalized UVs =
  donor uvrect in 1024×512 space, label daju_smarvelous), merged
  texturelist correct, ZERO game-side warnings. libafp RE round
  (libafp-win64.dll imported to the Ghidra project):
  * Friend's tip VERIFIED: `FUN_18011dcc0` BINARY-SEARCHES the label
    table (bsearch `FUN_180132610`, name comparator) — labels must be
    stored sorted by name; bemaniutils scans linearly and can't catch a
    violation. OUR table happens to sort correctly (`in_smarvelous`
    sorts after `in_perfect`; verified via a /tmp labelcheck harness on
    the real patched output — root + all sprite sections sorted). NOT
    the current bug, but a mandatory serializer invariant (→ Step-10
    hardening: sort-on-write + host test).
  * CRITICAL FINDING: `mc_op(0xF09)`'s internal label lookup failure is
    SWALLOWED — the op logs and returns SUCCESS with no seek. Our
    "flash live" INFO therefore never proved the label resolved.
  * Stream loader (`FUN_180021a00`) cleared as a suspect: frame/tag
    record arrays are sized from the recomputed section header (covers
    appended frames); the character dictionary (`FUN_18004cec0` ordered
    insert / `FUN_18011d480` bsearch-by-id) tolerates mid-file
    definition insertion (insert is position-sorted, not file-order).
  * SetFrame (`FUN_1800efe10`) walks intermediate frames with
    display=0 then plays the target — stock in_ng@263 already exercises
    long forward seeks; frame 600 is structurally fine.
  FIX SHIPPED (flash.rs + bm2d_api): re-drive now mimics the STOCK grade
  handler byte-for-byte — `mc_frame_by_label` (new bm2d_api fn, param
  0x1012 with label string — the failure IS observable) + `mc_op(0xF08,
  frame)`. The label-resolve WARN closes the blind spot: if the live MC
  turns out to be the inner sprite-62 timeline (both root and sprite 62
  carry the in_* label set; our clone patched ROOT), the next deploy
  says so explicitly instead of rendering blank.
  Gates: 93/77 host tests, Legs A–D, check/fmt/build clean.
- Deploy #7 (2026-08-30, Step 4 retry 5): outcome (c) — **"flash live
  (side 0, label frame 600)" and STILL BLANK.** High-value facts: the
  0x1012 lookup resolving 600 PROVES the live clip's loaded stream is
  the PATCHED bytes (600 exists only there) and the ROOT section was the
  right one. The 0xF08 clamp is cleared too (its bound comes from the
  recomputed section header = 638, `FUN_1800d2730` reads the parsed
  frame count). Remaining fork: the seek doesn't actually land (frame
  silently redirected), or it lands and the cloned frames execute
  without drawing.
  Diagnostics shipped for deploy #8: (1) flash.rs reads back the clip's
  CURRENT frame post-seek (param 0x1010, the pacemaker handler's own
  getter) into the "flash live" INFO — readback==600 ⇒ content problem,
  anything else ⇒ seek problem; (2) afp_patches.rs dev dump — with env
  `DDR_SMARV_DUMP=1` the EXACT patched bytes handed to the game are
  written to `data_mods/_cache/smarv_debug_dance_judge.bin` for offline
  re-render (localizes live-seam divergence vs the harness output).
  Gates: host tests + Legs A–D green, fmt/build clean.
- Deploy #8 (2026-08-30, Step 4 retry 6): **`label frame 600, readback
  Some(0)` — the discriminator PAID OFF.** Decoded: the 0xF08 numeric
  path's clamp (`FUN_1800d2730`) read the OUTER wrapper mc's total as 1
  (root-instance clips have no defining tag → the getter's fallback),
  clamping 600→0; the 0x1012 label lookup meanwhile consults the
  STREAM's root section (hence 600 resolved). Full model that reconciles
  every deploy: the dance_judge clip's VISIBLE word timeline is an
  `aep_dummy` CHILD instance playing the INNER sprite-62 section — which
  carries its OWN copy of the in_* label set and its own frame
  numbering. libafp's 0xF09 label-op handler AUTO-REDIRECTS to that
  child (flag 0x4000000 + name gate vs the aep_dummy global) and
  resolves the label against the CHILD's table. Deploys #6/#7 (0xF09):
  redirect hit the child, whose table had NO in_smarvelous (root-only
  clone) → lookup swallowed → wrote root-numbered 600 anyway / sought
  garbage → blank. Deploy #8 (0x1012+0xF08): consulted root table,
  clamped on the outer mc → frame 0. Stock works because its handler's
  actor-stored wrapper + numbering line up.
  FIX: (1) core/ap2 recipe now clones the segment into EVERY section
  carrying `in_marvelous` (`collect_sections_with_label` walk; sprite
  62 verified offline: `in_smarvelous @ 600` — its section also has 600
  stock frames, numbering matches root — both tables still name-sorted);
  definitions stay root-global so the single cloned chain serves all
  sections. (2) flash.rs back to 0xF09 (the engine's own child-redirect
  + per-clip label resolution do the right thing now that every timeline
  has the label), with the 0x1012 pre-check kept as a lookup-failure
  observable and the 0x1010 readback kept in the first-fire INFO.
  Gates: 93/77 host tests, Legs A–D green (multi-section clone), offline
  labelcheck verified, fmt/build clean.
- Deploy #9 (2026-08-30, Step 4 retry 7): **still blank** —
  `flash live (side 0, 0xF09, readback Some(0))` with the multi-section
  clone live (patched 10188 -> 11532, sprite 62 carries the label). The
  captured pool wrapper is conclusively the WRONG OBJECT (readback 0
  across both op families, two template shapes). Stop guessing at it:
  FIX shipped = drive the wrapper the stock handler drives.
  flash.rs now resolves the `NoteResultActor` in the judge_submit
  dispatch actor's OWN SUBTREE (child list +0x18 / sibling +0x10 —
  offsets straight from the judge_submit + FUN_18022eaa0 decompiles;
  vftable identity via the RTTI-resolved `note_result_actor_vtable`,
  pacemaker_swap precedent; depth ≤4 / ≤64 siblings, null-safe,
  read-only) and drives `NoteResultActor+0xA0` — the exact wrapper
  `FUN_18007b300`'s 0x1028 case drives every stock grade. Captured-clip
  path demoted to fallback. `on_smarvelous(side)` grew an actor param
  (data_feed passes the dispatch actor). First-fire INFO now logs
  `via_actor true|false` + readback. mod.rs feeds the vtable at init
  (best-effort). Gates: 93/77 host tests, Legs A–D, fmt/build clean.
  Maintainer offered a Windows + Cheat Engine MCP live session —
  DEFERRED until this deploy reports: via_actor=true + readback on the
  ACTOR's wrapper is stock-identical targeting; if THAT still blanks,
  live memory inspection is the right escalation (inspection list:
  the actor wrapper's mc id vs captured mc id, the aep_dummy child
  chain +0x168/+0x58 of each mc, and which mc the visible word actually
  ticks on).
- Deploys #10–#11 + TWO Cheat Engine live sessions (2026-08-30, Windows
  machine, CE MCP): **ROOT CAUSE FOUND AND FIXED — engine object-id ==
  DEATH FRAME invariant.**
  Session 1 (deploy #10, via_actor true, readback 0, still blank): found
  the live NoteResultActor by vtable scan (REAL vtable RVA +0x3631D8 —
  the boot log's RTTI line; my +0x363200 guess from the msg-handler xref
  was the middle of the table, msg handler sits at +0x40). Actor's
  wrapper mc was AT FRAME 606 — inside the cloned segment! — with ZERO
  children; the parsed section had frame 600's two PlaceObject records
  byte-perfect (src remapped 66, dictionary carrying 63–66). So the seek
  works and the engine REFUSES the creates. Jumptable → PlaceObject
  handler at `libafp FUN_1800d4520`+0x1CC: `MOVZX objid,[payload+6];
  CMP objid,[RSP+0x50]; JLE skip`.
  First theory (id watermark) shipped as a section-max rebase (302/303)
  in deploy #11 — WRONG: still blank. Prologue disasm settled it:
  `[RSP+0x50]` is the executor's TARGET-FRAME argument (spilled at
  +0x45F8). The real invariant: **object id doubles as the object's
  death frame; catch-up only creates when `object_id > target_frame`**
  (stock data obeys it everywhere: word 32 ≈ dies frame 32, in_perfect
  70/76 follow label 38, in_ng 295/301 follow 263).
  Session 2: verified deploy #11's 302/303 live (mc at 610, no
  children), poked all 5 cloned-frame records to old+600 ids (632/638)
  — poke does NOT persist across songs (per-song stream re-parse), so
  validation moved to the recipe fix itself.
  RECIPE FIX (edit.rs): cloned PlaceObject ids shift by the FRAME
  DISTANCE the segment moved (new id = old + (new label frame − old)),
  preserving id↔death-frame for any seek. Root: 32→632, 38→638 —
  byte-identical to the live poke. Gates: host tests + Legs A–D, fmt,
  build clean.
- LIVE VALIDATION (2026-08-30, CE session 3, running song — no freeze):
  poked the fresh song's 5 cloned-frame records to the shift-rule ids
  (302→638 create + 303→632 create+3 updates) mid-play; **maintainer
  CONFIRMED THE VIOLET MARVELOUS ART ON-SCREEN.** The id==death-frame
  invariant is proven end-to-end; the built deploy-#12 DLL emits these
  exact ids from the recipe.
- PENDING deploy #12 (BOTH machines, DLL only — data_mods unchanged):
  formality — the recipe fix is live-proven byte-identical. After deploy:
  mark Step 4 demo DONE. Step-10 hardening list: label-table
  sort-on-write + id/death-frame invariant host test + drop the
  DDR_SMARV_DUMP dev dump + trim the flash readback diagnostics to the
  one-line INFO.

## Deviations & open questions
- Commits: maintainer commits manually (house rule) — code-assist Commit
  steps skipped; task completion recorded as
  `Complete (uncommitted — maintainer commits manually)`.
- Per-song log emits at GAMEPLAY exit only; in-place resets clear counters
  silently (PUS precedent).

## Key facts for a cold resume
- Feature: presentation-layer S-Marvelous (±12 ms ⊂ Marvelous ±17); engine
  grade space untouched; classification in the judge_submit detour tap
  (data_feed.rs, calibration-tap precedent); display via runtime AFP
  synthesis (afp_patcher + core/ap2 NEW in Step 2/3) + per-surface detours.
- Mod id `s-marvelous`, global toggle only, config `s_marvelous.window_ms`.
  No PUS integration, no option rows, no wire fields.
- Cabinet build for RE reference: gamemdx 20260721 (Ghidra project
  DDRWorld_Ghidra has it open).
- `scripts/validate_s_marvelous.sh` auto-mounts `src/core/ap2/mod.rs` once
  Step 2 creates it.
