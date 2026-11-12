# Research: hosting, downloads, release identity

Topics the design depends on. Sources are cited inline; probes were run 2026-11-11.

## 1. GitHub release limits

- Each asset under 2 GiB; up to 1000 assets per release; no limit on a release's total
  size or on bandwidth
  (https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).
- With 23 packs and content-addressed names, the `content` release gains at most 23
  assets per full re-port — hundreds of revisions fit.

## 2. Keeping the `content` release out of `/releases/latest`

> Superseded 2026-11-11: the pack files moved to a separate repository
> (`skogaby/ddr-world-universal-modpack-assets`), whose "latest release" nobody queries.
> The section is kept for the record of why.

The installed v1.3 / v1.4 updater asks `/releases/latest` and skips the run when that
release has no `ddr-world-universal-modpack-*.zip` asset. If the shelf ever became
"latest", every old cabinet would stop updating until that was corrected (they resume on
their own afterwards — the skip is harmless per run).

GitHub documents two rules that do not obviously agree
(https://docs.github.com/en/rest/releases/releases):

- `make_latest` on create/update: "Specifies whether this release should be set as the
  latest release for the repository. Drafts and prereleases cannot be set as latest.
  Defaults to true."
- `GET /releases/latest`: "the most recent non-prerelease, non-draft release, sorted by
  the `created_at` attribute. The `created_at` attribute is the date of the commit used
  for the release, and not the date when the release was drafted or published."

Guards that satisfy both readings:

1. Create the shelf with `make_latest=false` (`gh release create content --latest=false`).
2. Point its tag at the repository's OLDEST commit, so its `created_at` precedes every
   modpack release.
3. Publish it before v1.5 and never re-publish it, so it is also never the newest by
   `published_at` (the rule the `--include-prerelease` path of the old updater uses;
   uploading assets does not change `published_at`).
4. `scripts/publish_packs.sh` ends by fetching `/releases/latest` and failing loudly if
   the answer is the shelf.

The new updater additionally ignores releases without a base asset when it selects from
the pre-release list. A separate repository would remove the risk outright; it was
considered and not chosen (register D3).

## 3. Ranged downloads (resume)

Probe against the v1.4 asset with `Range: bytes=1000-1999`:

```
HTTP/2 302   location: https://release-assets.githubusercontent.com/…
HTTP/2 206   accept-ranges: bytes
             content-range: bytes 1000-1999/26626795
             etag: "0x8DF1C4ED77BFA60"
```

- Release downloads redirect to a signed URL that honours `Range` and returns an `ETag`.
- The updater uses `ureq` 2.12.1 (`updater/Cargo.lock`), which follows redirects and
  reuses the request's headers on the way, dropping only `content-length`,
  `authorization` and `cookie`. **Observed 2026-11-11** (plan Step 1): the ignored test
  `live_range_resume_across_github_redirect` in `updater/src/download.rs` got a `206`
  for a ranged request to the v1.4 asset and resumed it to a file matching GitHub's
  published digest. No manual redirect handling is needed.
- A resumed transfer is only safe against the same bytes. Pack assets are immutable
  under their content-addressed name, and the catalog carries the zip's sha256, which is
  verified over the whole file after the last byte; a mismatch discards the `.part` and
  starts over.
- The current code hashes while streaming (`updater/src/download.rs`); resuming means
  hashing the kept `.part` first (or re-hashing the finished file).

## 4. Pack identity

- `scripts/pack_custom_models.py` writes arcs with no timestamps: members sorted by
  name bytes, fixed 64-byte alignment, no compression. Packing the same folder twice
  (with file mtimes changed in between) produced byte-identical output for all 12 files
  of `stages/X & X2`.
- **Host dependence found.** The model folders contain 845 text sidecars
  (`chara_resources.rlist.txt`, `map_resources.rlist.txt`, `dynbones.txt`, `flight.txt`,
  …), all LF in the repository. `.gitattributes` pins line endings for one `.bat` only,
  so a Windows checkout with `core.autocrlf=true` would turn them into CRLF and every
  pack would get a different content id there. The design needs
  `data_mods/custom_models/** -text` in `.gitattributes` (or publishing from one host
  only).
- A rebuilt zip is not byte-stable (timestamps, zlib version), which is why identity is
  the content id over extracted files, and why the catalog records the PUBLISHED asset's
  size and sha256 rather than the local rebuild's. The REST release object exposes both
  per asset (`size`, `digest: "sha256:…"`), readable without authentication.

## 5. Sizes

Measured by packing and zipping (`zip -9`) five sources:

| Source | Packed | Zipped | Ratio |
|---|---|---|---|
| dancers/3rd MIX | 77 M | 23 M | 29 % |
| dancers/SUPERNOVA 1 & 2 | 55 M | 19 M | 35 % |
| dancers/WINX CLUB | 93 M | 32 M | 35 % |
| stages/SUPERNOVA 1 & 2 | 92 M | 5.5 M | 6 % |
| stages/X & X2 | 19 M | 3.8 M | 20 % |

Extrapolated: all 23 packs ≈ 1 GB to download, ≈ 3.3 GB on disk; largest pack
(dancers/HOTTSTPARTY 1-3) ≈ 190 MB zipped, 570 MB on disk. The Hottest Party stage packs
were not measured individually.

### Measured for all 23 packs (2026-11-12, `scripts/build_packs.py build`)

| Pack | Models | Installed | Zip |
|---|---|---|---|
| dancers-3rd-mix | 16 | 76.9 MB | 21.9 MB |
| dancers-4th-mix | 18 | 86.7 MB | 24.5 MB |
| dancers-5th-mix | 15 | 72.2 MB | 20.6 MB |
| dancers-ddr-windows-2k2 | 49 | 158.7 MB | 67.5 MB |
| dancers-festival | 26 | 125.5 MB | 36.2 MB |
| dancers-hottstparty-1-3 | 137 | 560.9 MB | 184.1 MB |
| dancers-hottstparty-4 | 32 | 115.8 MB | 42.0 MB |
| dancers-hottstparty-5 | 18 | 67.7 MB | 24.9 MB |
| dancers-partycollection | 60 | 288.8 MB | 81.9 MB |
| dancers-strike | 45 | 217.5 MB | 62.7 MB |
| dancers-supernova-1-2 | 24 | 54.9 MB | 19.0 MB |
| dancers-ultramix-1-4 | 25 | 87.4 MB | 41.7 MB |
| dancers-universe-1-3 | 63 | 474.7 MB | 157.3 MB |
| dancers-winx-club | 24 | 92.9 MB | 32.0 MB |
| dancers-x-x2 | 33 | 102.2 MB | 43.4 MB |
| stages-hottest-party-1 | 42 | 125.5 MB | 30.3 MB |
| stages-hottest-party-2 | 59 | 109.8 MB | 21.3 MB |
| stages-hottest-party-3 | 17 | 240.9 MB | 37.5 MB |
| stages-hottest-party-4 | 26 | 131.5 MB | 39.9 MB |
| stages-hottest-party-5 | 14 | 119.7 MB | 33.3 MB |
| stages-supernova-1-2 | 22 | 91.8 MB | 5.4 MB |
| stages-winx-club | 6 | 29.2 MB | 5.6 MB |
| stages-x-x2 | 6 | 19.0 MB | 3.3 MB |
| **Total** | **777** | **3 450 MB** | **1 036 MB** |

The estimate held (about 1 GB; largest pack 184 MB, far below GitHub's 2 GiB asset
limit). A second build produced the same 23 content ids. The base release zip is
30.5 MB (131.8 MB uncompressed).

## 6. The timed prompt

`updater/src/console.rs` already implements a bounded wait: a thread parks in a blocking
`read_line` and the main thread waits on a channel with a timeout; the thread dies with
the process. Reused for the pack prompt with one difference: the existing wait treats
Enter, EOF and timeout alike (all mean "close"), whereas here only a real line counts as
"yes" — EOF (stdin closed or redirected, as some launchers do), a read error and the
timeout all mean "start the game". Type-ahead from earlier in the run should be
discarded before the prompt (`FlushConsoleInputBuffer` on Windows).

## Not accessible / uncertain

- `gh` is not installed on the build machine, so its exact JSON field names were not
  checked locally; the scripts can read the shelf through the public REST endpoint
  (`GET /repos/{owner}/{repo}/releases/tags/content`) and use `gh` only to upload.
- Whether `/releases/latest` in practice follows the `make_latest` flag or the
  `created_at` ordering was not tested (it would need a release on the live repository).
  The guards in §2 hold under either.
