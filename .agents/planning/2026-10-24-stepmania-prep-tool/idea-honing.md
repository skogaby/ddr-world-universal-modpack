# Decision register — StepMania prep tool

Revised 2026-10-24 after the maintainer changed the idea: the tool **replaces** the
in-game index rather than pre-warming it. The first register (pre-warmer) is
superseded; its still-valid items carry their IDs.

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | What the DLL does at boot without an index thread | The core contract: "songs load only if the tool ran" | `early_init` loads `index.bin` synchronously (ms) and publishes the lookup maps; a read-only **validation thread** re-stats each record (artefacts present + source stamps unchanged — today's warm check, ~200 ms/1,000 songs) and the `startup.arc` fingerprint waits for it (bounded by stat time; 15 s safety cap → unvalidated songs dropped with a WARN). Records that fail validation are dropped with ONE summary WARN "N song(s) changed since the last prep run — run `ddr_world_hook_prep.exe`". No conversion code runs in the game, ever | Proposed |
| D2 | Cache ownership (who writes what) | Race freedom is a property of ownership, not timing | **Tool writes** `index.bin`, `ssq/`, `xsb/`, `tn/`, `sync.bin`. **DLL writes** `xwb/`, `jacket/`, `lru.bin`, `thumbnail/<S>.arc` (series value is runtime-only). Neither touches the other's files. `mark_audio_failed` (the DLL's only `index.bin` write today) becomes session-only; the tool's sync pass fully decodes every song and records `audio_failed` itself | Proposed |
| D3 | Runtime sync measurement | The sweep is the other background job; on-decide is a 5 s stage-load hold | Remove **both** the sweep thread and on-decide measurement. The tool measures (parallel, default on); a song without a record plays as authored, one INFO per boot naming how many. `sync_apply` shrinks to ledger load + delta provider + freeze rule (~600 lines removed: `sync_sweep.rs`, most of `sync_apply.rs`) | Proposed |
| D4 | Where the conversion pipeline lives | Avoid two implementations (the `sm_pack_check` drift) | Today's `index.rs` cold path (discover, identity assignment, `convert_song`, warm reuse, `index.bin` write) **moves** to the tool crate; the DLL keeps a slim `index.rs` (load, validate, publish). Pure files (`identity`, `model`, `musicdb`, `convert`, `ceiling`, `sync_state`, `cache`, `core/*`) stay in `src/` and are `#[path]`-mounted by the tool. `tools/sm_pack_check` is folded into the tool as `--dry-run --out <dir>` and deleted | Proposed |
| D5 | Ceiling report | The user learns before booting what the game will show | The tool prints "N indexed; the game injects K (first by pack/folder); left out: <pack: count>…" and that it assumes Song Limit Expansion on (arena state is a runtime fact) | Proposed |
| D6 | Running game | Tool writes while the DLL reads | Refuse to run (message, exit 2) while `spice64.exe`/`spice.exe` is running. No lock file | Proposed |
| D7 | Tool interruption | Ctrl+C / closed console mid-run | Checkpoint `index.bin` every 250 songs / 15 s; the warm path reuses complete records on the next run | Proposed |
| D8 | Crate, exe, release | Build plumbing | `prep/` beside `updater/` (own `[workspace]`, `x86_64-win7-windows-msvc`, trim-paths); `ddr_world_hook_prep.exe`; in the release zip; README + `gamestart.bat` line before spice (like the updater) | Proposed |
| D9 | Separate exe vs. updater subcommand | Keep the self-updating updater dependency-light | Separate exe | Proposed |
| D10 | Sync measurement policy in the tool | Dominant cost (~0.8 s/song CPU) | Measure songs inside the ceiling whose fingerprint has no record, on the worker pool; estimate printed first; `--no-sync` skips; `auto_sync: false` in config skips | Proposed |
| D11 | Config | `index_timeout_s` has no meaning | Remove `index_timeout_s` (operator-only section → retired key, parsed and ignored); `audio_cache_mb`, `auto_sync`, the rest unchanged. The tool reads `mods` (toggle) and `stepmania_packs.auto_sync` from `mod-config.json` | Proposed |
| D12 | No-index UX in the game | A user who drops packs and boots without the tool | One WARN naming the tool; `StepmaniaPacks` init log says "no index — run the prep tool"; `songs_readme.txt` and README rewritten around the tool. No splash/on-screen text | Proposed |
| D13 | Existing caches | Testers already have a DLL-written `index.bin` v5 | Same format; the tool reads and reuses it (warm). `FORMAT_VERSION` unchanged unless the record changes; `rejects` stay in the file for the tool's warm path, the DLL ignores them | Assumed |
| D14 | Game folder, console, log, exit codes | Updater patterns | Exe directory (or `--game-dir`) gated on `spice64.exe`/`ddr_world_hook.dll`; pause-at-end when double-clicked; `ddr_world_hook_prep.log`; exit 0 success/nothing-to-do, 2 game running, 3 folder not recognised; other failures logged, exit 0 so `gamestart.bat` still starts the game | Assumed |
| D15 | Stock-song sync | Scope | Out of scope — separate feature | Assumed |
| D16 | Files the DLL no longer uses but the tool mounts (`identity.rs`, the conversion half of `convert.rs`) | Compile coverage | Stay declared in the DLL's module tree (crate-wide `dead_code` allowance) so `cargo check` still type-checks them | Assumed |

Dropped from the first register: *in-game worker priority after the deadline* (no
in-game workers remain), *in-game `index.bin` checkpointing* (no in-game writer).

## Notes per decision

### D1 — why validate at all, and why on a thread
Trusting `index.bin` blindly means a song whose folder was deleted lists on the
wheel and plays silence, and an edited simfile plays its old chart until the tool
runs again. The check is the same stat walk the warm path does today. It cannot
run synchronously in `early_init` (step 0' precedes LayeredFS install, which must
beat the game's first `shader.arc` read — seconds there are not available), so it
runs on a thread that only *reads*, and the `startup.arc` fingerprint — which
already blocks the open today — waits for it. With `gamestart.bat` running the tool
every launch the check never drops anything; it is the guarantee for the user who
does not.

### D2
After this change `index.bin` has exactly one writer and every DLL write is to a
file the tool never reads. The DLL's `audio_failed` persistence goes because it
was the one cross-writer; the tool's measurement pass (`decode_whole`) finds the
same decode failures offline and records them, and a `--no-sync` run leaves the
DLL's session-only flag (silence + WARN naming the song) as the fallback.

### D3
Removing on-decide too is the simplification the revised idea asks for: no hold at
stage load, no normal-priority thread during play, one fewer provider state
machine. Cost: a song the tool did not measure plays as authored — the same as
`auto_sync: false` today. Alternative kept open: keep on-decide only (bounded,
user-triggered). Recommend removing; the tool measures by default.

### D4
The tool must be byte-for-byte the pipeline the DLL's validation trusts. Moving the
code (rather than copying) keeps one implementation; the pure modules stay in
`src/` because `scripts/validate_stepmania.sh` and the DLL (validation, providers,
art) still use them. Folding `sm_pack_check` in removes the only other
implementation of the pipeline.

### D7
Checkpointing is now tool-only, which also simplifies it: no concurrent reader.
