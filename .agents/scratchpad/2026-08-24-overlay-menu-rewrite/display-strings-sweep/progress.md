# Progress — step09 task-01 display-strings-sweep

- [x] Fresh site enumeration (23 register_option call sites → 41 rows; matches survey)
- [x] Sweep: .display_name + .description on ALL 41 rows (16 files, 38 count-asserted
      hunks via python; display names = the canonical `scripts/option_strings.py` "en"
      labels; descriptions agent-authored one-liners)
- [x] Enum display labels: perspective (OVERHEAD/HALLWAY/DISTANT via .display_label on
      with_preview), training_progress_pos (EnumValue::new → with_display OFF/LEFT/RIGHT),
      customize_movie_size (build_fixed_enum_values adds .display_label(key.to_uppercase()))
- [x] webui cosmetics: .display_name(def.display_name) + shared description +
      .in_game_only() (10 categories); profile rows (is_disp_weight/weight) stay
      both-menus with explicit strings; headers → (id, display, desc) tuple table
- [x] Lint: grep-based leg appended to validate_custom_options.sh (per-call-site
      60-line window scan for .display_name/.description; bare EnumValue::new
      forbidden outside the service) — proven red on a mutated autoplay.rs, green after
- [x] Gates: harnesses 3/3 OK (incl. new lint), cargo check 0 warnings, fmt, build.sh
- [x] Cabinet: rebuild boot — 41/41 `custom_options: registered`, 0 new WARNs;
      overlay opened + GLOBAL/PLAYER tab screenshots archived in shots/ (visual
      verdicts ride the maintainer's final walkthrough)

Status: Complete (uncommitted — maintainer commits manually)

## Feedback round (2026-08-25, maintainer walkthrough) — Title Case labels

Maintainer style rule: ALL CAPS is reserved for header rows, tab labels,
and enum VALUE labels; option-row LABELS are Title Case. Applied (30
count-asserted hunks): all 27 non-header display_names title-cased
("SONG PLAYBACK SPEED" → "Song Playback Speed", etc.), plus the three
APPEARANCE-tab built-ins in model.rs ("Theme"/"Animated Background"/
"Menu Opacity" — value labels like RHYTHM/OFF/ON stay caps); the 4
decorative headers stay ALL CAPS; webui cosmetic display_names left as-is
(in_game_only ⇒ never rendered by the overlay). model.rs theme_tab_rows
test updated; the lint's chain window widened to i+8 (rustfmt splits
header chains BELOW the register_option call line). Convention recorded
in api.rs::display_name docs. Harnesses 3/3, check 0 warnings, build
clean, cabinet-verified by the maintainer: "Everything looks great."

Status: Complete (uncommitted — maintainer commits manually)
