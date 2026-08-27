# Idea Honing — Training Mode

Decision register. Status: `Proposed` | `Accepted` | `Overridden` | `Assumed` | `Open`.
User review round 1 applied 2026-08-13 (overrides on D4/D5/D7/D12/D13; new D15–D17;
"everything else approved as-is").

| ID | Decision | Why it matters | Resolution | Status |
|----|----------|----------------|------------|--------|
| D1 | Scope & phasing | Determines everything downstream | v1 = section practice (seek + loop/early-end + HUD + grouped options); v2 = RW/FF gestures; v3 = judgement-state rewind (sketched only) | Accepted |
| D2 | How section bounds are set (UX) | Core interaction model | BOTH: `SKIP FIRST (s)` / `OMIT LAST (s)` scalar rows (clamped per D14, effective-clamp at use time) + live A-B pinpad gestures to refine mid-play | Accepted |
| D3 | Gesture allocation | Permanent UX surface | ~~Triple-7 = set A, triple-9 = set B, triple-5 = clear; 4/6 reserved for v2 RW/FF~~ **Amended 2026-08-13 (maintainer, pre-Step-2 demo): marker gestures live on the pinpad's MIDDLE row — triple-4 = set A, triple-5 = clear, triple-6 = set B; 7/9 become the v2 RW/FF candidates.** Triple-1 restarts from A while training; triple-3 unchanged | Accepted (amended) |
| D4 | Loop engagement & accumulator policy | Defines the grind loop feel | Loop controlled by the `LOOP SONG` row (D15), not auto-engage. B unset ⇒ clamped chart end. Accumulators reset per iteration (restart semantics) | Accepted (revised) |
| D5 | Score submission policy | Score integrity | Suppress whenever the song was meaningfully altered: rate ≠ 100 (shipped), autoplay (shipped), quick-fail (shipped), **assist tick enabled (NEW — behavior change to the shipped mod)**, section skipped/omitted, seek/FF/RW used. Per-stage suppression + sanitised logout, fail-closed | Accepted (expanded) |
| D6 | Identity-arm + fail-open | The one song_rate lifecycle extension | Training arms a passthrough binding at 100 % (`passthrough_plan`, never `plan_entry(100)`); binding refusal ⇒ loop-from-A=0 only, one WARN | Accepted |
| D7 | HUD shape & placement | User-visible | Shared widget: progress bar + `current/total` content time + A/B ticks. **Bottom-center default**; per-player `PROGRESS BAR PLACEMENT` row (TOP/BOTTOM) in the training group | Accepted (revised) |
| D8 | Eligibility | Session-type surface | Ordinary solo + doubles only; versus and course/Dan excluded (mirrors song_rate gates) | Accepted |
| D9 | Options grouping | User-specified UI | "TRAINING OPTIONS" header row (full-width art, half-height via `+0xA8`, non-selectable via `+0x28` vtable swap). **No hardcoded group lists in code** — grouping expressed purely by `row_order` in mod-config.json (existing mechanism); shipped default config/README carry the grouped ordering | Accepted |
| D10 | Spanning-freeze policy on seek | Judge semantics at loop point | Neutralize freezes spanning A (post-pass marks them held/consumed) | Accepted |
| D11 | A/B marker persistence | Small | None — session/song-scoped only; gesture refinements never write back to the bound rows | Accepted |
| D12 | Config surface | Operator tuning | New `training_mode` block in mod-config.json for FF/RW skip increments (lands with its consumer — v2; v1 adds no keys) | Accepted (revised) |
| D13 | step_data_export placement | Grouping completeness | INCLUDED in the training group (via the shipped default `row_order`) | Accepted |
| D14 | Select-time clamping source | Range sanity for bound rows | Audio length from the XWB header via the existing slot-5 wavebank-create detour (`docs/training_mode_research.md` §8); no SSQ parsing; runtime hard clamp = ControlMessageActor thresholds | Accepted |
| D15 | `LOOP SONG` (ON/OFF) row | Loop vs play-once-to-results | New per-player row in the training group. ON ⇒ section loops until quick-fail. OFF ⇒ reaching the section end triggers the **early natural end** (ControlMessageActor threshold writes, research §4.4): banner → results with the partial play's stats; submission suppressed per D5. **Default OFF** (least surprise — an untouched option never produces a song that won't end) | Accepted |
| D16 | Mod structure / namespace | Kill-switch granularity, config back-compat | New top-level mod `training-mode` owns: bound rows, LOOP SONG, HUD + placement row, A/B gestures, seek machinery, FF/RW (v2). `assist-tick` and `song-playback-speed` STAY standalone top-level mods; grouping is purely visual via D9/`row_order` | Accepted |
| D17 | Header-row render policy | Prevents orphaned headers | Decorative header rows are rendered ONLY when listed in `row_order`; missing from the JSON ⇒ not rendered at all (unlike normal rows, which append at the end). Operators own header placement entirely | Accepted |

## Decision details & notes

- **D5 callout (explicit, needs no further action unless reconsidered):**
  suppressing scores when assist tick is enabled is a **change to the
  shipped Assist Tick mod's behavior** (today it does not taint). Accepted
  per user's "altered in a meaningful way" principle, 2026-08-13.
- **D9/D17 mechanics:** headers are registered rows with ids (participating
  in the ordering machinery); the only special-casing is the D17 render
  policy and the header row kind itself (non-selectable, display-only,
  slim). Research: `docs/option_header_rows_research.md`.
- **D15 mechanics:** LOOP OFF must write the ControlMessageActor end
  thresholds (early natural end); LOOP ON must NOT (the loop reset fires
  first; thresholds stay clamped-away). Live B-set via gesture while LOOP
  OFF updates the thresholds mid-song.
- **D16 resolved 2026-08-13:** standalone mods retained; no migration
  aliases needed.

Research citations: `docs/training_mode_research.md`,
`docs/option_header_rows_research.md`, `research/orientation.md`.

---

Readiness Confirmed 2026-08-13 — all 17 decisions Accepted; user approved
proceeding to detailed design.
