# Research: CAUTION-screen (scene 21) slowdown after preview-image volume increase

**Status:** ✅ FIXED + cabinet-verified (2026-06-11). Root cause identified from
logs + code; in-memory `_cache` index implemented; maintainer confirms the CAUTION
screen now loads reliably fast (no 90/10 nondeterminism).

## Implementation (landed)

All in `services/avs_layeredfs/`:
- `ifs_textures.rs` — new `CACHE_INDEX: Mutex<HashSet<String>>` + `build_cache_index()`
  (recursive `read_dir` of `CACHE_FOLDER` at init), `cache_has()`, `cache_index_insert()`.
  `handle_texture` now calls `cache_has(&cache_file)` instead of
  `Path::exists()` (the hot-path syscall). Index kept live on every cache write:
  `cache_texture` (write + the metadata-revalidation "fresh" branch) and
  `inject_new_textures`.
- `atlas_cloner.rs` — inserts cloned-atlas cache files into the index after write
  (they're written at mod `enable()`, after the init scan, so a cold-cache run
  would otherwise miss them).
- `mod.rs` — calls `ifs_textures::build_cache_index()` once in `init()`, right after
  `init_mod_paths()`.

Scope decisions:
- Only the **texture** hot path was converted (2,327 of 2,343 hot opens). The
  `file_hooks.rs:274` merged-texturelist `exists()` was **left as a real stat** —
  only 24 opens/load, and it can be written at runtime by `parse_texturelist`, so a
  static index risks a false negative for negligible gain.
- `/geo/` + `/afp/` opens were already in-memory-clean (`AFP_MAP` + in-memory
  `find_first_modfile`) — untouched.

Self-healing: if a cache file is ever on disk but not indexed, the first
`handle_texture` falls through to `cache_texture`'s freshness check, which inserts
it — so subsequent opens hit the index. No correctness dependence on the init scan
being exhaustive.

## Diagnostic on deploy

`build_cache_index` logs `LayeredFS: cache index built — N file(s) under
./data_mods/_cache` at boot — confirms the index populated (N should be in the
thousands for the heavy preview set).

## Symptom

After expanding WebUI-options preview images from ~20–30 to several hundred, the
CAUTION screen (scene 21) takes 20–30 s to advance ~90% of runs, but ~10% of runs
it's fast (a few seconds) — **with the same assets and same `_cache/` across game
reboots** (no asset changes between runs). Previews always render correctly either
way. Maintainer also recalls heavy AVSLZ log spam when first added (later gated).

## Quantified from log (`log.txt`, a slow run)

- Scene 21 entered `03:27:30`, left `03:27:49` → **~19 s** stuck.
- The window spans ~5,400 log lines: **4,380 `avs_fs_open` calls**, 2,327 distinct
  asset hashes (mostly opened once — no re-open loop).
- Of the window opens: **2,343 `/tex/`**, 1,608 `/geo/`, 338 `/afp/`.
- Open *rate* is NOT uniform: bursts at start (~810/s, ~691/s) and end
  (~397→1022→656/s), with a ~14 s **slow middle trickling ~40–60 opens/s**. During
  the slow middle the log contains *only* LayeredFS open/using lines.
- **AVSLZ/recompression is NOT happening this run** (only ~18 compress-ish lines
  total; the cold-cache compression the maintainer recalls was a first-run, now
  cached). So the slowdown is not re-conversion.

## Root cause: per-open filesystem `exists()` stats on the texture path

The per-`open` hot path (`file_hooks.rs::find_mod_replacement`) for a `/tex/<hash>`:
1. `find_first_modfile` — **already in-memory** in non-dev mode (checks
   `STATE.mods[].files: BTreeSet<String>` built at init; the `file_exists()` syscall
   branch is dev-mode only). ✓ not a syscall.
2. `ifs_textures::handle_texture(norm_path)` → `TEXTURE_MAP.lock()` then
   **`std::path::Path::new(&cache_file).exists()`** — a **filesystem stat syscall on
   every texture open** (`ifs_textures.rs:285`). Returns the cache file if present.

So each of the **2,343 texture opens does a real `exists()` syscall**. With
hundreds of mapped preview textures, that's hundreds–thousands of stats per CAUTION
load. **Filesystem stat latency varies with OS file-cache warmth** → cold cache
(~90% of reboots) = slow; already-warm (~10%) = fast. This matches the 90/10
nondeterminism far better than any fixed-work explanation, and explains why it's
volume-sensitive (more previews → more mapped textures → more per-open stats).

Secondary contributor: **verbose logging** issues an `OutputDebugStringA` per open
(4,380 calls). The committed `mod-config.json` has `verbose:false`, but this run had
it on ("LayeredFS: verbose logging enabled"). Not the root cause, but additive when
on, especially with a debugger/DebugView attached.

`/geo/` and `/afp/` opens are already in-memory-clean (`handle_afp` →
`AFP_MAP.lock()` + in-memory `find_first_modfile`, no per-open stat), so they are
NOT offenders. The fix is scoped to the texture path.

## Whose bug: ours

The game opens these assets regardless, but the **per-open `exists()` stat is our
hook's** added cost. Stock (no DLL) doesn't pay it. So it's ours to fix; the volume
increase exposed a latent O(opens × stat-latency) cost.

## Fix (designed — maintainer's proposal, refined)

Build an **in-memory index of the `_cache/` directory at init** and consult it
instead of the per-open `Path::exists()` syscalls. Refinements from the code:

- The gap is **`_cache/`, not `data_mods/`** — `data_mods` source files are already
  indexed (`STATE.mods[].files`); only the `_cache/<ifs>/<hash>` existence checks hit
  the filesystem per open.
- Replace the two per-open cache `exists()` checks with index lookups:
  `ifs_textures.rs:285` (texture cache) and `file_hooks.rs:274` (merged
  texturelist).
- **Keep the index live:** `cache_texture` writes new cache files at runtime (cold
  first run) — insert into the in-memory index after each successful write, so a
  freshly-cached texture is found on its next open without a stat.
- Build the index with one `read_dir` per cached IFS folder under
  `./data_mods/_cache/` at init (mirrors `scan_mod_folders`), guarded by a lock like
  the other LayeredFS state.
- (Optional, cheap) ensure verbose defaults off / note the per-open log cost.

Net: the 2,343-per-load texture-open stats collapse to in-memory set lookups,
removing the OS-cache-warmth-dependent latency and the 90/10 nondeterminism.

## Verification plan

After the fix: re-run CAUTION several times across reboots; the slow-middle trickle
should disappear and load time should be consistently fast (no 90/10 split). Confirm
previews still render and a cold `_cache` (deleted) still regenerates + then loads
fast on subsequent runs (index updated on write).
