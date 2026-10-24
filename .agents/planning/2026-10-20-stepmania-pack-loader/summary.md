# Summary: StepMania Pack Loader

PDD pass completed 2026-10-21. **Implementation complete 2026-10-24** (13 steps, 18 cabinet
runs; all uncommitted — the maintainer commits). Live resume point: `progress.md`.

## Final state (2026-10-24)

What shipped, and where it differs from the design below:

- **Delivery** = materialised files under `data_mods/_cache/stepmania/` served through the
  LayeredFS resolver registry (`mod_paths::register_resolver`); the gameplay bank is a
  progressive `.part` file whose reads `file_hooks` holds at the writer's watermark. No
  virtual fds.
- **Auto-sync (reworked 2026-10-24)**: the sweep and the on-decide measurer only MEASURE
  (`core/sync`, `services/stepmania/{sync_sweep,sync_apply,sync_state}.rs`); nothing on disk
  changes after indexing. The per-song delta is applied through the game's JUDGEMENT OFFSET
  by `per_song_judgement_offsets` (`applied = (per-song entry, else stock) + delta`, ±100),
  frozen per song per session; a song picked before it was measured is measured at
  song-selection time with the stage load (≤ 5 s) and first judge (≤ 3 s) held. ITG1/ITG2
  measure ≈ −2…+2 ms (near DDR's convention); SRPG5 +7…+10; ~25 % of ITG1 refused
  (`halves_disagree`, left as authored by decision).
- **Boot hardening**: `fast_bootup` quarantines injected files that fail a pre-flight
  `check_ssq` (background thread at enable; items release+advance, final item runs with
  `hasChart` neutered via a vtable copy) — never `ME1529`. E8 (radar maxima) was **refuted**
  by RE: the maxima are reset to constants at scene 7; the exclusion ships as insurance.
- **Visibility**: hidden from ALL MUSIC by default (`exclude_from_all_music: true`); folder
  bit = highest free (31), series = lowest free (22). Fail-closed hiding runs at enable or,
  on a slow cabinet, at the first scene change. On a warm boot `injected_entries()` falls
  back to the fingerprint snapshot (the transform does not run when the arc cache is current).
- **Titles**: native `#TITLE` when every glyph is in the song-title KBF fonts
  (`core/sm/title_glyphs.rs`, generated), else `#TITLETRANSLIT`; `title_yomi` from the
  romanised title. `FORMAT_VERSION` 5.
- **Dropped**: per-mod splash line (maintainer: no precedent); `index.bin` quarantine
  capability record; full-bank decode sharing with the measurer (re-decode instead).
- **Operator docs**: README section + config row; `stepmania/Songs/README.txt` in the
  release; `tools/sm_pack_check [--auto-sync]` for offline vetting.
- **Follow-up outside this feature**: fast-bootup's cache verifier loses its race under host
  load (3 of 8 boots) → full stock boot pass; a cheaper stat sweep or earlier start.

## Artifacts

| Path | Content |
|---|---|
| `rough-idea.md` | The original idea |
| `idea-honing.md` | Decision register D1–D21 (all Accepted/Overridden), rationale, `Readiness Confirmed 2026-10-21` |
| `research/orientation.md` | Blind-spot pass: what the codebase and `docs/stepmania_runtime_packs_feasibility.md` already establish, what changes the idea, the open experiments |
| `research/ddr-chart-tools.md` | Survey of the sibling converter repo: reusable parsers/writers, private glue, fidelity gaps, vendoring recipe |
| `design/detailed-design.md` | Self-contained design, `Status: Approved 2026-10-21` |
| `implementation/plan.md` | 13-step plan with checklist (all ticked 2026-10-24), `Status: Approved 2026-10-21` |
| `progress.md` | Live progress, deploy & test log (runs #1–#18), deviations, cold-resume facts |
| `prototypes/` | `make_spike.py`, `make_split_test_ssc.py`, `corrupt_ssq.py` (Step 11 demo) |

## Design in brief

A default-ON mod `stepmania-packs` indexes `./stepmania/Songs/<Pack>/<Song>/` at DLL init,
converts each eligible chart to SSQ (self-checked before its slot is declared), and appends
`<music>` entries to `musicdb.xml` through a new LayeredFS member transform on the
`startup.arc` repack — the only race-free injection point. Assets are materialised into a
bounded `data_mods/_cache/stepmania/` (SSQ/XSB/thumbnails for all songs; banks and jackets
in an `audio_cache_mb` LRU) and served by the existing path-redirecting hooks via a resolver
registry; the full gameplay bank is written progressively with the read hook holding reads
at the write watermark, so opens never wait on encoding. Previews and jackets are prefetched
from wheel-card binding events (the 3-per-row grid makes a newly scrolled row exactly three
jobs). Songs sit in a STEPMANIA folder and VERSION entry registered programmatically. Every
play is tainted through `score_guard` (stage save suppressed, targeted logout sanitiser,
wire filters), and if sanitisation is unavailable the songs are hidden. Converters are
ported from `ddr-chart-tools` as pure, harness-mounted code with `symphonia` decoding and
the tool's auto-sync estimator (gated by `auto_sync`).

## Plan in brief

Spikes first (E1 visibility contract; E2/E6 diagnostics), then LayeredFS infrastructure,
then the pure chart and audio pipelines with a host harness, then the first end-to-end
milestone (Step 6: drop a pack in, boot, play, nothing saved), followed by full score
isolation, prefetch/progressive audio, split SSQs, boot hardening, auto-sync and polish.

## Next steps

1. Run the code-task-generator sop against `implementation/plan.md`, one step at a time
   (Step 1 first — it is a cabinet spike with no code).
2. Run the code-assist sop on each generated task in order; maintain `progress.md` in this
   directory per the AGENTS.md PDD convention (deploy & test log is the real validation).

## Assumptions to watch

- **E1** (visibility contract) is the gate for everything; **E2** (open thread) decides
  whether any bounded wait is allowed in the open hook; **E6** fixes the sanitiser scope.
- E7 (1-tick quad rows, delay-as-early-stop), E8 (global radar maxima), E9 (level-20 art)
  only adjust policies.
- Harness byte-identity with `ddr-chart-tools` holds only for charts needing none of the
  new fixes; XWB entry order deliberately differs (main-first) and is compared per entry.
- `symphonia` and `rustfft` must build under both `./build.sh` and `./build_win7.sh`;
  confirm in Step 5 / Step 11 before relying on them.
- Disk: ~100–150 MB per 1,000 songs at boot plus the LRU budget; revisit virtual fds (v2)
  only if measurements demand it.
