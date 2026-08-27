# Task: Promote Toast to a Shared Service with Pulse and Hold Modes

## Description
Move the Training Mode gesture toast (`src/mods/training_mode/toast.rs`) to a
shared service `src/services/toast/` and extend it with the API the
auto-calibration feature needs: owned-`String` text, a caller-specified flash
hold, and a pulsing persistent mode that loops fade-in/out until dismissed.
Training Mode's call sites migrate to the new service; the old module is
deleted. A host-test harness script for the feature is created here because it
is how this task's tests run.

## Background
The auto-calibration feature (a timing-offsets sub-feature landing in later
steps) shows a pulsing "Calibrating..." toast for the duration of a song, a
5 s result toast (`CALIBRATED: 87 -> 93 (+6 MS)` — needs formatted `String`
text), and 3 s refusal toasts. The existing toast is `pub(super)` inside
training_mode, `&'static str` only, and hard-coded to a 100/250/300 ms flash.

The existing module's disciplines are load-bearing and must survive the move:
one lazily-created native `TextWidget` kept hidden for the process lifetime;
all widget work via `widget_renderer::run_on_render_thread`; a
generation-tokened self-requeueing animation callback (a newer toast
supersedes an in-flight one); no state mutex held across a render-thread
schedule; panic-free render callbacks.

Host testability constraint: the project's host-test harness pattern
(`scripts/validate_judgement_offsets.sh`) mounts individual module FILES into
a throwaway host crate via `#[path]`, so host-tested code must be
dependency-free. Therefore the fade-curve logic lives in its own
dependency-free file (`curve.rs`) and the widget/scheduling half (`mod.rs`)
consumes it.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-26-auto-calibration/design/detailed-design.md
  (§6 "src/services/toast.rs", plus the Overview and Requirement 7 for the
  toast's role; note this task realizes §6 as a `toast/` directory with a
  `curve.rs` pure layer — a structural refinement for host-testability, not a
  behavior change)

**Additional References (if relevant to this task):**
- src/mods/training_mode/toast.rs — the module being promoted (copy its
  disciplines verbatim where possible)
- scripts/validate_judgement_offsets.sh — the harness pattern to follow
- src/mods/training_mode/bounds.rs, src/mods/training_mode/mod.rs — call sites
  to migrate

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New module `src/services/toast/curve.rs` — dependency-free (std only):
   - `pub enum ToastMode { Flash { hold_ms: u64 }, Pulse }` (Copy/Clone).
   - `pub fn alpha_at(mode: ToastMode, elapsed_ms: u64) -> Option<f32>`:
     - `Flash`: 100 ms linear fade-in → `hold_ms` at 1.0 → 300 ms linear
       fade-out → `None` (matches the existing `fade_alpha` with the hold
       parameterized; existing behavior = `Flash { hold_ms: 250 }`).
     - `Pulse`: loops `elapsed_ms % 2800`: 800 ms linear fade-in → 800 ms hold
       at 1.0 → 800 ms linear fade-out → 400 ms dark gap at 0.0; NEVER returns
       `None` (only supersession or dismiss ends a pulse).
   - Timing constants public within the module so tests reference them.
   - `#[cfg(test)]` unit tests (see Acceptance Criteria 4–5).
2. New module `src/services/toast/mod.rs` — the promoted widget half:
   - `pub fn flash(text: impl Into<String>)` — `Flash { hold_ms: 250 }`
     (today's behavior).
   - `pub fn flash_with_hold(text: impl Into<String>, hold_ms: u64)`.
   - `pub fn show_pulsing(text: impl Into<String>)`.
   - `pub fn dismiss()` — unconditional hide + generation bump (unchanged
     semantics).
   - `ToastState.text` becomes `String`; state gains the active `ToastMode`.
   - Everything else preserved from the source module: widget creation/config
     (center alignment, scale 1.2, position 640/630, black outline, amber
     color), lazy creation on the render thread, generation supersession,
     `applied_generation` text application, re-queue AFTER dropping the state
     lock, silent drop when `widget_renderer` is unavailable or refuses a
     widget.
3. `src/services/mod.rs` gains `pub mod toast;`.
4. Call-site migration (exact behavior preservation):
   - `src/mods/training_mode/bounds.rs`: 3× `super::toast::show(...)` →
     `crate::services::toast::flash(...)`.
   - `src/mods/training_mode/mod.rs`: `toast::dismiss()` →
     `crate::services::toast::dismiss()` (drop the `mod toast;` declaration).
   - Delete `src/mods/training_mode/toast.rs`.
5. New script `scripts/validate_auto_calibration.sh` (executable), following
   the `validate_judgement_offsets.sh` temp-crate pattern: mounts
   `src/services/toast/curve.rs` via `#[path]` and runs `cargo test --quiet`;
   header comment says it grows as later auto-calibration steps add pure
   modules.
6. Project rules apply: no `println!` (logging via `log_*!` macros only, if
   any), no panics/unwraps in render-thread callbacks, `unsafe impl Send`
   retained for the widget state with the same SAFETY comment.

## Dependencies
- None — first task of the feature. Later steps consume the new API
  (`show_pulsing`, `flash_with_hold`).

## Implementation Approach
1. Create `src/services/toast/curve.rs` with `ToastMode`, constants,
   `alpha_at`, and its tests (TDD: write the curve tests first — they run via
   the new harness script, which can be created at this point).
2. Create `src/services/toast/mod.rs` by porting
   `src/mods/training_mode/toast.rs`, replacing the local `fade_alpha` with
   `curve::alpha_at(state.mode, elapsed)`, `&'static str` with `String`, and
   `pub(super) show` with the three public constructors.
3. Register in `src/services/mod.rs`; migrate the training_mode call sites;
   delete the old module.
4. Run `./scripts/validate_auto_calibration.sh`, then
   `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt` (whole crate),
   `./build.sh`.

## Acceptance Criteria

1. **Training Mode behavior unchanged**
   - Given the training-mode mod with markers being set/cleared
   - When `crate::services::toast::flash("Set beginning marker")` fires
   - Then the toast renders identically to the pre-move behavior (same
     position, scale, colors, 100/250/300 ms envelope)

2. **Flash hold is caller-specified**
   - Given `flash_with_hold("CALIBRATED: 87 -> 93 (+6 MS)", 5000)`
   - When the animation runs
   - Then the toast holds at full alpha for 5000 ms between the standard
     100 ms fade-in and 300 ms fade-out, then hides

3. **Pulse persists until dismissed or superseded**
   - Given `show_pulsing("Calibrating...")`
   - When minutes elapse with no other toast calls
   - Then the toast keeps looping its fade cycle (never self-terminates), and
     a subsequent `dismiss()` or any `flash*` call ends/supersedes it cleanly

4. **Flash curve unit tests (host)**
   - Given `ToastMode::Flash { hold_ms }` for hold 250, 3000, and 5000
   - When `alpha_at` is evaluated at 0, mid-fade-in, hold start/end, mid-fade-
     out, and past the envelope
   - Then alphas match the piecewise-linear envelope and the past-envelope
     result is `None`

5. **Pulse curve unit tests (host)**
   - Given `ToastMode::Pulse`
   - When `alpha_at` is evaluated at 0, 400, 800, 1600, 2000, 2400, 2600,
     2800, and 2800+k·2800 ms
   - Then alphas follow the 800/800/800/400 loop (0.0 at cycle start and in
     the gap, 1.0 across the hold), the function never returns `None`, and
     the curve is periodic

6. **Host harness runs green**
   - Given a non-x86 host with only a cargo toolchain
   - When `./scripts/validate_auto_calibration.sh` runs
   - Then the curve tests compile in the temp crate and pass

7. **Crate builds clean**
   - Given the migration and deletion are complete
   - When `cargo check --target x86_64-pc-windows-msvc` and `./build.sh` run
   - Then both succeed with no references to `training_mode::toast` remaining

## Metadata
- **Complexity**: Medium
- **Labels**: rust, refactor, service-promotion, ui, toast
- **Required Skills**: Rust, render-thread/widget patterns of this codebase
- **Generated By**: code-task-generator 2026-08-26
- **Source Plan**: .agents/planning/2026-08-26-auto-calibration/implementation/plan.md
- **Plan Step**: Step 1: Promote the toast to a shared service with pulse and hold modes
