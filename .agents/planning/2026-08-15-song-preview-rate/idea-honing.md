# Idea Honing: real-time rate preview at song select

Readiness Confirmed 2026-08-15 (maintainer, after the research phase closed
R-A/R-B/R-C/R-D/R-E; D4 amended: the debounce executor is the game-thread
input poll, not the 250 ms drain — effective latency 150 ms + ≤1 frame).

Decision register. Status: `Proposed` | `Accepted` | `Overridden` | `Assumed` | `Open`.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Core mechanism | Determines everything downstream | Create-time preview binding (virtual bank, `_s` entry stretched, main verbatim) + stop→unregister→recreate→replay re-trigger for live changes | Accepted |
| D2 | When previews play at rate | User-visible consistency | Every preview create binds while the controlling side desires ≠ 100 — not only after an edit | Accepted |
| D3 | Versus policy (one preview, two sides) | User-visible behavior | Mirror the gameplay eligibility policy (`classify_scene26`, `src/services/song_rate/lifecycle.rs`): both sides entered ⇒ excluded (stock preview, no binds/re-triggers — gameplay rate is `IdentityReason::LocalVersus` in versus); otherwise the single entered side controls; entered flags unreadable ⇒ fail closed to stock | Accepted |
| D4 | Re-trigger semantics | User-visible behavior | Restart preview from its start at the new rate/DSP mode, debounced **150 ms** after the last change tick (maintainer override; drain cadence permitting); `preserve_pitch` edits re-trigger too (when speed ≠ 100) | Accepted |
| D5 | Returning to 100% | User-visible behavior | One final debounced re-trigger with no binding → literal stock preview | Accepted |
| D6 | Slow-rate tail | User-visible behavior | No compensation: the game's own preview stop/loop timing governs; at slow rates you hear proportionally less of the song | Accepted |
| D7 | Gameplay-header safety (the R-A hazard) | Correctness of gameplay audio | Fail-closed: prove select→loading unregisters the preview bank, else force-unregister preview-bound banks before any gameplay create can reuse a preview-stretched header | Accepted |
| D8 | Isolation from the gameplay transaction | Architecture | Preview bindings never touch Q31 clock, score guard, movie policy, lifecycle phases, or XactSlots; separate registry slot consulted on the io miss path | Accepted |
| D9 | Failure policy | Reliability | Fail-open to stock preview on every fault, one bounded WARN via the drain; never block song select | Accepted |
| D10 | Scene scope | Scope | Scene 25 (song select) only | Accepted |
| D11 | Config surface | Operator control | None — on-by-default behavior of the song-playback-speed mod, no kill switch (maintainer override of the recommended switch) | Overridden |
| D12 | Re-trigger primitive | RE cost | Compose from already-held primitives (SoundBank Stop + unregister/create trampolines + `se_play(5, cue)`); derive the game's own load+play (`FUN_1801ccd10`) only if research shows the composition is unsafe | Accepted |
| D13 | Ring/generator tuning for previews | Perf | Reuse the existing defaults (16 MiB ring, capacity/2 pacing) unchanged | Assumed |
| D14 | Replayed cue pan | Fidelity | Match stock (expected 0.0 / center; confirm during R-B static read) | Assumed |
| D15 | Preview binding identity | Bookkeeping | Separate monotonic preview-generation counter; never the gameplay generation | Assumed |

## Details

### D1 — Core mechanism
**Question:** How does the preview come to play at the desired rate?
**Recommendation:** Reuse the streaming engine end-to-end: at preview-bank
create time (already detoured), qualify a *preview bind* and publish a
virtual bank whose `_s` entry is rate-stretched (WSOLA or resampler per
`preserve_pitch`) and whose main entry is verbatim — the exact inverse of
the gameplay plan. For a value change while a preview is playing, the
header is already parsed, so: stop cue → unregister bank → re-create →
re-play cue (fresh header carries the new stretched duration).
**Rejected:** XACT cue pitch variable (±1 octave ≠ 25–175 %, no
pitch-preserved mode, depends on XSB RPC wiring); mod-owned in-memory bank
with pre-synthesized audio (WSOLA is ~2.4× realtime under CrossOver —
multi-second stalls at slow rates; streaming starts in <1 s).

### D2 — When previews play at rate
**Question:** Only after an edit, or on every wheel settle?
**Recommendation:** Every wheel-settle create binds while the controlling
side's desired ≠ 100. The preview then always previews what gameplay will
actually sound like — which is the point of the feature. Identity keeps
zero footprint (no binds).

### D3 — Versus policy
**Question:** Two sides can desire different rates; the preview is
cabinet-global. Whose applies?
**Accepted (maintainer, 2026-08-15):** Follow the gameplay rate policy.
Verified in code: `classify_scene26` (`src/services/song_rate/lifecycle.rs`)
returns `Identity(IdentityReason::LocalVersus)` when both `PlayerWork+0x4`
entered flags are set — local versus is excluded from the gameplay rate
feature in v1. The preview mirrors this exactly: both sides entered ⇒ no
preview binding and no re-trigger (stock preview); one side entered ⇒ that
side's `song_speed`/`preserve_pitch` control; entered flags unreadable ⇒
fail closed to stock. When versus support ever lands for gameplay, the
preview inherits whatever side-selection rule gameplay adopts.

### D4 — Re-trigger semantics
**Question:** What happens at the moment of an edit?
**Accepted (maintainer, 2026-08-15):** Debounce **150 ms** after the last
change tick (maintainer override of the proposed 400 ms; the callback
fires on every scroll step, and the executor is the song-rate runtime's
drain — the effective latency is bounded below by the drain cadence, to
be confirmed in design), then restart the preview from its beginning at
the new rate. No seek exists in the engine (RE-proven), so restart is the
only possible semantic. Accepted quirk pending research: if the game's
preview had already ended naturally, the re-trigger plays it again once.

### D6 — Slow-rate tail
**Question:** At 50% the stretched preview is 2× longer than the stock
preview window. Extend the window? *(You may not have considered this.)*
**Recommendation:** No — let the game's own preview stop/loop logic
govern. Hearing the first portion slowed is a faithful preview of the
rate; touching the game's preview timing is new risk for marginal gain.

### D7 — Gameplay-header safety
**Question:** When a song is confirmed, does the gameplay create get a
fresh header parse, or could the duplicate guard reuse the live preview
bank (whose parsed header carries preview-stretched durations)?
**Recommendation:** Blocking research item (R-A). Design fail-closed:
if the natural unregister between select and loading cannot be proven,
the feature force-unregisters any preview-bound bank at scene-25 exit /
before gameplay qualification. Gameplay audio integrity outranks the
preview feature existing at all.

### D11 — Config surface
**Question:** Operator kill switch?
**Overridden (maintainer, 2026-08-15):** No config surface. The behavior
is on by default whenever the song-playback-speed mod is enabled; there
is no switch to turn it off. (The proposed optional kill switch was
rejected — disabling the mod itself remains the only off path.)
