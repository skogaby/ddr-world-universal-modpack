# Technologies: Auto-Updater

## Build probe — Rust exe for the Win7 target with TLS (2026-09-13)

Question: can a standalone Rust binary using `rustls` (ring provider) + `ureq` +
`zip` + `serde_json(preserve_order)` + `sha2` be cross-compiled with the SAME
recipe the DLL release build uses (`cargo xwin build --release --target
x86_64-win7-windows-msvc -Z build-std=std,panic_abort`, nightly from
`rust-toolchain.toml`)?

Prototype: `prototypes/win7-tls-probe/` (throwaway; not product code).

Result — **yes, and it runs**:

| Check | Outcome |
|-------|---------|
| Cross-build (macOS host, cargo-xwin 0.21.4) | Clean, 28 s incremental (std already cached from the DLL build); `ring 0.17.14` compiled and linked with no nasm/cmake |
| Output | `win7-tls-probe.exe`, 2.0 MiB with `lto = true`, `opt-level = 2` |
| Win7-hostile imports | **None**: no `ProcessPrng` / `bcryptprimitives.dll`; RNG via `SystemFunction036` (`RtlGenRandom`, advapi32) exactly like the DLL |
| Import table | `ADVAPI32`, `KERNEL32`, `ntdll`, `WS2_32`, `bcrypt.dll` (Vista+; fine on Win7), UCRT `api-ms-win-crt-*` (same UCRT set the hook DLL already imports — cabinets running the DLL have it) |
| Live run under CrossOver (`bemani` bottle) | `GET https://api.github.com/repos/skogaby/ddr-world-universal-modpack/releases/latest` succeeded over TLS with rustls + bundled `webpki-roots`; parsed `tag_name = "v1.2"`, asset name/size/`digest`; `zip` crate linked; exit 0 |

Implications for the design:
- **HTTP/TLS**: `ureq` 2.x (`default-features = false, features = ["tls","json"]`)
  → rustls + ring + webpki-roots. Independent of the OS TLS stack (unpatched Win7
  schannel has no TLS 1.2 by default; GitHub requires ≥1.2) and of the OS root
  store. Timeouts are per-connect / per-read (`AgentBuilder::timeout_connect` /
  `timeout_read`). A `User-Agent` header is REQUIRED by the GitHub API.
- **Zip**: `zip` 2.x with only the `deflate` feature (the release archive is
  produced by Info-ZIP `zip -r`, deflate only). Extract via `ZipArchive` with
  `enclosed_name()` to reject path traversal.
- **JSON**: `serde_json` with `preserve_order` (IndexMap-backed) so the merged
  `mod-config.json` keeps the user's key order and appends new keys at the end
  of their parent object. Pretty print = 2-space indent (matches the DLL's
  writer).
- **Hashing**: `sha2` for verifying the downloaded asset against the API's
  `digest` field and for the install manifest's per-file hashes.
- Optional hardening not yet probed: `-C target-feature=+crt-static` to drop the
  UCRT dependency from the exe (the DLL already needs UCRT on the cabinet, so
  this is belt-and-braces, not a requirement).

What the probe does NOT prove: execution on a physical Win7 machine (no Win7
host available in this session). The import-table evidence above is the same
evidence the DLL's Win7 build rests on.

## GitHub Releases API facts used

Source: live responses from
`https://api.github.com/repos/skogaby/ddr-world-universal-modpack/releases`
(2026-09-13).

- `GET /repos/{owner}/{repo}/releases/latest` → newest non-draft,
  non-prerelease release. Fields used: `tag_name`, `name`, `published_at`,
  `html_url`, `body` (Markdown changelog), `assets[]`.
- `assets[]` fields used: `name`, `size`, `digest` (`"sha256:<hex>"`, present
  on every asset of every release so far), `content_type`
  (`application/zip`), `browser_download_url`.
- Asset naming is `ddr-world-universal-modpack-<datecode>[_suffix].zip`; the
  datecode is not derivable from the tag → select the asset by
  `name.starts_with("ddr-world-universal-modpack-") && name.ends_with(".zip")`.
- Unauthenticated limit: 60 requests/hour/IP; the download URL is served by
  `github.com` → `objects.githubusercontent.com` (302 redirect, follows
  automatically in ureq) and is not API-metered.
- Draft/prerelease releases are invisible to `/latest` → the maintainer can
  stage a release as a prerelease for testers without pushing it to cabinets.
