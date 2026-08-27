# Context — observer-snapshot (Step 5 task-03)

## Provenance (verified)
- Task: `.agents/tasks/2026-08-24-overlay-menu-rewrite/step05/task-03-observer-snapshot.code-task.md`
  (Generated-By present; breakdown + press-path/no-prime/± decisions
  user-approved in session). Plan/design Approved. Depends on tasks 01+02
  (both Complete). Mode: auto.

## Verified facts
- Facade dispatch sites (mod.rs): set_value ~:305-315 (id in scope),
  resolve_from_load ~:325-350, set_value_silent ~:360-370 (currently
  discards tuple), reset_session_values ~:389-420 (tuples carry id),
  set_scalar_bounds ~:423-450 (tuples carry id).
- Press path: rows.rs press_body — after `cb(side, value);` the code has
  `option_id` bound (`let (cb, side, value, option_id) = dispatch;`);
  observer dispatch inserts right there, before update_children_visibility.
- is_show_when_satisfied: rows.rs free fn (3 call sites incl. itself);
  moves to a `FrameworkState::show_when_satisfied` method.
- registry.rs has NO crate:: imports (mountable); OptionHandle field
  pub(super) — visible crate-wide when api.rs mounts at harness root.
- ordering::display_order_for(&[&str], &[bool]) -> Vec<usize> and
  placement_override_for(id) -> (in_game, overlay) from task-01.
- api::format_scalar_value_utf8 + prettify_id/prettify_texture_suffix from
  task-02; static `Mutex::new(Vec::new())` precedent in chrome_loader.

## Interpretations (auto mode record)
- Bool detection = the exact bool_toggle signature: 2 enum values 0/1 with
  textures seop_op_off/seop_op_on (custom 2-value enums stay Enum).
- Snapshot composition is a pure registry fn taking injected
  `overlay_override: &dyn Fn(&str) -> Option<bool>` and
  `order_for: &dyn Fn(&[&str], &[bool]) -> Vec<usize>` — mod.rs facade
  passes ordering's real fns; tests inject synthetics. Candidates filter =
  available && resolved overlay (override wins), THEN order (identical
  composition shape to builder_hook's in-game path).
- Observer tests serialize via a tests-module lock (shared static subscriber
  list); each test unsubscribes on exit.
- observers list: `Mutex<Vec<(usize, Arc<dyn Fn>)>>` + AtomicUsize tokens;
  dispatch snapshot-clones then calls outside the lock, catch_unwind per
  subscriber (latched WARN).

## Gates
harness (api+registry+observers+ordering) → validate_mod_menu → check → fmt
→ build → boot regression (inert: no subscribers exist yet).
