# Customizer Friendly Names — Scraping Konami's PREMIUM CUSTOMIZER pages

Research + design notes for replacing the generic `Board #N` / `Character #N` /
`Background #N` value labels of the `webui_options` cosmetic rows with the real
item names Konami publishes, and for keeping that name table current
automatically (eventually from the auto-updater). No code ships with this doc;
it records what was verified against the live site, the community data and the
game install on 2026-09-14 so the implementation can be planned from facts.

Terminology: **asset id** = the number in the game's asset filename
(`appeal_board_0006.arc` → `6`) = the value stored in the `Customize` field and
sent on the wire as `key` (see `docs/player_customization_system_research.md`).
**premizer** = one PREMIUM CUSTOMIZER release batch (Konami's own URL parameter
is `premizer_id`).

---

## 1. Where names are missing today

`src/mods/webui_options/discovery.rs` scans `data/arc/custom/<dir>/` (plus every
`data_mods/<mod>/<dir>/`), keeps the leading digit run of every
`<prefix><digits>*.arc` stem (`extract_id`, `discovery.rs:306-320` — variants
`_1p/_2p/_result*/_v3/_lite` collapse to one id) and stores the **sorted,
deduplicated `Vec<u32>` of asset ids** per category. The scalar row value is the
0-based POSITION in that vector; the right-hand value text is
`ScalarFormat::PrefixedIndex { prefix, display_offset: 1 }` → `"Board #7"`
(`api.rs:733-736`, pushed every frame by `rows.rs::push_scalar_value_text`).
Nothing in the DLL knows an item's name. The number shown is a *position*, not
the asset id — `character_9000` renders as `Character #74`, and any gap in the
regular id range shifts every later label by one.

A friendly-name feature therefore needs one table: **`(category, asset_id) →
name`**, keyed by the real asset id (stable across builds and cabinets), never
by the display position.

Categories the mod exposes and how they line up with the three sources:

| DLL option id(s) | `Customize` field | Asset dir / prefix (`data/arc/custom/…`) | Konami page block `id` | PhaseII list id | Thumbnail size |
|---|---|---|---|---|---|
| `customize_appeal_board` | `+0x0C` | `appeal_board/appeal_board_` | `appeal_board` | `customize.1_0` | 280×113 |
| `customize_background`, `customize_background_gameplay` | `+0x10`, `+0x14` | `background/background_` | `game_bg` | `customize.3_1`, `customize.3_2` (identical lists) | 280×280 |
| `customize_character_p1`, `customize_character_p2` | `+0x18`, `+0x1C` | `character/character_` | `character` | `customize.2_1`, `customize.2_2` (identical lists) | 280×280 |
| `customize_lane_single` | `+0x20` | `lane_single/lane_single_` | `lane_bg_single` | `customize.4_1` | 280×280 |
| `customize_lane_double` | `+0x24` | `lane_double/lane_double_` | `lane_bg_double` | `customize.5_1` | 280×280 |
| `customize_lanecover_single` | `+0x28` | `lane_cover_single/lane_cover_single_` | `lane_cover_single` | `customize.6_1` | 280×280 |
| `customize_lanecover_double` | `+0x2C` | `lane_cover_double/lane_cover_double_` | `lane_cover_double` | `customize.7_1` | 280×280 |

(Konami calls lane *backgrounds* `lane_bg_*` while the game data calls them
`lane_single`/`lane_double`; the DLL option ids spell lane covers `lanecover_*`.
Seven name pools serve nine rows — the two character rows and the two
background rows each share a pool.)

---

## 2. The three sources and what each is good for

| Source | Provides | Does NOT provide | Trust |
|---|---|---|---|
| **Konami** `p.eagate.573.jp/game/ddr/ddrworld/shop/premium_customizer.html?premizer_id=N` | Authoritative Japanese item names, thumbnails, release batch + sale dates, for EVERY Premium Customizer item (19 batches, 691 items as of 2026-09-14) | Any numeric asset id; the stock/default items; event-reward items; the BPL team banners | Authoritative for names; ids must be *inferred* (§4) |
| **PhaseII** `PhaseWeb3-Vue/public/data-sources/customizations/ddr/20/{appeal,gameplay}.json` (MIT, actively maintained — last touched 2026-08-27) | `id → label` for every category incl. the stock items and the BPL banners, grouped per batch | Konami-exact spelling (adds romanizations, translates some katakana names, has typos); `background 100001–100004`; `9001` | Excellent for the stock prefix and as an independent cross-check of inferred ids |
| **Game install** `$DDR_WORLD_INSTALL/data/arc/custom/<dir>/` | Ground truth for WHICH ids exist on this cabinet + the real artwork (one PNG per `.arc`; backgrounds are an IFS with `tex/bg*.png`) | Any name (the arcs contain only the numbered PNG) | Ground truth for existence; artwork enables visual verification (§7) |

The right shape is a **pre-seeded baseline** (Konami names + PhaseII stock ids,
image-verified once, committed) plus an **incremental refresh** that only has to
handle premizers newer than the baseline (§6).

---

## 3. Anatomy of a premizer page

Everything below was verified with plain `curl` (no cookies, no login) on
2026-09-14.

### 3.1 Enumeration

- One page per batch: `…/premium_customizer.html?premizer_id=N`. Valid `N` on
  2026-09-14: **1–19**. An unknown `N` (20+, 0) answers **HTTP 302** to the bare
  URL; the bare URL renders the NEWEST batch (the `<option … selected>`).
- The **dropdown is the index of record**: `<select id="premizer_select">` lists
  every batch as `<option value="N">PREMIUM CUSTOMIZER 「title」 第k弾 [【販売終了】]</option>`,
  identical on every page. Do not rely on its visual order — it is newest-first
  EXCEPT that `10` is listed above `11`; **sort the values numerically**. The
  `【販売終了】` ("sales ended") suffix appears only in the dropdown text, not in the
  page's own `<h2>`.
- The shop index (`shop/index.html`) links only the batches currently on sale —
  useless for enumeration.
- Probing `N = max+1` until a 302 is a valid cross-check but the dropdown is
  cheaper and authoritative.

### 3.2 Access notes

- The item lists render for anonymous visitors. Login only affects
  `span.granted_num` (0 when anonymous) and the per-item `premizer_status`
  (always `未獲得` "not acquired" when anonymous).
- The host sits behind Imperva Incapsula (`_Incapsula_Resource` script,
  `incap_ses_*`/`visid_incap_*` cookies). Plain fetches work today; a scraper
  should send a browser-like `User-Agent`, keep the cookie jar for the run,
  space requests ≥ 1 s apart, and treat a 200 whose body lacks
  `class="premizer_box"` as "challenged / changed layout" → keep the previous
  data, do not write anything.
- `Cache-Control: private, no-store, no-cache` on the HTML — no conditional GET
  available; 19 pages × ~45 KB is the whole corpus, so a full refetch is cheap.
  Thumbnails carry `Last-Modified`.
- Encoding is UTF-8 (`Content-Type: text/html;charset=UTF-8`).

### 3.3 Page-level fields

```html
<h2>PREMIUM CUSTOMIZER 「BEMANI納涼祭2026」 第2弾</h2>                 <!-- title (no 【販売終了】) -->
<li class="term"><span>販売期間：</span><span>2026-08-06 10:00:00</span> ～ <span></span></li>
                                                                     <!-- sale start / end (end empty = ongoing) -->
<li class="btn"><a href="/gate/p/eamusement/coop/confirm.html?pid=3930">…</a></li>
                                                                     <!-- purchase id; ABSENT for premizers 10, 11, 16 -->
<span class="item_num"><span class="granted_num">0</span> / <span class="total_num">40</span></span>
                                                                     <!-- total_num = item count on this page -->
```

`total_num` is the parser's self-check: the number of `<li>` items parsed across
all category blocks MUST equal it (a partial parse — e.g. the quoting quirk in
§3.5 — showed up as 37 ≠ 40 on premizer 17).

### 3.4 Category blocks and items

```html
<div id="appeal_board" class="box-adjust"><div class="box-category">アピールボード</div></div>
<ul id="appeal_board" class="premizer_box">          <!-- NOTE: div and ul share the same id -->
  <li class="">
    <div class="premizer_image"><img src="/game/ddr/ddrworld/images/customize/appeal_board/260618_ovtp.jpg" alt='Over The "Period"' /></div>
    <div class="premizer_label">
      <div class="premizer_status">未獲得</div>
      <span class="item">「Over The "Period"」</span>
    </div>
  </li>
  …
</ul>
```

- Block ids (also the thumbnail sub-directory) are exactly the seven in the §1
  table: `character`, `game_bg`, `appeal_board`, `lane_cover_single`,
  `lane_cover_double`, `lane_bg_single`, `lane_bg_double`. Their Japanese
  captions are constant (システムキャラクター / ゲーム背景 / アピールボード /
  レーンカバー (SINGLE|DOUBLE) / レーン背景 (SINGLE|DOUBLE)). A block is present only
  when the batch has items in it (premizer 16 has one background; every batch so
  far has had all seven blocks).
- Per item: `img[src]` (thumbnail), `img[alt]` (name), `span.item` (`「name」`).
  `alt == name` for all 691 items, so either is usable and they cross-check each
  other. Strip the corner brackets `「 」`, HTML-unescape (`&uuml;`, `&amp;`,
  `&#…;`), trim.
- **Item order within a block is significant** — it is the asset-id order (§4).

### 3.5 Quirks the parser must survive

1. **Attribute quoting flips.** `alt` is double-quoted except when the name
   contains a double quote — then it is single-quoted (`alt='Over The "Period"'`,
   premizer 17, three blocks). A regex that assumes `alt="…"` silently drops the
   item. Use a real HTML tokenizer or accept both quote styles.
2. **Duplicate element ids** (`div#appeal_board` + `ul#appeal_board`). Select the
   `ul.premizer_box` and read its `id` (or the preceding `.box-category` text).
3. **Thumbnail slugs are not stable keys.** Konami names them
   `YYMMDD_<mnemonic>.jpg` from premizer 2 on (premizer 1 has no date prefix), the
   date is USUALLY the sale start but not always (premizer 5's lane-cover block
   reuses `250313_deet/ropa/endr` while its appeal-board block has
   `250612_deet/…`), and the same slug appears in several categories. Use slugs
   only as an image address.
4. **The same asset is spelled differently between blocks.** e.g. premizer 9
   `博麗 霊夢&東風谷 早苗` (lane cover) vs `博麗 霊夢 & 東風谷 早苗` (lane bg). Store
   verbatim per category; compare with NFKC + whitespace-stripped + `＆→&`
   normalisation.
5. **Typographic content**: `Ü`, `♫`, `♪`, `†`, `～`, `Ⅱ`, full-width parentheses
   inside `(パターン1)`-style suffixes, ASCII `&`, `%`, `"`. Five names are NOT
   Shift-JIS (cp932) encodable (`みゅ、みゅ、Müllる` and the four `♫` names) — this
   matters for the render path (§9).
6. Sale-end `<span>` is empty for open-ended sales; premizers 10/11/16 have an
   explicit end and no `coop` purchase link (sold through a different channel).
7. Stock and event items are **never** on these pages (§5).

---

## 4. The key finding: page order IS asset-id order

Konami's pages carry no ids, so the mapping has to be inferred. Cross-checking
all 19 pages against PhaseII's id-labelled lists and against the install proved
a simple rule for every one of the 7 × 19 = 133 (category, batch) blocks:

> **Within a category, the regular id space is the stock items (ids 1..S), then
> premizer 1's items in page order, then premizer 2's, …, then premizer 19's —
> contiguous, no gaps, in ascending `premizer_id` order.**

Concretely: for every block, PhaseII's group for that batch has the same item
count as the page (except one case below), the labels match in order (modulo
PhaseII's romanization suffixes / translations / one typo), the group's ids are
consecutive, and each group starts exactly one past the previous batch's last
id. The last regular id per category (129 / 67 / 73 / 116 / 135 / 90 / 107)
equals the max regular id on disk. Appendix A has the full block table.

The single exception is instructive: **appeal board, premizer 1** — the page
lists 5 items but the id block is 4–10 (7 ids). Ids 4 = EMI and 5 = RAGE are the
two default characters' boards, shipped in the same data update but never sold;
the page's ALICE…HYPERCORE are 6–10. Visual matching (§7) confirmed 0004/0005
are EMI/RAGE artwork and 0006–0010 match the page thumbnails at NCC ≥ 0.99. So:
**unlisted items can sit BEFORE a batch's block**, and a pure
"`last_known + 1`" rule would have named ALICE `4`. That is why the baseline is
anchored with PhaseII + images, and why every NEW block should be
image-anchored on at least its first item (§6.2).

Two more properties worth stating because the refresh logic depends on them:

- **Batch order = `premizer_id` order, not sale-date order.** Premizer 3
  (ひなビタ♪, on sale 2025-02-14) takes the ids right after premizer 2 (on sale
  2025-01-30) and before premizer 4 (on sale 2025-03-13) — consistent — but
  premizers 10/11 share a date and 18/19 share a date; the ids follow the
  numeric `premizer_id`. Always process batches in ascending `premizer_id`.
- **PhaseII's group dates are PhaseII's own import dates, not Konami's**
  (their 第2弾 is dated 2025-06-10; Konami's sale started 2025-03-13). Use
  Konami's `li.term` for release dating.

---

## 5. Ids that are NOT on any premizer page

These come from PhaseII (stock prefix, BPL banners) and from inspecting the
install. They belong in the committed baseline as hand-curated rows.

| Category | Stock prefix (id → name) | Placeholders | Special ranges |
|---|---|---|---|
| appeal_board | 1 プレーン (パターン1), 2 プレーン (パターン2), 3 プレーン (パターン3) — solid green/blue/orange boards; 4 EMI, 5 RAGE (default-character art) | 9000, 9001 | 100001 APINA VRAMeS, 100002 GiGO, 100003 GamePanic, 100004 Silk Hat, 100005 TAITO STATION Tradz, 100006 ROUND1, 100007 Leisureland (BPL team banners; `appeal_board_1000xx.png` carries the team logo) |
| background | 1 WORLD (パターン1), 2 WORLD (パターン2), 3 WORLD (パターン3) | none on disk (PhaseII lists a virtual 9000) | 100001–100004: four large group illustrations (event/collab reward art), **unnamed in every source → manual override** |
| character | 1 EMI, 2 RAGE | 9000, 9001 | — |
| lane_cover_single | 1 WORLD, 2 EMI, 3 RAGE, 4 ROCKET | 9000, 9001 | — |
| lane_cover_double | 1 WORLD, 2 EMI, 3 RAGE, 4 EMI&RAGE, 5 ROCKET | 9000, 9001 | — |
| lane_single (lane bg) | 1 EMI, 2 RAGE, 3 ROCKET | 9000, 9001 | — |
| lane_double (lane bg) | 1 EMI&RAGE, 2 EMI, 3 RAGE, 4 ROCKET | 9000, 9001 | — |

`9000`/`9001` are 18×17 grey dummy textures in every category (PhaseII labels
9000 "Blank/Default"). The DLL currently lists them as the last two positions of
every row; the name table should tag them `placeholder` so the DLL can render
"(none)" or skip them — a DLL-side decision, but the data must carry the flag.

The DLL's getters treat id 0 as "unset" → item 1 (P2 character → 2), so the
stock names double as the "default" labels.

---

## 6. Mapping algorithm

### 6.1 Baseline build (maintainer-side, Python, produces the committed table)

1. Fetch premizer 1, parse the dropdown → sorted list of `premizer_id`s.
2. For each id ascending: fetch, verify `total_num == parsed count`, collect
   `(category, page_order, name, thumb_url, batch_title, sale_start, sale_end)`.
3. Load PhaseII's two JSON files; flatten each list to `id → label` and to the
   ordered groups.
4. Per category: start with the stock prefix (PhaseII ungrouped ids `< 9000`,
   hand-checked against the artwork), `next = max(stock) + 1`. For each batch
   ascending: align the page's item sequence inside PhaseII's group by
   normalised name (allowing PhaseII's trailing `(romanization)` suffix);
   PhaseII items before the alignment point are unlisted stock (appeal 4/5) and
   take `next…`; then the page items take the following ids in page order. Assert
   the group's ids are exactly `next..next+len` and consecutive with the previous
   batch. Konami's spelling wins for the stored name; PhaseII's label is kept as
   `alt_label` (it is the source of the English/romanized variants).
5. Read `$DDR_WORLD_INSTALL/data/arc/custom/<dir>/`, reduce filenames the way
   `discovery.rs::extract_id` does (leading digit run of the stem). Assert:
   every assigned id exists on disk; every regular id on disk (`< 9000`) is
   assigned; report the leftovers (placeholders, `100001+`) against §5.
6. Image-verify every page item (§7). Any NCC below the category threshold, or a
   runner-up within 0.2 of the best, fails the build.
7. Emit `customizer_names.json` (§8) with `source: "konami"` for page items,
   `"phaseii"`/`"manual"` for the rest, and `verified: ["phaseii","image"]`.

Steps 4–6 are three independent witnesses of the same mapping (community ids,
disk existence, pixels). All three agreed for the 691 page items on 2026-09-14.

### 6.2 Incremental refresh (the updater's future job)

Inputs: the shipped baseline (with `max_premizer_id`), the live dropdown, the
cabinet's `data/arc/custom`.

1. Parse the dropdown from any premizer page. If no `premizer_id > baseline.max`
   → nothing to do. (Also diff titles of known ids: a renamed batch is
   informational only.)
2. For each new id ascending: fetch + parse (§3), `total_num` check.
3. Per category: `next = max regular id in baseline + 1`. Assign the block's
   items `next, next+1, …` in page order.
4. **Verify before trusting:**
   - *Disk*: every assigned id's `.arc` exists in `data/arc/custom/<dir>/`. If
     the cabinet's data predates the batch (page live, assets not yet shipped —
     PhaseII's history suggests Konami's data drops land ~2 days before the sale
     but that is not guaranteed), the ids are absent → store the rows as
     `provisional`; the DLL ignores names for ids it did not discover anyway.
   - *Anchor*: the contiguity rule cannot see an unlisted item inserted BEFORE
     the new block (the premizer-1 EMI/RAGE shape). Catch it by image-matching
     the block's FIRST (or every) item against the assigned id's artwork (§7).
     A miss → try the next few ids (`next+1 …`) and, if a later id matches,
     the skipped ids are new unlisted items (name them `?` for manual review);
     if nothing matches → leave the whole block generic and log once.
   - *Community*: if PhaseII has a group with the same normalised title,
     compare ids/labels; disagreement → prefer the image-anchored result, log.
5. Write the rows with `source: "konami"`, `verified: [...]` reflecting which
   witnesses passed; never overwrite a baseline row (baseline wins — it was
   image-verified on the maintainer's machine); never touch the user override
   file.

Failure policy is always "keep the previous names, fall back to `Board #N` for
unknown ids, one log line" — never a wrong name. Names are cosmetic; a stale
table is fine, a shifted table is not.

### 6.3 What can still go wrong

- Konami changes the page template → the `premizer_box` presence check and the
  `total_num` equality fail closed.
- A batch's block order on the page differs from the id order → caught by the
  image anchor (this has not happened in 133 blocks).
- Konami removes a delisted batch from the dropdown (nothing has been removed so
  far; 【販売終了】 batches stay) → baseline rows are kept regardless.
- A category is added to the game (e.g. `bgm`, field `+0x34`, currently a
  stub) → new block id on the page; unknown block ids are logged and skipped.

---

## 7. Image verification (optional but strong)

Each thumbnail is the real asset composited at a fixed position onto a
category-specific backdrop. Cropping that region and comparing it with the
game's PNG (from the `.arc`; for backgrounds the IFS's `tex/bg*.png`) by
normalised cross-correlation on greyscale is enough to identify the asset
unambiguously. Measured on 2026-09-14 (rects in thumbnail pixels, `x, y, w, h`;
re-derive with the scale/offset search described below if Konami re-renders):

| Category | Asset px (game) | Thumbnail | Asset rect in thumbnail | NCC correct / runner-up |
|---|---|---|---|---|
| appeal_board | 428×56 RGBA | 280×113 | `14, 41, 252, 33` (frame on green grid) | 0.99 / ≤ 0.31 |
| game_bg (background) | 1284×724 (IFS `tex/bg*.png`) | 280×280 | `12, 68, 256, 144` (16:9 frame on red) | 0.96 |
| lane_bg_single | 430×720 | 280×280 | `66, 16, 148, 248` (on yellow gradient) | 0.83 |
| lane_bg_double | 850×720 | 280×280 | `16, 40, 248, 210` | 0.73 |
| lane_cover_single | 442×720 | 280×280 | `64, 14, 154, 251` (on blue) | 0.86 |
| lane_cover_double | 884×720 | 280×280 | `12, 36, 256, 209` | 0.85 |
| character | 680×800 (`_1p.png`) | 280×280 | thumbnail = asset scaled to ~352×416, window at `(8,16)` — a CROP, not a fit | 0.74 / ≤ 0.11 |

Method used to find the rects (and to use if they drift): flatten the RGBA
asset onto black, resize it to width `w` for `w` in a range, slide the resized
asset over the greyscale thumbnail and keep the `(x, y, w)` with the best NCC;
for characters, resize the ASSET and slide a 280×280 window over it instead.
Scores below ~0.7 on the lane categories are due to transparent regions of the
asset showing the backdrop; masking by the asset's alpha raises them. In
practice thresholds of 0.9 (appeal/background) and 0.6 (others) with a
runner-up margin ≥ 0.2 separate correct from incorrect on the checked samples.
Backgrounds and lane covers with `_v3/_v0/_lite` variants: match against the
first variant that exists (the DLL's own preview probes `_v3, _v0, "", _lite`).

Cost: one ~25–100 KB JPEG per item; the whole corpus is ~40 MB, an incremental
batch ~1–3 MB. In the updater this needs PNG + JPEG decoding (the `image` crate
family); the `.arc` container is trivial (`core/arc` already parses it in the
DLL, `scripts/unpack_arc.py` on the host) and background IFS needs an IFS
reader (`ifstools` on the host). If the updater should stay lean, restrict its
image check to the first item of each new block (one JPEG + one arc per block)
and leave full verification to the maintainer-side baseline build.

---

## 8. Proposed data format

One release-owned file, e.g. `data_mods/custom_options/customizer_names.json`
(shipped and pruned like any other release file — plain `Write` in the
updater's plan; the updater's later refresh step rewrites it in place), plus an
optional user override `customizer_names.override.json` beside
`mod-config.json` that is NEVER written by tooling (same-shaped `items` array,
user rows win by `(category, id)`).

```json
{
  "schema": 1,
  "generated": "2026-09-14T00:00:00Z",
  "max_premizer_id": 19,
  "batches": {
    "19": { "title": "「BEMANI納涼祭2026」 第2弾", "sale_start": "2026-08-06 10:00:00", "sale_end": null }
  },
  "items": [
    { "category": "appeal_board", "id": 1,  "name": "プレーン (パターン1)", "source": "phaseii", "verified": ["image"] },
    { "category": "appeal_board", "id": 6,  "name": "ALICE", "alt_label": "ALICE", "premizer_id": 1,
      "thumb": "/game/ddr/ddrworld/images/customize/appeal_board/alice.jpg",
      "source": "konami", "verified": ["phaseii", "image"] },
    { "category": "appeal_board", "id": 9000, "name": "", "placeholder": true, "source": "manual" },
    { "category": "background", "id": 100001, "name": "", "source": "manual", "note": "event group art; unnamed everywhere" }
  ]
}
```

- `category` uses the DLL's asset-dir spelling (`background`, `lane_single`,
  `lane_cover_single`, …) — the thing `discovery.rs` already knows — not
  Konami's block id.
- `name` is Konami's verbatim UTF-8 string. Optional `name_en` for a curated
  English/romanized display string (PhaseII's `alt_label` is a starting point:
  `TSUGARU`, `Amado Kon`, `All Characters Assembly`). `option_strings.py` is the
  precedent for hand-maintained localisation tables in this repo.
- `verified` records which witnesses passed so a later run can distinguish a
  baseline row from a `provisional` one.
- Keep the file sorted by `(category, id)` and pretty-printed so release diffs
  are reviewable.

---

## 9. Consuming the table in the DLL (constraints to design around)

The natural hook is a `value → String` resolver for the scalar row: the row's
value is a position, `DiscoveredCategory.asset_ids[pos]` is the id, the table
gives the name. Two render paths exist and they have very different limits:

1. **The native value slot** (`rows.rs::push_scalar_value_text` →
   `textlayer_set_text(row+0x130, sso, 3)`). Bytes go through the game's
   Shift-JIS `string::assign` into a 32-byte MSVC SSO string; anything **> 15
   bytes** heap-promotes and leaks once per push (per frame while visible —
   `rows.rs:2487-2495` WARNs). Of the 691 Konami names, **421 exceed 15 bytes in
   cp932** and 5 are not cp932-encodable at all. The slot can therefore only
   show a curated SHORT form (≤ 7 CJK chars / 15 ASCII), never the raw name.
2. **A mod-owned `TextWidget`** (`widget_renderer::create_text_widget`,
   `set_text` mode 1 = UTF-8, no length limit, the same `2d_font_system` font the
   game uses for song titles so CJK renders). `preview_overlay.rs` already owns
   per-side placement (`CHROME_ORIGIN`) for the preview box and shows/hides with
   the row focus — a name caption under/inside the preview box is the low-risk
   place. Glyph coverage for `♫`/`Ü` needs a one-time visual check.

Recommendation: keep `Board #N` in the native slot (cheap, always fits) and
render the full name as a UTF-8 caption in the preview overlay; add an optional
`name_short`/`name_en` column later if a native-slot label is wanted. Whatever
path is chosen, unknown ids and `placeholder` rows must fall back to today's
behaviour — the table is additive.

---

## 10. Updater integration sketch

`updater/` already has the pieces the refresh needs: `ureq` + rustls HTTP
(`github.rs`), a game-dir gate, a journaled apply, and a merge stage for
user-owned files. The refresh would be a new optional stage after
`archive::extract` and before `plan::build`:

1. Read the staged (release) `customizer_names.json` → `max_premizer_id`.
2. GET the premizer page for `max_premizer_id` (or the bare URL), parse the
   dropdown. No new ids → stage file unchanged.
3. For each new id: GET, parse (§3), assign (§6.2 step 3), verify against
   `<gamedir>/data/arc/custom` (disk) and, if the image check is included,
   against the first item's artwork.
4. Rewrite the STAGED file with the appended rows; `plan::build` then ships it
   through the normal `Write` action (so rollback/journal semantics are free).
   The user override is not in the manifest and is never touched.
5. Network failure / challenge page / parse mismatch → log one line, ship the
   release file as-is (exit 0). The refresh must never fail the update.

Rate limit: at most one request per second, browser-like UA, ≤ ~20 requests
per run (1 dropdown + N new pages + N thumbnails). Run it only when an update is
actually being applied (the updater already runs once per boot; a refresh on
every boot is unnecessary — cache `checked_at` in the manifest and skip if
< 24 h).

HTML parsing in Rust: the structure is regular enough for a tolerant
hand-rolled tokenizer (find `<ul id="…" class="premizer_box">…</ul>`, then each
`<li>`; read `src=`, `alt=` accepting `"` or `'`, `<span class="item">`), but
it MUST handle §3.5 items 1–2 and HTML entities (`&uuml;`, `&amp;`, numeric).
A small dependency such as `html-escape` covers the entities; a full HTML5
parser is not required.

---

## 11. Snapshot of the current state (2026-09-14, install data through premizer 19)

| Category (asset dir) | Ids on disk | Named by Konami pages | Stock (PhaseII/manual) | Placeholders | Unnamed |
|---|---|---|---|---|---|
| appeal_board | 138 | 124 | 5 + 7 BPL banners | 9000, 9001 | 0 |
| background | 71 | 64 | 3 | — | 4 (100001–100004) |
| character | 75 | 71 | 2 | 9000, 9001 | 0 |
| lane_cover_single | 118 | 112 | 4 | 9000, 9001 | 0 |
| lane_cover_double | 137 | 130 | 5 | 9000, 9001 | 0 |
| lane_single | 92 | 87 | 3 | 9000, 9001 | 0 |
| lane_double | 109 | 103 | 4 | 9000, 9001 | 0 |

691 page items across 19 batches; all 133 blocks aligned with PhaseII at offset
0 except appeal_board/premizer 1 (offset 2, §4); PhaseII deviations from
Konami's spelling: romanization suffixes (`天戸紺 (Amado Kon)`), katakana→Latin
(`ツガル`→`TSUGARU`, `キャラクター大集合`→`All Characters Assembly`), `メテオラ-meteor`
(missing trailing `-`), and one wrong label (appeal board id 70 reads
`東堂コハク (DDR 山神カルタ)`; Konami: `山神カルタ (DDR デフォルメ)`). Konami's text is
the stored name in every case.

---

## Appendix A — premizer → asset-id block per category

Ids are contiguous within each cell and each cell starts one past the previous
row's cell. Konami's pages list every id in these blocks except appeal_board
4–5 (EMI/RAGE, unlisted stock). Sale start is Konami's `li.term`.

| `premizer_id` | Title | Sale start | character | background | appeal_board | lane_cover_single | lane_cover_double | lane_single | lane_double |
|---:|---|---|---|---|---|---|---|---|---|
| — | stock (§5) | — | 1–2 | 1–3 | 1–5 | 1–4 | 1–5 | 1–3 | 1–4 |
| 1 | 第1弾 | 2024-12-25 | 3–4 | 4–8 | 4–10 (page lists 6–10) | 5–9 | 6–11 | 4–6 | 5–8 |
| 2 | 「BPL -SEASON 4- DanceDanceRevolution」 | 2025-01-30 | 5–6 | 9–11 | 11–18 | 10–13 | 12–16 | 7–10 | 9–13 |
| 3 | 「ひなビタ♪」 第1弾 | 2025-02-14 | 7–13 | 12–15 | 19–29 | 14–20 | 17–23 | 11–17 | 14–20 |
| 4 | 第2弾 | 2025-03-13 | 14–15 | 16–21 | 30–35 | 21–27 | 24–32 | 18–21 | 21–26 |
| 5 | 第3弾 | 2025-06-12 | 16–17 | 22–24 | 36–40 | 28–33 | 33–39 | 22–24 | 27–30 |
| 6 | 「春のコナステ大感謝祭2024」 | 2025-07-11 | 18–22 | 25–27 | 41–46 | 34–39 | 40–46 | 25–30 | 31–37 |
| 7 | 第4弾 | 2025-08-07 | 23–24 | 28–30 | 47–51 | 40–45 | 47–53 | 31–33 | 38–41 |
| 8 | 第5弾 | 2025-09-18 | 25–26 | 31–33 | 52–56 | 46–51 | 54–60 | 34–36 | 42–45 |
| 9 | 「東方Project」 第1弾 | 2025-10-23 | 27–30 | 34–35 | 57–62 | 52–57 | 61–68 | 37–42 | 46–53 |
| 10 | 「にじさんじダンス部」 第1弾 | 2025-11-20 | 31–36 | 36–38 | 63–71 | 58–61 | 69–73 | 43–48 | 54–60 |
| 11 | 「にじさんじダンス部」 第2弾 | 2025-11-20 | 37–42 | 39–41 | 72–80 | 62–65 | 74–78 | 49–54 | 61–67 |
| 12 | 「音戯探偵ひなビタ♫ 調査依頼:BEMANI」 | 2025-12-25 | 43–48 | 42–43 | 81–86 | 66–71 | 79–86 | 55–60 | 68–73 |
| 13 | 「GITADORA」 第1弾 | 2026-01-14 | 49–50 | 44–46 | 87–91 | 72–76 | 87–92 | 61–64 | 74–78 |
| 14 | 「pop'n music」 第1弾 | 2026-02-26 | 51–52 | 47–49 | 92–96 | 77–81 | 93–98 | 65–68 | 79–83 |
| 15 | 「秋のコナステ大感謝祭2024」 | 2026-03-26 | 53–58 | 50–51 | 97–102 | 82–87 | 99–105 | 69–74 | 84–90 |
| 16 | 「不知火フレア」 | 2026-04-23 | 59–61 | 52 | 103–108 | 88–94 | 106–112 | 75–77 | 91–93 |
| 17 | 第6弾 | 2026-06-18 | 62–66 | 53–55 | 109–112 | 95–102 | 113–121 | 78–82 | 94–99 |
| 18 | 「BEMANI納涼祭2026」 第1弾 | 2026-08-06 | 67–70 | 56–61 | 113–120 | 103–109 | 122–128 | 83–86 | 100–103 |
| 19 | 「BEMANI納涼祭2026」 第2弾 | 2026-08-06 | 71–73 | 62–67 | 121–129 | 110–116 | 129–135 | 87–90 | 104–107 |

## Appendix B — reference extraction (Python, host-side)

Minimal parser used for this research; the production script should add the
dropdown walk, throttling, PhaseII alignment, disk verification and the NCC
check from §6–7.

```python
import html, re, urllib.request

UA = {"User-Agent": "Mozilla/5.0"}
BASE = "https://p.eagate.573.jp/game/ddr/ddrworld/shop/premium_customizer.html"

def fetch(premizer_id):
    req = urllib.request.Request(f"{BASE}?premizer_id={premizer_id}", headers=UA)
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read().decode("utf-8")

OPTION = re.compile(r'<option value="(\d+)"[^>]*>([^<]*)</option>')
BLOCK  = re.compile(r'<ul id="([a-z_]+)" class="premizer_box">(.*?)</ul>', re.S)
ITEM   = re.compile(r'<img src="([^"]*)" alt=(["\'])(.*?)\2 />.*?<span class="item">(.*?)</span>', re.S)
TOTAL  = re.compile(r'<span class="total_num">(\d+)</span>')
H2     = re.compile(r'<h2>([^<]*)</h2>')
TERM   = re.compile(r'<li class="term">.*?<span>([^<]*)</span>\s*～\s*<span>([^<]*)</span>', re.S)

def parse(src):
    if 'class="premizer_box"' not in src:
        raise RuntimeError("no item list (challenge page or template change)")
    page = {
        "title": html.unescape(H2.search(src).group(1)),
        "term": TERM.search(src).groups(),
        "premizers": sorted(int(v) for v, _ in OPTION.findall(src)),
        "blocks": {},
    }
    n = 0
    for cat, body in BLOCK.findall(src):
        items = []
        for src_url, _q, alt, name in ITEM.findall(body):
            name = html.unescape(name).strip().strip("「」")
            assert html.unescape(alt) == name
            items.append({"name": name, "thumb": src_url})
        page["blocks"][cat] = items
        n += len(items)
    assert n == int(TOTAL.search(src).group(1)), "partial parse"
    return page
```

Sources consulted: Konami premizer pages 1–19 (fetched 2026-09-14);
`PhaseII-eAmusement-Network/PhaseWeb3-Vue` commit `3843be5` (2026-08-27);
the game install's `data/arc/custom` (data through premizer 19). Related repo
docs: `docs/player_customization_system_research.md` (Customize layout, wire
mapping, id semantics), `docs/option_preview_image_box.md` and
`docs/scene_load_analysis.md` (why the rows are scalar + why per-value textures
were retired), `src/mods/webui_options/discovery.rs` (id discovery).
