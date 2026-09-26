# Decision register — DDR SELECTION: DDR A / DDR A3 (White) / DDR A3 (Gold)

Register accepted by the maintainer 2026-09-25 (D12, D13 overridden; D21–D26 added and accepted).

Readiness Confirmed 2026-09-25 (clarifications carried into the design: D15's "name hidden" means World's own `dance_name` element and marker stay hidden, the D21 widget draws the theme name; D1's numbering supersedes the feasibility doc's §6.1 table).

Design approved 2026-09-25. Maintainer note at approval: World has no edit charts, so A3's edit-data handling (`_edit_*` difficulty textures, edit-data hides) is not ported (D23 / D24).

Status values: `Proposed`, `Accepted`, `Overridden`, `Assumed`, `Open`. ★ = likely not
considered yet. Order = blast radius (user-visible behaviour and interfaces first).

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | Row values and labels | Persisted option values; what players see | Append 7 `DDR A`, 8 `DDR A3 (White)`, 9 `DDR A3 (Gold)`; 0..=6 unchanged (value 6 relabelled `2013-2014`, done) | Accepted (labels Assumed from the request's wording) |
| D2 | Theme identity inside the engine ★ | Which World branches fire; crash safety | Internal skins 6 / 7 / 8 (= row value − 1, as today) are also the **record skin** ("neutral" class: World's skin-0 branches on the theme's own packages). `GameWork+0xA8` = **0** for a theme | Accepted |
| D3 | AUTO for series 17 (DDR A) ★ | Changes today's AUTO result for DDR A songs | 17 → **DDR A**; 14–16 → `2013-2014` (A3's folder put 17 with 2013–A, but DDR A is its own entry now) | Accepted |
| D4 | AUTO for series 18–20: White or Gold ★ | Which A3 look AUTO picks | Follow the cabinet like A3 did: machine type 4 (gold cabinet, incl. the SMX GOLD force) → **Gold**, else **White**. Explicit values always win | Accepted |
| D5 | AUTO grouping of A20 / A20 PLUS / A3 | AUTO behaviour | 18, 19, 20 → the A3 theme (D4); 21 (World) and custom series stay World's UI | Accepted |
| D6 | Era cut-in | Pre-song presentation | None for the themes (A3's own UI had none); the five eras keep theirs and the Era Cut-In setting | Accepted |
| D7 | Stage panel | Pre-song presentation | A3's skin-0 fill on the theme's own root (`common_choice_v0/_v1/_v2`): `choice_stage_usr2` hidden, band `scene_choice_stage_{1st,2nd,3rd,4th,final,extra}` on the root's own `choice_stage_usr`, root's own background, song jacket, stage call `vo_stage_*`. A3's event-only special-stage art (flare / galaxy / encore) → `extra` | Accepted |
| D8 | End banners | Post-song presentation | `common_shutter_v0/_v1/_v2` (CLEARED / FAILED; PRAY FOR ALL on Tohoku EVOLVED — all three have the art) | Accepted |
| D9 | `_sel` movies | Background | Off for the themes | Accepted |
| D10 | Danger on doubles ★ | Doubles shows a single-lane danger flash otherwise | One scoped 2-byte patch (NOP the doubles-skip `JNZ`) while a theme's `dance_danger` is registered → `danger_double`, as A3 skin 0. The dead second clip is not reproduced (never played in A3 or World) | Accepted |
| D11 | S-Marvelous on the themes ★ | S-Marv users see a plain Marvelous on the themes otherwise | Include, as the last step: generated art (White and Gold share one set — their textures are pixel-identical — DDR A its own), masks widened past skin 7 | Accepted |
| D12 | Player name on the score frame | A3's own UI showed the dancer name in the difficulty frame | **Include** on all three themes (stock DDR A and A3 showed it in-frame; the legacy eras never did and keep it hidden) | Overridden |
| D13 | Panel score sets (high score / rank / FC mark / target) | A3 showed them on every skin | **Include** on all three themes (part of the stock DDR A / A3 experience); the eras keep them hidden | Overridden |
| D14 | Gold look for the eras' shared A3 pieces ★ | On a gold cabinet A3 drew the eras' panel root, skins 3–5 song info and skin-1 layout from `_v1`; the mod always uses `_v2` | Out of scope — eras unchanged | Accepted |
| D15 | HUD profiles | Look of each HUD element | A3 skin-0 rules on the adapters that exist: gauge segmented 26 cells + partial (A3's "skins 0 / 5" fill), P2 mirror, FLARE continuous; combo standard growth, full cells, per-grade sheets; score level-texture scheme (priority 7); A3 song-info panel from `dance_song_info0000_vN`; A3 option icons; A3 pacemaker; layout root `dance_common0000_vN`; BPM and name hidden; hit flash / lane filter / cover / measure stay World's (as for the eras) | Assumed |
| D16 | Announcer and crowd | Soundscape | A3 skin-0 rules: skins 4–5 voice rules plus A3's own crowd (`STG_APP02`/`STG_APP03` + `vo_ingame_cheer`, low `STG_BOO` + `vo_ingame_boo`); all cues already in the era bank | Assumed |
| D17 | READY / HERE WE GO | Intro | `dance_message_v0/_v1/_v2` by explicit name; no HERE voice; READY's own `vo_ingame_ready` | Assumed |
| D18 | Names elsewhere | Consistency | `policy::skin_name` (logs) matches the row labels; README, option preview text, module docs updated; dev knob `DDR_SELECTION_FORCE` accepts 1..=8 | Assumed |
| D19 | Scope exclusions | Keeps the plan tight | SD cabinets (`_sd` exports) not ported (as for the eras); courses / event chains stay stock (existing mode policy); no new import-manifest entries (stock World ships every theme file) | Assumed |
| D20 | Cross-package texture names | Consecutive theme songs could show another generation's pixels if name lookup is global | Plan Step 1 is a cabinet spike that plays the generations back-to-back before anything else lands | Assumed |
| D21 | Player-name look (opened by D12) | A3 and World draw the name differently | A3's own: a `2d_font_player` BmpString (font 6, A3's scale) following the theme frame's `name_usr` — the mod's `TextWidget` is already that object; it only needs a font parameter. Rejected: World's `cote_edge_*` glyph textures (World's look) | Accepted |
| D22 | Name text (opened by D12) | Guests and the bot side | World's own rule: the profile name, else `PLAYER1` / `PLAYER2` (guest), else `PLAYER`; the bot side shows whatever World's own HUD would | Accepted |
| D23 | Score-set contents (opened by D13) | What the panel shows per player | A3's full set — difficulty, dancer name, best score, rank, full-combo mark, area; target set with the target's name / area / score / rank / FC — from World's records. **Each field fails open (hidden)** where World has no data or the RE does not pin it | Accepted |
| D24 | Which target (opened by D13) | The target set's subject | World's own target selection (the rule World's panel uses to show its target score: type at `PlayerWork+0x1328`, rivals, machine / area / national); hidden whenever World would show none (edit data, event chains, no target) | Accepted |
| D25 | Dancer area (opened by D13) ★ | World's panel and HUD show no per-player area; the field may not exist in World's profile | Show it when the RE finds World's per-player area; otherwise the area child stays hidden (not a blocker) | Accepted |
| D26 | Score-set textures | Package loading | Request `common_texture_v0` (name glyphs, digits) and `common_area_lang_<cabinet language>_vN` (areas; Korean only at `_v1` / `_v2`) with the panel session; ranks, FC marks and difficulty come from the theme's own `common_choice_vN` | Assumed |

## D1 — Row values and labels

**Question.** How are the themes offered? **Answer.** Three appended values (7, 8, 9), so every
saved 0..=6 keeps its meaning; an older DLL clamps 7..=9 to OFF. Labels `DDR A` (5 bytes),
`DDR A3 (White)` (14), `DDR A3 (Gold)` (13) fit the 15-byte row budget. **Rationale.** Maintainer
request; appending is the only order that keeps existing caches valid.

## D2 — Theme identity inside the engine

**Question.** What skin value do World and the mod see for a theme? **Answer.** Record skin 6 / 7 /
8 and `GameWork+0xA8` = 0. **Rationale.**

- World's `int[6]` DPS table forbids `GameWork+0xA8` ≥ 6.
- `GameWork+0xA8` = 1 would hide the song info and option icons.
- Nothing else reads `GameWork+0xA8` once the helper runs; 0 is stock.
- Every World reader of the record skin is a compare. For ≥ 6 it lands on World's skin-0 path on
  the theme's own package: eased gauge, rainbow full lives, no intro, danger at the `filter`
  marker, record package for the stage frame / song info. Skin 0 itself would hand the stage
  frame and song info to World's loader art.
- The mod's adapters already use the record skin as identity, so 6..=8 flows through unchanged.

Rejected: reusing an era's record skin (would fire that era's branches).

## D3 — AUTO for series 17

**Question.** A3's DDR SELECTION folder put DDR A songs (series 17) under 2013–A. With DDR A its
own entry and 2013-A relabelled `2013-2014`, where does AUTO send them? **Answer.** DDR A.
**Rationale.** A `2013-2014` skin on DDR A songs contradicts the new label. Rejected: keep 17 →
`2013-2014` (A3-exact, but inconsistent now).

## D4 — AUTO White or Gold

**Question.** AUTO has two A3 looks to choose from. **Answer.** Follow the cabinet:
`arkMDXGetMachineType` == 4 → Gold, else White. **Rationale.**

- It is A3's own rule (its probe and its menu background both switch on the gold cabinet).
- It needs no new setting.
- It is already honoured by the SMX GOLD force.

Rejected: always White (ignores gold cabinets); a new GLOBAL SETTINGS row (more UI for a choice
the explicit values already offer).

## D7 — Stage panel

**Question.** What shows before a theme song? **Answer.** A3's skin-0 fill (Ghidra, A3
`FUN_180030d10`) on the theme's own `common_choice_vN` root. **Rationale.** It is what A3 drew for
its own UI, and it reuses the hosted-root machinery: session, row patch, READY dismissal, release.
Only the fill differs:

- no sub-clips loaded;
- the band texture goes on `choice_stage_usr` rather than `choice_stage_usr2`;
- 3rd / 4th stage textures exist.

A3's special-stage branch uses event art and a loader package World does not have, so those stages
show `extra`.

## D8 — End banners

**Answer.** The theme's `common_shutter_vN` through the existing banner host, with PRAY FOR ALL
enabled for all three themes. **Rationale.** A3's skin-0 banners are those packages. All three
`00_prayforall` clips have art.

## D10 — Danger on doubles

**Question.** A record skin ≠ 0 always creates `danger_single`. Fix it for the themes?
**Answer.** Yes: one 2-byte patch, applied while a theme's `dance_danger` is registered and
restored otherwise (the stage-frame / gauge / song-info patch pattern). It needs a new signature
and a sweep. **Rationale.** Without it, doubles shows a single-lane flash centred at the filter
marker. Rejected: accept the single-lane flash.

## D11 — S-Marvelous on the themes

**Answer.** Include as the final step. **Rationale.**

- S-Marvelous is a headline feature; without art it stands down cleanly (plain Marvelous), so the
  core does not depend on this step.
- The generator already has the recipes.
- White and Gold share one art set.
- `skin_bit` returns 0 above skin 7, so the masks must widen for skin 8.

Rejected: defer to a later project.

## D12 — Player name on the score frame

**Maintainer override (2026-09-25):** include the name on the three themes — stock DDR A and A3 drew it in the difficulty frame during gameplay; the legacy eras never did, even in A3. Original recommendation, kept for the record:

**Answer.** Defer. **Rationale.**

- A3 drew the name as font text in `frame_difficulty_*/name_usr`, a path not yet traced.
- World's own `dance_name` clip cannot be used, because the theme's score package has no such
  export.
- The eras hide the name too.

It is a clean follow-up.

## D13 — Panel score sets

**Maintainer override (2026-09-25):** include the stage panel's per-player score sets (high score, rank, full-combo mark, dancer name, target) on the three themes, as stock DDR A / A3 showed them. The eras keep them hidden (a separate follow-up for them).

## D14 — Gold look for the eras

**Answer.** Out of scope; the eras keep `_v2`. **Rationale.** It changes shipped, cabinet-proven
behaviour. It can later be a one-line choice (the D4 cabinet test) if wanted.
