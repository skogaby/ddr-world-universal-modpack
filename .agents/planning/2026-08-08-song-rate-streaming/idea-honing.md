# Idea Honing — Song Rate Streaming Redesign

Decision register for the streaming-only redesign. Settled maintainer decisions from the
pivot brief (scalar domain, streaming-only/no cache/no fallback, no server-side
validation, no latency knob, Step-4 score-containment semantics) are constraints, not
register entries.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Interception mechanism | Defines the entire audio authority | Detour BOTH XACT file-IO callbacks as a pair; bind by file_id pre-original in the existing create detour | Accepted |
| D2 | Generator source bytes | Use-after-free class vs memory | Copy the stock bank out of the FileManager RAM buffer once at bind | Accepted |
| D3 | Output buffering shape | Memory bound vs complexity | Bounded ring (~16 MiB) + loop-start checkpoint + deterministic regeneration | Accepted |
| D4 | Underrun & mid-song failure policy | New audible failure class | Transient: native ERROR_IO_PENDING deferral. Hard failure: silence-fill, keep clock+taint, one WARN | Accepted |
| D5 | Commit authority & transaction shape | Score/clock safety invariant | Bind pre-original; commit post-original on create success (Q31 LAST unchanged); LateFailed unbinds | Accepted |
| D6 | Dependent-feature scope (Assist Tick / Real Speed / PUS) | Slow-rate + assist tick IS the headline practice use case | Full Assist Tick content→wall integration REQUIRED for delivery (not cuttable); interim force-disable gate only until that plan step lands; Real Speed × rate + PUS CSV in the same late step | Overridden |
| D7 | Host-validator report evolution | Host scripts validate the streaming logic | NO versioning (feature never shipped): update `scripts/validate_song_playback_speed.sh` report in place — remove `cache`/`on_demand` sections, add `streaming`, keep Step-1 synthetic sections | Overridden |
| D8 | Fault injection surface | Release-hardening leverage | Keep `DDR_SONG_RATE_FAULT` (dev mode only), retarget legs to bind/header/generator/mid-song classes | Accepted |
| D9 | Config surface | Operator knobs | Simply drop the `cache_limit_gib` field (maintainer removes the key from his config himself; unknown keys are ignored); NO new knobs; NO stale-cache cleanup code | Overridden |
| D10 | Lifecycle phase renames (`RedirectReady` → binding-era names) | Cosmetic; code clarity | Rename in design; semantics unchanged | Accepted |
| D11 | New AOB + readiness gating | Cross-version survival | One pattern on the manager-ctor callback-setup region (0xFA imm + 3×LEA), RIP-decode both targets; feature self-disables via `integration_ready()` when unresolved; cross-verify on 0324/0421/0616 | Accepted |
| D12 | Module layout | Maintainability | `stretch.rs` gains a pure streaming state machine (whole-buffer fn becomes reference); `services/song_rate/generator.rs` replaces `worker.rs`; `io_callback_hook.rs` owns the two new detours | Accepted |
| D13 | Producer threading | Repo law: detours never allocate/log | One generation-tokened producer thread; detours touch only atomics + fixed buffers; logging via the existing 250 ms drain | Accepted |
| D14 | Quick Restart semantics | Player-visible | Bank recreate ⇒ fresh header read + reads from 0 ⇒ regenerate from 0 (absorbed by the loading screen at ≥11× realtime); `mark_reexposed` path retained | Accepted |
| D15 | Assist Tick bank capacity at slow rates | Opened by D6: 25 % × a 129 s chart = ~516 s of wall-time claps; the current immortal bank holds 300 s | Raise `TICK_CAPACITY_MS` to 1200 s wall (~28.8 MB, lazily allocated only when Assist Tick is used) so 300 s of chart content stays covered at every supported rate; graceful truncation beyond capacity | Accepted |

Maintainer batch response applied 2026-08-08 (D1–D5, D8, D10–D14 accepted as
recommended; D6/D7/D9 overridden as recorded above). D15 accepted 2026-08-08 after the
tick-ring alternative analysis (maintainer: extend to 1200 s now; revisit only if
cabinets ever hit memory constraints — considered unlikely).

Readiness Confirmed 2026-08-08 — register complete, no decision remains Proposed or
Open; research base: `docs/xact_streaming_research.md` + `research/` in this directory.

## D1 — Interception mechanism

**Question.** How does the mod become the byte authority for the rate-bound bank?

**Recommendation.** Detour gamemdx's registered XACT readFile (`FUN_1801aa250`) AND
getOverlappedResult (`FUN_1801aa350`) callbacks as a mandatory pair (stock
getOverlappedResult reports instant completion for any vector-listed handle, which would
corrupt deferral). Everything unbound goes through the trampolines — byte-exact stock,
including the song-select preview. Binding identity is `{file_id, generation}`, set in
the existing `wavebank_create` detour BEFORE the original runs — the engine's single
0x1000 header read is issued synchronously inside the original, so pre-original binding
is what makes the first read race-free. Read detour resolves handle→file_id with the
same locked sorted-vector walk the stock callback already performs per read.
**Rejected:** owning the handle-vector insert (racy vs the in-create header read);
patching the engine's stored callback pointers (engine object layout dependency, no
trampoline); redirecting the FileManager RAM copy (that IS the old whole-file model).

## D2 — Generator source bytes

**Question.** Where does the incremental stretcher read the stock (source) bank from?

**Recommendation.** At bind, memcpy the stock bank (typ. 6–15 MB) out of the game's
FileManager RAM buffer into a mod-owned buffer. Cost: one copy + source-size memory for
the song's duration. Buys: total decoupling from the game's file-release lifetime — no
stop-the-producer-before-unregister handshake, no use-after-free class. **Rejected:**
in-place reads of the game buffer (saves memory, but couples producer shutdown to the
unregister detour's timing and reintroduces the crash class the three-heaps rule
exists to prevent); re-reading from disk (pointless — the game already loaded it).

## D3 — Output buffering shape

**Question.** Progressive whole-bank buffer vs bounded ring for the generated ADPCM?

**Recommendation.** Bounded ring, default ~16 MiB (≈ 5 min of stereo ADPCM ahead of the
cursor), producer fills forward of the engine's read cursor; WSOLA per-step state
(~5 words) checkpointed at the stretched loop start; any read behind the window
triggers deterministic regeneration (from checkpoint or zero) behind a deferral. This
is the only shape that makes "arbitrary song length" a structural guarantee rather than
an empirical one. **Rejected:** progressive whole-bank buffer (simpler — no window
management — but memory grows with stretched length: ~50 MB for a 129 s song at 25 %,
>100 MB for marathon charts; re-imports the size-refusal pressure the pivot exists to
kill). Note both entries share one ring — only one wave is ever streamed during
gameplay (the `_s` preview entry is not played from the gameplay bank).

## D4 — Underrun & mid-song failure policy

**Question.** What happens when requested bytes don't exist yet (transient) or will
never exist (generator died)?

**Recommendation.** Transient: return `FALSE + ERROR_IO_PENDING` from the read detour
and complete via our getOverlappedResult detour when produced — the engine's native
polled-async contract (`bWait=0`, single outstanding read, ~250 ms look-ahead, 64 KiB
packets ≈ 1.3 s audio each) absorbs this with zero audible effect at ≥1× production
speed. Hard failure (producer panic/OOM): silence-fill — complete all further reads
with pre-encoded silent ADPCM blocks, keep the committed clock and score taint, WARN
once from the drain thread. The song stays playable and judgeable (the clock is
wall-driven and independent of audio delivery); score containment already covers the
run. **Rejected:** indefinite deferral (chart runs against stalled audio — worst
experience); aborting the song (no clean engine mechanism; a forced create-failure
window has passed by then).

## D5 — Commit authority & transaction shape

**Question.** Where does Q31 commit now that there is no convert/expose seam?

**Recommendation.** The `wavebank_create` transaction survives with "expose" replaced by
"bind": pre-original — verify (armed, slot-5, dance path, song match, source parsed,
header synthesized, generator started) then bind; original — engine reads and parses
OUR header; post-original — on success commit in the shipped order (score taint →
movie → snapshot → Q31 LAST), on failure unbind + quarantine-free LateFailed. All
bind-precondition failures are EarlyFailed → stock 100 % (fail-open), same as before.
The two-stage open-redirect invariant is deleted, not relocated — its hazard (rate
against stock audio) is structurally impossible when the callback binding is the only
audio path and Q31 publishes only after the bound create succeeds.

## D6 — Dependent-feature scope

**Question.** The retired plan's Step 7 (Assist Tick content→wall conversion, Real
Speed × effective rate, PUS CSV rate columns) was never implemented. In or out?

**OVERRIDDEN (maintainer, 2026-08-08).** Full Assist Tick cooperation with playback
speed is a REQUIREMENT for this feature's delivery — the headline use case is slowing
a song down WITH assist tick enabled to study and practice charts. The content→wall
conversion (old design req 43: convert tick positions and restart skips to wall time
with the exact committed rate) therefore lands inside this delivery as its own plan
step, NOT cuttable. The force-disable gate survives only as interim scaffolding during
implementation (earlier steps ship rate audio before tick conversion exists; the gate
prevents wrongly-timed claps in that window and is removed by the conversion step).
Real Speed's `Core BPM × rate` and PUS CSV rate columns ride the same late step.

## D7 — Host-validator report evolution

**OVERRIDDEN (maintainer, 2026-08-08): no versioning.** The feature has never shipped —
this is a mid-implementation internals change, not a v2. Update
`scripts/validate_song_playback_speed.sh` and its report IN PLACE: remove the `cache` +
`on_demand` sections (their machinery no longer exists), keep the Step-1 synthetic
audio sections, add `streaming`: (a) streaming-vs-whole-buffer byte-equality across
rates/loops, (b) a synthetic engine replaying the RE-pinned read pattern (0x1000 header
read, 64 KiB block-aligned packets, EOF clamp, loop restart) against the virtual bank,
(c) deferral and silence-fill legs, (d) a host throughput metric (informational). The
`#[path]` source-mounting harness pattern carries over unchanged. No schema
discriminator anywhere.

## D8 — Fault injection

**Recommendation.** Keep the `DDR_SONG_RATE_FAULT` boot-time env selector (dev mode
only); legs become: `source-read` (RAM copy unparseable), `header-synth`,
`generator-start`, `mid-song-failure` (kills the producer after N packets — exercises
silence-fill live), `bind-refused`. Transaction legs (pre/post-original, token
mismatch) survive as-is.

## D9 — Config surface

**OVERRIDDEN (maintainer, 2026-08-08): no legacy handling at all.** The maintainer is
the only person who has ever run the old build and will remove the `cache_limit_gib`
key from his config himself. The redesign simply drops the field from the config struct
(unknown JSON keys are already ignored) — no parse-but-ignore INFO, no stale-cache
cleanup code. No new operator knobs — ring size and pre-roll are internal constants.

## D15 — Assist Tick bank capacity at slow rates (opened by D6's override)

**Question.** The immortal tick bank declares `TICK_CAPACITY_MS` = 300 s of WALL time
(~7.2 MB). With ticks required to work at rate, a 129 s chart at 25 % needs ~516 s of
wall-time claps — the current capacity covers only 75 s of chart content at 25 %,
gutting the exact practice use case the maintainer named.

**Recommendation.** Raise the
declared capacity to 1200 s wall (= 300 s of chart content at the slowest supported
rate; ~28.8 MB), keeping today's lazy registration so the memory exists only when
Assist Tick is actually used. Content beyond capacity truncates gracefully (claps
simply end; one WARN), same contract as today at 300 s. Rejected: keeping 300–400 s
(breaks normal-length charts at slow rates); per-song re-registration at exact size
(the engine pairs banks by name globally and the bank is immortal by design — a
re-register leaks a bank per song and reopens the cross-pairing crash class).
