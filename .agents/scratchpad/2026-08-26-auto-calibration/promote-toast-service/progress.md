# Progress — task-01-promote-toast-service

Status: Complete (uncommitted — maintainer commits manually)
Date: 2026-08-26

- src/services/toast/curve.rs: pure ToastMode {Flash{hold_ms}, Pulse} + alpha_at, 4 host tests
- src/services/toast/mod.rs: promoted service (String text, flash/flash_with_hold/show_pulsing/dismiss)
- src/services/mod.rs: registered
- training_mode: 3 call sites -> toast::flash, dismiss -> services path, toast.rs deleted, doc ref fixed
- scripts/validate_auto_calibration.sh: temp-crate harness, green (4 tests)
- cargo check --target x86_64-pc-windows-msvc: clean
