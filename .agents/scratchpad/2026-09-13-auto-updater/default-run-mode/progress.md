# Progress — Step 4 (consolidated): GitHub feed, verified download, default run mode
Status: Complete (uncommitted — maintainer commits manually)

## Done
- [x] `github.rs` — `Release`/`Asset` (unknown fields ignored), `agent()` (10 s connect / 30 s read, UA), `endpoint`, `fetch_latest` (latest | list+`select_release`), `select_release` (non-draft, newest `published_at`), `select_asset` + `matching_asset_count`, `parse_digest`, `NetError` with operator-readable `Display`; 7 tests on a fixture trimmed from the live v1.2 response.
- [x] `download.rs` — streamed 64 KiB chunks through SHA-256, 10 % progress ticks, pure `verify` (size + digest; `SizeOnly` when no digest), partial file removed on failure; 2 tests (rule table; unresolvable host leaves no file).
- [x] `main.rs::run_default` — Checking → fetch → asset → digest → manifest decision (tag AND digest; tag-only when no digest) → `--check` exit 3 / up to date → release name + notes URL → download with progress → verified → shared `install()`. Placeholder mode + its test removed. `ureq` (rustls) added.
- [x] e2e adjusted: E4/E5/E8/E9 now use `--repo no-such-owner-ddr/no-such-repo-ddr` (deterministic "skipped" whether online (404) or offline). 103 unit + 21 e2e green; host + Win7 builds warning-free; exe 2 446 848 B (rustls), no `ProcessPrng`; imports add `bcrypt.dll` (Vista+) + `WS2_32.dll`.
- [x] **LIVE on the real install (CrossOver):** `--check` → `update available: v1.2 (installed: local:e926b958864c)` exit 3 in 0.3 s. Bare run → downloaded `ddr-world-universal-modpack-20260903_hotfix.zip` (6.1 MB in ~0.5 s), `Verified 6396575 bytes against the published sha256`, 368 extracted, config/CSV `no changes`, **366 written, 3 obsolete files removed** (the S-MFC lamp textures added 2026-09-12 — correct: v1.2 predates them), manifest `v1.2`/asset digest `bf3de838…`, 2.8 s total; re-run → `up to date (v1.2)`. Then restored the maintainer's dev build with `--from-zip <20260913 zip> --tag dev-20260913` (369 written, DLL sha back to the dev build, lamp textures back).

## Findings
- **Dev-cabinet caveat (README material for Step 6):** on a cabinet running a
  NEWER local build than the latest published release, a bare updater run
  DOWNGRADES to the published release (equality semantics, by design). The
  maintainer's install must not run the updater from `gamestart.bat` until the
  dev build is published — or the maintainer uses `--from-zip` for local builds
  and keeps the bare run out of the bat.
- Pruning was exercised on real data for the first time (3 files) and behaved.
- Under Wine the whole check-and-update took 2.8 s; an up-to-date check 0.3–0.4 s
  of updater time (≈ 6 s wall including Wine process start-up).

## Deviations
- The manifest decision in `run_default` compares tag AND digest when GitHub
  publishes a digest, tag only otherwise (design R6 assumed the digest is
  always present).
