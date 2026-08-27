# Implementation Plan — Assist Tick

**Status: Approved 2026-07-25**
**Date:** 2026-07-25

Decomposes `design/detailed-design.md` into six steps. Each step leaves the tree building and
demonstrable; risk is front-loaded so that the two things most likely to invalidate the design
(the audio bank actually playing, and the tick landing on the beat) are proven in Steps 2 and 3
rather than at the end.

Read the design document for *what* each piece does — this plan deliberately does not restate it.

## Checklist

- [x] **Step 1:** Generate and commit the clap bank pair (offline asset pipeline)
- [x] **Step 2:** `services::game_audio` — signatures, bank registration, cue playback
- [x] **Step 3:** `mods::assist_tick` — end-to-end ticking on the dispatched actor
- [x] **Step 4:** Side selection + eligibility predicate + one-tick-per-frame
- [x] **Step 5:** Option row, latching, and lifecycle
- [x] **Step 6:** Diagnostic pass, behavior matrix, docs, and final gates

---

## Step 1: Generate and commit the clap bank pair

**Objective.** Produce a validated XACT wave-bank + sound-bank pair containing the StepMania clap,
committed to this repository, so that every later step has a real asset to load. Nothing in the
hook DLL changes in this step.

**Implementation guidance.**

1. **Cross-repo prerequisite first.** In the sibling `ddr-chart-tools` repository, add an
   SE-profile entry point to the XSB writer alongside the existing song-profile one: mix
   **category 6**, a bare sound entry, and **no** runtime-parameter curve (design §4.4). Additive
   only — the song path, the CRC-16, and the cue-name hash are untouched, and its existing tests
   must still pass. Do not modify anything else in that repository.
2. Transcode the source sample (`data_mods/assist_tick/source/clap.ogg`, committed in this
   step) to 16-bit PCM mono 44.1 kHz with `ffmpeg`, then encode to
   MS-ADPCM and emit the wave bank using that repository's writers: buffer (non-streaming) bank,
   `header_version` 42, entry-name element size 64, alignment 4, **one entry** — the clap. Emit the
   sound bank with the new SE profile: a single cue named `asti` pointing at wave index 0, with the
   wave-bank name byte-identical to the wave bank's internal name.

   *(Amended 2026-07-25 during task generation: the design as written specified two entries, because
   the pre-existing **song** profile points its main cue at wave index 1. Since the SE profile is
   being authored from scratch in this same step, a single cue at index 0 is emitted instead, which
   removes a meaningless silent stub entry. Documented fallback: if any engine validator rule objects
   to a single-entry bank, revert to the two-entry shape, which was generated and validated during
   research. Task 02's offline validation catches this before anything reaches the game.)*
3. Drive both from a committed script, `scripts/build_assist_tick_bank.sh`, following the shape of
   the existing `scripts/build_shaders.sh`: it documents the one-time procedure, is re-runnable,
   locates the sibling tool, and fails loudly with an explanatory message if that tool or its
   SE-profile support is missing.
4. Commit the source sample alongside the outputs, per design §4.4:
   `data_mods/assist_tick/source/clap.ogg`, `data_mods/assist_tick/banks/tick.xwb`,
   `data_mods/assist_tick/banks/tick.xsb`.

**Tests covering this step** (all offline, per design §7.1 — this is the step where the format is
proven, because a malformed sound bank is rejected *silently* by the engine at runtime):

- Round-trip the generated wave bank through the sibling tool's parser; assert the parsed header
  fields match what was requested.
- Compare every wave-bank header field against both stock in-memory SE banks extracted from the
  local game install; differences must be only the intended ones (name, entry count, entry
  formats).
- Check the generated files against the engine's validator rules as transcribed in the research
  notes: exact segment offsets, the file-length-versus-wave-data-segment-length identity,
  entry-name element size, minimum alignment, name-field termination, entry flag bits.
- Decode the generated ADPCM back to PCM and compare against the transcoded source to confirm the
  encode chain (sample count, no clipping, no silence).
- Assert the sound bank's stated wave-bank name equals the wave bank's internal name, byte for
  byte including case.

Where the sibling repository has a test harness, these belong there as tests of the new SE
profile. Everything checked against the *local install's* stock banks belongs in
`scripts/build_assist_tick_bank.sh` as post-generation assertions, so re-running the script
re-validates.

**Integration.** Standalone; no DLL code touched. The committed asset is what Step 2 loads.

**Demo.** `scripts/build_assist_tick_bank.sh` runs clean and prints the two output paths with
their sizes and a validation summary. The two committed files exist, are of the expected size
(roughly 5.5 KB and 330 B), and re-running the script reproduces them byte-for-byte.

---

## Step 2: `services::game_audio`

**Objective.** Register a mod-owned bank pair with the game's XACT engine and play a cue from it,
proving the single riskiest assumption in the design: that our own sound bank survives in a free
manager slot and is audible.

**Implementation guidance.**

1. Add the three signature patterns from design §4.3 to `src/core/signatures.rs`, plus the
   derivation of the audio-manager global by RIP-relative decode from its landmark. Use the
   existing scanner primitives; do not hand-roll displacement decoding. Verify resolution logs on
   the local build before going further — this global's address differs on every game build, so a
   silent mis-derivation here would look like "no audio" much later.
2. Add `src/services/game_audio.rs` with the API in design §4.1, registered in
   `src/services/mod.rs` and initialized from `src/lib.rs` at a point after signature resolution.
   `init` resolves addresses and checks that the expected XACT engine module is loaded; it calls
   **no** game function (design §3.3).
3. Implement `register_bank` exactly in the order given in design §4.1: null-check the manager,
   sanity-check the slot layout, **compute** the free slot, create the wave bank, then the sound
   bank, then write only the slot's bank pointer — never its file id. Leak both byte buffers, with
   the reasoning in a comment (XACT does not copy in-memory wave data).
4. Implement `play_cue` over the game's public SE play entry point. Mind the ABI: the pan argument
   travels in XMM2, so declare it as `extern "system" fn(i32, *const c_char, f32) -> u32`.
5. Wire a **temporary** trigger so this step is demonstrable on its own: a one-shot call that
   registers the bank and plays the cue the first time the judge hook dispatches. This is
   scaffolding, replaced wholesale in Step 3 — keep it in one clearly-marked block.

**Tests covering this step.** No harness exists for game-ABI code, so verification is the
diagnostic logging that ships as part of this step and is read back out of the local install's
`log.txt`:

- signature resolution: each new pattern's resolved address, and the derived manager global
- `register_bank`: the slot layout check result, which slot index was chosen, and the HRESULT of
  each creation call
- `play_cue`: the cue index resolved, and a warn-once on failure

Negative paths are exercised deliberately, each confirming a single warning and no crash: rename
the bank files away; corrupt one byte of the sound bank's CRC-covered region; force the free-slot
search to find nothing.

**Integration.** Consumes Step 1's asset via the existing `data_mods` path resolver. Subscribes to
the existing judge dispatcher — no new detour anywhere.

**Demo.** Launch the local install and start any song: a single clap is audible on the first judge
frame, and `log.txt` shows the resolved addresses, the chosen slot, and successful bank creation.
Playing several songs in a row confirms the bank survives song loads and unloads — which is the
design's central claim about leaving the slot's file id untouched.

---

## Step 3: `mods::assist_tick` — end-to-end ticking

**Objective.** Claps on the beat, for real, for the dispatched actor's chart. This is the step that
proves the timing model.

**Implementation guidance.**

1. Add `src/mods/assist_tick.rs`, register it in `src/mods/mod.rs` and `src/lib.rs`. Implement the
   `Mod` trait per design §4.2: `init` verifies prerequisites and loads the bank bytes from
   `data_mods`; `enable` registers the judge pre-callback at `Normal` priority and the scene
   subscription; `disable` unregisters both.
2. On the gameplay scene event, set the rebuild flag and clear song state. Build the tick list on
   the first judge dispatch (design §4.2.2) using the existing Results-vector helpers. For this
   step, tick whatever actor was dispatched and accept **every** note with a non-negative
   timestamp — eligibility filtering is Step 4, so that a wrong predicate and a wrong clock can't
   be confused with each other.
3. Implement the cursor advance and the adaptive half-frame lead (design §4.2.3), with the lead in
   one named constant/function.
4. Remove Step 2's temporary trigger block.

**Tests covering this step.** Diagnostic logging read from `log.txt`: per song, the eligible-note
count, the first several timestamps, the observed frame delta and computed lead; per tick (debug
level only, and only for the first N ticks) the scheduled versus actual timestamp so lateness is
measurable rather than guessed.

Plus a listening pass using the scripted navigation harness (design §7.3) to reach gameplay
repeatably: claps land on the beat; they continue through deliberately missed notes (the core
consequence of chart-driven rather than judgment-driven triggering); the count of ticks in a song
is plausible for the chart.

**Integration.** Replaces Step 2's scaffolding with the real trigger path. Step 2's service is
otherwise unchanged.

**Demo.** Play a song and hear a clap on every note. Miss deliberately and the claps keep perfect
time. Quick-restart mid-song and ticks resume from the top.

---

## Step 4: Eligibility predicate, side selection, and tick coalescing

**Objective.** Tick exactly the right notes, from exactly the right side, exactly once.

**Implementation guidance.**

1. Implement the FR-2 predicate (design §2.1) as one small pure function taking a note pointer —
   `kind == 0`, not a shock, has a live panel, non-negative timestamp — and *not* consulting the
   freeze-length array. Document the `FREEZE ARROW: OFF` reasoning at the call site, since the
   temptation to "improve" this by reading `length[]` is exactly the trap.
2. Implement exact de-duplication plus coalescing within the named window (FR-3).
3. Implement one-tick-per-frame with cursor skip-ahead (FR-4).
4. Implement side selection (design §4.2.1): walk the sibling actors from the dispatched actor,
   vtable-compare against the dispatched actor's own vtable, read side and style, apply the FR-5
   table, and fall back to the dispatched actor's own side if enumeration yields nothing. Log the
   enumeration result once per song.

**Tests covering this step.** The behavior matrix from design §7.4, driven where possible by the
navigation harness and verified by listening plus per-song diagnostic counts:

- jumps produce one clap per row, not per panel
- freeze heads tick, freeze tails do not
- shock arrows do not tick
- mod-injected mines do not tick
- CUT and JUMP-OFF modifiers: removed notes do not tick
- `FREEZE ARROW: OFF`: heads still tick
- solo on the P1 side, and **solo on the P2 side**, each tick their own chart
- doubles ticks across all eight panels
- two players, same difficulty; two players, different difficulties; only the second side enabled

The maintainer runs these (see "Verification split" below) and has the required charts —
including ones with shock arrows and with this repository's own injected mines — already
identified.

**Integration.** Narrows Step 3's deliberately-permissive behavior. No new interfaces.

**Demo.** Walk the note-type cases on real charts and hear the predicate hold. Start a
two-player session on mismatched difficulties and hear a single stream aligned to P1; disable the
first side's option and hear the stream follow the second side instead.

---

## Step 5: Option row, latching, and lifecycle

**Objective.** Players turn it on themselves, from the game's own options screen, and the setting
follows their card.

**Implementation guidance.**

1. Generate the row's label texture and add its entry to the existing label-generation script;
   commit the texture alongside the other option labels.
2. Register the two-value OFF/ON row through the custom-options service with the framework's
   default persistence, defaulting to OFF (design §4.2). The change callback writes atomics only —
   it can fire on the init thread and on a spawned persistence-priming thread, not just the render
   thread.
3. Latch per-side enable state on the gameplay scene event; mark a song inert when the chosen side
   is not enabled (FR-8).
4. Verify `disable()` is clean: callbacks unregistered, song state cleared, banks deliberately left
   registered (design §6).

**Tests covering this step.**

- OFF on both sides: complete silence, and confirm via logging that no tick list is even built
- toggle ON mid-session: takes effect from the next song, not the current one
- card out and back in: the value returns (network persistence)
- offline: the value survives a relaunch (JSON cache)
- the row appears on the MODS tab in the expected position, with a label, after one relaunch
  (the label atlas flushes once at boot — first install shows an unlabelled row, which is expected
  and must be documented rather than "fixed")
- runtime disable from the overlay stops ticks immediately and does not crash
- with the option on, a completed play uploads its score normally (FR-10 — the one explicit
  check that no score-suppression path was wired in by accident)

**Integration.** Replaces Step 4's "tick the selected side unconditionally" with the real gate.
This is the last functional step; the feature is complete at its end.

**Demo.** Boot, card in, open the options overlay, set `ASSIST TICK` to `ON`, play a song and hear
ticks. Card out, card in, and the row still reads `ON`.

---

## Step 6: Diagnostic pass, docs, and final gates

**Objective.** Close the four questions static analysis could not answer, remove the scaffolding,
and land documentation.

**Implementation guidance.**

1. Run the diagnostic build described in design §7.2 and record the answers in the feature's
   progress notes: whether the sibling actor list holds both actors at the first judge dispatch;
   whether anything produces per-panel state above 1; doubles started from the P2 card reader; the
   real coalescing window needed on TPS-150 charts. Adjust the coalescing constant if the data
   says so.
2. Demote or delete per-tick logging; keep the once-per-song and warn-once lines. Confirm the
   per-frame path is O(1) and allocation-free.
3. Audit against the design's non-functional requirements: no panicking constructs in any callback
   body; no hardcoded game addresses; every failure path yields exactly one warning; no new crate
   dependency; no config section added.
4. Documentation: a README section (what it does, how to enable it, that the sound is swappable by
   replacing the two bank files, the one-relaunch label caveat, and StepMania attribution for the
   sample per D15); an `AGENTS.md` key-entry-points row pointing at the mod, the service, and the
   research notes; and a new research note under `docs/` consolidating the XACT findings, since
   they are reusable well beyond this feature.
5. Final gates in order: type check, formatter across the whole crate, release build.

**Tests covering this step.** Re-run the full §7.4 matrix once on the final build — the audit in
(2) and (3) changes code, so the earlier per-step passes do not carry over. Confirm the crash log
is empty after a multi-song session.

**Integration.** Completes the feature. Nothing outside the two new modules, the signature
registry, the mod registry, the label script, and documentation should have changed — plus the
committed asset and its build script.

**Demo.** A clean release build, a full session played end to end with ticks on both cabinet sides
and in doubles, an empty crash log, and a README section a stranger could follow to enable the
feature.

---

## Validation matrix summary

| Step | Gate | How verified |
|---|---|---|
| 1 | Bank pair is valid and reproducible | Offline: parser round-trip, comparison against stock banks, validator rules, ADPCM decode-back |
| 2 | Our bank registers and is audible; survives song loads | `log.txt` addresses/slot/HRESULTs + one audible clap; multi-song run |
| 3 | Ticks land on the beat and are chart-driven | Scheduled-versus-actual logging + listening pass, including deliberate misses |
| 4 | Right notes, right side, once each | Behavior matrix on charts with jumps, freezes, shocks, mines; solo P2, doubles, both 2P cases |
| 5 | Player-facing and persistent | Options overlay, card out/in, relaunch, runtime disable, score uploads |
| 6 | Production quality | NFR audit, full matrix re-run, empty crash log, build gates |

## Verification split

**The maintainer runs all gameplay verification.** Every listening pass and every behavior-matrix
row that depends on hearing where the clap lands is the maintainer's, run against the local
CrossOver install. The agent's share of verification is:

- offline asset validation (Step 1)
- build gates: type check, formatter, release build
- reading `log.txt` out of the local install and reporting what the diagnostic lines say
- the negative-path checks that are observable purely in the log (missing bank files, corrupted
  sound bank, no free slot)

Consequently, each step ends with the agent reporting "built, installed, here is what the log
says" and the maintainer confirming the audible behavior before the step is considered done. Step
4's chart selection (shocks, mines, freezes, jumps, the CUT / JUMP-OFF and `FREEZE ARROW: OFF`
modifier cases) is at the maintainer's discretion — the charts are already identified, so the plan
does not need to name them.

## Notes on sequencing

- **Step 1 gates everything** — no asset, nothing to play. Its cross-repo prerequisite is the only
  work outside this repository in the whole feature.
- **Steps 2 and 3 front-load the design's two biggest risks.** If the bank cannot be registered in
  a free slot, or if frame quantization sounds unacceptable, both surface before any player-facing
  code exists — and the design's escalation ladders (fall back to our own cue reaper; verify at
  higher frame rates) are still cheap to take.
- **Step 4 narrows rather than adds**, which is deliberate: an over-permissive Step 3 makes a
  timing bug and a predicate bug distinguishable by ear.
- No step exists solely to add tests for an earlier step; each step's verification ships with it.
