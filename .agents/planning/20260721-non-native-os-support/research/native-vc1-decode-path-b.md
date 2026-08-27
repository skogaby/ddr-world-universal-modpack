# Addendum (2026-08-19) — Native VC-1 decode ("Path B") supersedes the suppress-only conclusion

The original conclusion of this feature — "never build the DirectShow graph
under Wine" — is superseded. Background movies now **render natively** under
CrossOver with the stock VC-1 files, no conversion, via three layers on top of
the mod's `movie_mode: "fallback"`:

1. **spice2x `-audiohookdisable`** — removes the original crash at the source
   (spice2x's `CoCreateInstance` IAT wrap × Wine builtin `winmm` during
   devenum's audio-renderer enumeration). The suppress stub remains the
   crash-safe default for setups that keep audio hooks on.
2. **Native x64 Windows Media runtime in the bottle** — qasf/wmvcore/wmasf/
   wmvdecod/wmadmod (+ mfperfhelper, wmidx), `native,builtin` overrides,
   `regsvr32`, and the ASF `Media Type` byte-pattern → WM ASF Reader mapping.
   Plus `movie_policy::absolutize_request_path` (fallback mode) so Wine
   quartz's byte-pattern probe can open the game's relative paths.
3. **`services/mfplat_vih_fix.rs`** — the final root cause was a genuine Wine
   bug: builtin mfplat's `MFInitMediaTypeFromVideoInfoHeader(subtype=NULL)`
   derives the subtype from `biBitCount` only (WVC1 header → "RGB24"), so
   native wmvdecod rejected its own input type (`DMO_E_TYPE_NOT_ACCEPTED`)
   and the reader pin never offered decoded formats. The fix is a Wine-gated
   `GenericDetour` on the mfplat export that injects the `biCompression`
   FOURCC subtype exactly like Windows; installed by
   `non_native_os_support::enable()` in fallback mode.

Live-verified 2026-08-19: attract-demo movies visually render; zero
`graph build failed` log lines; the one-shot
`mfplat_vih_fix: injected FOURCC subtype "WVC1"` INFO fires at the first
movie open.

**Full recipe + investigation trail: `docs/native_wm_runtime_bottle_setup.md`**
(the authoritative record — bottle file/registry inventory, the lazy codec
discovery mechanism in wmvcore, the red-herring catalog, harness modes, and
the wine-11.0 source references).

Everything in `research/movie-player-re.md` (object model, BuildGraph
internals, state machine) remains authoritative; only the "movies must stay
suppressed" conclusion changed. `scripts/convert_movies.sh` ("Path A")
remains a valid no-runtime alternative and composes with fallback mode.
