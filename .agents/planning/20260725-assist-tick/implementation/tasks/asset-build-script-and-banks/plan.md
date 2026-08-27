# Plan — Assist-Tick Asset Build Script and Committed Banks (Step 1, task 03)

**Status: Approved 2026-07-26** — inherited from the verified upstream approval chain
(`context.md`). The one requirement conflict this task contains (R3's ffmpeg step versus R9's
reproducibility) is resolved in `context.md` → *The ffmpeg conflict*; it is a deviation from the
literal wording of R3 and is logged as such in `progress.md`.

## Files

| File | Change |
|---|---|
| `scripts/build_assist_tick_bank.sh` | **new**, executable |
| `data_mods/assist_tick/source/clap.ogg` | **new** — the committed source sample |
| `data_mods/assist_tick/banks/tick.xwb` | **new** — generated, committed |
| `data_mods/assist_tick/banks/tick.xsb` | **new** — generated, committed |
| `README.md` | new subsection under the asset-pipeline documentation |

Nothing in `src/` changes.

## Script structure

Modelled on `scripts/build_shaders.sh`: `set -euo pipefail`, `cd "$(dirname "$0")/.."`, a `die()`
helper, `${VAR:-default}` overrides, a comment header stating what it produces / requires / does
**not** do, and a closing summary with sizes and hashes.

```
usage:
  ./scripts/build_assist_tick_bank.sh [<input-audio>] [--chart-tools <path>] [--name <name>]
  ./scripts/build_assist_tick_bank.sh --self-check
```

Stages:

1. **Args.** Positional input defaults to `data_mods/assist_tick/source/clap.ogg`.
2. **Locate the generator** — `--chart-tools`, then `$DDR_CHART_TOOLS`, then `PATH`. Handles a repo
   checkout, a directory holding the binary, or the binary itself (`context.md` has the table).
   Failure names the flag, the variable and the tool.
3. **Conform the input.** `ffprobe` the codec / channels / rate. Conforming ⇒ pass through
   untouched. Non-conforming ⇒ require `ffmpeg`, transcode to mono 44100 Ogg in the temp dir, and
   **warn** that the result is not byte-reproducible across machines. No ffprobe ⇒ pass through and
   let the generator's own error speak, printing the ffmpeg command to run.
4. **Generate** into the temp dir with `--name asti`.
5. **Validate** the generated pair (a function, so `--self-check` can reuse it).
6. **Stock comparison**, only when `DDR_WORLD_INSTALL` is set and `data/arc/se_normal.arc` is
   readable. Otherwise skip with a notice; never fail for its absence.
7. **Install into the repo** — `banks/tick.xwb` / `banks/tick.xsb`, written only if the bytes differ,
   so re-runs are no-ops and mtimes stay stable.
8. **Summary.**

`mktemp -d` plus an `EXIT` trap, so nothing partial survives a failure (AC3).

### Validation function

`validate_pair <xwb> <xsb> <name>` — dumps the wave bank once into a variable, then asserts each
rule with a named message. The dump-key-to-engine-rule mapping is tabulated in `context.md`; each
assertion's failure message names the rule so AC6's "names the rule that was violated" holds.

### `--self-check` (AC6)

Generates, then flips one byte in a validator-covered field of a **temp copy** and re-runs
`validate_pair`, asserting that it *fails*. Makes AC6 a repeatable property of the script rather
than a one-off manual demonstration. Corrupts `dwHeaderVersion` (offset `0x08`), because rule H2 is
the single most unambiguous rule to trip.

## Requirement → mechanism

| Req | Mechanism |
|---|---|
| R1 | strict mode, executable, header, idempotent (stage 7 writes only on difference) |
| R2 | committed at `data_mods/assist_tick/source/clap.ogg`, the script's default input, overridable positionally |
| R3 | stage 3/4 — conditional ffmpeg, then the generator (deviation logged) |
| R4 | stage 2 |
| R5 | stage 5 + stage 6 |
| R6 | stage 6 skips with a notice and the script still exits 0 |
| R7 | no absolute path, username or home literal anywhere; verified by grep in stage-0 of validation below |
| R8 | no write outside the repo; header says installing is out of scope |
| R9 | conforming-source pass-through (no ffmpeg re-encode) + stage 7's `cmp` |
| R10 | summary |
| R11 | README subsection |

## Verification I will run

| # | Check | Expectation |
|---|---|---|
| V1 | run with no arguments | exit 0, both files written, summary printed |
| V2 | run again | exit 0, reports both files unchanged, bytes identical |
| V3 | `--chart-tools` unset, `DDR_CHART_TOOLS` unset, tool not on `PATH` | non-zero exit, message naming the flag + variable + tool, **no partial output left behind** |
| V4 | `DDR_WORLD_INSTALL` unset | exit **0**, stock comparison reported as skipped with a reason |
| V5 | `DDR_WORLD_INSTALL` set to the real install | stock comparison runs and passes |
| V6 | `--self-check` | exit 0, having *proved* that validation rejects a corrupted bank and named the rule |
| V7 | `grep` the committed script for `$HOME`, `/Users/`, the username | no matches |
| V8 | read the script for any copy into a game install | none |
| V9 | regenerate after `git stash`-free clean state | committed bytes reproduced byte-for-byte |
| V10 | `cargo check --target x86_64-pc-windows-msvc` | still clean (nothing in `src/` touched) |

## Risks

| Risk | Mitigation |
|---|---|
| An ffmpeg re-encode silently enters the default path and breaks reproducibility | V1/V2/V9; the ffprobe gate; the loud warning when a transcode does happen |
| The dump format changes underneath the script | It is documented as an interface in `src/xwb/dump.rs` and pinned by that crate's tests |
| `od` byte-order assumption in the ARC read | Only in the optional stock branch; hosts are LE (x86_64/arm64 macOS and Linux); noted in a comment |
| A committed absolute path slips in | V7, run as an explicit step, not by eye |

## Out of scope

- Installing anything into a game install (R8, explicitly).
- Any change under `src/` — the DLL does not read these files until Step 3.
- Extending the shader/label scripts or touching `build_shaders.sh`.
