#!/usr/bin/env bash
# Batch-convert DDR World background movies from VC-1 to H.264 for
# CrossOver/Wine playback (macOS has no VC-1 decoder; H.264 decodes via
# VideoToolbox). Output is H.264 High in an MP4 container, but keeps each
# file's original `.wmv` name — the game constructs the path itself and
# Wine's DirectShow source resolution is content-sniffed, not
# extension-based.
#
# Idempotent + resume-safe: every run probes each file's actual codec and
# only converts VC-1/WMV sources; already-converted (H.264/HEVC) files are
# skipped. Interrupted runs just re-run.
#
# Each converted file's pristine original is preserved side-by-side as
# `<name>.wmv.bak` (never overwritten once created). `--restore` puts all
# originals back.
#
# Usage:
#   ./scripts/convert_movies.sh [options] [movie_dir]
#
#   movie_dir      defaults to "$DDR_WORLD_INSTALL/data/mdb_apx/movie"
#
# Options:
#   --dry-run      probe + report what would happen; convert nothing
#   --restore      move every .wmv.bak back over its .wmv and exit
#   --software     encode with libx264 (crf 18, preset slow) instead of the
#                  default h264_videotoolbox hardware encoder — slower but
#                  marginally better quality per bit
#   --limit N      convert at most N files this run (for testing batches)
#
# Requires: ffmpeg + ffprobe on PATH.

set -uo pipefail

DRY_RUN=0
RESTORE=0
SOFTWARE=0
LIMIT=0
MOVIE_DIR=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)  DRY_RUN=1 ;;
        --restore)  RESTORE=1 ;;
        --software) SOFTWARE=1 ;;
        --limit)    shift; LIMIT="${1:-0}" ;;
        -h|--help)  sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*)         echo "unknown option: $1" >&2; exit 1 ;;
        *)          MOVIE_DIR="$1" ;;
    esac
    shift
done

if [[ -z "$MOVIE_DIR" ]]; then
    if [[ -z "${DDR_WORLD_INSTALL:-}" ]]; then
        echo "error: no movie_dir argument and \$DDR_WORLD_INSTALL is not set" >&2
        exit 1
    fi
    MOVIE_DIR="$DDR_WORLD_INSTALL/data/mdb_apx/movie"
fi
[[ -d "$MOVIE_DIR" ]] || { echo "error: not a directory: $MOVIE_DIR" >&2; exit 1; }
command -v ffmpeg  >/dev/null || { echo "error: ffmpeg not on PATH" >&2; exit 1; }
command -v ffprobe >/dev/null || { echo "error: ffprobe not on PATH" >&2; exit 1; }

# ── restore mode ─────────────────────────────────────────────────────
if [[ "$RESTORE" -eq 1 ]]; then
    restored=0
    for bak in "$MOVIE_DIR"/*.wmv.bak; do
        [[ -e "$bak" ]] || continue
        mv -f "$bak" "${bak%.bak}"
        restored=$((restored + 1))
    done
    echo "restored $restored original file(s)"
    exit 0
fi

probe_codec() { # file -> codec name of first video stream (empty on failure)
    ffprobe -v error -select_streams v:0 -show_entries stream=codec_name \
        -of csv=p=0 "$1" 2>/dev/null | head -1
}

probe_duration() { # file -> integer seconds (0 on failure)
    local d
    d=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$1" 2>/dev/null | head -1)
    printf '%.0f' "${d:-0}" 2>/dev/null || echo 0
}

# ── census ───────────────────────────────────────────────────────────
files=()
while IFS= read -r -d '' f; do files+=("$f"); done \
    < <(find "$MOVIE_DIR" -maxdepth 1 -name '*.wmv' ! -name '*.bak' -print0 | sort -z)
total=${#files[@]}
echo "movie dir: $MOVIE_DIR ($total .wmv files)"

# Disk-space sanity: converted output roughly matches source size, and
# originals stay on disk as .bak — require free space >= source total + 10%.
src_bytes=$(du -ck "${files[@]}" 2>/dev/null | tail -1 | cut -f1)
free_kb=$(df -k "$MOVIE_DIR" | awk 'NR==2 {print $4}')
need_kb=$((src_bytes + src_bytes / 10))
if [[ "$DRY_RUN" -eq 0 && "$free_kb" -lt "$need_kb" ]]; then
    echo "error: ~$((need_kb / 1024 / 1024)) GiB free space needed (originals are kept as .bak), only $((free_kb / 1024 / 1024)) GiB available" >&2
    echo "free up space or convert in batches with --limit N" >&2
    exit 1
fi

converted=0 skipped=0 failed=0 examined=0
failed_files=()
start_ts=$(date +%s)

for f in "${files[@]}"; do
    examined=$((examined + 1))
    name=$(basename "$f")
    codec=$(probe_codec "$f")

    case "$codec" in
        h264|hevc)
            skipped=$((skipped + 1))
            [[ "$DRY_RUN" -eq 1 ]] && echo "[skip     ] $name (already $codec)"
            continue
            ;;
        vc1|wmv3|wmv2|wmv1)
            ;; # convert below
        "")
            echo "[fail     ] $name (unprobeable — corrupt?)"
            failed=$((failed + 1)); failed_files+=("$name")
            continue
            ;;
        *)
            echo "[skip     ] $name (unexpected codec '$codec' — leaving untouched)"
            skipped=$((skipped + 1))
            continue
            ;;
    esac

    if [[ "$DRY_RUN" -eq 1 ]]; then
        echo "[would cvt] $name ($codec)"
        converted=$((converted + 1))
        continue
    fi

    if [[ "$LIMIT" -gt 0 && "$converted" -ge "$LIMIT" ]]; then
        echo "--limit $LIMIT reached; stopping (re-run to continue)"
        break
    fi

    src_dur=$(probe_duration "$f")
    tmp="$f.converting.mp4"
    rm -f "$tmp"

    if [[ "$SOFTWARE" -eq 1 ]]; then
        enc=(-c:v libx264 -preset slow -crf 18)
    else
        enc=(-c:v h264_videotoolbox -b:v 4000k -maxrate 6000k)
    fi

    echo "[convert  ] ($examined/$total) $name ($codec, ${src_dur}s)"
    if ! ffmpeg -hide_banner -loglevel error -y -i "$f" \
            "${enc[@]}" -profile:v high -pix_fmt yuv420p -an \
            -movflags +faststart -f mp4 "$tmp" </dev/null; then
        echo "[fail     ] $name (ffmpeg error)"
        rm -f "$tmp"
        failed=$((failed + 1)); failed_files+=("$name")
        continue
    fi

    # verify: h264 stream present, duration within 2 s of source
    out_codec=$(probe_codec "$tmp")
    out_dur=$(probe_duration "$tmp")
    dur_delta=$((src_dur - out_dur)); dur_delta=${dur_delta#-}
    if [[ "$out_codec" != "h264" || "$dur_delta" -gt 2 ]]; then
        echo "[fail     ] $name (verify failed: codec=$out_codec duration=${out_dur}s vs ${src_dur}s)"
        rm -f "$tmp"
        failed=$((failed + 1)); failed_files+=("$name")
        continue
    fi

    # atomic-ish swap; never clobber an existing .bak (pristine original)
    if [[ ! -e "$f.bak" ]]; then
        mv "$f" "$f.bak"
    fi
    mv "$tmp" "$f"
    converted=$((converted + 1))
done

elapsed=$(( $(date +%s) - start_ts ))
echo
echo "── summary ──────────────────────────────────────────"
if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "dry run: $converted would convert, $skipped already done/skipped, $failed unprobeable"
else
    echo "converted: $converted   skipped: $skipped   failed: $failed   (${elapsed}s)"
fi
if [[ "$failed" -gt 0 ]]; then
    printf 'failed: %s\n' "${failed_files[@]}"
    exit 2
fi
