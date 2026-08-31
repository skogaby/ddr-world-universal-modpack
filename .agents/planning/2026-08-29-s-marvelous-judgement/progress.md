# Progress — S-Marvelous Judgement

Updated: 2026-08-30
Status: **FEATURE COMPLETE (uncommitted — maintainer commits manually).**
All 10 plan steps done; both Step-9 surfaces cabinet-verified (per-stage
banner + end-of-credit badge); Step-10 hardening + docs done; feature
marked complete by the maintainer 2026-08-30. Residual regression-sweep
items accepted as deferred (list below) — pick them up opportunistically
in future sessions, none block the feature.
NEXT ACTION: none. Maintainer supplies `screenshots/s_marvelous.png` for
the new README hero section, then commits.

Resume protocol: read `implementation/plan.md` (checklist = step status),
`design/detailed-design.md` (Approved 2026-08-29), task files under
`.agents/tasks/2026-08-29-s-marvelous-judgement/step<NN>/`.

## Residual sweep items (accepted-deferred at completion, 2026-08-30)

Not blocking; verify opportunistically if ever in doubt (everything is
fail-open — worst case is stock visuals + one latched WARN):

- Negative controls with the mod ON: stock MFC / PFC ⇒ stock emblem +
  badge (trick: JUDGEMENT OFFSET ≈ 14 ms makes honest on-beat steps
  loose Marvelous ⇒ an FC is a stock MFC).
- Emblem loop persistence over a long results dwell (~5 s+; no revert to
  rainbow was reported on deploy #1).
- Versus (per-side counts/emblems); course/Dan on all results surfaces —
  incl. the judged-filter watch item: if the COURSE record's note vector
  is empty while its streams are populated, the course score tab falls
  back to stock silently.
- Disabled boot ⇒ stock everything (esp. the Step-7 sheet purge).
- song_reset paths (quick restart instant + delayed, training
  scrub/loop), rate play, quick-fail then later-song score tab (the
  judged-slot gating change).
- pacemaker_swap / overlay_element_styling / calibration interplay.
- Step-8 page-switch + display-mode + two-songs checks; Step-7
  versus/course checks (deferred from their deploy #2s).

## Step 10 — hardening + docs DONE (2026-08-30, uncommitted)

- Serializer label-table SORT-ON-WRITE (`write.rs` — stable by name;
  stock tables already sorted so Leg A's 76-template byte identity
  holds) + host test `write_sorts_label_table_by_name`.
- id==death-frame invariant covered by the `edit_clone_opts_*` host
  tests (rebased ids asserted) + Leg G's `create id > label frame` check
  on the real template.
- Diagnostics trim: `DDR_SMARV_DUMP` dev dump removed (afp_patches.rs);
  flash readback (0x1010/0x1012 pair) reduced to the one-line first-fire
  INFO; combo `phase()` bisect logging removed; results_score's
  per-populate "results row live" INFO latched to first-fire.
- Partial-play robustness: `records::read_streams` now filters to
  JUDGED slots via the note-entry flags (pure `filter_judged` core + 2
  host tests) — quick-failed songs no longer trip the marvelous-counter
  cross-check (unjudged slots carry grade-0 garbage). Full plays filter
  to identity. WATCH ITEM for the sweep: if the COURSE record's note
  vector turns out to be empty/absent while its streams are populated,
  the course tab falls back to stock (fail-open) — check on cabinet.
- MSVC ABI structs promoted to `core/msvc.rs`
  (`MsvcString`/`MsvcVec<T>`/`SharedPtrPair`, sso/sso_bytes/heap_ref/set):
  music_wheel_song_length, results_score, results_graph migrated (the
  0x28-stride pad now lives in ONE place).
- Learnings sweep DONE (`.agents/learnings/learnings.md`): scanner
  overlapping-AC bug + cabinet-DLL byte-verify habit; vtable byte-offset
  arithmetic; 0x28-stride MSVC lesson; "ride the game's own helper
  mid-call" pattern; the 8 AP2/AFP engine invariants.
- Docs DONE: `docs/s_marvelous_judgement_research.md` §12 (shipped
  implementation record); AGENTS.md Key Entry Points row; README feature
  table + operator-config rows + a Highlights hero section
  ("S-Marvelous Judgement", placed in the judgement/timing cluster —
  image slot `screenshots/s_marvelous.png`, maintainer supplies the
  screenshot); data_mods README table (Step-9 art).

## Done

- Step 9 implemented (FC emblems — uncommitted, maintainer commits):
  - RE settled (Ghidra both builds + template dumps): the results scene
    builds its ONE layer from template **`result_root`**
    (`FUN_1800b84c0`); the fc timeline = self-contained sprite 243
    (labels loop_fc@50/gfc@175/pfc@300/mfc@417/life4@550/assisted@617,
    700 frames) placed as `fc_usr` by BOTH player panes ⇒ ONE patch
    covers 1P+2P. `loop_mfc` = frames 417–549; the word object (src
    chain sprite 204→203→shape 202→region `scre_fc_marvelous` 232×18)
    carries per-frame **HSL-shift update records** (the rainbow flow)
    and the segment loops via a **`gotoAndPlay("loop_mfc")` DoAction at
    frame 518**; the `_base` shadow (197) and `_ef` sparkle (192) chains
    are separate and stay stock. Total results: `FUN_1800cb090` builds
    per-stage `total_result` pane layers (actor+0x1B0+pane*8, one pane
    per stage whose PRIMARY side (actor+0x9C) record is non-virgin) and
    loads bitmap `scre_total_player_%s` (table DAT_180486E80,
    [10]="fc_mfc") into `fullcombo_usr` leaves under `total_p%d_top_usr`.
    Per-stage suffix table DAT_180486410 [10]="mfc" ⇒ `loop_mfc` via
    `afp_mc_op(0xF09)` on `player_%dp_info_usr/fc_usr` (layer at
    actor+0x108, layer id +8; stage actor+0xEC; course branch = the
    SAME `*(**g+0x70)` gate results_score replicates).
  - core/ap2: **`SegmentCloneOpts`** + `clone_labeled_segment_placements
    _only_ex` / `clone_segment_with_new_shapes_ex` (plain fns delegate,
    byte-identical default — host-tested): `drop_hsl_updates_on_remapped`
    (drops update records with flag 0x1 + HSL 0x20000000 whose
    (object_id,depth) matches a create placing a remapped character —
    violet art must not inherit the stock hue rotation) +
    `retarget_actions` (DoAction 0x7A string-offset-table entries whose
    TARGET STRING == src_label re-pointed at the interned new_label;
    header: FF sentinel, flags&1 ⇒ u16 count@+2, u16 LE string-table
    offsets @+4 — content-matched, not offset-matched, duplicates safe).
    PLUS the **split-dictionary recipe variant**
    (`clone_segment_with_new_shapes_split`, auto-selected): result_root
    keeps its label in a NESTED sprite while ALL definitions live in
    ROOT — the resolution is a SEGMENT-SCOPED DOWNWARD closure from the
    segment's placed ids (a whole-section fixpoint would absorb the
    label sprite's ancestors and clone the scene); dance_fc's local
    topology keeps the original code path byte-identical. 3 new host
    tests (HSL drop scoped to remapped objects, DoAction retarget
    old-vs-clone, default-opts byte identity).
  - `core/signatures.rs`: `result_window_build` (@0x1800B8AA0 20260721 /
    @0x1800B88A0 20260616) + `total_result_populate` (@0x1800CB090 /
    @0x1800CB170) — both prologue AOBs verified exactly-once in BOTH
    Ghidra builds AND byte-verified against the on-disk cabinet DLL
    (file offsets 0xB7EA0 / 0xCA490).
  - `assets.rs::stage_emblems` (+ `EMBLEM_*` consts, `EMBLEM_CLONE_OPTS`
    shared staging/patch/harness): result_root extract/descramble,
    geo-first word-shape resolution (fc_region_rename rule — UNIQUE on
    result_root: shape 202), dry-run, rewritten geo
    (`result_root_shape<new>` → region `scre_fc_smarvelous`) + geo MD5
    mapping + afplist extension, per-image PNGs + ONE atlas batch of TWO
    sets: donor-anchored `scre_fc_smarvelous` (cloned geo UVs need the
    donor rect) + FRESH `scre_total_player_fc_smfc` (name-only
    mc_load_bitmap binding, combo-digit precedent).
  - `results_emblem.rs` (NEW): two best-effort post-original detours.
    Per-stage: when a side's record is S-MFC (predicate below), re-drive
    `0xF09 loop_smfc` on `player_%dp_info_usr/fc_usr` with the
    `mc_frame_by_label` (0x1012) pre-check as the resolve observable
    (deploy-#6 lesson) — gated on ASSETS_READY + PATCH_APPLIED. Total:
    replicate the populate's pane↔stage rule, and for each S-MFC
    (side, stage) walk `total_p%d_top_usr/fullcombo_usr` leaves
    (traversal-6) re-loading `scre_total_player_fc_smfc`. S-MFC
    predicate (record-only, never live counters): mcode != -1 &&
    clear_kind(+0x54) == 10 && smarv==marv && marv>0, with
    `state::last_armed_window(side)` (0 ⇒ stock). Course branch shared
    via `results_score::course_active()`.
  - Art: `data_mods/s_marvelous/scene_result/scre_fc_smarvelous.png`
    (232×18) + `scre_total_player_fc_smfc.png` (30×12) — the established
    colorize (hue 280°, sat floor 150, value ×0.82; verified max-diff-1
    vs the shipped dance_judge art). Wording kept ("Marvelous
    Fullcombo!!!" / "MFC"), violet hue — the maintainer's art language.
    STATIC violet by design (the HSL drop removes the rainbow flow).
  - Harness **Leg G** (`smarv-emblem` mode): the DLL's ACTUAL recipe on
    the REAL result_root — checks HSL-drop (0 survivors on the cloned
    word), DoAction → loop_smfc (and stock's stays loop_mfc),
    id==death-frame (create 804 > label 700), label tables sorted,
    serialize + bemaniutils parseafp acceptance (string table
    re-scrambled) + loop_smfc@700 in sprite 243. Validate script also
    grew `KEEP_TMP=1` (keeps the temp workdir for debugging).
  - mod.rs wiring: results_emblem::install (init, best-effort) /
    activate (enable) / deactivate (disable).
  - Gates: 116 lib + 83 bin host tests, Legs A–G green, cargo check
    clean, fmt, ./build.sh clean.
  - CABINET DEMO PENDING — see "Step 9 deploy #1".

- Step 8 implemented (judgement graph — uncommitted, maintainer commits):
  - `records.rs`: `NoteRef` + pure `smarv_per_second` (mirror-bucketing:
    one stream slot per flag≥0 note entry, judged gate entry+0x18==0,
    t_first = first judged timestamp, bucket = (t−t_first)/1000) +
    impure `read_note_refs` (0x60-stride note-entry reader, fail-closed).
    5 new host tests (unjudged slots advance index, window edge, empty,
    short streams, mismatch rejection).
  - `core/signatures.rs`: `graph_tab_rebuild` / `graph_chart_append` /
    `graph_legend_text` (all Ghidra-verified unique on 20260721 AND
    20260616; addresses in the Step-8 RE entry below).
  - `results_graph.rs` (NEW): the three-detour injection design from the
    RE entry — rebuild pre-original one-shot (per-tab registry: build our
    per-second vector, pad to the game series length, subtract from
    marvelous(+0x5D8)/shimmer(+0x5F8) clamped, leftover WARN); chart-
    append detour injects our violet series before the PERFECT append
    (vec-identity gate `vec == tab+0x5B8`, live-captured lambda vftable,
    stack callable+vec — the append deep-copies both); legend detour
    re-calls the original with "■S-MARVELOUS" (SJIS ■) + violet after the
    stock "■MARVELOUS" line (rgba 0xF0F0F0FF gate, still-live stack ctx
    does the layout). Registry cleared on EVERY scene change (mod.rs
    scene callback); re-entry from injected calls fails the gates; the
    registry lock is never held across game calls. Gates: window>0,
    entered side, non-virgin record, has-data byte, judge page only,
    course branch shared with results_score (`course_active` now
    pub(super)).
  - mod.rs: `results_graph::install` at init (best-effort),
    activate/deactivate, scene-change registry reset.
  - Gates: 113 lib + 80 bin host tests, Legs A–F green, cargo check
    clean, fmt, ./build.sh clean.
  - CABINET DEMO PENDING — see "Step 8 deploy #1".

- Step 8 RE settled (2026-08-30, Ghidra both builds):
  - Ingest (`FUN_1800EB9C0`, vslot 6): ONE grade/ms stream slot per
    flag≥0 note entry (unjudged entries ADVANCE the index but add
    nothing — the judged gate is entry+0x18==0); t_first = first judged
    entry's timestamp (entry+0x08); bucket = (t−t_first)/1000. Judge
    series at tab+0x538+k*0x20 (MSVC vector<double>, 0x20-stride groups):
    [0]=filler grey, [1]=miss, [2]=good, [3]=great, [4]=perfect,
    [5]=+0x5D8 marvelous **+ O.K. combined**, [6]=+0x5F8 all-marv
    shimmer (post-pass swaps a second's 5↔6 when pure). has-data =
    tab+0x1C4; page = tab+0x138 (0=judge); display mode = tab+0x1C0.
  - Rebuild (`FUN_1800ED610`, vslot 7, per frame): clears+rebuilds
    charts (tab+0x178) and legend texts (tab+0x1A0) every frame behind a
    current-frame≤label-frame gate. Page-0 legend via
    `FUN_1800F15E0(&ctx{rect*,cursor*,tab}, &string, rgba)` — cursor is a
    running x-advance the fn updates; MARVELOUS = "\x81\xA1MARVELOUS"
    (SJIS ■) rgba 0xF0F0F0FF. Series appends: marvelous pair via
    `FUN_1801CFEE0` (two-color), others via `FUN_1801CFF60`
    (single-color) — callable = 0x20-byte MSVC std::function
    {vft, rgba u32, pad, impl_ptr→self}; cff60 DEEP-COPIES the series
    data (FUN_1801D10F0 push-back) and CONSUMES the callable (impl ptr
    nulled) — stack-local args are safe.
  - STRATEGY (zero new editing primitives, no rect/cursor math, no
    static lambda-vft derivation): detour the rebuild (pre-original
    per-tab ONE-SHOT: build our per-second smarv f64 vector from the
    record, mirror-bucketing; subtract per second from series 5 then 6,
    clamped; registry keyed by tab ptr) + detour cff60 (when
    vec==registered_tab+0x5B8 (perfect) on page 0: INJECT our series
    first — callable vft CAPTURED LIVE from the incoming argument's impl
    (same (uint,double,double) lambda family), violet rgba — then pass
    through ⇒ our series sits between marvelous and perfect) + detour
    f15e0 (when ctx.tab registered, page 0, rgba==0xF0F0F0FF: pass
    through MARVELOUS then call the original AGAIN with
    "\x81\xA1S-MARVELOUS" + violet — the still-live stack ctx advances
    the cursor for us). Re-entry from our own injected calls fails the
    gates (different vec/rgba); drop the registry lock before calling
    game code (same-thread Mutex re-entry = deadlock). Registry cleared
    on every scene change (recycled tab allocations).
  - Signatures (unique on 20260721 AND 20260616): `graph_tab_rebuild`
    @0x1800ED610/@0x1800ED1B0, `graph_chart_append` (cff60)
    @0x1801CFF60/@0x1801CF410, `graph_legend_text` (f15e0)
    @0x1800F15E0/@0x1800F1180. cfee0 NOT needed (injection rides cff60).
  - Violet: 0xB05CE0FF (art-matched deep violet, the combo tint's deep
    member). Legend may be tight horizontally — if the cabinet shows
    overflow, shorten to "\x81\xA1S-MARV.".

- Step 7 implemented (results score tab — uncommitted, maintainer commits):
  - `records.rs` (NEW, std-only, harness-mounted): pure `count_smarv` /
    `count_grade` cores + fail-closed raw record-stream readers
    (`read_streams` — MSVC vector bounds checks, 64K note cap, empty
    null/null vectors legal, marvelous-counter cross-check guards layout
    drift; `smarv_count_from_record` / `marv_count_from_record`). 7 host
    tests. `state.rs` grew `last_armed_window(side)` (sticky across the
    GAMEPLAY-exit disarm — the results recompute needs the song's window).
  - `core/ap2/edit.rs`: `shift_row_translates(rows, expected_each)` —
    generic (depth, ty)-keyed row mover over f0 placements AND update
    records, validate-then-mutate via a scratch-clone dry run (any count
    mismatch ⇒ doc untouched). 2 host tests on a results-tab-shaped
    fixture (dual timeline, guest updates, decoy row).
  - `core/signatures.rs`: `playdata_tab_update` (populate vslot 7,
    prologue AOB — giant 0xB70 frame + 0x151/0x110 reads; the
    "marvelous_num_usr" string's ONLY xref is inside; Ghidra-verified
    unique on 20260721 @0x1800F6BC0 AND 20260616 @0x1800F6140) +
    `playdata_row_write` (the game's row-write helper @0x1800F8370 /
    @0x1800F78F0 — make_shared SpriteLayer, glyph conversion, push into
    tab+0x158 so the GAME owns layout + destruction) +
    `derive_smarv_results_course_gate` (the populate's own record-branch
    global: `*(**g+0x70)!=0` ⇒ course record; byte-identical shape both
    builds).
  - `results_score.rs` (NEW): ROW_MOVES table (6 rows → 16px grid,
    Leg F extracts it mechanically — one source of truth) +
    `apply_row_moves` (the transform the patch fn, staging dry-run AND
    harness share); afp_patcher patch on `body_tab_detail_result`
    (byte-gate vs staged stock; refusals PURGE the sheets so art and row
    positions always move together); post-original detour on the populate
    (dirty byte read PRE-call = exact populate detection): recomputes
    smarv from the record (same course/stage branch as the game),
    rewrites the stock marvelous widget's glyphs to `stock − smarv` via
    `spritelayer_set_names` (widget found by anchor-name in tab+0x158;
    ours distinguished by its −16 offset_y), then creates the S-MARV row
    through the game's OWN row-write helper (anchor `marvelous_num_usr`,
    offset_y −16 ⇒ inherits the anchor's guest-move + fade updates; the
    game lays it out per frame and destroys it with the tab; our
    shared_ptr ref released with the full MSVC dtor dance). Idempotent
    across re-populates (existing row reused). Gates: entered-side +
    non-virgin record + window>0 (silent bails), latched WARNs otherwise.
  - `assets.rs`: `stage_results` (template extract/descramble/dry-run +
    sheet staging) / `restage_result_sheets` / `purge_results`. The
    sheets are STOCK-NAME replacements served passively from disk, so
    `mod.rs::init` purges them unconditionally (init runs even
    config-disabled — a disabled boot must not leave 7-row art under
    6-row positions) and enable restages; `ifs_textures` grew
    `purge_texture_replacement` (cache file + CACHE_INDEX entry).
  - Harness Leg F: `smarv-rows` mode runs the DLL's real transform on the
    real template (rows extracted from results_score.rs via sed) — all 6
    rows moved (4 records each: f0+f127, root+sprite130), zero stale
    positions, serializes; bemaniutils cross-check confirms the six named
    instances at ty 59/75/91/107/123/139 in both timelines.
  - Gates: 108 lib + 80 bin host tests, Legs A–F green, cargo check
    clean, fmt, ./build.sh clean.
  - CABINET DEMO PENDING — see Deploy & test log "Step 7 deploy #1".

- Step 7 strategy settled + art shipped (2026-08-30, maintainer-approved):
  - Package dump (scene_result_v3): ONE template `body_tab_detail_result`
    (exported name = the afp_patcher key) serves BOTH judgement-count tabs
    via frame labels — `loop_registered`@f18 = Details (numbers tx=139),
    `loop_guest`@f130 = Simple results (numbers tx=264 via translate
    UPDATE records at f127). Row labels are ONE stacked sheet texture per
    style: `scre_tab_detail_judge` (Details, baked via shape 74 / instance
    `judge` at (56,91)) and `scre_tab_detail_base` (Simple — no geo
    references it ⇒ runtime bitmap swap into the same instance; Ghidra
    confirm pending). Six num-row instances `marvelous_num_usr`..
    `miss_num_usr` at ty=43..138 (19px pitch), depths d23..d18, duplicated
    root + sprite 130 (dual timeline), alpha fades at f150/151. Digit
    glyphs `scre_tab_num_0..9` are 10×12 px.
  - STRATEGY (Option 1 of 3, approved): sheet swap + translate-only
    compression. Replace both sheets with 7-row art (same 108×118 canvas,
    16px pitch, S-MARVELOUS top); AP2 patch = translate-only
    `adjust_placements` splices moving the 6 stock num instances to the
    16px grid (f0 placements + f127 guest updates, both timelines) — NO
    new objects/shapes/labels ⇒ none of the engine invariants in play; the
    S-MARV number = mod SpriteLayer anchored on `marvelous_num_usr` with
    offset_y=−16 ⇒ inherits the guest-tab move AND the fades from the
    anchor's own update records. Rejected: full structural patch (new
    instance needs cloned f127/f150/f151 update records = new editing
    primitive + id/death-frame care in a 300-frame section); zero-edit
    overlay (crams the 7th row against the gauge/panel edge).
  - New-row grid (template coords): ty 43,59,75,91,107,123,139
    (S-MARV..Miss); sheet-local row centers 11,27,43,59,75,91,107.
  - Art SHIPPED to `data_mods/s_marvelous/scene_result/` (maintainer will
    hand-polish; likely ships as-is): stock sheet uniform-scaled ×16/19
    (LANCZOS), right-aligned (paste x = 108−91=17), 6-row block at y=+18;
    S-MARV row = scaled-sheet crop y0..20 with 4px bottom alpha feather,
    colorized hue 280° / sat floor 150 / value ×0.82 (parameters recovered
    pixel-exact from `dance_judge/smarvelous.png` vs its donor, max diff
    2), pasted at y=+2. Known flaw (maintainer accepted, hand cleanup
    planned): base sheet's violet row carries a sliver of Perfect's shadow
    (the crop cuts into the row below). Generator (one-shot, PIL):
    extract sheets via `arcutils`+`ifsutils --convert-texture-files` from
    `scene_result_v3.arc`, then per sheet:
    `scaled = stock.resize((91,99), LANCZOS)`; compose 108×118 with
    `scaled @ (17,18)`; `marv = scaled.crop((0,0,91,20))` + linear alpha
    feather rows 16..19 + colorize(280,150,0.82) `@ (17,2)`.

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
- Step 9 deploy #2 check (2026-08-30): **PASS.** Maintainer verified the
  end-of-credit summary badge — the S-MFC stage's pane shows the violet
  MFC badge (the total_result_populate detour + FRESH texture path).
  BOTH Step-9 surfaces cabinet-confirmed. FEATURE MARKED COMPLETE by the
  maintainer; remaining regression-sweep items accepted as deferred (see
  "Residual sweep items" below).
- Step 9 deploy #1 (2026-08-30, DLL + data_mods): **PASS (core path).**
  Maintainer confirmed the violet S-MFC "Marvelous Fullcombo!!!" caption
  on the per-stage results screen — the result_root patch applied live,
  the 0xF09 loop_smfc re-drive resolved, and the cloned segment renders.
  STEP 9 DEMO DONE. Not individually confirmed (maintainer can't
  reliably produce a stock MFC/PFC by hand — folded into the Step-10
  sweep): the TOTAL-results violet badge, the ~5 s loop-persistence
  watch (no revert to rainbow reported), and the negative controls
  (stock MFC / PFC ⇒ stock art; trick if wanted: a manual play with
  JUDGEMENT OFFSET ≈ 14 ms turns honest on-beat steps into loose
  Marvelous — an FC then is a STOCK MFC with the mod on).
- Step 9 deploy #1 pre-flight expectations (kept for reference):
  `[+] result_window_build @ +0xB8AA0` / `[+] total_result_populate @
  +0xCB090` / `SMarvelous: FC emblem detour(s) installed`; at enable
  `SMarvelous: emblems staged (word shape 202 -> 440, region
  scre_fc_marvelous -> scre_fc_smarvelous, badge
  scre_total_player_fc_smfc)`; at the first results-scene template load
  `SMarvelous: result_root patched (95420 -> 101448 bytes, loop_smfc)`
  (sizes assuming the stock template matches the dev copy); on an S-MFC
  result `SMarvelous: S-MFC stage emblem re-drive (side S, mc ..., label
  frame 700)` and on total results `SMarvelous: S-MFC total badge
  re-drive (side S, stage N, K leaf(s))`. NOTE: first boot after the new
  atlas inputs may need ONE reboot (mounted-texturelist staleness rule —
  atlas-cloner inputs changed). Demo checklist:
  1. Autoplay (all-S) one song → per-stage results: violet "Marvelous
     Fullcombo!!!" caption (STATIC violet — no rainbow flow; the sparkle
     + dark shadow stay stock) instead of the stock rainbow; the emblem
     must persist (loop) for the whole screen, not revert to rainbow
     after ~2 s (the DoAction retarget check).
  2. Total results: the same stage's pane shows the violet MFC badge.
  3. A stock MFC (loose marvelous > ±12 ms present): BOTH surfaces show
     the STOCK rainbow emblem/badge (predicate smarv==marv fails).
  4. A PFC stage: stock PFC emblem everywhere (kind != 10).
  5. Versus if convenient: per-side emblems (side 0/1 records
     independent).
  6. Course/Dan if convenient: per-stage panes on total results, course
     record on the per-stage screen.
  7. Watch item (Step 4/6 precedent says harmless): the cloned create
     records carry stale ON_LOAD `aep_set_set_frame(417)` bytecode — if
     the emblem misbehaves on SEEK-heavy paths, this is the first
     suspect.
- Step 8 deploy #2 (2026-08-30, DLL only): **PASS.** Play Graph tab
  shows the violet S-Marv series LEADING the judge stack (adjacent to
  the white marvelous segments), violet ■MARVELOUS legend entry first,
  white second, whites correspondingly reduced. STEP 8 DEMO DONE.
- Step 8 deploy #1 (2026-08-30): **FAIL — root-caused same day, CORE
  SCANNER BUG.** Boot log: `[-] graph_tab_rebuild -- pattern not found`
  (other two graph sigs resolved; fail-open kept the graph stock, no
  crash). The pattern's bytes ARE in the cabinet DLL (file offset
  0xECA10 — byte-verified against the on-disk gamemdx). Host repro
  (temp crate over the real DLL bytes + all 109 signature patterns)
  pinned it: `scan_patterns_batch` builds ONE aho-corasick automaton
  over every signature's literal run and iterates with `find_iter` —
  which is NON-OVERLAPPING across the whole automaton, so one pattern's
  needle hit gets consumed by a DIFFERENT pattern's earlier-ending hit
  over the same bytes (shared prologue idioms — graph_tab_rebuild
  starts `40 55 41 54 41 55 41 56 41 57`, an extremely common frame
  setup). A LATENT bug that could silently eat any future signature;
  individual `scan_pattern` calls also drop periodic self-overlapping
  needles. FIX: `find_overlapping_iter` in all three AC paths
  (scan_pattern_inner / scan_pattern_all_inner /
  scan_patterns_batch_inner); host repro confirms graph_tab_rebuild
  resolves and no other signature changes address. LEARNINGS-SWEEP
  ITEM (Step 10): scanner-core entry + "byte-verify AOBs against the
  CABINET DLL file, not just Ghidra" habit.
  ALSO (maintainer directives from the demo, implemented): the S-Marv
  legend entry goes FIRST (before the stock white ■MARVELOUS, injection
  moved pre-original) and reads just "■MARVELOUS" in violet (no "S-"
  prefix — matches the shipped art language); the series injection
  moved from after-marvelous to after-FILLER (vec gate tab+0x538,
  post-original with the functor vftable captured BEFORE the original
  consumes the callable) so the violet series leads the judge stack.
  Gates re-green: 113/80 host tests, Legs A–F, check/fmt/build clean.
- Step 8 deploy #2 (PENDING, DLL only): same checklist as deploy #1
  (violet bars now FIRST among judge colors; violet ■MARVELOUS legend
  entry before the white one); boot log must show
  `[+] graph_tab_rebuild @ +0xED610`.
- Step 8 deploy #1 pre-flight expectations (kept for reference):
  `[+] graph_tab_rebuild` / `[+] graph_chart_append` /
  `[+] graph_legend_text` + `SMarvelous: judgement-graph detours
  installed`; on the Play Graph tab
  `SMarvelous: graph series prepared (side S, N buckets)` (first tab
  only, latched). Demo checklist:
  1. Manual play with a smarv/marv mix → Play Graph tab: violet bars in
     the NOTES/SEC chart adjacent to the white marvelous segments; the
     white segments correspondingly reduced; "■S-MARVELOUS" legend entry
     (violet) after ■MARVELOUS. Watch for legend overflow past the panel
     edge — fallback is shortening to "■S-MARV.".
  2. Autoplay (all S-Marv): white marvelous bars ≈ only O.K. counts;
     violet carries the rest; shimmer seconds (all-marv) show
     shimmer-minus-violet + violet.
  3. Page switch (Play Graph → timing page and back): no violet residue
     on the timing page; series/legend reappear on return.
  4. Display-mode toggle (NORMAL → detail/gauge/bpm, the "Switch
     display" control): violet series behaves on all modes' judge chart.
  5. Two songs in a session: second song's graph correct (per-tab
     one-shot + scene reset).
- Step 7 deploy #1 (2026-08-30, autoplay song): **NEAR-PASS.** Both tabs
  (Simple + Details) show the 7-row sheet at 16px pitch, violet
  S-MARVELOUS on top with the full count (514), alignment good on both
  layouts, S-MARV row follows the guest move. ONE discrepancy: the
  exclusive MARVELOUS row rendered NO glyph instead of "0". Root cause
  pinned same day: results_score's re-derived `MsvcString` was 0x20 bytes
  but the game's `vector<string>` elements are 0x28-STRIDE (16-byte
  buf/ptr union + len + cap + 8 TRAILING PAD — music_wheel's
  cabinet-proven GameString had the pad; the re-derivation dropped it).
  `set_names` walks the source at 0x28 stride ⇒ my 1-element 0x20-stride
  array measured ZERO elements ⇒ empty glyph list. The S-MARV row was
  immune (single-string params are read by pointer, no stride). FIX:
  `_pad: u64` on the struct (both ctors zero it). Step-10 note: promote
  GameString/GameVec to a shared core module — two hand-copies now exist.
  Gates re-green: check/fmt/build clean.
- Step 7 deploy #2 (2026-08-30, DLL only): **PASS.** Autoplay: MARVELOUS
  row shows "0" (stride fix confirmed), S-MARV carries the full count.
  Manual play: correct smarv/marv split, rows sum to the note total,
  alignment good on both tabs. STEP 7 DEMO DONE. Deferred to the Step-10
  sweep (maintainer choice): versus, course, disable-toggle stock revert.
- Step 7 deploy #1 pre-flight expectations (kept for reference):
  1. Normal 1P song → BOTH tabs (Simple results + Details): 7 rows at the
     compressed pitch, violet S-MARVELOUS on top with its count;
     MARVELOUS shows `total − smarv` (cross-check vs the song-end log
     line `smarv=N marv_total=M`); the seven rows sum to the stock total.
  2. Label↔number alignment on both tabs (the Simple tab is the moved
     +125px guest layout — our row must follow it).
  3. Autoplay: MARVELOUS row 0, S-MARVELOUS row = all marvelous.
  4. Versus: per-side counts on each side's window.
  5. Digit size vs the smaller labels (maintainer wanted an on-cabinet
     look before deciding on scaling — one-line change if crowded).
  6. Course/Dan if convenient (course-record branch).
  7. Mod toggle OFF → next boot: stock 6-row sheet + stock positions
     (init purge log line `SMarvelous: unstaged ...`).
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
