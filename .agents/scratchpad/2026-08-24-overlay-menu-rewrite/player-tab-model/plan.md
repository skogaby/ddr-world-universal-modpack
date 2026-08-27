# Plan — player-tab-model (Step 6 task-01)

Status: Approved 2026-08-25 (auto mode — verified chain + in-session
breakdown approval)

## Approach
1. TabId::PlayerSettings + ALL + label + ALL.len() test → 3.
2. RowKind::Scalar.formatted: Option<String> + literal/destructure updates
   (model test helper, tabs.rs convert_kind None, input.rs `..`).
3. NavState.pinned_focus + literal updates (`..NavState::default()`).
4. Navigator.pinned (new_with_pinned; new ⇒ false): selected None while
   pinned-focused; up/down wrap cycle incl. selector at top;
   clamp_after_rebuild parks on selector when pinned + nothing selectable.
5. MirroredRowSnap/MirroredKindSnap + build_player_tab(rows, editable).
6. SelectorState {Free, Locked, AllGated} + editable_sides +
   resolve_selected_side + selector_state.
7. tabs.rs placeholder arm (Vec::new()).
8. Tests: red checkpoint via todo!() on build_player_tab/eligibility; pinned
   nav tests written against the implemented extension; existing 13 nav
   tests must pass UNCHANGED (only NavState literal syntax updated).

## Test scenarios
- Builder: visible omission; kind mapping incl. formatted passthrough;
  greyed-all (editable=false greys non-headers only); header passthrough;
  key/label/description mapping.
- Eligibility: entered None fail-closed; attract band gates both;
  [true,true] Free; one Locked; zero AllGated; resolve: desired editable ⇒
  desired, else single editable, else desired unchanged.
- Pinned nav: UP from first selectable ⇒ selector (selected() None); UP
  again ⇒ last selectable (wrap); DOWN from selector ⇒ first selectable;
  DOWN from last ⇒ selector; all-greyed clamp parks on selector; leading
  header skipped in "first selectable"; non-pinned Navigator::new unchanged.
