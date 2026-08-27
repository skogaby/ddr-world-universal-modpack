# Plan — task-02 lifecycle arm + mapping API

Status: Approved 2026-08-13 (via approved upstream plan/design — code-assist
auto mode; see context.md approval chain)

## Implementation approach

1. **lifecycle.rs** — `EligibilityInputs.training_arm: bool`;
   `classify_scene26` returns `IdentityRate` for a 100 % desire only when
   `!training_arm` (all other gates untouched — versus/course/stage fail
   closed identically); `on_scene26`'s Arm branch suppresses movies only for
   non-identity percents (identity arms emit `Movie(false)`).
2. **transaction.rs** — `FaultSelector.identity_bind_refused` +
   `"identity-bind-refused"` parse; `finish_create`'s commit leg skips the
   rate-save ledger, session taint, and movie confirmation when
   `token.requested_percent == IDENTITY_PERCENT` (arm alone never taints);
   snapshot publication + phase advance unchanged.
3. **binding.rs** — `prepare_binding` splits at `percent == 100`:
   `plan_identity_bank` → private copy → `new_identity_passthrough`, NO
   producer spawn, identity-specific fault gate first; registry gains
   `set_active_content_mapping(shift, lead) -> bool` (false when no live
   binding).
4. **runtime.rs (windows glue)** — `TRAINING_ARM` atomic +
   `set_training_arm`/`training_arm_requested`; scene-26 gathers session
   inputs when training is requested even at 100 %; public
   `set_content_mapping` delegating to the registry.

## Test scenarios (host)

| # | Criterion | Test |
|---|---|---|
| 1 | Identity arm gated on the request | lifecycle_tests: 100 % + training ⇒ `Arm{100}`; 100 % without ⇒ `IdentityRate` (pin); versus/course + training ⇒ fail-closed Identity reasons |
| 2 | Identity arm binds passthrough, no movie, snapshot identity | lifecycle_tests: identity arm emits `Movie(false)`; wavebank_hook_tests composition: arm(100) → create → `Committed`, snapshot committed at 100 %, `!is_non_identity_commit()`, Q31 factor stays IDENTITY, ledger empty, taint probe empty, movie confirm never called |
| 3 | Mapping API contract | binding_tests: `set_active_content_mapping` false on empty registry, true + serving reflects the mapping with a live binding |
| 4 | Fail-open on refusal | binding_tests: `identity-bind-refused` refuses percent-100 preflight (`Injected`), leaves percent-50 preflight unaffected; composition: refusal lands EarlyFailed + one mailbox note (existing shape) |
| 5 | No regression | full suite green (189 baseline) |

## Risks

- The commit-leg skip must not disturb rate commits — conditioned strictly on
  `requested_percent == IDENTITY_PERCENT` (only reachable via training arms).
- `eligible_inputs()` + runtime.rs are the only EligibilityInputs literal
  sites; struct-update sites inherit the new field.
