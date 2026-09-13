# Idea Honing: Auto-Update

Decision register. IDs are stable; the user accepts/overrides by ID. Ordered by blast
radius (data model / user-visible behavior first, cosmetic last). Findings backing
each recommendation: `research/orientation.md`.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Version identity + "update available" rule | Nothing in the DLL knows its own version today; a wrong rule loops (re-download every boot) or misses hotfix re-uploads | Single source `env!("CARGO_PKG_VERSION")` (bump `Cargo.toml` → `1.2.0`, splash reads it, tags are `vX.Y[.Z]`); rule: installed-stamp digest match ⇒ up to date; else remote > local ⇒ update; else remote == local AND a stamp exists ⇒ update (hotfix); else nothing (never downgrade) | Proposed |
| D2 | Apply model (in-session vs next boot) | Races the boot's readers of `data_mods` and the old build's config writers; decides how many restarts | Split: download/verify/extract to a stage + DLL swap (rename trick) IN-SESSION; `data_mods` overlay + config/CSV merge at NEXT boot step 0 by the NEW DLL, gated on PE identity of the running module == staged DLL | Proposed |
| D3 | `mod-config.json` merge semantics | User-visible config behavior; arrays and per-player caches would be wrong under a naive rule | Recursive add-missing-keys over objects; scalars + arrays are leaves (local wins); `custom_options.p1/p2` opaque leaves; atomic tmp+rename write; enable `serde_json/preserve_order` so key order is stable | Proposed |
| D7 | Which archive entries install, and how | Data loss (CSV edits, runtime caches) if wrong | DLL ⇒ swap; `data_mods/**` ⇒ overlay add/replace, never delete (`_cache/`, `_update/` entries ignored); `mod-config.json` ⇒ D3 merge; `judgement_offsets.csv` ⇒ ROW-merge (add missing codes, keep local rows); `README.md` ⇒ overwrite; anything else ⇒ ignore + WARN; strict path sanitisation | Proposed |
| D4 | Restart prompt behavior | The stated UX; the DLL cannot safely relaunch the game | Persistent bottom banner "Update installed — restart the game to finish" + "hold START (either side) 3 s to close the game now" ⇒ `TerminateProcess`; no self-relaunch | Proposed |
| D5 | HTTP/TLS stack | Binary size, Win7 + Wine behavior, build complexity | WinHTTP via the `windows` crate (`Win32_Networking_WinHttp`): system TLS + proxy, zero new crates, Wine-implemented; TLS 1.2 forced; 5 s connect / 15 s receive; fail-open | Proposed |
| D9 | Toggle + config surface | Operator control; dev-build safety | Mod id `auto-update` ("Auto Update"), MODS-tab toggle, default ON, toggle = next boot; optional operator section `auto_update {repo, connect_timeout_ms, max_archive_mb}`; skipped entirely when `layeredfs.dev_mode` is true | Proposed |
| D10 | Rollback / bad-update recovery | A broken new DLL = game won't boot with the mod | Keep `ddr_world_hook.dll.old` until the new build reaches its READY line, then delete; README "recovering from a bad update" (rename `.old` back); no automatic rollback; staged apply idempotent + resumable (manifest deleted last) | Proposed |
| D13 | Release-process hardening (in scope) | The updater only works if releases are built consistently | `build_release_archive.sh <tag>`: asserts tag == Cargo version, stages from `git ls-files` (runtime dirs can never leak), names the asset by tag; keep shipped `mod-config.json` values neutral (new keys are inherited by every user) | Proposed |
| D6 | Offline / failure posture | Most cabinets are offline or firewalled | Any failure ⇒ one log line (INFO for connectivity, WARN otherwise), zero UI, nothing retried until next boot; no UI at all when up to date | Assumed |
| D8 | Progress UI shape | Cosmetic, tunable | Bottom-center: label (the requested text) + status line ("Downloading 3.2 / 6.4 MB", "Installing…"); 600×12 px bar at y≈690 built from the mod-menu strip texture (track + tinted fill), text-only fallback; `on_frame` poller over atomics; created only once an update is confirmed | Assumed |
| D11 | Threading + containment | Project rules 1/3/4 | One named worker thread, `catch_unwind`, atomics/Mutex status, render-thread widget work only, tmp+rename disk writes, kick from a new post-init latch at the READY line | Assumed |
| D12 | Second restart for texture rebuild | UX expectation | Accepted: updates touching PNGs trip the existing red "REBOOT ONCE" warning on the next boot; pre-warming atlas caches from the stage is a Phase-2 idea | Assumed |
| D14 | Security posture | Executing downloaded code | TLS with system roots, repo pinned by default config, no code signing; GitHub account compromise is out of the threat model | Assumed |
| D15 | Integrity check | Corrupt/truncated downloads | Content-Length match + per-entry ZIP CRC32 (already in the format) — no local SHA-256 needed; GitHub's `digest` string is recorded as the installed asset identity only | Assumed |
| D16 | ZIP reader | Dependency footprint | Minimal hand-written central-directory reader in `core/zip.rs` (stored + deflate via `flate2`, already a transitive dep), host-tested; rejects zip64/encryption/other methods | Assumed |
| D17 | Asset selection | Robustness to naming | First asset whose name ends in `.zip` (prefer the `ddr-world-universal-modpack-` prefix when several); none ⇒ treat as no update | Assumed |

---

## D1 — Version identity + update rule

**Question.** How does the running DLL decide the latest release is "newer"?

**Recommendation.** Introduce `pub const VERSION: &str = env!("CARGO_PKG_VERSION")`
(bump `Cargo.toml` to `1.2.0`; the splash title becomes `format!("… v{}", short(VERSION))`
so the literal `"v1.2"` at `src/lib.rs:624` disappears). Release tags are `vX.Y` or
`vX.Y.Z`, parsed to a numeric triple. Decision procedure (pure, host-tested):

1. `data_mods/_update/installed.json` exists AND its `asset_identity` equals the remote
   asset's identity (GitHub `digest`, falling back to `id + updated_at`) ⇒ up to date.
2. else remote version > local ⇒ update.
3. else remote == local AND a stamp exists (identity differs) ⇒ update — this is the
   "hotfix re-upload under the same tag" case, which is how v1.2 shipped.
4. else nothing — never downgrade; a fresh manual install at the current version (no
   stamp) does not re-download itself.

**Rationale.** Rule 1 breaks the infinite-loop failure mode if a release ships without
the version bump; rule 3 recovers the hotfix case the maintainer already uses; rule 4
protects dev builds and manual installs. Rejected: comparing asset names (the live
asset was hand-renamed `_hotfix`); comparing `published_at` to the DLL's PE timestamp
(fragile across rebuilds; unrelated clocks).

## D2 — Apply model

**Question.** Which parts install while the game is running, and which at next boot?

**Recommendation.** In-session: check → download to
`data_mods/_update/stage/<asset>.zip` → CRC-verified extraction to
`data_mods/_update/stage/tree/` (mirrors the install root) → write
`pending.json {tag, asset_identity, staged_dll_identity}` → DLL swap NOW (rename the
running file to `.old`, move the staged DLL into place; rollback the rename if the
move fails) → prompt. At the NEXT boot, `init()` step 0 (before `config::init()` and
the race-critical LayeredFS install): if `pending.json` exists and the running module's
PE identity (`TimeDateStamp`, `SizeOfImage`) equals `staged_dll_identity`, overlay
`data_mods`, merge config + CSV, overwrite README, write `installed.json`, remove the
stage; otherwise discard the stage with one WARN (the swap didn't take or the user
reverted `.old`). Delete `.old` at the READY line.

**Rationale.** The DLL swap can only be in-session (orientation §6 — a staged swap
costs two restarts). Everything else is safer at boot: no mid-boot reader of
`data_mods` (texture packing, shader synthesis) sees half-replaced inputs; the merge is
performed by the build that owns the new schema before any of its whole-section config
writers run (orientation §10); the running session stays 100 % the old build, so
"restart to finish" is literally true. Alternative considered: apply everything
in-session with per-file atomic renames — simpler code, but inherits both races and
the old build's `save_mod_states`/`persist_section` writers can drop freshly merged
keys before the restart.

## D3 — Config merge

**Question.** Exact semantics of "keys missing locally are added; existing values win".

**Recommendation.** `merge_missing(local: &mut Value, incoming: &Value)`: for each
`(k, v)` in `incoming` (object): absent locally ⇒ insert clone; both objects ⇒ recurse;
otherwise keep local. Arrays are leaves (no element merge — `option_menu_settings`,
`custom_series`, `presets` stay exactly as the user has them; new option ids fall back
to registration order at the end of their menus, the documented behavior). Keys
`custom_options.p1` / `custom_options.p2` are treated as opaque leaves even though they
are objects (per-player caches). The local file is parsed strictly — unparseable local
JSON ⇒ no merge + WARN (never `{}`-and-overwrite). Written via tmp + rename. Enable
`serde_json`'s `preserve_order` feature so the operator's hand-ordered file keeps its
order and merged keys append at the end of their section (today every write
alphabetizes the whole file). Host-tested on fixture pairs incl. the depth-3
`gameplay_timing_fixes.audio_clock` case.

## D7 — File scope

Entry classification (pure, host-tested): `ddr_world_hook.dll` ⇒ swap; `data_mods/…`
⇒ overlay add/replace, never delete (`data_mods/_cache/**` and `data_mods/_update/**`
inside an archive are ignored); `mod-config.json` ⇒ D3 merge; `judgement_offsets.csv`
⇒ row-merge by `code` (missing codes appended, local rows untouched — the same "new in,
yours wins" rule as the config, and the per-song offsets mod already appends missing
basenames itself); `README.md` ⇒ overwrite; anything else ⇒ ignored + one WARN.
Rejected: mirror-delete of `data_mods` (would destroy `_cache/` and the s_marvelous
enable-time `*_ifs/`); CSV overwrite (destroys user edits); CSV install-only-if-absent
(users never receive new community rows).

## D4 — Restart prompt

Persistent bottom banner after the swap: "Update to vX.Y installed — RESTART THE GAME
to finish. Hold START on either side for 3 s to close now." Holding START ⇒
`TerminateProcess` (the smx `touch.rs` precedent — graceful CRT exit wedges under
Wine). The game stays fully playable meanwhile (the session is entirely the old
build). Rejected: self-relaunch via `CreateProcessW(GetCommandLineW())` — spice2x
window/audio teardown under Wine, double-launch with operator restart loops; can be
revisited as an opt-in.

## D5 — HTTP

WinHTTP (`WinHttpOpen` with an explicit `User-Agent`, `WINHTTP_ACCESS_TYPE_DEFAULT_PROXY`
for Win7 compatibility, `WINHTTP_OPTION_SECURE_PROTOCOLS = TLS1_2`, timeouts, GET with
`Accept: application/vnd.github+json` + `X-GitHub-Api-Version`, streamed body read with
`Content-Length` progress). Redirects (the asset URL 302s to the CDN) are followed by
WinHTTP's default policy. Rejected: `ureq`+`rustls` (+2 MB, `ring` under
cargo-xwin/`-Z build-std`, but independent of the OS cert store — the fallback if Win7
cabinets turn out unable to reach GitHub over SChannel).

## D9 — Toggle + config

`mods["auto-update"]` default ON (a cabinet operator can turn it off from the 0-0-0
menu; effective next boot). Optional `auto_update` section, operator-authored only:
`repo` (default `skogaby/ddr-world-universal-modpack`), `connect_timeout_ms` (5000),
`max_archive_mb` (200). `layeredfs.dev_mode == true` ⇒ the check is skipped with one
INFO (dev builds are never clobbered).

## D10 — Rollback

No automatic rollback (needs an out-of-process watchdog). `.old` is retained until the
new build logs READY, then deleted; README documents "rename `ddr_world_hook.dll.old`
back". Boot-time apply is per-file rename-over and idempotent; `pending.json` is removed
last, so a crash mid-apply resumes on the next boot.

## D13 — Release hardening

`scripts/build_release_archive.sh vX.Y`: asserts `vX.Y` == `Cargo.toml` version (the
D1 loop guard at the source), stages via `git ls-files` (never ships `_cache/`,
generated `*_ifs/`, `*.arc`), names the asset `ddr-world-universal-modpack-vX.Y.zip`.
Documented release checklist in the script header. Shipped `mod-config.json`: every new
key's shipped value is what all users inherit — one-time review for neutral defaults.
