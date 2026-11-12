#!/usr/bin/env python3
"""Generate dense StepMania test songs that reach marathon step counts fast.

A real marathon takes an hour to reach a five-digit combo. These songs pack
the same number of steps into a few minutes — a 32nd-note single-tap stream
at a high BPM — so the limits a long chart crosses can be tested in one
sitting with Autoplay on:

  *  8,196 judged notes — the size of the ghost field in the game's save
     staging buffer (`docs/s_marvelous_judgement_research.md`, "Save
     marshal");
  *  9,999 combo — the stock combo counter's display cap;
  * 10,000 of one judgement — a fifth digit on the results screen;
  * 65,535 steps — the most one SSQ chart can hold.

Writes `<out>/Marathon Test/Marathon Test NNNNN/song.ssc + song.wav`, one
song per requested step count (one Hard single chart each), over a mono WAV
with a quiet click on every measure. The default set brackets the first
three limits; add `--steps 50000 65535` for the largest charts.

Usage:
  scripts/gen_stepmania_marathon_test.py --out <game>/stepmania/Songs
  scripts/gen_stepmania_marathon_test.py --out ... --steps 20000 --nps 60
  scripts/gen_stepmania_marathon_test.py --out ... --remove

Run `ddr_world_hook_prep.exe` (the game's start script does) before the game
so the new songs are indexed. `--remove` deletes the `Marathon Test` pack
under --out and nothing else. Writes nothing into the repository.
"""

import argparse
import os
import shutil
import struct
import sys

RATE = 44_100
PACK = "Marathon Test"
ROWS_PER_MEASURE = 32
ROWS_PER_BEAT = ROWS_PER_MEASURE // 4
LEAD_IN_MEASURES = 8
TAIL_SECONDS = 2.0
CLICK_SECONDS = 0.002
CLICK_AMPLITUDE = 5000
MAX_STEPS = 65_535
DEFAULT_STEPS = [8_000, 9_000, 12_000, 20_000]
# One tap per row, never the same panel twice in a row.
PANELS = ["1000", "0100", "0010", "0001"]


def bpm_for(nps: float) -> int:
    """The whole BPM whose 32nd-note stream is closest to `nps` steps/second."""
    return max(1, round(nps * 60.0 / ROWS_PER_BEAT))


def measures_for(steps: int) -> int:
    return LEAD_IN_MEASURES + -(-steps // ROWS_PER_MEASURE)


def seconds_for(steps: int, bpm: int) -> float:
    return measures_for(steps) * 4 * 60.0 / bpm + TAIL_SECONDS


def wav_bytes(bpm: int, seconds: float) -> bytes:
    """16-bit MONO `seconds` of silence with a click on every measure of `bpm`."""
    n = int(RATE * seconds)
    data = bytearray(n * 2)
    measure = 4 * 60.0 / bpm
    click = int(RATE * CLICK_SECONDS)
    index = 0
    while index * measure < seconds:
        start = int(round(index * measure * RATE))
        for i in range(start, min(start + click, n)):
            amp = CLICK_AMPLITUDE if (i - start) % 2 == 0 else -CLICK_AMPLITUDE
            struct.pack_into("<h", data, i * 2, amp)
        index += 1
    header = (
        b"RIFF"
        + struct.pack("<I", 36 + len(data))
        + b"WAVEfmt "
        + struct.pack("<IHHIIHH", 16, 1, 1, RATE, RATE * 2, 2, 16)
        + b"data"
        + struct.pack("<I", len(data))
    )
    return header + bytes(data)


def notes_text(steps: int) -> str:
    empty = "\n".join(["0000"] * 4)
    measures = [empty] * LEAD_IN_MEASURES
    placed = 0
    while placed < steps:
        rows = []
        for _ in range(ROWS_PER_MEASURE):
            if placed < steps:
                rows.append(PANELS[placed % len(PANELS)])
                placed += 1
            else:
                rows.append("0000")
        measures.append("\n".join(rows))
    return "\n,\n".join(measures)


def ssc_text(title: str, steps: int, bpm: int) -> str:
    return (
        "#VERSION:0.83;\n"
        f"#TITLE:{title};\n"
        "#ARTIST:synthetic;\n"
        "#MUSIC:song.wav;\n"
        "#OFFSET:0.000;\n"
        "#SAMPLESTART:1.0;\n"
        "#SAMPLELENGTH:6.0;\n"
        f"#BPMS:0.000={bpm}.000;\n"
        "#NOTEDATA:;\n"
        "#STEPSTYPE:dance-single;\n"
        "#DIFFICULTY:Hard;\n"
        "#METER:19;\n"
        "#NOTES:\n"
        f"{notes_text(steps)}\n;\n"
    )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, help="the songs root (…/stepmania/Songs)")
    ap.add_argument(
        "--steps",
        type=int,
        nargs="+",
        default=DEFAULT_STEPS,
        help=f"step count per song (default {' '.join(map(str, DEFAULT_STEPS))}; max {MAX_STEPS})",
    )
    ap.add_argument("--nps", type=float, default=120.0, help="steps per second (default 120)")
    ap.add_argument("--remove", action="store_true", help=f"delete the '{PACK}' pack under --out and exit")
    args = ap.parse_args()

    out = os.path.expanduser(args.out)
    if not os.path.isdir(out):
        print(f"error: {args.out} is not a directory", file=sys.stderr)
        return 2
    pack_dir = os.path.join(out, PACK)

    if args.remove:
        if os.path.isdir(pack_dir):
            shutil.rmtree(pack_dir)
            print(f"removed '{PACK}'")
        else:
            print(f"no '{PACK}' pack under {args.out}")
        return 0

    if args.nps <= 0:
        print("error: --nps must be positive", file=sys.stderr)
        return 2
    bad = [s for s in args.steps if not 1 <= s <= MAX_STEPS]
    if bad:
        print(f"error: step counts must be 1..{MAX_STEPS}: {bad}", file=sys.stderr)
        return 2

    bpm = bpm_for(args.nps)
    nps = bpm * ROWS_PER_BEAT / 60.0
    total = 0
    for steps in sorted(set(args.steps)):
        title = f"{PACK} {steps:05d}"
        song_dir = os.path.join(pack_dir, title)
        os.makedirs(song_dir, exist_ok=True)
        seconds = seconds_for(steps, bpm)
        wav = wav_bytes(bpm, seconds)
        with open(os.path.join(song_dir, "song.ssc"), "w", encoding="utf-8", newline="\n") as f:
            f.write(ssc_text(title, steps, bpm))
        with open(os.path.join(song_dir, "song.wav"), "wb") as f:
            f.write(wav)
        total += len(wav)
        print(f"  {title}: {steps} steps, {bpm} BPM ({nps:.0f} steps/s), {seconds:.0f} s")
    print(f"wrote {len(set(args.steps))} song(s) into '{PACK}', ~{total / 1e6:.0f} MB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
