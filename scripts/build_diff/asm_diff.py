#!/usr/bin/env python3
"""asm_diff.py — normalised disassembly diff of the `changed` pairs of a build_diff mapping.

build_diff.py says WHICH functions changed; this says WHAT changed inside them.
For every pair of class `changed` (SEH funclets skipped) it runs Ghidra headless
with GhidraDecompile.java on both builds, then diffs the disassembly with the
relocation noise masked out:

  - absolute image addresses (0x18xxxxxxx)          → ADDR
  - stack slots [RSP+d] / [RBP+d]                    → [RSP+S] / [RBP+S]
  - image-relative displacements [reg + RVA]         → [… + RVA]
    (MSVC's `lea rbx,[image_base]; mov eax,[rbx+rax*4+0x6f2ed0]` idiom; pescan
    masks RIP-relative operands but not these, so a function that only moved
    its tables shows up as `changed` with an empty diff here — the diff count
    is the real signal)

Output: <work>/asm_diff_<old>__<new>.md, one section per pair sorted by diff
size, plus a one-line summary per pair on stdout. Decompiler text is Konami
code: everything stays in the scratch dir, nothing is written under the repo.

  python3 scripts/build_diff/asm_diff.py --diff <work>/diff_OLD__NEW.json --old 20260915 --new 20261027
      [--work DIR] [--context 2] [--pairs 0x1234,0x5678   # new-side RVAs, default: every `changed` pair]
      [--skip-decompile   # reuse <work>/dec/<build>/gamemdx_<rva>.txt]

Env: GHIDRA_HOME (default /opt/homebrew/opt/ghidra/libexec), GHIDRA_PROJECT
(default ~/Projects/DDRWorld_Ghidra), BUILD_DIFF_WORK (default
${TMPDIR:-/tmp}/ddr_build_diff). The Ghidra GUI must be closed.
"""
from __future__ import annotations

import argparse
import difflib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
FUNCLET = re.compile(r"^(Unwind|Catch|Catch_All|Filter|Finally)@")


def work_dir() -> Path:
    return Path(os.environ.get("BUILD_DIFF_WORK", str(Path(os.environ.get("TMPDIR", "/tmp")) / "ddr_build_diff")))


def decompile(build: str, rvas: list[str], out_dir: Path) -> None:
    ghidra_home = Path(os.environ.get("GHIDRA_HOME", "/opt/homebrew/opt/ghidra/libexec"))
    analyze = ghidra_home / "support/analyzeHeadless"
    if not analyze.exists():
        sys.exit(f"analyzeHeadless not found at {analyze} (set GHIDRA_HOME)")
    project = Path(os.path.expanduser(os.environ.get("GHIDRA_PROJECT", "~/Projects/DDRWorld_Ghidra")))
    if not Path(f"{project}.gpr").exists():
        sys.exit(f"ghidra project not found: {project}.gpr (set GHIDRA_PROJECT)")
    out_dir.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, RUSTOLUTION_RVAS=",".join(rvas), RUSTOLUTION_OUT_DIR=str(out_dir))
    cmd = [str(analyze), str(project.parent), project.name, "-process", f"gamemdx_{build}.dll", "-noanalysis",
           "-scriptPath", str(HERE), "-postScript", "GhidraDecompile.java"]
    log = out_dir.parent / f"decompile_{build}.log"
    with open(log, "w") as fh:
        r = subprocess.run(cmd, env=env, stdout=fh, stderr=subprocess.STDOUT)
    if r.returncode != 0:
        sys.exit(f"headless decompile failed for {build} (see {log})")


def asm_lines(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8", errors="replace")
    if "// ---- disassembly ----" not in text:
        return []
    out = []
    for line in text.split("// ---- disassembly ----", 1)[1].splitlines():
        line = line.strip()
        if not line:
            continue
        line = re.sub(r"^[0-9a-f]+\s+", "", line)
        line = re.sub(r"0x18[0-9a-f]{7}", "ADDR", line)
        line = re.sub(r"\[RSP \+ 0x[0-9a-f]+\]", "[RSP+S]", line)
        line = re.sub(r"\[RBP \+ -?0x[0-9a-f]+\]", "[RBP+S]", line)
        line = re.sub(r"(\[[^\]]*\+ )0x[0-9a-f]{5,7}\]", r"\1RVA]", line)
        out.append(line)
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--diff", type=Path, required=True)
    ap.add_argument("--old", required=True, help="old build id, e.g. 20260915")
    ap.add_argument("--new", required=True, help="new build id, e.g. 20261027")
    ap.add_argument("--work", type=Path, default=None)
    ap.add_argument("--context", type=int, default=2)
    ap.add_argument("--pairs", help="comma-separated new-side RVAs to restrict to")
    ap.add_argument("--skip-decompile", action="store_true")
    args = ap.parse_args(argv)
    work = args.work or work_dir()

    diff = json.loads(args.diff.read_text())
    names: dict[str, str] = {}
    inv_new = work / f"inventory_gamemdx_{args.new}.json"
    if inv_new.exists():
        inv = json.loads(inv_new.read_text())
        base = int(inv["image_base"], 16)
        names = {"0x%x" % (int(f["address"], 16) - base): f.get("name", "") for f in inv["functions"]}
    pairs = [p for p in diff["pairs"] if p["class"] == "changed" and not FUNCLET.match(names.get(p["new"], ""))]
    if args.pairs:
        want = {x.strip().lower() for x in args.pairs.split(",")}
        pairs = [p for p in pairs if p["new"].lower() in want]
    if not pairs:
        print("no pairs to diff")
        return 0

    dec_old, dec_new = work / "dec" / args.old, work / "dec" / args.new
    if not args.skip_decompile:
        decompile(args.old, [p["old"] for p in pairs], dec_old)
        decompile(args.new, [p["new"] for p in pairs], dec_new)

    results = []
    for p in pairs:
        a = asm_lines(dec_old / f"gamemdx_{p['old']}.txt")
        b = asm_lines(dec_new / f"gamemdx_{p['new']}.txt")
        hunks = [l for l in difflib.unified_diff(a, b, lineterm="", n=args.context) if not l.startswith(("+++", "---"))]
        n = sum(1 for l in hunks if l[0] in "+-")
        results.append((n, p, len(a), len(b), hunks))
    results.sort(key=lambda r: r[0])

    out = work / f"asm_diff_{args.old}__{args.new}.md"
    with open(out, "w", encoding="utf-8") as fh:
        fh.write(f"# normalised asm diff {args.old} → {args.new}\n\n")
        for n, p, la, lb, hunks in results:
            fh.write(f"## {p['old']} → {p['new']} {names.get(p['new'], '')}  ({la}/{lb} insns, {n} diff lines"
                     f"{', low confidence' if p.get('low_confidence') else ''})\n\n```\n" + "\n".join(hunks) + "\n```\n\n")
    for n, p, la, lb, _ in results:
        flag = "  (relocation-only: displacement moves)" if n == 0 else ""
        print(f"{p['old']} -> {p['new']} {names.get(p['new'], ''):<16} {la:>5}/{lb:<5} insns  {n:>5} diff lines{flag}")
    print(f"written: {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
