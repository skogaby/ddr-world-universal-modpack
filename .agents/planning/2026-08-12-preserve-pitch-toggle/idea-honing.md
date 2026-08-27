# Idea Honing: Preserve Song Pitch sub-option

Decision register for the `preserve_pitch` boolean child option under
`song_speed`. Status: `Proposed`, `Accepted`, `Overridden`, `Assumed`, `Open`.

**Readiness Confirmed 2026-08-12** — register accepted (D1-D9 Accepted, D10
Overridden, D11-D14 Assumed); user approved proceeding to detailed design.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | What "preserve pitch OFF" does | Defines the entire DSP scope | Plain resampler (vinyl-style) inside the existing `Feed`, plan-driven at the exact effective ratio; NOT the engine's XSB pitch-cents mechanism | Accepted |
| D2 | Default value | User-visible behavior; back-compat | ON (1) — pitch-preserved, i.e. today's shipped behavior | Accepted |
| D3 | Persistence (incl. backend) | Profile round-trip | `PersistMode::Full` → wire `mod_preserve_pitch`, JSON cache, auto-seeding; **bemani-buddy backend change IN SCOPE**, following the in-flight 012/013 pattern (JSON model → codegen → migration 014 → DB model/DAO → handler + tests → sqlx prepare) | Accepted |
| D4 | Visibility mechanism | Framework change | Add `ShowWhen::NotEquals { parent_id, value }` (4 touch points); child declares `NotEquals { "song_speed", 100 }` | Accepted |
| D5 | Which side's flag wins per song | Gameplay semantics | Same rule as percent: latched at scene-26 from the single entered side; rides `LifecycleState` → `prepare_binding` → `Binding` (not `RateSnapshot`) | Accepted |
| D6 | Resampler quality | Audio quality vs complexity | Linear interpolation, shared Q32 phase across both channels, integer rounding (deterministic); windowed-sinc as future upgrade only if audibly needed | Accepted |
| D7 | Preview panel(s) | Asset scope; user specified text | Two per-value panels (`_off`/`_on`), house voice, both carrying the user's sentence as the summary line + one state line each | Accepted |
| D8 | Score containment interaction | Integrity policy | Unchanged — containment keys on rate ≠ 100 % regardless of pitch mode | Accepted |
| D9 | Validation scope | No-test-harness repo discipline | Reference-oracle resampler + streaming-equivalence host tests + a new harness leg with inverted pitch expectation (`f_out ≈ f_source · S/O`) | Accepted |
| D10 | PUS CSV rate columns | Diagnostics completeness | ~~Add a pitch-mode cell~~ **No PUS/CSV changes** (user override) | Overridden |
| D11 | Row placement / row_order | Cosmetic | Register immediately after `song_speed` in `enable()`; add `preserve_pitch` after `song_speed` in README's `row_order` example | Assumed |
| D12 | Label/preview generation | Asset workflow | `scripts/gen_option_labels.py` (actual name; prompt said `gen_options_labels.py`): one LABELS row + PREVIEWS rows; opportunistically drop the duplicate `arrow_opacity` PREVIEWS entry | Assumed |
| D13 | Value retention while hidden | UX nuance | Hidden ≠ reset: the stored value persists and reappears when speed leaves 100 (framework's existing behavior; no code needed) | Assumed |
| D14 | Loop seam under resampling | Correctness for looping banks | Reproduce WSOLA's two-domain mapping (global phase outside loop, loop-relative proportional map inside) so source_end↔output_end align | Assumed |

---

## D1 — Semantics of "preserve pitch OFF"

**Question:** When the player turns PRESERVE SONG PITCH off, what actually
happens to the audio?

**Recommendation:** A plain resampler replaces WSOLA inside the existing
`Feed` (src/services/song_rate/generator.rs). It reads the decoded source PCM
at the plan's exact effective ratio (`source_frames/output_frames`, Q32 phase)
and emits exactly the plan's output frame count — so the virtual bank, clock,
tick domain, and score machinery are byte-level indifferent to the mode.
Pitch shifts with rate (75 % ⇒ ~4.98 semitones down), which is the point.

**Rejected:** the engine-native XSB pitch-cents route (original research §4 of
docs/song_playback_speed.md) — never live-proven, cent-quantized (breaks the
exact-integer RateRatio clock), bypasses the shipped architecture.

## D2 — Default ON

Current shipped behavior is pitch-preserved. Default ON means enabling the
mod changes nothing for existing players; OFF is the new opt-in behavior.

## D3 — PersistMode::Full + bemani-buddy backend in scope

Follows the parent (`song_speed` is Full). Wire field `mod_preserve_pitch`
auto-derived; JSON offline cache works without any server.
`load_transform` clamps to {0,1} (insurance against hand-edited JSON).

**Backend (user-directed, in scope):** the sibling `bemani-buddy` checkout has
uncommitted in-flight changes adding `mod_song_speed` (migration 012) and
`mod_assist_tick_volume` (migration 013) — our change follows that exact
pattern, stacked on top: JSON model (`models/ddr_world/playdata_3.json`, both
load + save `<option>` shapes) → codegen
(`cargo run -p codegen -- models/ddr_world/ crates/bemani-protocol/src/ddr_world/`)
→ migration `014_ddr_world_preserve_pitch.sql`
(`opt_mod_preserve_pitch INT NULL DEFAULT NULL`, verbatim storage, nullable
no-default) → DB model + MySQL DAO → playdata handler (load map, new-player
None, save only-when-present) + handler tests → `sqlx migrate run` +
`cargo sqlx prepare --workspace`. Verified against the in-flight diff; details
in the research notes. Maintainer instruction: widespread `cargo fmt` churn in
bemani-buddy is fine to leave — it gets folded into one commit by the
maintainer.

## D4 — ShowWhen::NotEquals

The framework only has `Always`/`Equals`. `NotEquals` is the minimal,
symmetric addition; exactly four sites pattern-match `ShowWhen`
(api.rs enum, registry.rs parent validation, rows.rs evaluator,
rows.rs update_children_visibility). A generalized predicate closure was
rejected: heavier, nothing else needs it.

Live behavior comes free: the framework already remasks the side's rows after
every user value press on the parent, so the child appears/disappears
same-frame as `song_speed` scrolls through 100. Per-side independent.

## D5 — Flag latch follows the percent's rule

Exactly one entered side selects the session's rate at the scene-26 arm; the
same side's `preserve_pitch` value latches with it. Mid-song option edits
can't change the current song (next-song semantics, same as speed). Rides
Quick Restart re-binds automatically via the lifecycle's latched values.

## D6 — Linear resampler, deterministic fixed-point

Determinism (host-replay byte-identity) is the hard requirement, not
audiophile quality — matching the WSOLA implementation's bar. Linear
interpolation's artifact floor is comparable to WSOLA's linear-crossfade OLA.
Orders of magnitude cheaper than WSOLA (no SAD search) ⇒ loading is faster
with preserve OFF. Interleaved stereo with one shared phase accumulator ⇒
inherent channel coherence.

## D7 — Preview panels

User-specified copy: "Decides whether the song's pitch should be preserved
when the playback speed is adjusted." House style (per
scripts/gen_option_labels.py) is a summary paragraph + per-state paragraph.
Recommendation: `seop_image_preserve_pitch_off.png` and `_on.png`, each with
the user's sentence as paragraph 1 and a state line as paragraph 2 (e.g.
ON: "The song is time-stretched — it plays slower or faster at its original
pitch."; OFF: "The song is resampled — its pitch falls or rises with the
playback speed, like a record player."). Alternative (closer to the literal
request): a single fallback panel `seop_image_preserve_pitch.png` with only
the user's sentence.

## D8 — Score containment unchanged

The pending-rate-save ledger keys on "rate ≠ 100 played" only. Pitch mode
does not weaken or alter containment; no change.

## D9 — Validation

Same three-layer pattern as the stretch path: (a) whole-buffer reference
resampler as the frozen oracle; (b) streaming state proven byte-identical
across chunk sizes/checkpoint-restore/engine-replay in host tests;
(c) new `scripts/validate_song_playback_speed.sh` leg where the sine
frequency expectation is inverted (`f_out ≈ f_source · S/O` instead of
`f_out ≈ f_source`).

## D10 — No PUS CSV changes (overridden)

Originally proposed adding a pitch-mode cell to the CSV rate columns; the
user overrode this — the Power User Statistics CSV export is untouched by
this feature.

## D11–D14 — Assumed

See table. Settled by existing framework behavior or trivially reversible.
