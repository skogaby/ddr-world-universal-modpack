# Progress — skill-retune (post-Step-6 tuning, 2026-09-14)

Status: Complete (uncommitted — maintainer commits manually)

Maintainer request after the first cabinet playtest + distribution review of the 2026-09-13
constants (L10 71 % MFC, L1 60 % fail, S-Marv ≥ 50 % from L6, EX 96.6/99.0/99.9 at L8–10):
(1) L10 MFC ≈ 10 %, L1 fail ≈ 10 %; (2) exclusive Marvelous more common than S-Marvelous almost
everywhere, S-Marv common only from ~L7 and dominant at L9/10; (3) a smooth EX% ramp.

## What changed
- `src/mods/multiplayer_bot/skill.rs`: `Params`/`DEFAULT` + `curve_from` (shared with the
  simulator); `Curve { lean_ms, tight_ms, drift_ms, loose_ms, p_tight, p_miss, form_sd }`;
  `Form { sign, drift, factor }` per song; `decide(rng, form, c)` = Miss w.p. `p_miss·factor`
  else `round(sign·lean + drift + N(0, tight | loose·factor))`, `|d| > 124 ⇒ Miss`;
  `Curve::describe()` for the three log sites. Tests rewritten (11): anchors/monotonicity,
  Marv > S-Marv through the knee, S-Marv rare-low/common-high, L1 spread + miss rate, L10
  P(Marv) window, form sign/drift/factor statistics, plus the unchanged window/RNG/seed tests.
- `planner.rs`: `SongState.form: Option<Form>` rolled lazily at the first decision (a rebuilt
  state — song reset — re-rolls).
- `filler.rs` / `self_test.rs` / `impersonation.rs`: log lines use `Curve::describe()`.
- `tools/bot_sim`: `skill_override` = a `Params` copy through `curve_from` (`--set key=value`,
  repeatable; the `--sigma-l1/--sigma-l10/--pmiss-l1/--pmiss-exp` flags are gone);
  `judge_model` test curves updated; `report.rs` summary table gained S-Marv/Marv/Perf/Great/
  Good/Miss shares, mean score, PFC%, per-difficulty MFC%; JSON `meta.curves` rows are now
  `[L, lean, tight, drift, loose, p_tight, p_miss]` and the HTML overview shows lean/tight/loose/
  pocket/p_miss.

## Tuning trail (3 seeds unless noted)
| candidate | L10 MFC | L1 fail | S-Marv vs Marv | note |
|---|---|---|---|---|
| lean 14.5 flat, knee 8, l10 9.5, pocket_exp 0.7, p_miss 0.055 | 54 % | 10 % | L1 19.5 > 18.6 ✗ | flat lean too small at L1 |
| lean 17→14.5@7→10, pocket_exp 1.0, p_miss 0.055 | 41 % | 27 % (form_sd 0.35 made L1 fail worse) | ✓ | L10 lean too small |
| … knee 8, l10 11, p_miss 0.03 | 19 % | 19 % | ✓ | |
| … l10 11.5, loose_l1 60, p_miss 0.02 (1 seed) | 11.7 % | 12.6 % | ✓ | close |
| **shipped: l10 11.7, p_miss 0.015** | **9.8 %** | **11.6 %** | **Marv > S-Marv through L9** | EX 62/68/74/79/83/86/90/92/97/99 |

## Follow-up (same day): side as a sticky Markov chain
Maintainer: "all FAST or all SLOW within an attempt reads as unnatural". The per-song sign was a
byproduct of the simplest lean implementation, not a requirement — only the MAGNITUDE (the dip
at zero) is forced by the Marv > S-Marv target. `Form` gained `p_late` (per song, `clamp(0.5 +
0.15·z, 0.1, 0.9)`) and the side became a Markov chain (keep w.p. `sign_stickiness` 0.7, else
redraw from `p_late`); the drift now multiplies the magnitude (`sign · (lean + drift)`) so it is
side-independent. Two new `Params` (`late_bias_sd`, `sign_stickiness`, `--set`-able). Summary
table gained `slow%` (mean SLOW share of the stock FAST/SLOW counters) + its per-song `±sd`:
50 % ± 9 (L1) … ± 31 (L10). Grade mix unchanged (new host test pins every bucket within 1 % of
a frozen-side run; corpus table identical to 0.1 %). 78 tests.

## Gates
`cargo check` clean → `cargo fmt` (both crates) → `./build.sh` clean →
`./scripts/validate_multiplayer_bot.sh` 76/76 → HTML report META shape verified.

## Docs
Design §4.6 rewritten (+ R6, §7.1 usage line; status line re-dated), README hero paragraph,
AGENTS.md row skill sentence, feature `progress.md`.
