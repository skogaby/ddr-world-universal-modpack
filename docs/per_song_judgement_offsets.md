# Per-Song Judgement Offsets — RE & Mechanism Notes

Consolidated reverse-engineering record for the `per-song-judgement-offsets`
mod (see `.agents/planning/2026-08-17-per-song-judgement-offsets/` for the
full planning history). Addresses are file-relative to gamemdx's
`0x180000000` base unless stated; verified builds 20260324/20260421/20260616/
20260721 via the shipped signatures.

## 1. The judgement-timing field

- `ddr::player::Option+0x24` = `timing_music` (the stock JUDGEMENT OFFSET,
  ±100 ms, positive = judged later). The Option struct is inlined into
  PlayerWork at `+0xE0`, so the field is `PlayerWork+0x104`.
- Per-side access: `*(*(player_option_table + side*8)) + 0xE0 + 0x24`
  (derived `player_option_table` signature; same chain assist_tick reads).
- Consumption: judge-compare time inside judgeNotes — not baked into note
  timestamps — so a write before a side's first judge dispatch governs the
  whole song, and a restore afterwards is invisible to gameplay.

## 2. Why the restore is scene-timed (the profile-clobber hazard)

`<timing_music>` is marshalled into EVERY player-data save (savekinds 1/2/3;
bemani-buddy parses it on all three and echoes it on load). The marshal
(`ReflectSavePlayerData`) copies PlayerWork into ess's per-side staging
buffer BEFORE the DLL's save-sender trampoline runs (the trampoline already
reads `savekind`/`playside` from that buffer pre-`original.call`), so a
memory restore inside the trampoline is too late. The mod therefore uses
three independent layers:

1. **Scene-timed memory restore** on any transition with `prev == GAMEPLAY
   (28)` — fires synchronously inside `createNextSequence`, an entire loader
   scene before the savekind-2 marshal (ResultSequence's first frames), and
   covers every exit shape: natural, quick-restart/fail fast paths and
   redirected fallbacks (redirects rewrite `next`, never `prev`), and course
   inter-stage transitions. In-place restarts never leave scene 28 and
   correctly keep the override.
2. **SONG_SELECT-entry sweep** (restore + WARN; unreachable by design).
3. **Save-tree fix**: if a leak is detected at save-build time, the built
   tree's `<timing_music>` is rewritten with the cached stock value —
   find (ordinal 162) → remove (164) → re-add (163, s32) via
   `custom_options_persistence::replace_option_s32` (the add needs the
   DERIVED context from ordinal 175, not the tree root).

## 3. Per-stage song identity (D21 — courses/training apply overrides)

Three sources feed one `LOCKED_CODE`, last writer wins; the offset value is
resolved LAZILY at the side's first judge dispatch:

| Source | When it fires | Why it exists |
|--------|--------------|---------------|
| Wheel latch (`ui::current_code` copied at scene-26 entry) | song confirm | baseline for flows with no fresh loads |
| SSQ-open observer (LayeredFS `avs_fs_open` hook, `mdb_apx/ssq/<basename>[_1..5].ssq`) | every chart load | wheel-independent identity |
| **Dance-bank create observer** (`wavebank_hook::publish_selected_song`, `sound/win/dance/<code>.xwb`) | every stage's audio load | **the course fix** |

Cabinet-proven: courses **batch-preload all stage SSQs at course start**
(zero per-stage SSQ opens mid-course), so the SSQ observer alone
misidentifies stages 2+. Each stage still creates its own dance bank,
strictly after the batch and before the stage's first judge dispatch — the
bank observer is the authoritative course source. Value resolution can't be
eager (at `createNextSequence(28)` the stage's bank isn't created yet).

Arming: per-side pending flag at scene-28 entry (`side_entered` +
`event_mode == 0` belt-and-braces). No course veto (D21 removed it).

## 4. Judge-dispatch priority is load-bearing

The override write registers at **`Priority::Early`** on the judge pre-hook.
assist_tick's `tick_clock` (`Priority::Normal`) reads `Option+0x24` when it
builds the song's tick list on the SAME first dispatch — the override must
already be in place so claps mark the true (overridden) judgement moment.
Within a priority, dispatch order is registration order, which the service
forbids relying on. real_speed (`Normal`) reads only speed fields; autoplay
pre is `Late`.

## 5. The string wire channel (kbin `str` mod fields)

First non-s32 `mod_*` wire field. Conventions Ghidra-verified against
ess.dll 20260324's own `ghost` round-trip (`sys_ghostdata_save_sender` /
`sys_ghostdata_load_receiver`):

- **Emit**: the ordinal-163 add-child function is variadic in its value
  slot — for kbin type 11 (`str`) it takes a **pointer to a NUL-terminated
  string** (ess: `LEA` of the staging buffer into `[RSP+0x20]`, `R8D=0xb`),
  vs by-value for s32/u64/bool. DLL side: a second typed transmute of the
  same export (`FnXmlAddChildStr`).
- **Read**: ordinal 176 with `(ctx, node, name, 11, byte_buf, capacity)`;
  negative return = absent. ess reads `ghost` with capacity 0x2001; the DLL
  uses 64 KiB against a client-side 2000-entry producer cap.
- Load-side application is deferred (side unresolvable at receive time):
  ddrcode-keyed pending buffer drained at SONG_SELECT entry, ordered
  card-in callbacks → s32 loads → string loads.

Generic registry: `custom_options_persistence::register_string_field(name,
save_fn, load_fn)` + `register_card_in_callback(fn)`.

## 6. Stock scalar value formatting (`ScalarFormat::SignedUnit`)

The stock DISPLAY/JUDGMENT TIMING rows format their value via
`FUN_18016e4e0` (20260721): `"%+dms"` for nonzero (`-41ms`, `+10ms`) and,
for zero, Shift-JIS `±` (bytes `81 7D`) + `"%dms"` (`±0ms`). The injected
scalar rows replicate this with `ScalarFormat::SignedUnit { unit }` — the
formatter emits raw BYTES (the SJIS `±` is not valid UTF-8) into the game's
SJIS-native `string::assign`/BmpString compositor. Constraints for reuse:
value must fit the 15-byte SSO scratch; each character needs a compositor
glyph (digits, `-`, `+`, `.`, `±`, `m`, `s` proven on-screen).

## 7. musicdb crawl (boot CSV seeding)

Disk-based on purpose: the AVS read trampolines
(`xml_merger::load_xml_from_avs_path` → `orig_fs_open`) only work for
in-hook game-thread callers — from a mod background thread `avs_fs_open`
fails while the game itself reads fine (cabinet-diagnosed 2026-08-18).
The crawl instead parses `data/arc/startup.arc` directly (`core::arc` +
AVSLZ + kbin guards) for the stock musicdb, honors a whole-file mod override
first, and unions every mod's `musicdb.merged.xml` fragment via `mod_paths`
— the same resolution order the open hook applies, custom songs included.
