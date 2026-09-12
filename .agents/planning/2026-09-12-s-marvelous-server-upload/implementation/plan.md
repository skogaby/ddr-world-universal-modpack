# Implementation Plan — S-Marvelous Server-Side Awareness

Status: Approved 2026-09-12 (maintainer)

Design: `design/detailed-design.md` (Approved 2026-09-12). Register:
`idea-honing.md`. Two repositories: the DLL (this repo) and bemani-buddy
(sibling checkout `../bemani-buddy`). Steps alternate so the wire contract is
exercised end to end as early as possible; each step leaves both repos
building and every existing test green.

Solo-maintainer rules apply: agents never commit; run each step's validation
and continue to the next in the same session.

## Checklist

- [x] Step 1: bemani-buddy protocol model re-sync (codegen diff empty)
- [x] Step 2: DLL pure payload builder + raw stream reader
- [x] Step 3: DLL `/data` subtree-producer registry + `s_marv` emission
- [x] Step 4: bemani-buddy migration, models, DAO (incl. tie-break)
- [x] Step 5: bemani-buddy save handler parses `s_marv`
- [x] Step 6: bemani-buddy load emits `option/smarv_scores`
- [x] Step 7: DLL S-MFC set (wire + local feed) and codec
- [x] Step 8: DLL song-select S-MFC lamp (signature, detour, texture)
- [x] Step 9: Docs, cabinet validation, repo guidance

## Steps

### Step 1: bemani-buddy protocol model re-sync

**Objective.** Restore `models/ddr_world/playdata_3.json` as the single source
of the generated protocol code so Step 6 can add a load field the sanctioned
way (design §4.6, FR13).

**Guidance.** Add `"mod_skip_results_fast_exit": "s32?"` and `"mod_sync_movie":
"s32?"` to both `PlayerdataLoadOption` and `PlayerdataSaveOption`, positioned to
match the hand-edited struct order. Run `cargo run -p codegen --
models/ddr_world/playdata_3.json crates/bemani-protocol/src/ddr_world/`, then
`cargo fmt`. Compare against the committed generated file. If the generator's
output differs in anything other than the two fields (formatting drift,
ordering), fix the model or generator — never the output.

**Tests.** `git diff --stat crates/bemani-protocol/src/ddr_world/playdata_3.rs`
empty; `cargo clippy --workspace --all-targets` clean; existing `playdata.rs`
tests pass.

**Integration.** No behaviour change; unblocks Step 6.

**Demo.** Regenerating the protocol crate from the model is a no-op.

### Step 2: DLL pure payload builder + raw stream reader

**Objective.** The S-Marv-aware recomputation as a host-tested pure function
(design §4.2, §4.3 `build_payload`/`to_node_spec`; FR3–FR6).

**Guidance.** In `src/mods/s_marvelous/records.rs` add `RawStreams` +
`read_raw_streams` and re-express `read_streams` as its judged filter. New
`src/mods/s_marvelous/upload.rs` with `RecordInputs`, `SMarvPayload`,
`CLEAR_KIND_SMFC = 11`, `GHOST_SMARV_CHAR = b'8'`, `build_payload`,
`to_node_spec` (the `NodeSpec`/`NodeLeaf` types land here as plain structs in
`custom_options_persistence` in this step too — types only, no emission yet).
Ghost overlay indexes the unfiltered stream; eligibility uses the judged mask.

**Tests.** Unit tests in `upload.rs`: all-S-Marv → 11; mixed; loose FAST/SLOW
split by stream sign (`ms > 0` ⇒ FAST); unjudged tail slots keep `'0'` and
don't count; consistency-gate refusal; `stock_marv == 0`; leaf order/names of
`to_node_spec` match design §5.1. All §5.1 invariants asserted per case.
`read_streams` ≡ filter of `read_raw_streams` on the existing fixtures.
Run via `cargo test` (pure module) — add the file to the s_marvelous host
harness in `scripts/validate_s_marvelous.sh` if the existing legs mount
`records.rs` explicitly.

**Integration.** No runtime behaviour yet; `records.rs` callers unchanged.

**Demo.** `cargo test upload::` shows the invariants holding on synthetic
records, including the S-MFC → 11 case.

### Step 3: DLL `/data` subtree-producer registry + `s_marv` emission

**Objective.** Put the node on the wire (design §4.1, §4.3 producer, §4.4
latch; FR1, FR2, FR7).

**Guidance.** `custom_options_persistence`: `register_data_node_producer` /
`unregister_data_node_producer`, a `create_void_node` helper (ordinal 163,
kbin type 1, no value), and the post-original emission loop after
`emit_string_fields` under the `PERSIST_NETWORK` gate — container + leaves,
partial failure ⇒ ordinal-164 removal + one latched WARN per producer.
`s_marvelous::state`: `ARMED_THIS_SONG` set in `arm_for_play_scene`, cleared
in `disable`. `s_marvelous::upload::produce(side, savekind)` with the gate
ladder from design §4.3 (each refusal its own latched WARN except the silent
ones), reading the record via `stage_records`, window via
`state::last_armed_window`, and `stage_records`' course predicate for the
course gate. Register in `enable`, unregister in `disable`. INFO line per
emitted node.

**Tests.** `cargo check --target x86_64-pc-windows-msvc`; `cargo fmt`;
`./build.sh`. Cabinet: one play with the mod on against bemani-buddy with
packet logging → `/data/s_marv` present, values equal the results tab; the
stock `<result>` diffed against a mod-off capture is identical; a failed-out
song emits a ghost the same length as the stock one with `'0'` tail; mod off
⇒ no node, no WARN. Log check for the void-create assumption (design §2.3).

**Integration.** First end-to-end contact with the server (it ignores the node
until Step 5 — confirm no status-1 responses).

**Demo.** Packet log shows the new node beside `<result>`; the game and a
stock backend behave exactly as before.

### Step 4: bemani-buddy migration, models, DAO

**Objective.** Storage for the S-Marv-aware values with the PB semantics of
design §4.7–§4.8 (FR9, FR10).

**Guidance.** `migrations/019_ddr_world_smarv.sql` (both tables, seven nullable
columns, header comment per §4.7). `DdrWorldSmarv` + `smarv:
Option<DdrWorldSmarv>` on the four score structs; all-or-nothing row mapping
keyed on `smarv_window_ms IS NOT NULL`. Score DAO: `SELECT_SCORES`, `ScoreRow`,
`row_to_score!`, INSERT, UPDATE, attempts INSERT. Upsert: `points >` OR
(`points ==` AND new `smarv.count` > old, `None` counting as 0). Check
`ddr-score-proxy`'s raw INSERT still applies (nullable columns).

**Tests.** DAO round trip on the test DB: insert `Some` → read back equal;
update with `None` NULLs all seven; attempt rows carry the values; tie-break
cases (equal points + higher count replaces; equal points + `None` does not;
lower points never). `cargo clippy` clean.

**Integration.** Handler still passes `smarv: None` everywhere (Step 5 wires
it).

**Demo.** Migration applies to a live DB; existing saves keep working with
NULL S-Marv columns.

### Step 5: bemani-buddy save handler parses `s_marv`

**Objective.** Persist what Step 3 sends (design §4.9; FR8).

**Guidance.** `parse_smarv_node(node, mcode, chart) -> Option<DdrWorldSmarv>`
(pure; lenient `parse().ok()`; identity mismatch ⇒ `warn!` + `None`; any
required child missing ⇒ `None`; empty ghost ⇒ `None` ghost). Call it in
`handle_save_scores` after result selection; store on the score and copy to
the attempt. Reuse the DLL's Step 2 fixtures as shared test vectors (same
grade/ms inputs → same expected node values) so both ends agree.

**Tests.** Unit: present / absent / mcode mismatch / chart mismatch / missing
child / empty ghost. Integration on the test DB: a synthetic kind-2 request
with `s_marv` lands in both tables. Cabinet: replay Step 3's play → row shows
`smarv_*` populated; a mod-off play on the same chart with a higher score NULLs
them; an S-MFC after an MFC replaces the PB row.

**Integration.** Completes the upload half end to end.

**Demo.** `SELECT … FROM ddr_world_scores` shows S-Marv and stock values side
by side for the same play.

### Step 6: bemani-buddy load emits `option/smarv_scores`

**Objective.** Tell the client which charts are S-MFC (design §4.10, §5.2;
FR11).

**Guidance.** Add `"smarv_scores": "str?"` to `PlayerdataLoadOption` in the JSON
model, regenerate (Step 1 made this safe), `cargo fmt`. `build_smarv_scores`
over the profile's scores (entries with `smarv.clear_kind != clear_kind`,
sorted, `mcode:chart:clearkind|…`, `None` when empty); set it in
`handle_playerdata_load`, `None` in `build_new_player_response`.
`rivaldata_load` / `ghostdata_load` untouched.

**Tests.** Unit: empty → `None`; filtering; ordering; encoding matches the
shared codec vectors. Snapshot of a load response confirms every stock field
(score CSV, ghost keys, clear kinds) is byte-identical to before.
`git diff` of the generated file shows only the new field.

**Integration.** The DLL ignores the field until Step 7 (unknown option
children are already tolerated by the load trampoline — confirm no WARN).

**Demo.** Packet log of a card-in shows `<smarv_scores>` listing the S-MFC
chart from Step 5.

### Step 7: DLL S-MFC set (wire + local feed) and codec

**Objective.** The client knows its S-MFC charts (design §4.5 state + codec;
FR12 first half, D15).

**Guidance.** New `src/mods/s_marvelous/lamp.rs`: per-side
`HashSet<(i32, u8)>`, pure `decode`/`encode` for §5.2 (skip malformed entries),
`on_load` (replace), `clear` (card-in callback), `insert_local(side, mcode,
chart)`. Register the string field (`save: None`) and the card-in callback in
`enable`; unregister in `disable`. `upload::produce` calls `insert_local` when
it emits `clearkind == 11`. One INFO per side on load with the set size.

**Tests.** Codec round trip + malformed/empty cases (shared vectors with
Step 6). Cabinet: card-in log shows the set populated from the server; an
S-MFC play logs the local insert before any load.

**Integration.** Consumes Step 6's field through the existing registry; no
rendering yet.

**Demo.** Log line `SMarvelous: lamp set side=0 loaded N chart(s)` after
card-in.

### Step 8: DLL song-select S-MFC lamp

**Objective.** Show it (design §4.5 badge; FR12 second half).

**Guidance.** New signature `selectmusic_card_refresh` (prologue of the
header-card refresh whose lamp block formats `fullcombo_%dp_usr` /
`muca_card_%s`); `GenericDetour`, post-original, `catch_unwind`. Scene 25 gate;
per entered side: current chart from `(PlayerWork+0x54, +0x5C, GameWork+0)`,
set membership, `layer = *(this+0xD0)` and `layer_id = *(layer+0x08)` both
`memory::is_readable`-probed, then `bm2d_api::layer_find_child(layer_id,
"fullcombo_{n}p_usr")` → `mc_load_bitmap(id, "muca_card_fc_smfc")` for the id
and each `mc_traversal(id, 6)` sibling. Texture: FRESH `atlas_cloner` clone at
enable, donor `muca_card_fc_mfc`, PNG `data_mods/s_marvelous/select_music/muca_card_fc_smfc.png`
(violet recolor consistent with the results emblem art). Every miss ⇒ stock
lamp + one WARN.

**Tests.** `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` green;
`shape_diff.py` on the new signature shows the `this+0xD0` load and the lamp
block stable on all four builds (adjust the anchor or add a `_vN` alternate
otherwise). Cabinet: S-MFC chart shows the violet lamp at song select
immediately after the play and after card-out/card-in; MFC/PFC/no-lamp charts
unchanged; scrolling the wheel keeps the correct lamp per song; 2P with one
S-MFC side badges only that side; mod off ⇒ stock.

**Integration.** Completes the echo-back half; both halves now demonstrable in
one credit.

**Demo.** Play an S-MFC, return to song select: violet S-MFC lamp on the card.

### Step 9: Docs, cabinet validation, repo guidance

**Objective.** Leave both repos explaining themselves (D20, D21 rule).

**Guidance.** DLL: addendum to `docs/s_marvelous_judgement_research.md` (marshal
facts — ghost = all slots, wire clearkind `+0x54` (`+0x270` is `folder`; also fix `docs/premium_free_stale_record_bug.md:37`), course record on kind 2;
card-lamp table with no bounds check; ghost alphabet + `'8'`); AGENTS.md
S-Marvelous row gains the upload/echo-back summary and the subtree-producer
registry note; `.agents/learnings` entry for "unknown clearkind indexes an
unchecked table — never widen a stock enum on the wire". bemani-buddy:
AGENTS.md note that `models/*.json` is the only input to the protocol crate and
the `smarv_*` contract; fix the stale "play_count on savekind=1" line in its
summary docs. Final full cabinet pass of the Step 3/5/8 checklists on one
build; record results in `progress.md`.

**Tests.** `cargo fmt` (whole crate) + `cargo check` + `./build.sh` in the DLL;
`cargo clippy` + tests in bemani-buddy; `git grep -nE "/(Users|home)/[^/ ]+/"`
adds no hits in either repo.

**Integration.** No code change beyond docs; closes the feature.

**Demo.** A fresh agent can read the AGENTS.md rows and reproduce the wire
contract without this planning directory.
