#!/usr/bin/env python3
"""Post-match shape diff across builds.

Consumers of a resolved signature routinely read bytes at `match + N`
(patch imm8/imm32 at +N, decode a disp32 at +N, verify an opcode at +N).
An AOB can HIT on every build while the instructions after the literal
prefix differ — that is exactly the class of break the 2026-09 field bug
reports were about. This tool takes the per-build resolved offsets from the
harness JSON, disassembles a window after every match on every build, and
reports the first offset at which the normalized instruction stream
diverges from the reference build.

Normalization keeps mnemonics, register operands, memory shapes, scale/
index, non-RIP displacements and immediates (consumers compare those), and
wildcards only RIP-relative displacements and direct branch/call targets
(they legitimately differ between builds).

Usage:
  shape_diff.py --json sweep.json --dir ~/Desktop/ddr_modules [--ref 20260721]
                [--window 0x180] [--names a,b,c] [--verbose]
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

import capstone
import pefile

RIP_RE = re.compile(r"\[rip [+-] 0x[0-9a-f]+\]")
# `[reg + reg*n + 0xRVA]` — module-base-relative addressing (jump tables,
# base-register-anchored globals). The displacement IS an image RVA and moves
# between builds exactly like a RIP displacement does.
RVA_DISP_RE = re.compile(r"(\[[^\]]*?) \+ 0x([0-9a-f]{5,7})\]")


class Image:
    def __init__(self, path: Path):
        self.path = path
        self.pe = pefile.PE(str(path), fast_load=True)
        self.image = self.pe.get_memory_mapped_image()
        self.text = [
            (s.VirtualAddress, s.VirtualAddress + max(s.Misc_VirtualSize, s.SizeOfRawData))
            for s in self.pe.sections
            if s.Characteristics & 0x20000000  # IMAGE_SCN_MEM_EXECUTE
        ]
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
        self.md.detail = False

    def in_text(self, rva: int) -> bool:
        return any(lo <= rva < hi for lo, hi in self.text)

    def disasm(self, rva: int, window: int) -> list[tuple[int, str]]:
        code = self.image[rva : rva + window]
        out = []
        for insn in self.md.disasm(code, 0):
            out.append((insn.address, normalize(insn)))
        return out


def normalize(insn) -> str:
    mn = insn.mnemonic
    ops = insn.op_str
    ops = RIP_RE.sub("[rip+D]", ops)
    ops = RVA_DISP_RE.sub(lambda m: f"{m.group(1)} + RVA]", ops)
    if mn.startswith("j") or mn in ("call", "loop", "loope", "loopne"):
        # direct target immediates -> T; keep register/memory forms
        if re.fullmatch(r"0x[0-9a-f]+", ops):
            ops = "T"
    return f"{mn} {ops}".strip()


def find_dll(dir_: Path, build: str) -> Path | None:
    for cand in [dir_ / f"gamemdx_{build}.dll", dir_ / build / "gamemdx.dll",
                 *dir_.glob(f"*{build}*/gamemdx*.dll")]:
        if cand.exists():
            return cand
    return None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", required=True, type=Path)
    ap.add_argument("--dir", required=True, type=Path)
    ap.add_argument("--ref", default=None)
    ap.add_argument("--window", default="0x180")
    ap.add_argument("--names", default=None)
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()
    window = int(args.window, 0)

    data = json.loads(args.json.read_text())
    builds = sorted(data["builds"])
    ref = args.ref or ("20260721" if "20260721" in builds else builds[-1])
    if ref not in builds:
        sys.exit(f"reference build {ref} not in sweep")
    images: dict[str, Image] = {}
    for b in builds:
        p = find_dll(Path(str(args.dir).replace("~", str(Path.home()))), b)
        if not p:
            sys.exit(f"cannot find gamemdx for build {b} under {args.dir}")
        images[b] = Image(p)

    consumers = data.get("consumers", {})
    only = set(args.names.split(",")) if args.names else None

    names = sorted(set(data["builds"][ref]["resolved"]))
    rows = []
    for name in names:
        if only and name not in only:
            continue
        offs = {}
        for b in builds:
            v = data["builds"][b]["resolved"].get(name)
            if v is None:
                continue
            offs[b] = int(v, 16)
        if ref not in offs:
            continue
        # Non-text matches (RTTI strings, vtables, globals) are not code.
        if not images[ref].in_text(offs[ref]):
            rows.append((name, "data", {}, offs))
            continue
        ref_stream = images[ref].disasm(offs[ref], window)
        ref_len = ref_stream[-1][0] if ref_stream else 0
        diverge: dict[str, int | None] = {}
        for b in builds:
            if b == ref or b not in offs:
                continue
            if not images[b].in_text(offs[b]):
                diverge[b] = -1
                continue
            other = images[b].disasm(offs[b], window)
            first = None
            for (ra, rs), (oa, os_) in zip(ref_stream, other):
                if ra != oa or rs != os_:
                    first = min(ra, oa)
                    break
            if first is None and len(other) != len(ref_stream):
                first = min(ref_len, other[-1][0] if other else 0)
            diverge[b] = first
        rows.append((name, "code", diverge, offs))

    identical = []
    print("=" * 78)
    print(f"POST-MATCH SHAPE DIFF  (reference build {ref}, window 0x{window:X})")
    print("=" * 78)
    print("first divergence offset (bytes after match) per build; '=' = identical through window\n")
    hdr = f"  {'signature':<44}" + "".join(f"{b:>10}" for b in builds if b != ref)
    print(hdr)
    for name, kind, diverge, offs in rows:
        if kind == "data":
            continue
        cells = []
        clean = True
        for b in builds:
            if b == ref:
                continue
            d = diverge.get(b)
            if b not in offs:
                cells.append(f"{'n/a':>10}")
            elif d is None:
                cells.append(f"{'=':>10}")
            elif d == -1:
                cells.append(f"{'DATA':>10}")
                clean = False
            else:
                cells.append(f"{('+0x%X' % d):>10}")
                clean = False
        if clean:
            identical.append(name)
            if not args.verbose:
                continue
        print(f"  {name:<44}" + "".join(cells))
    print(f"\n  ({len(identical)} signature(s) byte-shape-identical through the window; "
          f"use --verbose to list)")

    print("\n" + "-" * 78)
    print("DIVERGENT SIGNATURES — consumers to review")
    print("-" * 78)
    for name, kind, diverge, offs in rows:
        if kind != "code":
            continue
        ds = {b: d for b, d in diverge.items() if d is not None}
        if not ds:
            continue
        cons = consumers.get(name, {})
        cons_txt = "; ".join(f"{u}[{'/'.join(k)}]" for u, k in sorted(cons.items())) or "(derivation input only)"
        print(f"\n  {name}")
        print(f"    match rva : " + ", ".join(f"{b}=+0x{o:X}" for b, o in sorted(offs.items())))
        print(f"    diverges  : " + ", ".join(
            f"{b} at +0x{d:X}" if d >= 0 else f"{b} NOT CODE" for b, d in sorted(ds.items())))
        print(f"    consumers : {cons_txt}")
        if args.verbose:
            first = min(d for d in ds.values() if d >= 0) if any(d >= 0 for d in ds.values()) else 0
            lo = max(0, first - 0x10)
            for b in builds:
                if b not in offs or not images[b].in_text(offs[b]):
                    continue
                print(f"    -- {b}")
                for a, s in images[b].disasm(offs[b], min(window, first + 0x30)):
                    if a >= lo:
                        print(f"       +0x{a:03X}  {s}")

    data_rows = [r for r in rows if r[1] == "data"]
    print("\n" + "-" * 78)
    print(f"NON-CODE MATCHES (RTTI / vtables / globals) — {len(data_rows)} name(s), not shape-diffed")
    print("-" * 78)
    if args.verbose:
        for name, _, _, offs in data_rows:
            print(f"  {name:<44}" + ", ".join(f"{b}=+0x{o:X}" for b, o in sorted(offs.items())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
