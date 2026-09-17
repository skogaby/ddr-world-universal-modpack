#!/usr/bin/env python3
"""
gen_anm_fixtures.py — Python-reference fixtures for the Rust `core/anm` codecs.

Reads the stock DDR World install (never the repo), evaluates every dance clip,
stage loop and stage camera through the VERIFIED reference codecs
(`anm_dump.py`, `ktmdl_dump.py`) and writes VALUES-ONLY JSON under
`tests/fixtures/anm/`. No Konami bytes are committed: the Rust fixture tests
re-read the same arcs from `$DDR_WORLD_INSTALL` at test time and must reproduce
these numbers (1e-5; quaternions up to sign).

Outputs (all values rounded to 6 significant decimals):
  dance_clips.json    every .anm of mc_male.arc + mc_female.arc
  stage_loops.json    every *_play_loop.anm of every mapset_*.arc
  stage_cameras.json  every .camanm of camera/stage_camera.arc (raw slot samples)
  rlists.json         the four startup.arc rlists
  pl_emi00.json       pl_emi00.b2it + the pl_emi00.model bone table

Per clip: header fields, parents (type-1 hierarchy chunk), the track table,
`fully_tracked` (every bone has a rotation AND a translation track — the only
case where the Python identity-seeded `evaluate_pose` equals the game's
bind-seeded chain), and at 8 fractional frames either the world matrices
(fully tracked) or the per-track samples (partial).

Usage:
    python3 scripts/gen_anm_fixtures.py [--install DIR] [--out tests/fixtures/anm]
"""
import argparse
import contextlib
import io
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

from anm_dump import decode_key, evaluate_pose, parse_anm, sample_track  # noqa: E402
from ktmdl_dump import parse_b2it, parse_model, parse_rlist  # noqa: E402
from unpack_arc import ARC  # noqa: E402

FRAME_SPEC = ("0", "0.25", "1.0", "7.5", "fc/3", "fc/2+0.75", "fc-1.5", "fc")


def quiet(fn, *args):
    """unpack_arc prints per member; keep the generator's stdout clean."""
    with contextlib.redirect_stdout(io.StringIO()):
        return fn(*args)


def frames_for(frame_count):
    fc = float(frame_count)
    vals = [0.0, 0.25, 1.0, 7.5, fc / 3.0, fc / 2.0 + 0.75, fc - 1.5, fc]
    return [round(max(0.0, v), 6) for v in vals]


def r6(x):
    return round(float(x), 6)


def rl(xs):
    return [r6(x) for x in xs]


def parents_of(anm):
    hier = next((c for c in anm["chunks"] if c["type"] == 1), None)
    if hier is None:
        return None
    return [(-1 if p == 0xFF else p) for _, p in hier["pairs"]]


def clip_fixture(arc_name, path, data):
    anm = parse_anm(data)
    H = anm["header"]
    parents = parents_of(anm)
    tracks = []
    rot, pos = set(), set()
    for C in anm["chunks"]:
        if C["type"] != 0:
            continue
        for T in C["tracks"]:
            tracks.append(dict(kind=T["kind"], target=T["target"], key_count=T["key_count"],
                               uniform=T["times"] is None))
            if T["channel"] == "rotation":
                rot.add(T["target"])
            elif T["channel"] == "translation":
                pos.add(T["target"])
    bone_count = len(parents) if parents is not None else 0
    fully = parents is not None and len(rot) == bone_count and len(pos) == bone_count
    has_cam = any(c["type"] == 4 for c in anm["chunks"])
    fps = float(H["fps_or_one"]) if has_cam and H["fps_or_one"] else 60.0
    fx: dict = dict(arc=arc_name, path=path, frame_count=H["frame_count"], loops=bool(H["flag"] & 1),
              fps=fps, bone_count=bone_count, parents=parents, tracks=tracks, fully_tracked=fully,
              frames=frames_for(H["frame_count"]))
    if fully:
        worlds = []
        for f in fx["frames"]:
            pose = evaluate_pose(anm, f, parents)
            worlds.append([rl(v for row in p["world"] for v in row) for p in pose])
        fx["world"] = worlds
    else:
        samples = []
        for f in fx["frames"]:
            per = []
            for C in anm["chunks"]:
                if C["type"] != 0:
                    continue
                for T in C["tracks"]:
                    per.append(dict(target=T["target"], channel=T["channel"],
                                    value=rl(sample_track(anm["data"], T, f))))
            samples.append(per)
        fx["samples"] = samples
    return fx


def camera_fixture(path, data):
    anm = parse_anm(data)
    H = anm["header"]
    cam = next(c for c in anm["chunks"] if c["type"] == 4)
    fx: dict = dict(path=path, frame_count=H["frame_count"], fps=float(H["fps_or_one"]),
              loops=bool(H["flag"] & 1), frames=frames_for(H["frame_count"]),
              slot_kinds=[(T["kind"] if T else None) for T in cam["tracks"]], slots=[])
    for f in fx["frames"]:
        row: list = []
        for T in cam["tracks"]:
            if T is None:
                row.append(None)
                continue
            v = sample_track(anm["data"], T, f)
            row.append(rl(v) if len(v) > 1 else r6(v[0]))
        fx["slots"].append(row)
    # first raw key of every slot (parser check independent of sampling)
    fx["first_keys"] = [None if T is None else rl(decode_key(anm["data"], T, 0)) for T in cam["tracks"]]
    return fx


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--install", default=os.environ.get("DDR_WORLD_INSTALL"),
                    help="World install root (default: $DDR_WORLD_INSTALL)")
    ap.add_argument("--out", default=os.path.join(HERE, "..", "tests", "fixtures", "anm"))
    args = ap.parse_args()
    if not args.install or not os.path.isdir(os.path.join(args.install, "data", "arc")):
        sys.exit("error: --install / DDR_WORLD_INSTALL must point at a World install (data/arc missing)")
    arcdir = os.path.join(args.install, "data", "arc")
    os.makedirs(args.out, exist_ok=True)

    def open_arc(rel):
        with open(os.path.join(arcdir, rel), "rb") as fh:
            return quiet(ARC, fh.read())

    # --- dance clips -------------------------------------------------------
    clips = []
    for arc_name in ("mc_male.arc", "mc_female.arc"):
        a = open_arc(arc_name)
        for path in sorted(a.list_files()):
            if path.endswith(".anm"):
                clips.append(clip_fixture(arc_name, path, quiet(a.get_file, path)))
    write(args.out, "dance_clips.json", clips)
    print("dance_clips.json: %d clips (%d fully tracked)" % (len(clips), sum(c["fully_tracked"] for c in clips)))

    # --- stage loops -------------------------------------------------------
    loops = []
    for arc_name in sorted(f for f in os.listdir(arcdir) if f.startswith("mapset_") and f.endswith(".arc")):
        a = open_arc(arc_name)
        for path in sorted(a.list_files()):
            if path.endswith("_play_loop.anm"):
                loops.append(clip_fixture(arc_name, path, quiet(a.get_file, path)))
    write(args.out, "stage_loops.json", loops)
    print("stage_loops.json: %d loops (%d fully tracked)" % (len(loops), sum(c["fully_tracked"] for c in loops)))

    # --- stage cameras -----------------------------------------------------
    a = open_arc(os.path.join("camera", "stage_camera.arc"))
    cams = [camera_fixture(p, quiet(a.get_file, p)) for p in sorted(a.list_files()) if p.endswith(".camanm")]
    write(args.out, "stage_cameras.json", cams)
    print("stage_cameras.json: %d camanms" % len(cams))

    # --- rlists ------------------------------------------------------------
    a = open_arc("startup.arc")
    rlists = {p: parse_rlist(quiet(a.get_file, p)) for p in sorted(a.list_files()) if p.endswith(".rlist")}
    write(args.out, "rlists.json", rlists)
    print("rlists.json: %s" % {os.path.basename(k): len(v) for k, v in rlists.items()})

    # --- pl_emi00 ----------------------------------------------------------
    a = open_arc("pl_emi00.arc")
    b2 = parse_b2it(quiet(a.get_file, "data/chara/pl_emi00/pl_emi00.b2it"))
    m = quiet(parse_model, quiet(a.get_file, "data/chara/pl_emi00/pl_emi00.model"))
    emi = dict(b2it=[[n, i] for n, i in b2],
               bones=dict(parents=[b["parent"] for b in m["bones"]],
                          names=[b["name_folded"] for b in m["bones"]],
                          bind=[rl(b["bind"]) for b in m["bones"]],
                          inverse=[rl(b["inverse_bind"]) for b in m["bones"]]))
    write(args.out, "pl_emi00.json", emi)
    print("pl_emi00.json: %d b2it names, %d bones" % (len(b2), len(m["bones"])))


def write(out_dir, name, obj):
    path = os.path.join(out_dir, name)
    with open(path, "w") as fh:
        json.dump(obj, fh, separators=(",", ":"))
        fh.write("\n")
    print("  wrote %s (%d KB)" % (os.path.relpath(path, os.path.join(HERE, "..")), os.path.getsize(path) // 1024))


if __name__ == "__main__":
    main()
