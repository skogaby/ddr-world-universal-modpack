# Progress — task-01-hide-overlays-suppress-readouts

Status: Complete (uncommitted — maintainer commits manually)
Date: 2026-08-26

- overlay_element_styling/mod.rs: CALIBRATION_HIDE + pub set_calibration_hide(on)->bool (liveness); checked first in opacity_pct + opacity_pct_fast; cleared in disable()
- power_user_statistics/mod.rs: CALIBRATION_SUPPRESS + pub set_calibration_suppress + pub(crate) calibration_suppressed
- timing_stats_widget::update_text: suppressed early-return before show()
- pacemaker_swap_inner: suppressed => original_esi, no force-visible
- calibration.rs: hide+suppress set on Collecting entry (WARN when styling mod inactive), cleared at exit + disable(); ConsumeOnly never sets them
- cargo check clean; fmt applied; validate script 14/14; ./build.sh release clean
