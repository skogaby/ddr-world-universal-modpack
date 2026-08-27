# Idea Honing: Per-Song Judgement Offsets

Readiness Confirmed 2026-08-17

## Decision Register

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Override mechanism & restore | Save marshal reads PlayerWork on every save; a leaked override permanently clobbers the profile's stock JUDGEMENT OFFSET | Write `Option+0x24` per side at first judge dispatch (real_speed pattern); cache stock value at write time; restore at scene-change **gated on `prev == GAMEPLAY`** (covers every exit shape incl. quick-restart/fail redirects); belt-and-braces = post-`original.call` **tree fix** in the save trampoline (find 162 → remove 164 → re-add 163 `<timing_music>` with stock value) — research proved a trampoline-time *memory* restore is too late (staging buffer already marshalled before the trampoline runs) | Accepted (mechanism refined by research) |
| D2 | Replace vs delta | Different mental models for stored values | Override **replaces** the stock value outright | Accepted |
| D3 | OFF representation | Scalar rows have no OFF state today; an override of 0 is a valid use case (distinct from OFF) | **Parent bool row** "ADJUST OFFSET FOR CURRENT SONG" + child scalar visible via `ShowWhen::Equals` (assist_tick volume precedent, zero framework change). Parent OFF = no entry for the song; parent ON + value (incl. 0) = override | Accepted (user-refined) |
| D4 | Row range/step | Value domain of the option and CSV | **−100..+100 ms**, fine step 1, coarse step 10 (user reverted from ±150 after reviewing real offset data) | Accepted (user) |
| D5 | Wire format | Server persistence shape | `mod_judge_offsets` kbin **str** child, `code\|offset\|code\|offset...`, only set songs, per-profile (no side dimension) — str emit/read conventions confirmed in Ghidra (ess.dll `ghost` round-trip: emit passes `*const i8` in the value slot with type 0xb; read via ordinal 176 with byte buffer + capacity) | Accepted (conventions verified) |
| D6 | CSV ↔ server merge | Prevent a friend's empty profile wiping local CSV | Session model: per-side in-memory map seeded from CSV; server load replaces the side's session map (CSV untouched); explicit edits update session map + targeted CSV upsert + next save; card-out resets side to CSV baseline; server-absent → CSV baseline | Accepted |
| D7 | Guest players | Behavior without a profile | CSV offsets apply to the side regardless of login (cabinet-local file) | Accepted |
| D8 | Stock menu purity | Avoid visual conflict with stock JUDGEMENT OFFSET row | Write memory only during gameplay | Accepted (was Assumed) |
| D9 | Row PersistMode | s32 channel is wrong for per-song data | `PersistMode::None` for the row; mod owns persistence via CSV + new string-field extension in custom_options_persistence | Accepted |
| D10 | Song key | Identity across CSV/wire/wheel | The wheel's song code string (= musicdb `basename`, same key chart_length uses) | Accepted |
| D11 | Backend shape | Server storage | Migration 016: `opt_mod_judge_offsets TEXT NULL DEFAULT NULL`, verbatim, omitted-when-NULL; standard 9-file commit; also backfill the missing 015 field into `models/ddr_world/playdata_3.json` | Accepted |
| D12 | Size guard | Stay under TEXT 64 KiB | Client-side soft cap (~2000 entries), warn once | Accepted |
| D13 | Naming | IDs and textures | Mod id `per-song-judgement-offsets`; row id `current_song_offset` ("CURRENT SONG OFFSET"); `option_strings.py` entries + regenerated eng/jpn/kor label textures | Accepted |
| D14 | Bootup musicdb crawl | CSV pre-seeded with all song IDs; updates append new songs without clobbering | At mod init/boot, obtain the **merged** musicdb (LayeredFS custom/injected songs INCLUDED — user requirement), collect all `basename`s, append missing rows (blank offsets) to `judgement_offsets.csv`, never modify existing rows. Capture mechanism = research (xml_merger observation point vs disk parse + mod fragments) | Accepted (user) |
| D15 | One-time pre-seed scrape | Generate a repo-committed CSV from the friend's mcode-keyed offsets file | New `scripts/gen_judgement_offsets_csv.py`: parse musicdb from the install (reuse `arc_tool.py` like `validate_musicdb.py` does), map mcode→basename, emit CSV with P1=P2 identical values; songs without a friend value get blank offsets. Committed at repo root next to `mod-config.json`. Verified: all 1441 friend mcodes resolve; values −23..+34; 20 musicdb songs lack friend values | Accepted |
| D16 | Malformed friend line | mcode 449 (`aaaa`) has TWO values (`2 -6`); everything else is 2-field | Script takes the **first** value and prints a warning listing anomalous lines for manual review | Accepted |
| D17 | assist_tick interplay | assist_tick sanity-clamps judgement timing to ±100 | Moot — range reverted to ±100 (D4); assist_tick stays as-is | Resolved (moot) |
| D18 | Game-side clamp on written value | Whether the game tolerates out-of-stock-domain writes | Moot — ±100 is the stock domain the game already produces | Resolved (moot) |
| D19 | Row labels | Two rows now (D3) | Parent `adjust_song_offset` ("ADJUST OFFSET FOR CURRENT SONG"), child `current_song_offset` ("CURRENT SONG OFFSET"); both need `option_strings.py` entries + eng/jpn/kor regeneration | Assumed |
| D20 | No headless mode | Behavior when option rows can't inject | Mod goes **fully inert** when `row_injection_available()` is false — no override, no persistence, no bootstrap writes; one WARN. (Design draft proposed headless CSV-only operation; user overrode at design review) | Overridden (user, design review) |
| D21 | Course/Dan + Training Mode coverage | Requirement 8 said course mode = no override; deploy #3 showed courses arm with a STALE code (the wheel latch identifies the course's first song only) | **Overrides apply in Course/Dan Ranking AND Training Mode** (user, 2026-08-18). Per-stage song identity comes from an `avs_fs_open` observer on SSQ opens (`.../ssq/<basename>[_N].ssq` — every stage load, courses included); the course veto is removed; arming becomes lazy (value resolved at first judge from the freshest code) | Accepted (user, supersedes requirement 8) |

## Notes / rationale

- **D3/D4 evolution:** originally single-row-with-OFF at ±150; user reverted range
  to ±100 after reviewing the friend's real data (−23..+34) and chose the
  parent-toggle model so that an override of *0* remains expressible (distinct
  from OFF). This eliminated the ScalarFormat extension and the assist_tick
  change entirely.
- **D14:** the crawl also guarantees the CSV exists on first boot (blank template)
  even if the operator never deploys the pre-seeded one. Custom/injected songs
  must be included (user requirement) — capture mechanism is a research item.
- **D15:** one-time tooling, not runtime; the friend's defaults are data
  (repo-committed CSV), never codified in code.
- **D6/D14 interplay:** bootstrap appends only; server loads touch only the
  in-memory session maps; explicit edits are the only per-row CSV writes. Three
  writers, disjoint scopes.
- **Per-song stored state (D3 consequence):** a song either has an entry (value
  −100..+100, including 0) or it doesn't. CSV cell empty = unset; any number
  (incl. 0) = set. Wire carries only set songs.
