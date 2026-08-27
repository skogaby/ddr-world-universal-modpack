# Progress: rows-and-seeding

Updated: 2026-08-18
Status: Complete (uncommitted — maintainer commits manually); cabinet demo pending

- New ui.rs: row registration (parent bool_toggle + child scalar -100..100
  step 1/10, ShowWhen::Equals, BOTH rows explicitly PersistMode::None —
  caught during self-review: the builder defaults are PersistMode::Full,
  which would have leaked mod_adjust_song_offset / mod_current_song_offset
  s32s onto the wire; design D9 requires None).
- Wheel poll via input_manager::on_frame: scene-25 gate, selectmusic_model
  weak_ptr walk (+0x1B0/+0x1B8, ctrl strong count), guarded inner vtable
  getter for the code (music_wheel_song_length shape); selection change →
  seed rows silent both sides; CURRENT_CODE published for on_change +
  later steps (ui::current_code()).
- Edit capture: parent ON → set_entry(child value) + CSV upsert; parent OFF
  → clear_entry + blank upsert; child change → set_entry gated on parent ON.
  All handlers gated on ROWS_READY + is_active + current code present.
- mod.rs: init stores signature addrs via ui::init; enable = row_injection
  gate → ui::enable() → bootstrap::start() → MOD_ACTIVE.
- Validation: check clean (0 warnings), harness 23/23, release build clean.
- Cabinet demo (deploy #2): negative render, seeding, live show/hide,
  persistence, versus independence — PENDING.

Status: Complete (uncommitted — maintainer commits manually)

## Addendum 2026-08-18: ScalarFormat::SignedUnit (stock ms-unit parity)
- Maintainer requested stock-parity value text ("-41ms" / "+10ms" / "±0ms",
  per the DISPLAY/JUDGMENT TIMING rows). Ghidra (gamemdx 20260721,
  FUN_18016e4e0): stock uses format "%+dms" for nonzero and SJIS bytes
  81 7D ("±") + "%dms" for zero.
- Framework extension: new ScalarFormat::SignedUnit { unit } variant
  (api.rs — the doc comment there IS the how-to for embedding units in any
  scalar row); format_scalar_value now returns Vec<u8> (raw display bytes —
  the SJIS ± is not valid UTF-8; the pipeline feeds the game's SJIS-native
  string::assign/BmpString compositor directly, so bytes pass through).
- current_song_offset switched to SignedUnit{"ms"}; label texts dropped the
  "(MS)" suffix; textures regenerated. Cabinet re-check of the value render
  added to Deploy #2 leftovers (glyph existence proven by the stock rows
  rendering the same chars through the same compositor).
