# Implementation Plan: Per-Song Judgement Offsets

Status: Approved 2026-08-17

## Checklist

- [x] Step 1: Pure state layers — `store.rs` and `csv.rs`
- [x] Step 2: One-time pre-seed script and repo-committed CSV
- [x] Step 3: Mod skeleton, musicdb bootstrap crawl, baseline load
- [x] Step 4: Option rows, wheel-poll seeding, edit capture, label textures
- [x] Step 5: Gameplay override, restore layers, tree-fix safety net
- [x] Step 6: String-field persistence extension and wire round-trip (client)
- [x] Step 7: bemani-buddy backend field
- [x] Step 8: Integration hardening and full cabinet validation

## Steps

### Step 1: Pure state layers — `store.rs` and `csv.rs`

**Objective:** the host-testable foundation: per-side session/baseline store
with merge semantics, wire encode/decode, and the CSV parse/serialize layer.

**Guidance:** create `src/mods/per_song_judgement_offsets/{mod.rs,store.rs,csv.rs}`
with `mod.rs` as a stub module gate only (no Mod registration yet). Implement
per the design's Data Models section: `Store` (baseline + two session maps),
`reset_to_baseline` / `apply_server_string` / `set_entry` / `clear_entry` /
`lookup`, wire encode (sorted, empty-map → empty string, 2000-entry cap) and
decode (clamp ±100, skip malformed pairs, aggregated warn signal), CSV
parse/serialize (header, blank cells, clamping, malformed-line skip,
append-merge preserving order). No game APIs touched.

**Tests (same step):** host tests for everything listed in the design's
Testing Strategy for `store.rs`/`csv.rs`: round-trips, clamping, malformed
tolerance, cap truncation, deterministic ordering, unknown-code preservation,
merge semantics, append-only CSV merge.

**Integration:** none yet (module compiled into the crate, unused).

**Demo:** `cargo test` green over the new pure layers; `cargo check` clean.

### Step 2: One-time pre-seed script and repo-committed CSV

**Objective:** the operator deliverable — `judgement_offsets.csv` pre-seeded
from the friend's mcode-keyed list, committed at the repo root.

**Guidance:** write `scripts/gen_judgement_offsets_csv.py` per the design's
Tooling section (reuse `scripts/arc_tool.py` the way
`scripts/validate_musicdb.py` does; mcode → basename from
`data/arc/startup.arc!data/gamedata/musicdb.xml`; P1 = P2; first value on
≠2-field lines with a printed warning; unmapped mcodes printed and skipped).
Run it against the real install + friend file; commit the script and generated
CSV.

**Tests (same step):** script self-checks its output (row count = musicdb
basename count, all friend values mapped or reported); manual spot-check of a
few known songs. Expected from real data: 1441 mapped, one 3-field warning
(mcode 449), 20 blank rows.

**Integration:** the CSV's format is exactly Step 1's `csv.rs` schema — parse
the generated file with a Step 1 host test fixture to prove compatibility.

**Demo:** running the script produces the committed CSV; a host test parses it
successfully.

### Step 3: Mod skeleton, musicdb bootstrap crawl, baseline load

**Objective:** the mod exists, boots on the cabinet, and self-manages the CSV:
crawl the merged musicdb, append missing codes, load the baseline into the
store.

**Guidance:** implement the `Mod` trait in `mod.rs` (id
`per-song-judgement-offsets`, `required_signatures()` = `player_option_table`
+ `selectmusic_model`), register in `src/lib.rs`. Gate `enable()` on
`custom_options::row_injection_available()` — fully inert with one WARN when
false (design requirement 9). Implement `bootstrap.rs`: background thread
(chart_length-style), `xml_merger::merge_xmls` reuse with the whole-file /
stock fallbacks, string-level `<basename>` scan, append-merge via `csv.rs`,
baseline load into the store, plus the coalesced mpsc CSV writer used by later
steps. Make `merge_xmls`/`load_xml_from_avs_path`/`find_first_modfile` `pub`
as needed.

**Tests (same step):** host tests for the basename scan against fixture XML
(stock-shaped and fragment-merged shapes) and the crawl→append pipeline with a
temp-dir CSV. Cabinet: deploy and observe logs.

**Integration:** first consumer of Step 1's store and csv layers; mod visible
in the registry.

**Demo:** delete `judgement_offsets.csv` on the cabinet, boot — the file is
recreated with the full song list; boot with a custom-song fragment installed
— its basename is appended; existing rows never modified (verify by diff).

### Step 4: Option rows, wheel-poll seeding, edit capture, label textures

**Objective:** the visible end-to-end editing loop: two menu rows that track
the highlighted song per side, with edits persisted to the CSV.

**Guidance:** add `LABELS` + `PreviewSpec` entries for both ids to
`scripts/option_strings.py`, run `scripts/gen_option_labels.py`
(5 PNGs × 3 languages). Register the rows in `enable()` (parent
`bool_toggle("adjust_song_offset")`, child
`scalar("current_song_offset", -100, 100, 1, Integer)` with `.step_coarse(10)`,
`.default_value(0)`, `ShowWhen::Equals` on the parent, both
`PersistMode::None`). Implement the wheel poll in `mod.rs` via
`input_manager::on_frame` (scene-25 gate, `selectmusic_model` weak_ptr walk,
guarded code getter — the music-wheel-song-length pattern), the shared
`CURRENT_CODE` cell, per-entered-side row seeding via `set_value_silent`, and
the `on_change` handlers (armed-flag guard; set/clear entry; CSV upsert via the
Step 3 background writer).

**Tests (same step):** host tests for the seed-decision and edit-application
logic factored into pure functions in `store.rs` (given code + maps → row
values; given change event → store mutation). Cabinet: the checks below.

**Integration:** consumes Step 1 store + Step 3 baseline/writer; rows appear in
the same options menu as existing mod rows.

**Demo (cabinet):** rows render with localized labels; **negative values
render** ("-7", "-100" — the minus-glyph verification); wheel navigation
re-seeds both rows and OFF-state on unset songs; toggling the parent
shows/hides the child same-frame; edits survive a reboot via the CSV; versus
mode keeps sides independent.

### Step 5: Gameplay override, restore layers, tree-fix safety net

**Objective:** the feature's core value and its safety guarantee: the stored
offset replaces the stock judgement offset during gameplay, and the stock
value is provably never persisted.

**Guidance:** implement in `mod.rs` per the design's override lifecycle:
`LOCKED_CODE` latch at scene-26 entry; arm at scene-28 entry per entered side
(`stage_records::side_entered`; skip course/event via the same accessors the
quick-restart predicate uses); first-judge-dispatch write via
`judge_hook::register_pre` (stock read + ±100 sanity refusal, `STOCK_CACHE`,
override write to `*(*(player_option_table + side*8)) + 0xE0 + 0x24`,
`ACTIVE`); restore on the `prev == GAMEPLAY` scene callback; redundant
restore-and-WARN sweep at SONG_SELECT entry. Add the
`replace_option_s32(tree, name, value)` helper (find 162 → remove 164 → re-add
163 type 6) to `custom_options_persistence.rs` and call it post-`original.call`
in the save trampoline when `ACTIVE` leaked for the saving side (WARN).

**Tests (same step):** host tests for the pure arm-decision logic (entered
sides × course flag × map lookup → pending set). Everything else is
cabinet-validated (no harness for hooks).

**Integration:** consumes Steps 1–4 (store lookups, locked code from the
poll); touches the shared save trampoline only in the leak branch.

**Demo (cabinet):** a +50 override audibly/measurably shifts judgement versus
the same song with the row OFF; **profile purity**: after a per-stage save and
after card-out, the server's `opt_timing_music` equals the stock value and the
stock JUDGEMENT OFFSET row shows the stock value — repeated across natural
exit, quick-restart, quick-fail, and in-place restart; course mode applies no
override; no WARNs on the happy path.

### Step 6: String-field persistence extension and wire round-trip (client)

**Objective:** the generic str-valued wire channel in
`custom_options_persistence`, with the mod as its first consumer — emitting
`mod_judge_offsets` on every save and applying server loads.

**Guidance:** per the design's extension section: `register_string_field`
registry; str emit post-`original.call` via the second ordinal-163 transmute
(`kbin type 11`, value = `*const i8`); str read in the load receiver via
ordinal 176 (64 KiB buffer, negative return = absent);
`PENDING_STRING_LOADS` keyed by ddrcode drained at SONG_SELECT entry after the
new `register_card_in_callback` notifications (mod resets side to baseline,
then server string replaces the session map). Wire the mod's save closure
(encode entered side's map; `None` while un-armed) and load closure
(`apply_server_string`).

**Tests (same step):** host tests for encode/decode already exist (Step 1);
add tests for the registry's dispatch bookkeeping where it is pure. Cabinet:
observe the emitted `<mod_judge_offsets>` in save traffic (server logs or
packet dump) — the server safely ignores the unknown child until Step 7.

**Integration:** extends the existing persistence service alongside
`emit_network_children`/`apply_pending_loads`; the mod's Step 1 store gains
its network path.

**Demo (cabinet):** with verbose logging, a save shows the encoded string
emitted for the entered side; game behavior otherwise unchanged (server
ignores the field for now).

### Step 7: bemani-buddy backend field

**Objective:** the server stores and echoes `mod_judge_offsets`.

**Guidance:** the design's nine-file change set in the bemani-buddy repo:
migration 016 (`opt_mod_judge_offsets TEXT NULL DEFAULT NULL` with the
standard header comment), model field, DAO three-spot edit + `.sqlx` regen,
`playdata_3.json` (`"mod_judge_offsets": "str?"` in both shapes **and** the
`mod_training_progress_pos` backfill), `playdata_3.rs` field with
`skip_serializing_if`, handler save-parse / load-emit / new-player `None`.

**Tests (same step):** handler unit tests mirroring the `mod_song_speed`
family plus the empty-string round-trip case (design Testing Strategy).

**Integration:** completes the Step 6 wire; no DLL changes.

**Demo:** full round-trip — set offsets on the cabinet, card out, verify the
DB column; card in again and watch the rows seed from server data; an offline
boot falls back to the CSV baseline; a profile with server data carding into a
side does not modify the CSV.

### Step 8: Integration hardening and full cabinet validation

**Objective:** close out the design's cabinet checklist end-to-end and settle
the carried verifications.

**Guidance:** run the design's Cabinet Validation items 1–8 as one pass on a
real session flow (guest + carded, versus, doubles, course, quick
restart/fail, in-place restart, reboot persistence, bootstrap append). Fix
anything found; where a fix changes behavior described in the design, update
the design document (and re-mark approval). Update `AGENTS.md` (feature row),
`README.md` operator docs, and `docs/` research notes if new RE facts emerged.

**Tests (same step):** any bug fixed here gets a host test if it is in a pure
layer; hook-layer fixes get a diagnostic log assertion noted in the deploy
log.

**Integration:** final; no new surfaces.

**Demo:** the complete feature demonstrated in one session: pre-seeded offsets
active out of the box, per-song editing on the wheel, server round-trip, and a
verified untouched stock `timing_music` on the server profile afterward.
