# Idea Honing: Song Playback Speed

Decision Register Accepted: 2026-08-05. Runtime research resolved D10; the user
accepted the D21 late-XACT-failure amendment on 2026-08-05.

Readiness Confirmed 2026-08-05

## Decision Register

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | Audio model | Determines whether the work patches XSB pitch or generates transformed XWB audio | Ship pitch-preserving XWB generation | Accepted |
| D2 | Delivery sequence | The load-bearing audio and clock mechanisms are not live-proven | Validate one pre-generated 75% bank with a developer-only build before implementing release UI/runtime generation | Accepted |
| D3 | User-facing control | Changes persistence, active-side resolution, and shared-stream semantics | Use a persisted per-player `SONG SPEED` option after the diagnostic; use no cabinet-global release setting | Accepted |
| D4 | Initial rates | Slowdown and speedup stress different time-stretch behavior | Expose 75%, 100%, and 125% in the first release | Accepted |
| D5 | Supported modes | Synchronized multi-cabinet and multi-song flows have additional clocks/lifecycles | Support normal e-amusement-connected solo and doubles; force 100% only in specifically excluded modes | Accepted |
| D6 | Shared-stream conflicts | P1 and P2 cannot hear independent rates | Never use “P1 wins”; local versus is 100% in v1 | Accepted |
| D7 | Latch timing | Audio and chart clocks must use one immutable song snapshot | Resolve at scene 26; mid-song edits apply to the next newly selected song | Accepted |
| D8 | Quick Restart | Restart bypasses song selection and may not reload audio | Preserve applied rate, generation, and score taint across restart | Accepted |
| D9 | Rate authority | ADPCM block alignment slightly quantizes generated duration | Derive every consumer from source/output frame counts and the resulting exact effective rate | Accepted |
| D10 | Activation transaction | Partial audio/clock state would desynchronize play and endanger score integrity | Commit only after `wavebank_create` reports XACT success for a call-nonced token carrying the exact generated path/cache identity | Assumed |
| D11 | XWB source compatibility | Static LayeredFS replacements and custom songs may be the effective source | Transform every entry in a strict supported streaming XWB profile while preserving XSB-visible identity | Accepted |
| D12 | Cache and concurrency | AVS open/lstat/path conversion can race on worker threads | Use immutable, content-addressed, atomically published cache entries | Assumed |
| D13 | Score policy | Any non-100% play is assisted, including faster rates | Suppress both participating sides' stage scores and sanitize logout; uncertain partial application remains tainted | Accepted |
| D14 | Background movies | DirectShow runs on an independent clock and will drift | Suppress movies at non-100% through a shared movie-policy service | Accepted |
| D15 | Assist Tick | Its separate normal-rate voice otherwise accumulates drift | Make it rate-aware before release; auto-silence it at non-100% until validated | Accepted |
| D16 | Judgment windows | Content-time and wall-time strictness produce different training behavior | Keep the game's native content-time windows in v1 | Accepted |
| D17 | Timing statistics | Existing `ms` values become ambiguous at non-100% | Retain content-domain values, label them `chart ms`, and include rate in CSV output | Accepted |
| D18 | Other profile effects | Score sanitation does not necessarily undo calories or play counters | Preserve calories and ordinary profile/options; suppress competitive result/ranking data only | Accepted |
| D19 | Real Speed behavior | The option is a target velocity that derives the hidden multiplier, not a passive measurement | Keep the selected target unchanged and derive its multiplier from `Core BPM * effective_rate` | Accepted |
| D20 | Disable and identity behavior | Unpatching or resetting during playback cannot restore the already-pitched voice | Install the clock patch permanently once needed; 100% is identity; disable affects future songs only | Assumed |
| D21 | Failure behavior | Audio and chart speed must never disagree while a score remains trusted | Early failures fall back to 100%; late XACT rejection aborts stage loading rather than attempting an unsafe same-stage retry | Accepted |
| D22 | Release acceptance | Compile success cannot validate audio drift or save safety | Gate release on long-song, restart, suppression, movie, and native/CrossOver cabinet tests | Accepted |
| D23 | DSP and code reuse | Determines quality, dependency shape, and stereo behavior | Port the focused `ddr-chart-tools` XWB/ADPCM code and implement a pure-Rust stereo-coherent WSOLA-like stretcher | Accepted |
| D24 | First-play generation | Pitch-preserving banks take substantial CPU and memory to create | Generate after song/rate selection, extend loading until ready, then persistently cache | Accepted |
| D25 | Cache retention | Each rate variant is roughly another full song bank | Default to a configurable 10 GiB limit with safe least-recently-used eviction | Accepted |

## Decisions

### D1: Audio model

**Question:** Should rate changes preserve pitch?

**Accepted answer:** Preserve pitch. Decode the selected song's streaming XWB,
time-stretch its PCM, re-encode it as MS-ADPCM, rebuild the XWB, and play it
through XACT at its native sample rate. Do not ship XSB pitch-changing as the
production mechanism.

**Rationale:** The sibling `ddr-chart-tools` repository already supplies the XWB
v43 parser/writer and stereo MS-ADPCM codec. The remaining DSP work is material
but bounded: a deterministic stereo-coherent time stretcher. This directly
meets the requested listening behavior.

### D2: Delivery sequence

**Question:** Should the first implementation immediately expose a player option?

**Accepted answer:** Before building the menu and on-demand cache generator,
create one 75% XWB outside the game and use a developer-only build that always
selects it for one test flow. This is an implementation checkpoint, not a mode
or setting users will see. It proves that the game accepts the transformed bank
and that the scaled chart remains synchronized from first note through song end.

**Rationale:** It separates the two riskiest facts from all surrounding product
work. If the bank or central clock model fails, no time was spent implementing
runtime generation, cache management, and UI around it.

### D3: User-facing control

**Question:** Is the released control cabinet-global or per-player?

**Accepted answer:** A persisted per-player custom option named `SONG SPEED`,
using `PersistMode::Full`. The hidden diagnostic can hard-code 75% and does not
need a release setting.

**Rationale:** Song speed is a player training preference and should follow the
card/offline custom-option cache. A cabinet-global setting avoids side
classification but does not satisfy the intended player-facing experience.
Rejected close alternative: ship a cabinet-global overlay enum as v1.

### D4: Initial rates

**Question:** Which values are exposed initially?

**Accepted answer:** `75%`, `100%`, and `125%`.

**Rationale:** There is no architectural reason to expose slowdown only. These
values test both repeat-heavy slowdown and skip-heavy speedup while keeping the
initial audio-quality range conservative. More granular/extreme presets can be
added after listening and performance tests.

### D5: Supported modes

**Question:** Which gameplay modes may apply a non-100% rate in v1?

**Accepted answer:** Support ordinary solo and doubles sessions while the
cabinet is connected to e-amusement, including normal card/profile loading and
saves. “Unsupported network play” means synchronized cabinet-to-cabinet modes
such as matching/BPL, not ordinary server connectivity. Force 100% initially
for local versus, courses, matching/BPL, demos/attract, event flows, and any
mode that cannot be positively classified.

**Rationale:** Ordinary e-amusement connectivity is the normal operating mode
and is required. The exclusions are about shared audio, remote gameplay clocks,
or multi-song bank lifecycle, not about whether the cabinet has a server
connection.

### D6: Shared-stream conflicts

**Question:** How should conflicting P1/P2 preferences be resolved?

**Accepted answer:** Do not resolve them in v1 because local versus is forced to
100%. Never silently choose P1's value.

**Rationale:** One physical song stream cannot satisfy two rates. “P1 wins” can
change P2's training conditions without consent. Equal-rate local versus can be
considered after the solo/doubles path is proven.

### D7: Latch timing

**Question:** When does an editable option become the applied song rate?

**Accepted answer:** Resolve and latch it at entry to scene 26, after song/options
selection and before stage construction. Changes after that point affect the
next genuinely selected song.

**Rationale:** Audio, score policy, movies, clock scaling, and dependent features
need one immutable generation. Scene 26 is the best existing hook point, subject
to ordering proof.

### D8: Quick Restart

**Question:** Does restart re-resolve the preference?

**Accepted answer:** No. Preserve the same rate, effective factor, generation,
and rate score taint.

**Rationale:** Restart bypasses scene 26 and may reuse the loaded bank. Changing
only the clock or option snapshot would desynchronize it from the existing
audio.

### D9: Rate authority

**Question:** Is the requested percentage or the XACT pitch field authoritative?

**Accepted answer:** Choose an ADPCM-block-aligned output frame count for each
entry and compute `effective_rate = source_frames / output_frames`. Use the
main song's exact effective rate everywhere, including the deterministic
fixed-point gameplay-clock multiplier.

**Rationale:** The generated audio duration is quantized to whole ADPCM blocks.
Using the raw UI percentage would introduce small but accumulating audio/chart
drift.

### D10: Activation transaction

**Question:** At what point is a non-100% rate considered applied?

**Assumed answer:** Scene 26 arms the generated XWB and keeps the clock at
identity. The `wavebank_create(file_id)` detour creates a thread-local call nonce;
LayeredFS records that nonce plus the exact generation, normalized virtual path,
cache-key digest, and generated-path digest when it redirects the streaming XWB.
The detour calls the original exactly once and commits only that matching token
when the original returns success after `CreateStreamingWaveBank`.

**Rationale:** This is the first synchronous point proving that XACT accepted
and installed the exact generated wave bank. It occurs before cue preparation
and gameplay, while avoiding false activation from lstat or path conversion.

### D11: XWB source compatibility

**Question:** Should pitch preservation compose with static LayeredFS/custom-song
XWB replacements?

**Accepted answer:** Yes. Read the effective source and accept a strict XWB v43
streaming MS-ADPCM profile. Transform every entry, preserving bank name, entry
count/order/names, sample rate, and XSB-visible wave indices. At 100%, perform no
dynamic rate replacement.

**Rationale:** Transforming all validated entries handles both observed
main/preview orderings and keeps the original XSB valid. Unsupported bank
profiles fall back to 100% rather than being guessed at.

### D12: Cache and concurrency

**Question:** How are generated XSBs published under concurrent AVS probes?

**Assumed answer:** Key immutable entries by source-content digest, requested
percentage, exact output-frame counts, and algorithm/codec/cache-format versions.
Serialize generation per key, write and validate a temporary XWB, atomically
publish it, then atomically publish an immutable manifest as the commit marker.
Keep mutable LRU timestamps in a separate index.

**Rationale:** Open, lstat, and path conversion can arrive on different worker
threads. Existing path/mtime hashing and direct overwrite are not strong enough
for an audio/score integrity boundary.

### D13: Score policy

**Question:** Which saves are trusted after non-100% play?

**Accepted answer:** Suppress stage-result saves for both participating sides.
Reuse the existing session-sticky logout sanitizer so profile/options persist
without competitive result/league data. Require score-guard readiness before
serving a modified XSB. If partial consumption cannot be disproved, keep the
stage tainted.

**Rationale:** Slower and faster rates both alter competitive conditions. A
partially modified but trusted score is the unacceptable failure mode.

### D14: Background movies

**Question:** What happens to DirectShow background movies at non-100%?

**Accepted answer:** Suppress them and use the static background. Promote the
existing `movie_build_graph` detour into a shared service whose suppression
policy is `non-native workaround OR non-100% song rate`.

**Rationale:** The movie has an independent clock and will drift. A second detour
would violate hook ownership, while the existing fake-open behavior is already
proven playable.

### D15: Assist Tick

**Question:** Must Assist Tick work at non-100% in the first release?

**Accepted answer:** Yes, but only after its content-to-wall-time conversion is
live-validated. Until that validation passes, automatically silence Assist Tick
for non-100% songs rather than emitting drifting claps.

**Rationale:** Drift would defeat Assist Tick's timing-reference purpose. The
safe fallback is silence, not approximate timing.

### D16: Judgment windows

**Question:** Should timing windows retain stock wall-time strictness?

**Accepted answer:** No. Keep native content-time windows in v1. At 75%, a content
window lasts longer in wall time.

**Rationale:** This is coherent with a training-rate model and requires no second
timing-policy patch. Changing both playback and window semantics would make the
first live results harder to diagnose.

### D17: Timing statistics

**Question:** What do PUS `ms` values mean at non-100%?

**Accepted answer:** Keep the game's content-domain values, relabel them as
`chart ms`, and add the effective playback rate to CSV output/metadata.

**Rationale:** Partial conversion would make the widget, threshold, and CSV
disagree. Explicit domain labeling prevents users from reading content time as
wall time.

### D18: Other profile effects

**Question:** Should non-score profile effects from a slowed song persist?

**Accepted answer:** Preserve calories and ordinary profile/custom-option
changes. Suppress score, grade, league, and other competitive result/ranking
data. Audit additional aggregates during research and add them to sanitization
if they are competitive.

**Rationale:** The existing policy intentionally forwards profile writes while
stripping tainted results. Calories reflect actual elapsed activity; silently
discarding all profile changes would regress the current sanitizer's purpose.

### D19: Real Speed display

**Question:** Should Real Speed continue to represent the player's requested
physical arrow velocity when song playback changes?

**Accepted answer:** Keep the player-selected Real Speed target unchanged. When
the game derives its hidden normalized multiplier, use
`effective_core_bpm = Core BPM * effective_rate` as the divisor. For example, a
400 Real Speed target on a 200 BPM song normally derives 2.00x; at 75% playback,
effective BPM is 150 and the derived multiplier becomes approximately 2.67x, so
the arrows still move at the requested physical 400 rather than slowing to 300.

**Rationale:** `Option::SetScrollSpeed` stores the player's target at `Option
+0x14` and computes `hispeed_normalized = target * 100 / bpm_reference` at
`Option+0x10`. The existing Real Speed fix swaps Max BPM for Core BPM in that
divisor. Rate-aware behavior therefore modifies the divisor, not the visible
target. The current cave is full, so this still requires a redesigned near stub.

### D20: Disable and identity behavior

**Question:** How should the mod disable or return to 100% after the clock patch
has been installed?

**Assumed answer:** Never unpatch live code. Store an exact identity multiplier
at 100%. Disabling the mod blocks future non-100% generations but leaves the
active song's audio, clock, taint, movie, and Assist Tick policy intact until its
lifecycle completes.

**Rationale:** The already-pitched voice cannot be restored by changing the
clock. Permanent identity-controlled patches are the existing safe pattern.

### D21: Failure behavior

**Question:** What happens when readiness, mode detection, parsing, cache writes,
or generated opens fail?

**Accepted answer:** If generation, mode detection, cache access, or hook
readiness fails before the generated path is exposed, play the normal 100% song.
If XACT rejects a generated bank after native path conversion, abort that stage's
loading with the clock still at identity and no trusted score; do not retry the
stock path in the same attempt until native-handle cleanup is proven safe. If a
failure occurs after modified audio has begun, suppress that stage's score.

**Rationale:** The stock wave-bank loader inserts native-handle bookkeeping
before `CreateStreamingWaveBank` and does not demonstrably unwind it on failure.
Calling it again immediately with stock audio could duplicate stale manager
state. Failing the load is safer than risking corrupted audio-manager state.

### D22: Release acceptance

**Question:** What evidence is required before calling the feature releasable?

**Accepted answer:** Require the repository build gates plus cabinet tests for a
long song's first/last notes and natural end, Quick Restart, 75→100→75 changes,
score/backend capture, Assist Tick, movies, native Windows, CrossOver, and
failure injection. The hidden diagnostic must pass before the release option is
implemented.

**Rationale:** This feature crosses an undocumented audio runtime, an inline game
clock patch, and score policy. Compilation cannot validate its core invariants.

### D23: DSP and code reuse

**Question:** How should the XWB/codec and time-stretch implementation enter this
repository?

**Accepted answer:** Port and adapt the focused XWB v43 and stereo MS-ADPCM
modules from the sibling `ddr-chart-tools` repository. Implement a small pure-
Rust WSOLA-like stretcher that scores both stereo channels jointly, selects one
correlation offset, and applies it to both channels.

**Rationale:** Depending on the entire sibling CLI crate would pull unrelated
Vorbis/CLI dependencies. A joint-channel score handles anti-phase material while
the shared offset avoids the image instability of the local StepMania-derived
implementation's per-channel matching.

### D24: First-play generation

**Question:** What does the player experience when a song/rate variant is not in
the cache?

**Accepted answer:** Start generation once song and rate selection are final,
keep the normal stage-loading flow active until the generated bank is ready,
then continue. Later plays reuse the persistent cache.

**Rationale:** Whole-song decode/stretch/re-encode can take seconds and over
100 MB peak memory with a naive pipeline. It cannot safely be deferred to the
last native file-open call, and silently falling back to 100% would make the
selected option unreliable.

### D25: Cache retention

**Question:** How should transformed XWB disk usage be bounded?

**Accepted answer:** Default to a 10 GiB operator-configurable limit and evict
least-recently-used entries only when they are not active. Perform eviction
outside the AVS hook path.

**Rationale:** Every cached rate is approximately another full song bank; an
unbounded cache can grow by many gigabytes.
