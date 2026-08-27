# Orientation — Quick Logout (PDD Step 2)

Blind-spot pass done **before** the decision register, against the repo as of
`ad468be` (2026-07-27). Purpose: find the unknown unknowns while they are cheap.

## 1. Is this greenfield?

Yes. `docs/quick_logout_research.md` landed in the tip commit; there is no
`.agents/planning/*quick-logout*` directory and no `src/mods/quick_logout.rs`.
Nothing in the research doc has been implemented or cabinet-tested.

**Name collision to keep straight:** `quick-restart-or-fail` already exists and is
about *per-song* restart/fail during GAMEPLAY. The new mod is *per-session* and
song-select-gated. The two are naturally disjoint (different scenes, different
redirect keys) but the names are one word apart, so mod id, log prefix and
README wording should make the distinction loud.

## 2. Everything the mechanism needs already exists in the modpack

The research doc's Mechanism A depends on two `scene_manager` behaviours that are
already shipped and load-bearing for other mods, so neither is new risk:

| Need | Where it already lives | Existing consumer |
|---|---|---|
| Rewrite the scene id the game is about to construct | `scene_manager::add_redirect_once(from, to)` (0-indexed keys) | `quick_restart_or_fail` (key 29), `skip_intros` |
| Make the redirect *stick* (repair `TS+0x68 / m_currentID` after the framework's clobber) | `scene_manager::advance_to_scene_hook` | same |
| The live `TransitionSequence*` to hang a forced transition off | `scene_manager::current_transition_sequence()` | `quick_restart_or_fail` |
| The active gosub child (`TS+0x58`) — the sequence we must call `finish` on | `quick_restart_or_fail::ACTIVE_CHILD_OFFSET = 0x58` | same |
| Edge-detected numpad gestures, scene-independent (polled from the render hook) | `input_manager::on_input_event` + `types::buttons::NUM_*` | `mod_menu` (triple-0), `quick_restart_or_fail` (triple-1 / triple-3) |
| GameWork pointer-global + course-field offset, decoded from matched signature bytes | `premium_free::init` decoding `stage_record_accessor` | `premium_free` |
| Per-side `PlayerWork` resolution (`table[side] → wrapper → PlayerWork`) | derived `player_work_table` signature | `premium_free`, `webui_options`, `power_user_statistics` |
| Native on-screen text for a confirm prompt | `widget_renderer::create_text_widget()` (render thread only) | `power_user_statistics`, `mod_menu`, `hello_world` |
| Operator config block + defaults + read-modify-write | `src/mods/config.rs` (7 existing typed blocks) | most mods |

**Net new game-side surface: one AOB signature** (`agcs::Sequence::finish`) and
one function-pointer call. No new detour, no new service. That is unusually cheap
for this repo, and it means the risk is concentrated almost entirely in *runtime
behaviour of the game's own tail* rather than in our plumbing.

## 3. Findings that change the shape of the idea

### 3.1 A direct jump to TOTAL RESULTS crashes — the summary needs a loader hop

`TotalResultSequence` case 0 dereferences the `scene_result` BM2D package
**without a null check**, and that package is *not resident* on the song-select
screen (the 0-idx 24 loader that runs into song select unloads mask `0x31800`,
which includes `scene_result`'s `0x10000`). So "logout with the summary" cannot be
`finish(child, 33)`; it must route through the 0-idx 29 loader (the only loader
that sets `0x10000`) and then redirect `30 → 32`. Requesting the load manually is
not a shortcut — the load is async and only `LoadingSequence` waits on it.

⇒ There are genuinely **two** shippable behaviours (with-summary and
without-summary), differing in risk, not just in polish. That is a requirements
decision, not an implementation detail (register D1).

### 3.2 The one real unknown is not in gamemdx at all

Whether a *forced* `EAmExitRootSequence` actually performs the logout save
depends on ark's per-side entry-flow scene advancing to `0x1B` (GAMEMODE) in
response to `arkExpireCredit` / `arkEACoinExpire`. All static evidence supports it
(the login sequence is structurally symmetric, and `GameOverSequence` acks scenes
`0x1B`/`0x29` itself when e-amusement is off), but the flow lives in
`arkmdxbio2.dll` and was not analysed. **Failure mode is silent**: both window
actors report the terminal scene `0x42` immediately, the exit sequence finishes in
~1 frame, and the player lands on THANK YOU with no save.

⇒ The plan must front-load a cabinet test of the *save*, separately from the
summary, and the mod must instrument the no-op case rather than assume success.

### 3.3 `score_guard` can silently swallow the save we are trying to trigger

`custom_options_persistence::save_sender` suppresses `savekind == 3` for any side
whose `SESSION_TAINTED` flag is latched (Autoplay used, or a Quick Failure). So
"quick logout on a tainted session" ends the session **without** writing the
profile — correct existing policy, but invisible. This interacts with the feature's
whole purpose (getting the profile/customize write-back to happen on demand).

⇒ Worth surfacing to the operator/player at trigger time, not just in the log
(register D8).

### 3.4 Under Premium Free the summary will be empty

A frozen counter means every play reuses the same per-stage record, and
`premium_free`'s own stale-record fix virginizes it (`mcode = -1`) at every
song-select entry. `TotalResultSequence`'s row builder skips records with
`mcode == -1`. So the marquee "with summary" path shows an **empty** TOTAL RESULTS
after the exact sessions this feature exists to end. Stable, just uninformative.

⇒ Argues for the with/without-summary choice being operator-configurable rather
than hardcoded (register D1/D9).

### 3.5 Two scene-id numbering systems, one off-by-one, in adjacent code

`agcs::Sequence::finish(this, id)` takes a **1-indexed** scene id;
`scene_manager`'s redirect table and `types::scenes` are **0-indexed** (the hook
subtracts 1 on entry and adds 1 back on the call). Mechanism A therefore mixes
both in three adjacent lines: `add_redirect_once(30, 32)` (0-idx) next to
`finish(child, 30)` (1-idx = 0-idx 29). This is a live footgun; the design should
name the convention at every boundary and the code should carry `_1IDX` suffixes.

### 3.6 `types::scenes::scene` has no constants past `RESULTS_DETAIL`

The name map knows 32/33/35, but `pub mod scene` stops at 30 — this feature needs
32/33/34/35. Also, 29/30's *names* are semantically off (29 is the post-song
`LoadingSequence`, 30 is the real `ResultSequence`), yet `quick_restart_or_fail`
depends on those exact values. ⇒ Add the missing constants, add a clarifying
comment, and **do not rename** 29/30.

### 3.7 Course and event modes diverge in the tail

- Course/Dan (`GameWork+0x70 != 0`): `TotalResultSequence` short-circuits and the
  course record has different semantics.
- Event/special (`GameWork+0xD0 ∈ {1,2}`): the tail uses 0-idx **56**, not 32, for
  total results — so a `30 → 32` redirect would force the wrong summary scene.

Both are reachable from scene 25, so a scene gate alone does not cover them.
⇒ Explicit mode gates are required (register D6).

### 3.8 `finish` is safe to call from our input callback

Nothing is freed inside `finish` — it sends one message (`0x201`) and sets tree
flags; the reaper runs once per frame from the main loop between the update and
draw broadcasts. `input_manager::poll` already runs on that frame thread from the
render hook, so there is no thread hop and no `run_on_render_thread` hop needed.
Double-fire is memory-safe but *wrong* (two live `TotalResultSequence` siblings),
so the trigger must be latched — and `advanceToScene` writes `TS+0x68`
synchronously, so `current_scene() != SONG_SELECT` is a free latch.

## 4. Repo conventions this feature must satisfy

- Readiness gates before handing over a build: `cargo check` clean → `cargo fmt`
  (whole crate) → `./build.sh` clean. No unit tests exist; validation is a cabinet
  deploy plus DebugView log observation.
- No panics across `extern "C"`; graceful degradation on missing signatures;
  layout constants decoded from matched signature bytes and **failed closed** when
  they look wrong (`premium_free::init` is the reference implementation);
  `log_*!` macros only; one detour per target function.
- `progress.md` in this planning directory is the live resume point and must be
  updated after each implementation step (AGENTS.md → Custom Instructions).

## 5. What I could not check

- `arkmdxbio2.dll` internals (§3.2) — not analysed in the research doc, not
  analysed here. Stays an assumption until the cabinet test.
- Whether a numpad press reaches `input_manager` while the song-select **options
  modal** is open (`custom_options` takes no exclusive input, but the game's own
  handling of the modal is untested here). Minor: affects only whether the gesture
  can fire from inside the modal.

## Sources

- `docs/quick_logout_research.md` §§1–12 (all game-side claims above)
- `src/services/scene_manager.rs`, `src/services/input_manager.rs`,
  `src/services/score_guard.rs`, `src/services/custom_options_persistence.rs`
- `src/mods/quick_restart_or_fail.rs`, `src/mods/premium_free.rs`,
  `src/mods/config.rs`, `src/types/scenes.rs`, `src/types/buttons.rs`
- `AGENTS.md`, `CLAUDE.md`, `.agents/summary/{architecture,components,interfaces}.md`
