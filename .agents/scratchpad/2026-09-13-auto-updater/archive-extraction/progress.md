# Progress — Step 2 Task 01: archive extraction
Status: Complete (uncommitted — maintainer commits manually)
- [x] `relpath.rs` (+4 tests) — validation table, join, parent, serde
- [x] `archive.rs` (+7 tests) — two-pass extract (validate all → empty stage → write), `enclosed_name`, `is_symlink()`, entry/size caps (declared AND produced bytes), `NotAModpack`
- [x] fmt/test: 47 unit + 9 e2e green
Deviations: symlink detection uses `ZipFile::is_symlink()` (the writer masks
mode bits, so the hand-rolled `unix_mode` check was untestable and redundant);
fixtures use `ZipWriter::add_symlink`. `serde`/`serde_json` added here (RelPath
serde) rather than in Task 02.
