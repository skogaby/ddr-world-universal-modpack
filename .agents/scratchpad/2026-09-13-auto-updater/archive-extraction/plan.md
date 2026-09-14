# Plan — Step 2 Task 01: archive extraction
Status: Approved 2026-09-13 (auto; approved chain)
Tests: RelPath table (accept `a/b.png`, `x`; reject ``, `/a`, `a//b`, `a/../b`, `./a`, `a\b`, `C:/a`, `..`); extract nested zip → files + on-disk; `../evil` → UnsafePath; `/abs` → UnsafePath; `C:\x` → UnsafePath; symlink entry → SymlinkEntry; entry cap (use a small cap constant injectable via `extract_with_limits`); size cap (declared sizes); no DLL → NotAModpack; stage root pre-populated junk is emptied.
Shape: `Limits { max_entries, max_total_bytes }` with `DEFAULT_LIMITS`; `extract(zip, stage) = extract_with_limits(zip, stage, DEFAULT_LIMITS)`; two passes: validate all names/modes/sizes first (no writes), then extract.
