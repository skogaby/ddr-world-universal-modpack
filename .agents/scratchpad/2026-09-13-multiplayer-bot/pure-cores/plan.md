# Plan — pure-cores

Status: Approved 2026-09-13 (auto mode — upstream approval stands in)

## Tests (all `#[cfg(test)]` in the three modules; AC numbers from the task file)
- eligibility: AC1 (either side), AC2 (each refusal + each Unavailable), AC3 (clamp).
- skill: AC4 (window boundaries), AC5 (monotone curves; L10/L1 1e6-sample endpoints; seed
  determinism; zero seed advances), `#[ignore] report_grade_histogram` (task TR6).
- planner: AC6 (single note hit early/late; miss ⇒ blocked_until), AC7 (miss blocks next
  same-panel note), AC8 (10k-stream property: monotone per-panel events, cursor monotone,
  lookahead respected), AC9 (jump shares E; freeze body; shock), AC10 (tally).

## Implementation order
1. stub mod.rs + mods/mod.rs decl → cargo check.
2. eligibility (red → green). 3. skill (red → green). 4. planner (red → green).
5. temp-crate harness run; cargo check; cargo fmt.
