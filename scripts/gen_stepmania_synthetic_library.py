#!/usr/bin/env python3
"""Generate a synthetic StepMania library for scale testing the pack loader.

Writes `<out>/<Synthetic Pack NNN>/<Song NNNNN>/song.ssc + song.wav` — real,
playable charts (one tap per beat for 16 measures at a per-song BPM) over a
tiny mono WAV with a click on every beat, no art (the placeholder jacket is
used), so a library of tens of thousands of songs costs ~90 KB per song on
disk and converts at full speed. Use it to exercise the injection ceiling,
the thumbnail arc and the boot pass with a library size no real tester has.

Usage:
  scripts/gen_stepmania_synthetic_library.py --out <game>/stepmania/Songs --songs 14000
  scripts/gen_stepmania_synthetic_library.py --out ... --songs 14000 --remove

Pack folders are named `Synthetic Pack NNN` (never prefixed with `_` or `.`,
which the index skips). `--remove` deletes every `Synthetic Pack *` folder
under --out and nothing else. Writes nothing into the repository.
"""

import argparse
import math
import os
import shutil
import struct
import sys

RATE = 44_100
MEASURES = 16
CLICK_SECONDS = 0.002


def wav_bytes(bpm: int, seconds: float) -> bytes:
    """16-bit MONO `seconds` of silence with a click on every beat of `bpm`."""
    n = int(RATE * seconds)
    data = bytearray(n * 2)
    beat = 60.0 / bpm
    click = int(RATE * CLICK_SECONDS)
    t = 0.0
    while t < seconds:
        start = int(round(t * RATE))
        for i in range(start, min(start + click, n)):
            amp = 12000 if (i - start) % 2 == 0 else -12000
            struct.pack_into("<h", data, i * 2, amp)
        t += beat
    header = (
        b"RIFF"
        + struct.pack("<I", 36 + len(data))
        + b"WAVEfmt "
        + struct.pack("<IHHIIHH", 16, 1, 1, RATE, RATE * 2, 2, 16)
        + b"data"
        + struct.pack("<I", len(data))
    )
    return header + bytes(data)


def ssc_text(title: str, bpm: int) -> str:
    measure = "1000\n0100\n0010\n0001\n"
    notes = ",\n".join([measure] * MEASURES)
    return (
        "#VERSION:0.83;\n"
        f"#TITLE:{title};\n"
        "#ARTIST:synthetic;\n"
        "#MUSIC:song.wav;\n"
        "#OFFSET:0.000;\n"
        "#SAMPLESTART:0.5;\n"
        "#SAMPLELENGTH:1.0;\n"
        f"#BPMS:0.000={bpm}.000;\n"
        "#NOTEDATA:;\n"
        "#STEPSTYPE:dance-single;\n"
        "#DIFFICULTY:Hard;\n"
        f"#METER:{(bpm // 10) % 19 + 1};\n"
        "#NOTES:\n"
        f"{notes}\n;\n"
    )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, help="the songs root (…/stepmania/Songs)")
    ap.add_argument("--songs", type=int, default=14_000, help="total songs (default 14000)")
    ap.add_argument("--per-pack", type=int, default=100, help="songs per pack (default 100)")
    ap.add_argument("--seconds", type=float, default=2.0, help="audio length per song (default 2.0)")
    ap.add_argument("--remove", action="store_true", help="delete every 'Synthetic Pack *' folder under --out and exit")
    args = ap.parse_args()

    out = os.path.expanduser(args.out)
    if not os.path.isdir(out):
        print(f"error: {args.out} is not a directory", file=sys.stderr)
        return 2

    if args.remove:
        removed = 0
        for name in sorted(os.listdir(out)):
            if name.startswith("Synthetic Pack "):
                shutil.rmtree(os.path.join(out, name))
                removed += 1
        print(f"removed {removed} synthetic pack(s)")
        return 0

    # A handful of distinct BPMs so charts and audio differ a little.
    bpms = [100, 120, 128, 140, 150, 160, 175, 180, 190, 200]
    wavs = {bpm: wav_bytes(bpm, args.seconds) for bpm in bpms}
    packs = math.ceil(args.songs / args.per_pack)
    written = 0
    for p in range(packs):
        pack_dir = os.path.join(out, f"Synthetic Pack {p + 1:03d}")
        os.makedirs(pack_dir, exist_ok=True)
        for s in range(args.per_pack):
            if written >= args.songs:
                break
            idx = written + 1
            bpm = bpms[idx % len(bpms)]
            song_dir = os.path.join(pack_dir, f"Song {idx:05d}")
            os.makedirs(song_dir, exist_ok=True)
            with open(os.path.join(song_dir, "song.ssc"), "w", encoding="utf-8", newline="\n") as f:
                f.write(ssc_text(f"Synthetic {idx:05d}", bpm))
            with open(os.path.join(song_dir, "song.wav"), "wb") as f:
                f.write(wavs[bpm])
            written += 1
        if (p + 1) % 20 == 0 or written >= args.songs:
            print(f"  {written}/{args.songs} songs in {p + 1} pack(s)")
    per_song = len(next(iter(wavs.values()))) + 400
    print(f"wrote {written} songs in {packs} pack(s), ~{written * per_song / 1e6:.0f} MB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
