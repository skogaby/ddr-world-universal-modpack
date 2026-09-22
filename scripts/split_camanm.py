#!/usr/bin/env python3
"""Split one long `.camanm` into per-shot stage-camera clips.

The DDR A3 song-camera workflow (`tools/blender_ddr_addon/examples/stage_camera.py`)
authors ONE clip the length of the song with the shots baked in as hard cuts.
DDR World's Background Dancers mod drives a stage in A3 STAGE mode instead: a
LIST of short clips (stock: 6-7.5 s each), cycled main → main, `_non` names
cut away at dance changes. This tool turns the former into the latter: each
`--shot NAME:F0-F1` becomes `NAME.camanm`, re-timed to `--frames` frames
(default: the shot's own length) by game-equivalent sampling of every camera
slot (slerp on rotation), single-key slots (near/far/aspect) copied verbatim.

    ./scripts/split_camanm.py music_lesa.camanm -o out/ --frames 450 \\
        --shot griffin_st01:0-899 --shot griffin_st02:900-2399 \\
        --shot griffin_st03:2400-5399 --shot griffin_st04:5400-6238

Drop the outputs anywhere inside the stage's model folder
(`data_mods/custom_models/stages/<Name>/mapset_<key>/camera/`); the mod picks
every `*.camanm` there up as the stage's camera set. Verifies each output by
re-parsing it and comparing samples against the source.
"""
import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import anm_dump as ad  # noqa: E402


def parse_shot(spec):
    name, _, rng = spec.partition(":")
    f0, _, f1 = rng.partition("-")
    if not name or not f0 or not f1:
        raise argparse.ArgumentTypeError("shot must be NAME:F0-F1, got %r" % spec)
    f0, f1 = int(f0), int(f1)
    if f1 <= f0:
        raise argparse.ArgumentTypeError("shot %s: F1 must be > F0" % name)
    return name, f0, f1


def slice_shot(parsed, f0, f1, frames):
    """Camera-chunk spec for source frames [f0, f1] re-timed to 0..frames."""
    data = parsed["data"]
    cam = next(c for c in parsed["chunks"] if c["type"] == 4)
    slots = []
    span = float(f1 - f0)
    for T in cam["tracks"]:
        if T is None:
            slots.append(None)
            continue
        base = dict(kind=T["kind"], target=T["target"], sub=T["sub"])
        if T["key_count"] == 1:
            slots.append(dict(base, times=[0], keys=[ad.decode_key(data, T, 0)]))
            continue
        keys = [ad.sample_track(data, T, f0 + span * k / frames) for k in range(frames + 1)]
        slots.append(dict(base, times=None, keys=keys))
    hdr = parsed["header"]
    return dict(frame_count=frames, flag=hdr["flag"], fps=hdr["fps_or_one"], camera=slots)


def verify(src, out_bytes, f0, f1, frames):
    """Samples of the written clip must match the source at the mapped frame."""
    dst = ad.parse_anm(out_bytes)
    cam_s = next(c for c in src["chunks"] if c["type"] == 4)
    cam_d = next(c for c in dst["chunks"] if c["type"] == 4)
    worst = 0.0
    for k in (0, frames // 3, frames // 2, frames):
        fs = f0 + (f1 - f0) * k / float(frames)
        for Ts, Td in zip(cam_s["tracks"], cam_d["tracks"]):
            if Ts is None:
                assert Td is None
                continue
            a = ad.sample_track(src["data"], Ts, fs)
            b = ad.sample_track(dst["data"], Td, float(k))
            if Ts["channel"] == "rotation" and sum(x * y for x, y in zip(a, b)) < 0:
                b = tuple(-x for x in b)
            worst = max(worst, max(abs(x - y) for x, y in zip(a, b)))
    return worst


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("path", help="source .camanm")
    ap.add_argument("-o", "--out", required=True, help="output directory")
    ap.add_argument("--shot", action="append", type=parse_shot, required=True, metavar="NAME:F0-F1")
    ap.add_argument("--frames", type=int, default=None,
                    help="re-time every shot to this many frames (default: keep each shot's length)")
    args = ap.parse_args()

    src = ad.parse_anm(open(args.path, "rb").read())
    if not any(c["type"] == 4 for c in src["chunks"]):
        sys.exit("%s has no camera chunk" % args.path)
    last = src["header"]["frame_count"]
    os.makedirs(args.out, exist_ok=True)
    for name, f0, f1 in args.shot:
        if f1 > last:
            sys.exit("shot %s ends at %d but the clip has %d frames" % (name, f1, last))
        frames = args.frames or (f1 - f0)
        spec = slice_shot(src, f0, f1, frames)
        out = ad.write_anm(spec)
        worst = verify(src, out, f0, f1, frames)
        dst = os.path.join(args.out, name + ".camanm")
        with open(dst, "wb") as fh:
            fh.write(out)
        print("%-24s frames %5d-%5d -> %4d frames (%.2f s @%d fps)  %6d bytes  max sample error %.2e" % (
            os.path.basename(dst), f0, f1, frames, frames / float(spec["fps"]), spec["fps"], len(out), worst))


if __name__ == "__main__":
    main()
