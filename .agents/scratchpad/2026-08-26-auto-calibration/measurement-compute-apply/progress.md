# Progress — task-01-measurement-compute-apply

Status: Complete (uncommitted — maintainer commits manually)
Date: 2026-08-26

- compute.rs: CalibStats/Outcome/compute (MIN_SAMPLES 30, MAX_ABS_MEAN 500, delta=mean.round() then clamp ±1000) + 7 new host tests (14 total green). NOTE: rounds the MEAN (displayed delta) not the sum — keeps toast delta == written delta.
- data_feed.rs: idempotent install + is_installed(); CALIB_SIDE/SUM/SUM_SQ/COUNT tap (grade<=4, side match, relaxed atomics) + arm/reset/take
- calibration.rs: seams filled (arm/reset/take -> autoplay guard -> rate re-check -> compute -> set_offset(0) + 5s CALIBRATED toast + INFO old/mean/count/stddev/new; 3s failure toasts); enable() now gated on data_feed::is_installed()
- timing_offsets init: calls data_feed::install (idempotent, non-fatal)
- cargo check clean; validation script 14/14
- Cabinet verification pending: sign direction (INFO log makes it conclusive)
