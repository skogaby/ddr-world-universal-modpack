# Progress — theme-danger-doubles

- [x] RE check — Ghidra (20260825): DanceDangerActor slot 4 = `FUN_180068ce0`, site A at init + 0xAC,
      a dword `CMP [RDI+0xB4],0` (record skin), `JNZ +7` at match + 0x18; offline AOB unique on all
      five builds; stock `dance_danger0000_v0` has `danger_double`
- [x] `policy::wants_danger_doubles(base, decision)` + test (red first; harness 185, was 184)
- [x] `signatures.rs` (additions only): `ddr_sel_danger_double_skip`, `derive_ddr_sel_danger` (RTTI
      slot-4 range, both strings, `75 07`), `ddr_sel_danger_double_jnz()` accessor
- [x] `danger.rs` (`init` stock-shape check, idempotent `sync(want)`, `restore`); `package_helper.rs`
      (`register_legacy` syncs every registered `dance_danger`; `after_stock` restores); `mod.rs`
      (`init`, `disarm`; module doc Surfaces / scoped patches / degradation)
- [x] Gate: `cargo check` / `cargo fmt` / `./build.sh` clean; harness 185; sweep ALL GREEN (30 gaps,
      all covered by alternates, as before); `ddr_sel_danger_double_jnz` resolves on all five
      builds at the design's file offsets + 0xC00 + 0x18; `shape_diff.py` identical through 0x40
      (`75 07 45 84 ED 4D 0F 45 C7` at the JNZ on every build)
- [ ] Cabinet demo (maintainer)

Status: Complete (uncommitted — maintainer commits manually); cabinet demo pending
