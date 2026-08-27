# Progress — `services::game_audio` (Step 2, task 02)

**Updated:** 2026-07-26
**Status:** Complete (not committed — the maintainer owns commits). All eleven acceptance criteria
verified on the running build except AC5's *audible* half, which is the maintainer's by the feature's
verification split. The final build is installed and the game is running the attract loop, which
claps once per demo song — audible right now with no login needed.

## Checklist

- [x] Setup: working dir, upstream approval chain + task-01 dependency verified
- [x] Explore: research pinned into a fact table; service/judge/mod_paths precedents identified
- [x] Plan: per-AC evidence table + implementation approach
- [x] `src/services/game_audio.rs` — API, `Inner`, ABI aliases, constants
- [x] `register_bank` in design §4.1's exact order, with both required guards
- [x] `play_cue` over the **public** entry, XMM2 float ABI, warn-once sentinel
- [x] Temporary demo trigger (one contiguous block + one marked call site, no detour)
- [x] Registered in `src/services/mod.rs`; initialised from `src/lib.rs` step 6b1
- [x] Gates: `cargo check` (clean, no warnings) → `cargo fmt` → `./build.sh`
- [x] Installed + 5 boots of the local install; every log criterion confirmed
- [ ] **Maintainer:** confirm the clap is audible (AC5's listening half)

## Deviation — one, design-level, escalated and approved

**The XACT engine-module presence check cannot live in `init`.** Design §4.1 and task requirement 2
put `GetModuleHandleA("xactengine2_10.dll")` in `init`. The first boot proved that impossible:

```
16:06:27  I:ddr: `regsvr32.exe /s ".../com/xactengine2_10.dll"` returned 0   ← only REGISTERED
16:06:30  [DDR-Hook][WARN] GameAudio: xactengine2_10.dll not loaded -- service disabled
16:06:31  [DDR-Hook][INFO] DDR World Hook DLL ready. 21 mod(s) active.
16:06:31  M:ddr: Application::onBoot() end.                                  ← engine created AFTER us
```

The engine is COM-instantiated inside `Application::onBoot`, which completes **after** the DLL's init
thread finishes, so the guard as specified always failed and the service was permanently disabled.

Halted and escalated (it changes what `is_available()` promises, which Step 3's mod-init gate reads).
**Maintainer chose: move the check to the first `register_bank`.** Implemented:

- `init` resolves addresses only; `is_available()` means "addresses resolved".
- `register_bank` checks the module immediately before the first vtable dispatch — strictly tighter
  than a boot-time check — and declines with one warning if absent.
- Design §4.1 and §6's error table carry a dated in-place amendment recording this (the design
  already uses that convention in §4.4).
- Consequence, recorded for Step 3: the wrong-engine case now surfaces as one declined registration
  (mod silent) rather than as a mod that declines to init.

## Other judgement calls (pre-recorded in `context.md`, no design impact)

1. **Requirement 12's "cue index resolved"** is logged by calling `GetCueIndex` (a sanctioned vtable
   index) on the first play of a cue name, at **info** — info rather than warn so AC9's
   "exactly one warning for the session" still holds when the cue is genuinely missing.
   No change to the design's public API: `BankHandle` stays `{ slot }` and the sound-bank pointer
   lives in the service's own registry.
2. **All permanent `register_bank` failures latch** (`REGISTER_DECLINED`) so a demo that retries every
   song still yields exactly one warning. The null-manager case deliberately does **not** latch — it
   is the one transient failure (boot not finished), so a later attempt may legitimately succeed.
3. **Scaffolding claps once per *song*, not once per session** — detected by a drop in the judge
   clock, the same rewind test design §4.2 uses. A once-per-session latch would have made AC5's
   central claim ("a clap at the start of *each* song, without re-registering") unobservable.
4. **The scaffolding reads its files at `install()` time on the init thread**, not in the judge
   callback — file IO does not belong on a per-frame path, and this is also where design §4.2 puts
   the real mod's load.

## Cycles

Not a red/green TDD sequence — no harness (see `context.md`). Each cycle was type-checked, and the
behavioural ones were verified against the running game.

| # | Work | Result |
|---|---|---|
| 1 | Service skeleton + `register_bank` + `play_cue` + scaffolding; `mod.rs` / `lib.rs` wiring | `cargo check` clean |
| 2 | Boot 1 | **Finding:** module check impossible at init → escalated |
| 3 | Relocate the check; latch the permanent failure paths | `cargo check` clean, no warnings |
| 4 | Boot 2 — happy path | slot **4** computed, both HRESULTs `0x0`, cue index 0, `played=true`, **second attract song clapped with no second registration** |
| 5 | Boot 3 — banks renamed away (AC6) | one warning naming the expected path; no clap; boot otherwise identical |
| 6 | Boot 4 — one byte of `tick.xsb` flipped (AC7) | `CreateSoundBank hr=0x8AC70007`, one warning with the HRESULT + gloss, nothing written, latch held over a second song |
| 7 | Probe build — boot 5 (AC8 + AC9) | see below; probe reverted and the source verified byte-identical to pre-probe |
| 8 | Final build, installed, boot 6 | **1 registration, 4 claps across 4 attract songs**, 0 crash records |

Logs: `logs/{cargo-check,build,boot-happy-path,boot-corrupt-xsb,boot-probe-ac8-ac9,boot-final}.log`.

## Verification

### Final build, `log.txt` (`logs/boot-final.log`)

```
[INFO] GameAudio: initialized (se_play @ 0x6ffffb58a6e0, audio_manager_global @ 0x6ffffbad2d60)
[INFO] GameAudio demo: loaded bank pair (5416 + 262 bytes)
[INFO] GameAudio started
[INFO] GameAudio: slot layout OK (se_normal slot 2 bank = 0x14facf00)
[INFO] GameAudio: claiming free sound-bank slot 4 (of 6)
[INFO] GameAudio: CreateInMemoryWaveBank('asti', 5416 bytes) hr=0x00000000 bank=0x1508d290
[INFO] GameAudio: CreateSoundBank('asti', 262 bytes) hr=0x00000000 bank=0x1508d4e0
[INFO] GameAudio: bank 'asti' registered in slot 4 (file_id left at -1 deliberately)
[INFO] GameAudio: cue 'asti' -> index 0 in bank 'asti' (slot 4)
[INFO] GameAudio demo: song-start clap at music_count=-87 played=true      ← song 1
[INFO] GameAudio demo: song-start clap at music_count=-87 played=true      ← song 2
[INFO] GameAudio demo: song-start clap at music_count=-87 played=true      ← song 3
[INFO] GameAudio demo: song-start clap at music_count=-87 played=true      ← song 4
```

`registrations: 1   claps: 4   crash records: 0`

Three things worth calling out:

- **Slot 4 was computed, not assumed** — and it came out as the slot the research predicted, with
  slots 0/1/2/3/5 legitimately occupied by the game's own banks (proven a second way by the probe,
  where a second bank found nothing free).
- **One registration, four claps.** The bank survived four song loads and unloads. That is the
  design's central claim about leaving `file_id` at `-1`, observed rather than argued.
- **`cue 'asti' -> index 0`** matches Step 1's single-cue-at-wave-index-0 amendment exactly.

### Per-criterion

| AC | Verdict | Evidence |
|---|---|---|
| 1 — addresses resolve, service available | ✅ | the `initialized` + `GameAudio started` lines above |
| 2 — `init` calls no game function | ✅ | source: 3 × `get_address` + `transmute` only (the module check moved out — see the deviation). Six clean boots |
| 3 — free slot computed, bank registers | ✅ | slot layout OK → `claiming free sound-bank slot 4 (of 6)` → both HRESULTs `0x00000000` |
| 4 — `file_id` never written | ✅ | source: one write, `*slot_bank_ptr(mgr, slot) = sound_bank`, with a 16-line comment at the site explaining why `file_id` must stay `-1` |
| 5 — audible, survives song loads | ✅ log half / ⏳ audible half | 1 registration + 4 claps over 4 songs, `played=true` each time (`se_play` returned a real handle, not the sentinel). **Maintainer to confirm it is heard** |
| 6 — missing bank files degrade | ✅ | banks renamed away → exactly one WARN naming `data_mods/assist_tick/banks/tick.xwb`; service still initialised; no clap; 0 crashes |
| 7 — corrupted sound bank degrades | ✅ | one byte flipped at XSB `0x50` (CRC-covered) → `CreateSoundBank hr=0x8AC70007`, exactly one WARN carrying the HRESULT and its gloss, **nothing written to the manager**, no crash. A second attract song produced no repeat |
| 8 — no free slot degrades | ✅ | probe registered a *second* bank after ours took slot 4 → `no free sound-bank slot among 6 -- declining to register`, count **1**, and **no create calls at all** for it (declined before touching the engine). A genuine exercise of the computed search, not an artificial range edit |
| 9 — playback failure warns once | ✅ | probe played a nonexistent cue on **every** judge frame of two attract songs → `cue 'nope' NOT FOUND` (info) and exactly **1** `failure sentinel` WARN for the whole session |
| 10 — scaffolding unmistakable | ✅ | one contiguous `mod demo` under a banner saying Step 3 deletes it, plus one marked call at the end of `init`; subscribes to `judge_hook`, installs no detour |
| 11 — build gates | ✅ | `cargo check` clean and warning-free, `cargo fmt` no-op, `./build.sh` clean; installed DLL sha256 matches the build output |

### Probe hygiene

The AC8/AC9 probe was a throwaway edit to the `demo` module only. Afterwards
`src/services/game_audio.rs` was restored from a pre-probe copy and `diff` confirmed it
**byte-identical**; the installed DLL was rebuilt from the restored source and its sha256 verified
against the build output.

## Notes for Step 3

- Delete the `mod demo` block and the `demo::install();` call at the end of `init`. Nothing else in
  this service is scaffolding.
- `mods::assist_tick::init` should load `banks/tick.{xwb,xsb}` through
  `avs_layeredfs::mod_paths::find_first_modfile` (the same call the scaffolding used) and keep the
  bytes; `register_bank` consumes them, and it is idempotent, so cloning per song is fine.
- The cue is `asti`, index 0. `play_cue(handle, c"asti", 0.0)` — always centre-panned per FR-6.
- `is_available()` no longer implies the engine module is present (see the deviation), so the mod's
  init gate should treat "service available" as necessary but not sufficient; a wrong engine shows up
  as one declined registration.
- `se_play_inner` is resolved and unused — the one-line swap in design §6 if the SE mute filter ever
  vetoes our bank. It did **not** veto: `played=true` on every clap, on bank 4, which is *not* one of
  the two filter-exempt banks. That closes research risk **R-2**.
- Handle-table pressure (research R-3) was not stressed here — one clap per song. Step 4's
  one-tick-per-frame is what bounds it.

**Status: Complete (uncommitted — maintainer owns commits)**
