# Orientation — S-Marvelous Judgement

Date: 2026-08-29. Findings pass before requirements clarification. The RE
foundation is `docs/s_marvelous_judgement_research.md` (complete, current-session
deep dive); this file records the *codebase-side* verification of the
infrastructure that research assumes, plus the constraints that shape the
decision register.

## 1. Architecture baseline (from the research doc)

`docs/s_marvelous_judgement_research.md` §5.3/§11 recommends **Option C**:
engine grade space untouched; S-Marvelous := `judge_code == 0x1028 (Marvelous)
&& |Δms| ≤ 12`, classified in a post-original `judge_submit` detour; all
player-visible discreteness (counter, gameplay flash, results row) implemented
mod-side. Score/EX/gauge/combo/MFC/save/ghost stay bit-identical to stock.
Option A (native 9th grade) is documented as structurally hostile (§5.1:
message-code collision `0x1028+8 == 0x1030`, four closed 8-slot counter arrays
with live neighbors, closed server schema, ghost-stream semantics). Option B
(insert-and-shift) is excluded by requirement and by the first-match walk
(§5.2).

## 2. Verified infrastructure seams (all shipped, none speculative)

### 2.1 Judge event + ms delta — `src/mods/power_user_statistics/data_feed.rs`

- **No subscriber API exists** on the `judge_submit` detour. The hook body
  (`judge_submit_hook`, ~lines 235–314) hardwires its consumers; the shipped
  precedent for an external feature tapping it is the **calibration tap**
  (lines 146–187): armed/disarmed via static atomics, disarmed cost = one
  relaxed load, policy lives in the consuming mod. S-Marvelous classification
  goes in as a sibling block (per-side count atomics), preserving the
  one-detour-per-target rule.
- **Marvelous delta confirmed available**: opcode `0x1028` ⇒ `grade_index 0`;
  `ms_error = *(scratch + 4)` (line 257), present for all grades except
  freeze-OK. The `|ms_error| <= 12` test is a two-instruction addition next to
  the calibration tap (line ~263).
- Install is idempotent (`install()` returns true if already installed; both
  PUS and timing_offsets call it), so S-Marv classification works even with the
  PUS mod disabled — same guarantee calibration relies on.
- Side comes from `*(actor + 0x84)`; hot path uses `try_lock` for buffers but
  atomics for taps (contended-lock samples are dropped — atomics are the safe
  choice for counters).

### 2.2 Per-frame / per-judgement hooks

- `src/services/judge_hook.rs` is **frame-level** (judgeNotes, once per frame
  per actor, no grade/delta) — not useful for classification; per-judgement
  data exists only in data_feed's judge_submit detour.
- `src/services/input_manager.rs::on_frame` (line ~1166) is the general
  per-frame driver (Arc<dyn Fn>, panic-contained, render/game thread) — the
  natural animation driver for a flash widget. The toast service
  (`src/services/toast/`) shows the alternative self-requeueing
  render-thread-tick pattern with a generation token.

### 2.3 Judgement flash placement — `src/mods/overlay_element_styling/`

- `capture.rs` classifies `"dance_judge"` clips per side into a 64-slot
  registry: wrapper ptr, AFP `layer_id`, side binding, original x/y anchor.
  All accessors are `pub(crate)` — reachable from a new mod (same crate), but
  the clean precedent is a small `pub fn` on `overlay_element_styling/mod.rs`
  (the `set_calibration_hide(on) -> bool` pattern, line ~115, which returns
  enabled-state so callers fail open).
- `set_calibration_hide` forces both opacity accessors to 0 while set — the
  existing mechanism for hiding all judgement feedback (calibration consumes
  it). Registry is game-thread-only, deliberately lock-free.
- Note: `pacemaker_swap` does NOT use this registry — it's an independent
  inline patch; its PUS-side analog is `set_calibration_suppress`.

### 2.4 Reset discipline — `src/services/song_reset/mod.rs`

- `on_song_reset(cb: impl Fn(i32)) -> usize` (line ~1057): fires on the frame
  thread after every completed in-place reset (quick restart, training
  scrubs/loops) — cases where scene 28 never exits. Per-song counters need
  BOTH this and a `scene_manager::on_scene_change` reset at GAMEPLAY entry
  (the exact pattern PUS `mod.rs` lines ~143–172 ships).

### 2.5 Widgets + scene gating

- `widget_renderer::create_text_widget()` / `create_image_widget(cfg)`;
  `run_on_render_thread(f)` for creation/mutation. **Render-list nodes are
  permanently consumed** (`destroy()` only hides) — create once, show/hide
  forever after (timing_stats_widget/toast/autoplay pattern).
- Scene constants (`src/types/scenes.rs`): `GAMEPLAY = 28`,
  `STAGE_RESULT = 29` (naming trap: this is the post-song *loader*),
  `RESULTS_DETAIL = 30` (the real per-stage results UI),
  `FINAL_RESULTS = 32`.
- **Results-scene widgets are precedented**: the autoplay watermark
  (`src/mods/autoplay.rs` ~141–243) keeps a TextWidget visible across scenes
  28/29/30 — proving mod widgets render fine on the results detail scene. No
  shipped mod renders per-stage *stat* text there yet; the pattern (show at
  next==30, hide when leaving {28,29,30}) is proven.

### 2.6 PUS display/CSV integration points

- `timing_stats_widget.rs`: adding a line = extend one `write!` format
  (~line 167) + placeholder (~line 43); gated per side on the `timing_stats`
  option.
- `csv_export.rs`: `StepRecord` (data_feed.rs ~67–71) currently does NOT store
  grade per step; adding an S-Marv/grade column means extending StepRecord +
  header/row format (~line 143).

### 2.7 Option row + per-song latch

- Bool row: `RegisterSpec::bool_toggle("s_marvelous")` defaults to
  `PersistMode::Full` ⇒ wire field `mod_s_marvelous` (server column needs a
  bemani-buddy migration; JSON cache carries it until then). Label textures
  `seop_item_<id>` + `seop_image_<id>_{off,on}` must ship as PNGs
  (`scripts/gen_option_labels.py`) — a DLL-only deploy leaves the row blank.
- Per-song latch at GAMEPLAY entry (playfield_styling/assist_tick pattern):
  on_change writes live per-side atomics; scene callback snapshots to latched
  atomics at next==28 so mid-song edits can't split a song's classification.

### 2.8 Net-new art

- `atlas_cloner::generate_cloned_atlases_xml_fresh` (FRESH mode) is the shipped
  path for net-new textures (music_wheel_song_length recipe: PNG under
  `data_mods/<mod>/<ifs>_ifs/tex/`, donor supplies encoding only, ~40 lines at
  enable). Resulting texture resolves by name — usable from an `ImageWidget`.
- The C-afp variant (edit `dance_judge0000_v0.arc` timeline, add
  `in_smarvelous` label) needs AP2 timeline tooling (bemaniutils-class) — the
  only genuinely new *tooling* in the whole feature (research §6.2 row 3).

### 2.9 Mod registration

- `src/mods/mod_trait.rs`: id/name/description/required_signatures/init/
  enable/disable (+ optional is_active, early_apply). Register in
  `src/lib.rs` mods_to_register vec. `required_signatures` for the core =
  `judge_submit` (+ `judge_notes` transitively via data_feed) — all shipped.

## 3. Constraints that shape the decision register

1. **Classification location is effectively fixed** (data_feed hook body,
   calibration precedent) — one-detour rule; no real alternative.
2. **The flash has a fidelity fork** (C-widget vs C-afp) that trades AP2
   tooling work against native look — user decision.
3. **Results-row exclusivity** (MARV shown as stock−S or inclusive) decides
   whether stock results widgets must be covered/re-rendered — user decision
   with real effort delta.
4. Marvelous never counts FAST/SLOW (stock gate) ⇒ S-Marv inherits — do not
   "fix".
5. Autoplay ⇒ Δ≈0 ⇒ all S-Marv; already score-tainted; display simply accurate.
6. Rate play: windows are content-time ms; ±12 scales identically with stock —
   no special handling.
7. Calibration hide (`set_calibration_hide`) suppresses all judgement feedback
   — the S-Marv flash must respect it or calibration's D18 guarantee breaks.
