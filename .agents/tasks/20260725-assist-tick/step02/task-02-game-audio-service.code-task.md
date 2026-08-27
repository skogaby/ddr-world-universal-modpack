# Task: `services::game_audio` — Bank Registration and Cue Playback

## Description

Add a new service that owns every interaction with the game's XACT audio engine: it registers a
mod-supplied wave-bank + sound-bank pair into a free slot on the game's audio manager, and plays a
cue from it by name.

This is the step that proves the single riskiest assumption in the whole feature's design — that our
own sound bank can live in a free manager slot and be audible. To make that provable on its own, the
task also adds a small, clearly-marked block of **temporary scaffolding**: a one-shot that loads the
banks committed in Step 1 and plays the cue the first time the judge hook dispatches. That block is
the only way to hear anything at this point, and the next step deletes it wholesale and replaces it
with the real trigger.

## Background

**Working directory: this repository** (the DDR World hook DLL / modpack).

The game's audio is Microsoft XACT 2, wrapped by an in-house audio manager that owns **six**
sound-bank slots and a small "play a sound effect" façade. Everything audible in the game — menu
music, voice, every sound effect, and the song audio itself — passes through one engine instance and
one final mix, which is exactly why the assist tick plays through it rather than through an audio
path of its own.

Three findings from the reverse-engineering shape this task, and all three are load-bearing:

- **A slot can be claimed with a single pointer write, and it then survives forever.** The only code
  in the game that ever destroys a sound-bank slot is a linear "find the slot whose file id equals
  this one" search that destroys nothing when nothing matches. A slot whose file id stays at `-1` can
  never be matched — so writing *only* the bank pointer, and never the file id, is what makes our
  bank outlive every song load, song unload and scene transition.
- **XACT does not copy an in-memory wave bank's data.** The byte buffer handed to it must stay valid
  for as long as the bank exists, which here is the process lifetime.
- **The play façade's third argument is a float.** In the Microsoft x64 convention it travels in
  **XMM2**, not in a general-purpose register, so a naive binding that declares it as an integer
  passes garbage.

The game already plays a per-note sound effect from inside its own judge loop (the shock-arrow hit
sound), so this is an established pattern rather than a novel one.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` — §4.1
  `services::game_audio` is the specification for this task: the public API, the vtable indices to
  use (and the explicit rule that no index the game does not itself call may be used), and
  `register_bank`'s **ordered** procedure. Also §3.2 for why the game's own engine, §3.3 for the
  thread rule, and §6's error table for the required behaviour of every failure path
- `.agents/planning/20260725-assist-tick/implementation/plan.md` — Step 2, whose demo requirement
  this task satisfies

**Additional References (if relevant to this task):**
- `.agents/planning/20260725-assist-tick/research/bank-slot-and-anchors.md` — the proof that the free
  slot is safe, and the two **required runtime guards** ("Required guards" subsection): compute the
  slot rather than hard-coding it, and null-check the manager global before every play. Its "Notes
  that shape (but do not block) the design" subsection records that handle-table exhaustion *leaks*
  a cue and returns a sentinel — relevant to requirement 8
- `.agents/planning/20260725-assist-tick/research/xact-bank-format.md` — §6.3 for the required
  creation order and why, §7 for the vtable layouts (note which entries are observed versus merely
  positional), and Appendix B for the HRESULT vocabulary worth naming in log messages
- `.agents/planning/20260725-assist-tick/research/game-sound-engine.md` — §"Play/Stop API" for the
  façade's argument shape, and §"Audio manager object layout" for the slot array
- `.agents/steering/rust-hooking.md` — allocator rules and the service-singleton pattern
- `src/services/judge_hook.rs` — the shape this service's `init`/`is_available` should follow, and the
  dispatcher the temporary trigger subscribes to
- `src/services/avs_layeredfs/` — the existing resolver for `data_mods/` paths, used to find the two
  bank files Step 1 committed at `data_mods/assist_tick/banks/`

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Add `src/services/game_audio.rs`, declared in `src/services/mod.rs`, exposing exactly the API in
   design §4.1. Follow the established service shape in this codebase — a lazily-initialised
   singleton behind a mutex, an `init` called from a fixed point in the DLL's start-up sequence, and
   an `is_available()` consumers check before use.
2. `init` **resolves addresses only**. It must call no game function, because at that point in
   start-up the backing globals may not exist — a documented crash class in this codebase. It must
   additionally refuse to become available if the expected XACT engine module is not loaded, since a
   different engine version would mean the vtable indices are unverified.
3. Implement `register_bank` in **exactly** the order design §4.1 gives. The ordering is not
   stylistic: the wave bank must exist before the sound bank is used, and the free slot must be
   *computed* rather than assumed.
   - Null-check the manager global first. The game's own code dereferences it unconditionally, so
     this check is ours to make.
   - Sanity-check the slot array's layout before trusting it, using the assertion the design names.
   - Compute the free slot. **Do not hard-code an index.** Decline if none is free.
   - Create the wave bank, then the sound bank.
   - Write **only** the chosen slot's bank pointer. **Never write its file id** — leaving it at `-1`
     is the entire reason the bank survives. Say so in a comment at the write site; this is the one
     line in the feature where a well-meaning "fix" would break everything silently.
   - Leak both byte buffers, with the reason in a comment.
4. `register_bank` is **game-thread only** and must be idempotent per bank name: a repeat call
   returns the existing handle without creating anything.
5. Implement `play_cue` over the game's **public** play entry point (not the inner one — design §4.1
   is explicit that this preserves the lock semantics the game itself uses; §6 records switching to
   the inner entry as the documented mitigation should the sound-effect mute filter turn out to veto
   our bank).
6. Declare that entry point with the correct ABI. Its third argument is a float and must travel in
   XMM2 — see design §4.3's closing paragraph for the exact Rust declaration.
7. Use **only** vtable indices the game itself exercises, per design §4.1. One of the interfaces
   involved provably deviates from the public XACT 3 headers, so a "reasonable" guess at an unused
   slot is not safe.
8. `play_cue` must treat the façade's failure sentinel as a failure: return `false` and warn
   **once** (never per call). The game leaks a cue rather than crashing when its shared handle table
   is exhausted, so this is the only signal that it happened. No rate limiting in this task — one
   tick per frame arrives in a later step.
9. Add a **temporary** trigger so this step is demonstrable on its own: a one-shot that, on the first
   dispatch from the existing judge dispatcher, reads the two bank files committed at
   `data_mods/assist_tick/banks/` through the existing mod-path resolver, registers them, and plays
   the cue. Keep it in **one clearly-marked block**, commented as scaffolding that the next step
   deletes. It must subscribe to the existing shared dispatcher — **no new detour anywhere.**
10. The cue name to play is `asti`. That is the name inside the committed sound bank; the files are
    named `tick.xwb` / `tick.xsb`, which is deliberate and unrelated — the engine pairs banks and
    finds cues by their *internal* names, never by filename.
11. Every failure path in design §6's table must behave as that table says: exactly one warning, no
    crash, the mod's behaviour degraded and nothing else affected. No panicking constructs
    (`unwrap`, `expect`, indexing, slicing that can panic) anywhere that a hook callback can reach.
12. Diagnostic logging is this step's only means of verification, so it is a requirement rather than
    an afterthought. Log: the resolved addresses and the derived manager global; the slot-layout
    check's result; **which slot index was chosen**; the HRESULT of each creation call; and the cue
    index resolved. One warn-once on the first playback failure.
13. No new crate dependencies. No config section. No changes to game data files.

## Dependencies

- **Task 01 (audio signature patterns and derivations)** must be complete — this task consumes the
  addresses it resolves
- **Step 1's committed asset** — `data_mods/assist_tick/banks/tick.xwb` and `tick.xsb`. Already
  present and validated
- `src/services/judge_hook.rs` — subscribed to by the temporary trigger, as a per-frame clock only
- `src/services/avs_layeredfs/` — the existing `data_mods/` path resolver
- `src/lib.rs` — must call the new service's `init` at a point after signature resolution
- No new crate dependencies

## Implementation Approach

1. Read design §4.1 end to end first, then the research note's "Required guards" subsection. Between
   them they fully determine `register_bank`; there is no design latitude left in it.
2. Write the service's skeleton — the singleton, `init`, `is_available` — and confirm on the local
   install that `init` resolves everything and reports available, before writing a line that calls
   into the engine.
3. Implement `register_bank`, in the design's order, logging at each stage. The slot-layout
   sanity check and the computed slot index are the two things to read out of the log before
   believing anything else works.
4. Implement `play_cue`, minding the ABI.
5. Add the temporary trigger block, install, and listen.
6. Exercise the negative paths deliberately (see the acceptance criteria) — each should produce one
   warning and no crash.

## Acceptance Criteria

1. **Addresses resolve and the service reports available**
   - Given the DLL is loaded into the game
   - When start-up runs
   - Then the log shows the resolved play entry point and the derived audio-manager global, and the
     service reports itself available

2. **`init` calls no game function**
   - Given the service's `init`
   - When it is read, and when the game is launched
   - Then it performs address resolution and a module-presence check only, and start-up completes
     with no crash attributable to it

3. **A free slot is computed, not assumed, and the bank registers**
   - Given a first gameplay frame and the committed bank files
   - When `register_bank` runs
   - Then the log reports the slot-layout check passing, names the slot index it **computed**, and
     reports a success HRESULT for both the wave-bank and the sound-bank creation

4. **The slot's file id is never written**
   - Given the registration succeeded
   - When the source is inspected
   - Then only the slot's bank pointer is written, the file id is left untouched, and a comment at
     that site explains that leaving it at `-1` is what stops the game's slot destroyer from ever
     matching our slot

5. **A cue is audible, and the bank survives song loads**
   - Given the temporary trigger is in place
   - When a song is started
   - Then a single clap is audible on the first judge frame
   - And when several songs are played in one session, a clap is audible at the start of **each** of
     them without re-registering the bank — which is the design's central claim about leaving the
     file id alone

6. **Missing bank files degrade gracefully**
   - Given the two bank files are renamed away
   - When a song is started
   - Then exactly one warning naming the expected path is logged, no clap plays, and the game behaves
     normally in every other respect

7. **A corrupted sound bank degrades gracefully**
   - Given one byte inside the sound bank's CRC-covered region is flipped
   - When registration is attempted
   - Then exactly one warning is logged carrying the returned HRESULT, no crash occurs, and the game
     behaves normally otherwise

8. **No free slot degrades gracefully**
   - Given the free-slot search is forced to find nothing
   - When registration is attempted
   - Then registration is declined with exactly one warning, and nothing is written to the manager

9. **A playback failure warns once, not per call**
   - Given `play_cue` is called with a cue name that does not exist in the bank
   - When it is called repeatedly
   - Then it returns `false` every time but logs exactly **one** warning for the whole session

10. **The temporary scaffolding is unmistakable**
    - Given the completed change
    - When the source is inspected
    - Then the trigger occupies one contiguous, clearly-marked block, commented as scaffolding for
      this step that the next step removes, and it installs no detour of its own

11. **The build gates pass**
    - Given the completed change
    - When `cargo check --target x86_64-pc-windows-msvc`, then `cargo fmt`, then `./build.sh` are run
    - Then all three complete cleanly

## Metadata
- **Complexity**: High
- **Labels**: service, xact, audio, ffi, abi, unsafe, vtable, diagnostics
- **Required Skills**: Rust with `unsafe` discipline; COM vtable dispatch from Rust; the Microsoft
  x64 calling convention, in particular float argument registers; in-process hook-DLL constraints
  (thread affinity, no panics across FFI, allocator matching); reading reverse-engineering notes and
  respecting the difference between observed and inferred findings
- **Generated By**: code-task-generator 2026-07-26
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 2 — `services::game_audio` — signatures, bank registration, cue playback
