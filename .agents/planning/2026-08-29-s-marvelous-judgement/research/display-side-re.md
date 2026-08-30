# Display-Side RE — S-Marvelous surfaces (gamemdx 20260721)

Date: 2026-08-29. Read-only Ghidra sessions against `gamemdx_20260721.dll`
(addresses file-relative to base `0x180000000`, per house convention).
Complements `docs/s_marvelous_judgement_research.md` §3. Covers: results score
tab, results graph tab, FC emblems, FC splash, combo digits.

---

## 0. Headline findings

1. **The stage record stores per-note streams**: grade byte per judged note
   (`record+0xB8..0xC0`, `vector<u8>`: 0=M, 1=P, 2=Gr, 3=Gd, 6=OK) AND
   **signed ms error per judged note** (`record+0xD8..0xE0`, `vector<i16>`),
   plus a note-entry vector (`+0x98..0xA0`, 0x60-byte entries, timestamp ms at
   `+0x08`). ⇒ **S-Marv counts and graph data can be recomputed at results
   time purely from the record** (`grade==0 && |ms| ≤ 12`), independent of the
   mod's live counters.
2. **Results score-tab numbers are `sequence::SpriteLayer`** — the exact class
   the modpack already constructs and drives (music_wheel_song_length). The
   native path for the S-MARV row's number is a mod-owned SpriteLayer anchored
   on a new named instance.
3. **The FC splash clip is NOT visible to the existing CMovieClip::Create
   capture** (creation is inlined) — a new detour on the FullcomboActor
   message handler is needed (module-unique AOB found).
4. The graph tab **rebuilds all charts/texts every frame** — the mod must
   participate via a detour, not one-shot injection. Legend entries are real
   font text (no texture needed for "■S-MARVELOUS").

---

## 1. Results scene dispatch skeleton

- Per-stage results = `sequence::result::ResultSequence`; main builder
  `FUN_1800b8aa0` creates two `sequence::result::WindowActor` (one per side,
  scene+0x498; ctor `FUN_1800c3360`, vftable `0x1803696d8`).
- `WindowActor` vslot 4 (`FUN_1800c35c0`) builds the tab vector
  (WindowActor+0x88..0x98). Tab kind 1/6 = **PlaydataTab** (ctor
  `FUN_1800f6940`, vftable `0x18036e0a8`), kind 7 = **GraphTab** (ctor
  `FUN_1800eb3a0`, vftable `0x18036d188`). Each tab: `+0x128 ResultSequence*`,
  `+0x130 side`, `+0x134 versus`, `+0x148 record side`, `+0x14C stage`,
  `+0x110 CMovieClip wrapper` of the tab template.
- `WindowActor::onUpdate` (`FUN_1800c4df0`, per frame) → tab base
  `FUN_180045af0` → once: vslot 6 (+0x30) setup/ingest (guard byte tab+0xBA);
  every frame while visible: **vslot 7 (+0x38)** = the populate/update fns
  below.

## 2. Score tab — `FUN_1800f6bc0` (PlaydataTab vslot 7)

- **Widget lookup is fully name-string driven** — `afp_layer_mc_refer(layer_id,
  "path/name")` where `layer_id = *(u32*)(wrapper+8)`,
  `wrapper = *(tab+0x110)`. Arbitrary new names (a patched-in
  `smarvelous_num_usr`) resolve identically. Helpers: `FUN_18026f0e0`
  (child-clip wrapper by path from the global wrapper pool `DAT_180c2be20`,
  0x210-byte slots, max 0x400), `FUN_18026f5e0` (load bitmap into all leaf
  clips), `FUN_18026f830` (visibility).
- **Numbers are SpriteLayers, not text.** Row-write helper
  `FUN_1800f8370(ctx, out_sp, name_str, text_str)`:
  `make_shared<sequence::SpriteLayer>` (`FUN_180038f50`, ctor
  `FUN_1801d2e00` — the modpack's `spritelayer_ctor`), glyph conversion
  `FUN_1801d2c00(&glyphs, text, "scre_tab_num_%s")` (digits →
  `scre_tab_num_0..9`, `+-` → `scre_tab_num_plusminus`, other symbols via
  `FUN_1801d25a0`), widget fields: `+0x60` parent wrapper, `+0x68` anchor
  instance name (e.g. `"marvelous_num_usr"`), `+0x94/+0x98` clip range,
  `+0x9c/+0xa0` alignment, `+0xa4/a8/ac` RGB tint, `+0xe0` scale; set-names =
  `FUN_1801d3070` (the modpack's `spritelayer_set_names`); pushed into the
  tab vector `tab+0x158`.
- **Population gating**: heavy populate runs behind dirty flag
  `PlaydataTab+0x151` (set in ctor; no other setter found — likely
  create-once; minor uncertainty). BUT the tail loop calls every widget's
  vtable[0] layout **every frame**, re-applying glyphs. ⇒ direct MC pokes get
  overwritten; a mod-owned SpriteLayer on a NEW instance name (stock never
  touches it) is stable; alternatively rewrite the stock widget's glyph list
  via `FUN_1801d3070`.
- Row values (record offsets, confirm prior RE): score +0x10, EX +0x14,
  maxcombo +0x20/+0x24, M +0x28, P +0x2C, Gr +0x30, Gd +0x34,
  miss = NG(+0x44)+Miss(+0x3C)+Boo(+0x38), OK +0x40, fast +0x6C, slow +0x70,
  gauge% record+0x1C8 (float); best score from
  `ResultSequence+0x450+side*0x28`.
- **AFP template exported name = `"detail_result"`** (set into tab+0xc8;
  created via `FUN_18026ecb0(wrapper, layer, "detail_result", 0)`). GraphTab
  template = `"graph"`. Results package family: `scene_result_*`. Textures:
  `scre_tab_num_%s`, `scre_tab_detail_*`, etc.
- **Row label words (MARVELOUS/…) are baked template art** — no runtime
  bitmap loads for them. The S-MARVELOUS label requires the AFP patch (cloned
  placement referencing an injected texture, e.g. `scre_tab_detail_smarv`).
- **Total results (scene 32) has NO per-grade counts** —
  `TotalResultSequence` populate `FUN_1800cb090` (from onUpdate
  `FUN_1800c9a90`), package `"total_result"`: jacket/rank/score/EX/flare/FC
  per stage pane only.

**Mod mechanism**: AFP patch on `detail_result` (clone marvelous row label
placement + add `smarvelous_num_usr` instance, reposition rows as needed) +
texture injection; detour `FUN_1800f6bc0` post-call (read `+0x151` pre-call to
detect populate), then create/refresh a mod-owned SpriteLayer anchored on
`smarvelous_num_usr` under parent `*(tab+0x110)`; S-Marv count from the mod's
tracker or recomputed from record streams (§0.1); also rewrite the stock
marvelous widget's glyphs to (stock − n) via `FUN_1801d3070` on the vector
entry in `tab+0x158`.
**AOB**: `FUN_1800f6bc0` anchors on the unique `"marvelous_num_usr"` LEA xref
(+ imm 0x2B8/0x590 nearby); helpers derived via call-site scanning from the
anchor. `spritelayer_ctor`/`set_names` signatures already ship and match.

## 3. Graph tab — `FUN_1800ed610` (GraphTab vslot 7)

- **One-time ingest** `FUN_1800eb9c0` (vslot 6) reads the record streams (§0.1)
  and aggregates into **per-1-second section `vector<double>` series** stored
  on the GraphTab (section = `(t_ms − t_first)/1000`):
  judge graph at `tab+0x538..0x610` — 7 series: [0] +0x538 filler 0x888888,
  [1] +0x558 miss 0xEC2136, [2] +0x578 good 0x0770FF, [3] +0x598 great
  0x33C400, [4] +0x5B8 perfect 0xFFCE0B, [5] +0x5D8 marvelous 0xF0F0F0,
  [6] +0x5F8 **all-marvelous shimmer** 0xDEA7EF/0xA9FEEC (a post-pass moves a
  second's column 5→6 when the section is marv-only — native precedent for a
  special-marvelous visual tier). Combo graph +0x618, timing histogram
  +0x378, fast/mav/slow +0x4B8, trend +0x2F8 (+ polyline +0x238/+0x258).
  Series-array sizes come from `_eh_vector_constructor_iterator_` calls in the
  GraphTab ctor — re-derive per build.
- **`"scre_tab_graph_judge_%s"` (0x18036D018) is NOT judge markers** — its
  suffixes are `fast`/`mav`/`slow` (statistics-box icon + timing-page
  markers). The judgement graph is a custom chart renderer:
  `FUN_1800f3940` make_shared<Chart>; **`FUN_1801cff60(chart, &vec<double>,
  &color_callable)`** appends a series (data copied; callable = 16-byte object
  `{game_lambda_vftable, u32 rgba}`); `FUN_1801cfee0` two-color variant.
  Charts pushed into `tab+0x178`.
- **Everything (`tab+0x178` charts, `tab+0x1A0` texts) is cleared and rebuilt
  EVERY FRAME** by `FUN_1800ed610`.
- **Legend is dynamic font text**: `FUN_1800f15e0(ctx, string, rgba)` per line
  (→ text ctor `FUN_1800f13f0` / `FUN_1800a2c70`, scale 0.6, width-advance
  cursor). Strings: `"■MARVELOUS"` @0x18036cee0 … `"COMBO"`. A mod adds
  "■S-MARVELOUS" as real text — no texture.

**Mod mechanism**: one `GenericDetour` on `FUN_1800ed610`. Pre-original (once,
after ingest): subtract the mod's per-second S-Marv counts from the marvelous
series (+0x5D8/+0x5F8 — persistent vectors, adjust once + flag). Post-original
(page 0 only, `*(int*)(tab+0x138)==0`, has-data `tab+0x1C4`): fetch the judge
chart (last element of `tab+0x178`), append the S-Marv series via
`FUN_1801cff60` reusing a game lambda vftable (derive from the LEA preceding
the stock `FUN_1801cff60` calls, pinned by the color immediates); add the
legend line by replicating `FUN_1800f13f0` (text objects position absolutely
from the graph rect via `afp_mc_get_param` on `"graph_usr"`). Per-second
S-Marv vector built from the record streams (grade==0 && |ms|≤12, timestamps
from the note-entry vector).
**AOB**: anchor `FUN_1800ed610` on the unique `"scre_tab_graph_judge_%s"`
xref; all helpers via call-site derivation from within.

## 4. FC emblems (clear kind displays) — three surfaces

Clear kind at `record+0x54` (7=FC, 8=GFC, 9=PFC, 10=MFC):

1. **Per-stage results emblem** (in `FUN_1800b8aa0`, runs ONCE at scene
   build): suffix table `DAT_180486410` ([7]="fc", [8]="gfc", [9]="pfc",
   [10]="mfc", [6]="life4", [2]="assisted", others NULL);
   `mc = afp_layer_mc_refer(layer, "player_%dp_info_usr/fc_usr")`;
   visibility via `afp_mc_set_param(0x1007)`;
   **`afp_mc_op(mc, 0xF09, "loop_" + suffix)`** — the emblem variants are
   frame labels of the `fc_usr` clip. ⇒ S-MFC: AFP-patch a `loop_smfc`
   labeled segment (cloned from mfc, art re-pointed to injected texture) and
   re-drive `afp_mc_op(mc, 0xF09, "loop_smfc")` once after scene build.
   Stable (build runs once). Rank sibling: `rank_usr` ← `scre_rank_%s`.
2. **Total results** (`FUN_1800cb090`): table `DAT_180486e80`
   (`fc_mfc` etc.), **bitmap load** `"scre_total_player_%s"` into
   `fullcombo_usr` under `total_p%d_top_usr` (package `"total_result"`).
   ⇒ S-MFC = inject a texture (e.g. `scre_total_player_fc_smfc`) + re-drive
   `FUN_18026f5e0(wrapper, name)` post-populate.
3. **Song-select score popup** (`FUN_18015a420`): `fullcombo_%dp_usr`,
   `"muca_card_%s"` bitmaps, clear kind from the SERVER score sheet
   (`FUN_1800ff7b0`) — out of scope per D21 (would need mod-side persistent
   per-song data).

Optional authenticity: results voice line `"vo_result_01_fullcombo_merv"` via
results voice dispatcher `FUN_1800c28e0` case 0.
**AOB**: `FUN_1800b8aa0` anchors on the unique `"scre_rank_%s"` xref; suffix
tables derived RIP-relative from their index sites; `FUN_1800cb090` anchors on
`"total_result"`/`"fullcombo_usr"`.

## 5. FC splash — `sequence::dance::FullcomboActor` (`FUN_180069c50`)

- `FUN_180069c50` IS the full onMessage: only handles 0x1034. Vtable
  `0x180361788`; init vslot 4 `FUN_180069920`, release vslot 5
  `FUN_180069c20`; **onUpdate is an empty stub** — no timers, no latches.
  Layout: `+0x88` side-info ptr (`*(int*)` = side, `+4` reverse flag),
  `+0x90` play style (1=double), `+0x98` splash clip wrapper.
- **Creation is INLINED** (pool scan + `afpu_get_afp_info_at_package` +
  `afp_layer_create_with_property` directly — NOT via the pooled
  `FUN_18026ecb0`) ⇒ **the existing CMovieClip::Create detour never sees this
  clip**. Template names `"%s_%s"`: base `01_fullcombo_single` /
  `02_fullcombo_double`, variant `normal`/`reverse` — **four templates**, all
  in package key **`"dance_fullcombo"`** (bm2d category, optional `%04d` skin
  suffix from GamePlayActor+0x190).
- 0x1034 case: SE `"se_game_fullcombo"` (pan by side) →
  **`afp_mc_op(mcid, 0xF09, label)`** with `"marbelous_in"` / `"perfect_in"` /
  `"great_in"` / `"good_in"` for type 0..3 → `afp_layer_play(layer, 1.0)` →
  `afp_layer_set_attribute(layer, 1, 1)`. NOTE: goto-label here is the direct
  libafp export `afp_mc_op(0xF09, string)` — no gamemdx-internal label-lookup
  needed (simpler than the judgement flash path).
- **MFC type computed inside judge_submit**: goods≥1→3, greats≥1→2,
  perfects≥1→1, else 0. All four `MOV EDX,0x1034` sites live in judge_submit.
  ⇒ S-MFC condition = `type==0 && mod all-S-Marv bit`.
- Splash art texture names are NOT in gamemdx strings — dump the
  `dance_fullcombo` package (disk/LayeredFS logs) during implementation.

**Mod mechanism**: detour `FUN_180069c50` (AOB: `81 FA 34 10 00 00`
(`cmp edx,0x1034`) @ 0x180069c79 — **verified module-unique**). Post-original:
if type==0 and the side's all-S-Marv bit → `afp_mc_op(*(clip+0x110), 0xF09,
"s_marbelous_in")` (clip = `*(this+0x98)`; play/visible already set by stock;
do NOT re-play the SE). The AFP patch must add the `s_marbelous_in` segment to
**all four templates**. Verify live that a missing label is a no-op (expected)
before shipping.

## 6. Combo digits — `sequence::dance::ComboActor` (`FUN_180066950`)

- **The 10-pointer block @ 0x180483350 is THREE tables**: suffix[0..3] =
  `"_marvelous"`, `"_perfect"`, `"_great"`, `"_good"` (grades 0–3 ONLY);
  then digit-count labels `"usr"/"1"/"10"/"100"/"1000"`-family; then
  `"combo_usr"`. Index source `this+0x6C` ∈ 0..3 or 0xFF.
- ComboActor: `+0x58` side-info, `+0x64` digit count, `+0x68` combo value
  (display-clamped 9999), `+0x6C` worst-judgement index, `+0x70/+0x78/+0x80`
  three clip wrappers `dance_combo_root1/2/3` (pooled create — ALREADY
  captured by overlay_element_styling's classifier), `+0x94` stop latch
  (msg 0x103C). Package key `"dance_combo"`.
- **Refresh `FUN_180066950`** (event-driven only — called from init when
  combo>0 and from the 0x1033 handler when combo ≥ 4; never per-frame):
  digit-count layout labels `loop_1/10/100/1000` via `afp_mc_op(0xF09)` (on
  change only); then for each layer × place {1,10,100,1000}:
  name = `"daco_combo%s_%d"`, path = `"combo_usr/number_usr/%d_usr"`,
  applied via **traversal-6 walk** (`afp_layer_mc_refer` then
  `afp_mc_traversal(id, 6)` loop — multiple same-name instances exist) with
  `afp_mc_load_bitmap`. **Quirk: layer 0's ONES place is always unsuffixed
  `daco_combo_%d`**; layers 1/2 always use `"_dummy"` art + get the TINT:
  wrapper **vfunc+0x98** with `float[4]{r,g,b,1.0}`.
- **Tint constants** (root2/root3 pairs): marvelous **0xA9FEEC / 0xDFA6EF**;
  perfect 0xFFE63A/0xD2BB1A; great 0x17BC1B/0x239106; good 0x14B1F2/0x0E95CC.
  Multiply-vs-add semantics of vfunc+0x98 unverified statically (likely
  multiplicative — the color-twin sibling +0x90 is overlay_element_styling's
  known opacity-composing SetColor); verify live.
- **Worst-judgement writer** = msg 0x1033 handler `FUN_180066790`:
  combo<1 ⇒ `+0x6C = 0xFF` (reset); else `g = grade; if (g==6) g=0`
  (**O.K. maps to marvelous tier**); max-track `+0x6C`. combo>3 ⇒ show +
  `afp_mc_op(refer(layer,"combo_usr"), 0xF03, "in")` + refresh; combo≤3 ⇒
  hidden.

**Mod mechanism (recommended)**: detour `FUN_180066950` post-patch — single
choke point, `this` in RCX gives side/combo/wrappers free, runs once per
visual update. Post-original, when the side's all-S-Marv bit holds AND stock
`+0x6C == 0`: re-load places {10,100,1000} on layer 0 with
`daco_combo_smarvelous_%d` (replicating the traversal-6 walk) + apply the
S-Marv tint pair on layers 1/2 via vfunc+0x98. Self-healing: if the bit drops,
the next stock refresh restores marvelous art (no cleanup path needed).
The mod's all-S-Marv combo bit must mirror the stock 6→0 treatment (freeze
O.K. does not degrade the S-Marv status — it carries no delta).
**AOB**: the inline tint-immediates run
(`C7 45 F8 EC FE A9 00  C7 45 FC EF A6 DF 00 …`) is near-certain unique;
anchor there, walk back to the prologue.

## 7. New-signature summary

| target | anchor | confidence |
|---|---|---|
| `FUN_1800f6bc0` score-tab populate | `"marvelous_num_usr"` xref (unique) | high |
| `FUN_1800f8370`/`FUN_1801d2c00` etc. | call-site derivation from the anchor | high |
| `FUN_1800ed610` graph rebuild | `"scre_tab_graph_judge_%s"` xref (unique) | high |
| `FUN_1801cff60` chart append + lambda vftables | call-site + LEA derivation, pinned by color immediates | medium-high |
| `FUN_1800b8aa0` results build (emblem) | `"scre_rank_%s"` xref (unique) | high |
| `FUN_1800cb090` total-results populate | `"total_result"`/`"fullcombo_usr"` xrefs | high |
| `FUN_180069c50` FC splash handler | `81 FA 34 10 00 00` (verified module-unique) | high |
| `FUN_180066950` combo digit refresh | tint-immediates run | high |

Existing signatures reused: `judge_submit`, `judge_notes`,
`spritelayer_ctor`, `spritelayer_set_names`, CMovieClip capture
(`dance_combo_root*` already classified), libafp exports (no AOBs).

## 8. Implementation-time verifications (not design blockers)

- Dump `dance_fullcombo` package for splash texture names; confirm on-disk
  arc names for `dance_fullcombo` / `dance_combo` via LayeredFS logs.
- Verify combo tint vfunc+0x98 semantics live (set pure red, observe).
- Verify `afp_mc_op(0xF09, missing_label)` is a benign no-op.
- Confirm `PlaydataTab+0x151` has no mid-scene re-arm (or make the detour
  robust to re-population — it should be anyway).
- Confirm unsuffixed `daco_combo_0..9` exist in the texture list (code implies
  yes).
- Non-alphanumeric `scre_tab_num_*` glyph names (`FUN_1801d25a0`) if any
  string beyond digits is needed.


---

## 9. Maintainer screenshot reference (2026-08-29, results screen)

Three cabinet screenshots reviewed (maintainer-supplied, not committed):
Play Graph tab, Simple results tab, Details tab. Findings:

1. **The per-grade count list renders on TWO tabs**: "Simple results"
   (grade list + ENERGY gauge + FAST/SLOW + MAX COMBO) and "Details" (grade
   list + FAST/SLOW + SCORE/BEST/EX/MAX COMBO). These are the two
   PlaydataTab instances (tab kinds 1/6, §2) — the S-MARVELOUS row must
   appear on BOTH; one populate fn (vslot 7) serves both, but the AFP
   layout edit + row repositioning applies to both layout subtrees of
   `detail_result` (or per-kind widget sets — confirm in Step 7).
2. **Judgement graph** (Play Graph tab): legend = single horizontal row
   `■MARVELOUS ■PERFECT ■GREAT ■GOOD ■MISS` above the chart (space exists
   to append ■S-MARVELOUS); an all-MFC run visibly renders the
   **all-marvelous shimmer series** (violet-pink bars) — the native
   precedent the S-Marv series sits beside. Axis = NOTES/SEC, per-second
   bars, "Switch display: NORMAL" toggle.
3. **Per-stage FC accolade** = the text "Marvelous Fullcombo!!!" (rainbow
   'Marvelous' + white 'Fullcombo!!!') under the dance-grade emblem — the
   `fc_usr` clip's `loop_mfc` frame content; `loop_smfc` replaces this text
   art for S-MFC (Step 9).
4. Row labels on the count list are colored word-art textures (per-grade
   colors) — consistent with baked template art (§2, label art via AFP
   patch + injected texture).


## 10. dance_judge template structure (dev-machine parse, 2026-08-29)

From bemaniutils parse of `dance_judge0000_v0.arc` → `afp/dance_judge`
(pre-Step-4 investigation; shapes verified via geo draw-param regions):

- **Labels live on the ROOT timeline** (638 frames post-Step-3-demo; 600
  stock): `in_marvelous@0, in_perfect@38, in_great@80, in_good@124,
  in_boo@170, in_miss@212, in_ok@253, in_ng@292` — the in_marvelous segment
  is frames 0..37 (38 frames).
- **Everything is LOCAL — no imported characters** (single import:
  `aeplib.__Packages.aeplib` bytecode lib). No import-table surgery needed.
- **Frame 0 carries ALL definitions** (sprites 3, 6, 35, 53; shapes 5, 8,
  11..32) PLUS the segment's two placements:
  `PlaceObject(source 35, depth 2)` (the word movie) and
  `PlaceObject(source 8, depth 3)`.
  ⇒ a naive labeled-segment clone duplicates every definition — the Step 3
  demo did exactly that (tolerated by parsers/renderer: dictionary
  redefinition is idempotent), but the REAL patch needs a
  **placements-only segment clone** + a separate
  **clone-sprite-definition-with-remap** primitive.
- **The word chain (1:1 all the way down)**:
  `PlaceObject(35)` → sprite 35 places character 32 → shape 32 → geo
  `dance_judge_shape32` → region `dance_judge0000_marvelous`. Every other
  judgement word mirrors this (shape 11=ng, 14=ok, 17=miss, 20=boo,
  23=good, 26=great, 29=perfect). Shapes 5/8 have no texture regions
  (solid/flash art).
- Exported tags: `dance_judge`→53, `marvelous`→35(!), plus aep helpers —
  the word movie is itself exported as `marvelous` (the standalone
  `marvelous` template file in the IFS is a separate small AFP).
- **Geo naming rule**: a new shape id N binds geo `dance_judge_shape{N}`
  (shape-tag → geo name convention `{exported}_shape{id}` — note it uses
  the exported name of the TEMPLATE the shape sits in).
- **Patch shape for Step 4** (all local, no timeline synthesis):
  1. new texture region (donor-anchored clone of `dance_judge0000_marvelous`)
  2. new geo = byte-clone of `dance_judge_shape32` + region-name label
     rewrite (note: name length CHANGES — verify/extend the GE2D label
     rewriter for length-changing rebuilds)
  3. AP2: add Shape id 54 → clone sprite 35's DEFINITION as id 55 with
     internal remap {32→54} → placements-only clone of `in_marvelous` →
     `in_smarvelous` with remap {35→55}
- **Skin caveat**: textures are skin-suffixed (`dance_judge0001_*` …) but
  geo names are NOT — v1 scope: default skin 0000; the patch fn should read
  the actual region name from the donor geo and degrade to stock for
  unknown skins (one WARN).
