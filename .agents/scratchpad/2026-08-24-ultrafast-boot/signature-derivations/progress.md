# Progress — task-01 signature-derivations (Step 2)

- [x] `derive_ultrafast_boot()` added to `core/signatures.rs`, called right
      after `find_check_step_data_actor()` in `resolve_all`
- [x] Four decodes from the onUpdate body, module-range validated, soft-fail:
      music_db_global (first `48 8B 0D`, != mgr), variable_bpm_threshold
      (`F2 44 0F 10 05`), find_music_by_mcode (CALL 5 bytes before the
      `C6 80 B0 01 00 00 01` flag write), step_data_release (`MOV RCX,[mgr]`
      whose +7 is `E8`)
- [x] cargo check (win) 0 warnings; cargo fmt; ./build.sh clean
- [x] DEPLOY 2026-08-24 01:49 — all four `[+]` lines at the exact expected
      offsets: music_db_global +0x6F2D78, variable_bpm_threshold +0x393F40,
      find_music_by_mcode +0x1B4290, step_data_release +0x1FF1B0. Boot →
      TITLE_SCREEN, 0 exceptions.
- [x] Cross-version (20260616, static Ghidra): onUpdate = FUN_180032c90;
      FLAG_WRITE landmark unique module-wide @ 0x180032fc0 (in body); MOVSD
      XMM8 first-in-body @ 0x180032dc5. Structural derivations hold.

## Deviations
- None. (Acceptance #2 satisfied by static confirmation of the two
  distinctive anchors on 20260616; the two `48 8B 0D`-based derivations are
  structural and anchored to already-cross-version-resolved
  onUpdate + manager globals.)

Status: Complete (uncommitted — maintainer commits manually)
