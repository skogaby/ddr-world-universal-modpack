#!/usr/bin/env python3
"""report_changes.py — human-readable "what changed" report from a build_diff mapping.

build_diff.py maps every function of the OLD build onto the NEW build and labels
each pair (unchanged / changed:refs / changed / callee-changed), plus the lists
of functions only present on one side. This script joins that mapping back onto
the two Ghidra inventories (names, sizes, callers) and, optionally, the modpack
signature sweep (`validate_signatures.sh --json`) so each function can be
labelled with the signature names whose resolved sites fall inside it.

  python3 scripts/build_diff/report_changes.py --diff diff.json
      --old-inv inventory_old.json --new-inv inventory_new.json
      [--old-pescan pescan_old.json --new-pescan pescan_new.json]
      [--sig sig_all.json] [--out report.md] [--max-callers 6]

Classes reported (callee-changed pairs are derivative and omitted):
  changed        instruction shape differs (real code change, or a pass-4 fuzzy match)
  changed:refs   same instruction shape, different referenced data (string,
                 constant, vftable, global) — e.g. a changed immediate or table.
                 A callee's shape is part of the caller's fp_deep, so this
                 cascades: pairs whose direct callees are not all `unchanged`
                 are reported separately as callee-induced.
  new / removed  no counterpart on the other side. The matcher only pairs
                 build-unique fingerprints, so identical duplicate stubs land
                 here in bulk; with the pescan files the report reconciles them
                 (equal fp_deep → identical body, equal fp_shape → refs only)
                 and lists only the residual as genuinely new / removed.

SEH funclets (Unwind@/Catch@…) are counted but not listed: their shape follows
the parent frame layout and carries no information of its own.

Everything here is read-only and stdlib-only. Addresses are printed as RVAs
(file-relative to the image base) — the modpack's docs/ convention.
"""
from __future__ import annotations

import argparse
import bisect
import json
from collections import defaultdict
from pathlib import Path
from typing import Any


def _read(p: Path) -> Any:
    with open(p, encoding="utf-8") as fh:
        return json.load(fh)


class Inv:
    """One Ghidra inventory, indexed by RVA."""

    def __init__(self, path: Path):
        inv = _read(path)
        self.build = inv.get("build", path.stem)
        self.base = int(inv["image_base"], 16)
        self.funcs: dict[int, dict] = {}
        self.callers: dict[int, list[int]] = defaultdict(list)
        ranges: list[tuple[int, int, int]] = []
        for f in inv["functions"]:
            if f.get("external"):
                continue
            rva = int(f["address"], 16) - self.base
            self.funcs[rva] = f
            for c in f.get("callees") or []:
                self.callers[int(c, 16) - self.base].append(rva)
            for s, e in f.get("body") or []:
                ranges.append((int(s, 16) - self.base, int(e, 16) - self.base, rva))
        self.ranges = sorted(ranges)
        self.starts = [s for s, _, _ in self.ranges]
        self.max_len = max((e - s for s, e, _ in self.ranges), default=0)

    def containing(self, site: int) -> int | None:
        i = bisect.bisect_right(self.starts, site) - 1
        while i >= 0 and self.starts[i] >= site - self.max_len:
            s, e, r = self.ranges[i]
            if s <= site <= e:
                return r
            i -= 1
        return None

    def name(self, rva: int) -> str:
        f = self.funcs.get(rva)
        return f.get("name", "?") if f else "?"

    def size(self, rva: int) -> int:
        f = self.funcs.get(rva)
        return int(f.get("size", 0)) if f else 0


def sig_names_by_function(inv: Inv, sig: dict | None, build_label: str) -> dict[int, list[str]]:
    """function RVA -> sorted signature names whose resolved site lies in it."""
    out: dict[int, list[str]] = defaultdict(list)
    if not sig:
        return out
    builds = sig.get("builds", {})
    info = builds.get(build_label)
    if info is None:
        # tolerate labels like "gamemdx_20260915.dll" vs "20260915"
        for k, v in builds.items():
            if build_label in k or k in build_label:
                info = v
                break
    if info is None:
        return out
    for name, addr in info.get("resolved", {}).items():
        r = inv.containing(int(addr, 16))
        if r is not None:
            out[r].append(name)
    for r in out:
        out[r].sort()
    return out


def _h(rva: int) -> str:
    return f"0x{rva:x}"


def _callers(inv: Inv, rva: int, limit: int) -> str:
    cs = sorted(set(inv.callers.get(rva, [])))
    shown = ", ".join(f"{inv.name(c)}@{_h(c)}" for c in cs[:limit])
    more = f" (+{len(cs) - limit} more)" if len(cs) > limit else ""
    return shown + more if cs else "—"


FUNCLET_PREFIXES = ("Unwind@", "Catch@", "Catch_All@", "Filter@", "Finally@")


def is_funclet(name: str) -> bool:
    return name.startswith(FUNCLET_PREFIXES)


def load_prints(path: Path | None) -> dict[int, dict]:
    """RVA -> {fp_shape, fp_deep} from a pescan output (empty when absent)."""
    if path is None:
        return {}
    pe = _read(path)
    return {int(r, 16): {"fp_shape": f["fp_shape"], "fp_deep": f["fp_deep"]} for r, f in pe["functions"].items()}


def reconcile_unmatched(diff: dict, po: dict[int, dict], pn: dict[int, dict]) -> dict[str, Any]:
    """Pair the matcher's leftovers by fingerprint multiset, greedily in RVA order.

    The matcher refuses to pair functions whose fingerprint is not unique in
    their own build (identical stubs, template instances), so those pile up in
    `new` / `removed` even when nothing changed. Equal fp_deep counts on both
    sides mean identical bodies; equal fp_shape means body-identical refs
    aside. What is left is genuinely one-sided.
    """
    new = [int(x, 16) for x in diff["new"]]
    rem = [int(x, 16) for x in diff["removed"]]
    if not po or not pn:
        return {"deep": 0, "shape": 0, "new": new, "removed": rem, "shape_new": [], "shape_removed": []}

    def pair_by(key: str, ns: list[int], os_: list[int]) -> tuple[int, list[int], list[int], list[tuple[int, int]]]:
        by_o: dict[str, list[int]] = defaultdict(list)
        for o in os_:
            by_o[po[o][key]].append(o)
        left_n: list[int] = []
        paired: list[tuple[int, int]] = []
        for n in ns:
            bucket = by_o.get(pn[n][key])
            if bucket:
                paired.append((bucket.pop(0), n))
            else:
                left_n.append(n)
        left_o = sorted(o for b in by_o.values() for o in b)
        return len(paired), left_n, left_o, paired

    n_deep, new, rem, _ = pair_by("fp_deep", new, rem)
    n_shape, new, rem, shape_pairs = pair_by("fp_shape", new, rem)
    return {"deep": n_deep, "shape": n_shape, "new": new, "removed": rem,
            "shape_pairs": shape_pairs}


def callee_induced(diff: dict, new: Inv) -> set[int]:
    """New-side RVAs of changed:refs pairs with a direct callee that is not `unchanged`."""
    cls_of = {int(p["new"], 16): p["class"] for p in diff["pairs"]}
    out: set[int] = set()
    for p in diff["pairs"]:
        if p["class"] != "changed:refs":
            continue
        n = int(p["new"], 16)
        f = new.funcs.get(n, {})
        for c in f.get("callees") or []:
            cr = int(c, 16) - new.base
            if cr not in new.funcs or new.funcs[cr].get("thunk"):
                continue
            if cls_of.get(cr, "new") != "unchanged":
                out.add(n)
                break
    return out


def render(diff: dict, old: Inv, new: Inv, sig_old: dict[int, list[str]], sig_new: dict[int, list[str]],
           max_callers: int, po: dict[int, dict], pn: dict[int, dict]) -> str:
    s = diff["summary"]
    rec = reconcile_unmatched(diff, po, pn)
    induced = callee_induced(diff, new)
    lines = [
        f"# gamemdx build diff: {diff['old_build']} → {diff['new_build']}",
        "",
        f"Participating functions: old {s['old_functions']}, new {s['new_functions']}; "
        f"pairs {s['pairs']} (unchanged {s['classes']['unchanged']}, callee-changed {s['classes']['callee-changed']}, "
        f"changed:refs {s['classes']['changed:refs']}, changed {s['classes']['changed']} "
        f"[low-confidence {s['low_confidence']}]); matcher leftovers: new {s['new']}, removed {s['removed']}.",
        "",
    ]
    if po and pn:
        lines.append(
            f"Leftover reconciliation by fingerprint multiset: {rec['deep']} pairs body-identical (equal fp_deep), "
            f"{rec['shape']} pairs refs-only (equal fp_shape); residual genuinely new {len(rec['new'])}, "
            f"removed {len(rec['removed'])}.")
        lines.append("")
    lines += [
        "Addresses are RVAs (image base 0x180000000). `sigs` = modpack signature names whose "
        "resolved site falls inside the function (old side / new side). SEH funclets are counted, not listed.",
        "",
    ]

    def pair_rows(rows: list[dict], title: str) -> None:
        funclets = sum(1 for p in rows if is_funclet(new.name(int(p["new"], 16))))
        rows = [p for p in rows if not is_funclet(new.name(int(p["new"], 16)))]
        lines.append(f"## {title} ({len(rows)}{f' + {funclets} SEH funclets' if funclets else ''})")
        lines.append("")
        if not rows:
            lines.append("_none_")
            lines.append("")
            return
        lines.append("| old RVA | new RVA | old name | new name | size old→new | pass | sigs | callers (new) |")
        lines.append("|---|---|---|---|---|---|---|---|")
        for p in rows:
            o, n = int(p["old"], 16), int(p["new"], 16)
            sigs = sorted(set(sig_old.get(o, [])) | set(sig_new.get(n, [])))
            lc = " (low)" if p.get("low_confidence") else ""
            lines.append(
                f"| {_h(o)} | {_h(n)} | {old.name(o)} | {new.name(n)} | {old.size(o)}→{new.size(n)} | "
                f"{p['pass']}{lc} | {', '.join(sigs) or '—'} | {_callers(new, n, max_callers)} |")
        lines.append("")

    def side_rows(rvas: list[int], inv: Inv, sigs: dict[int, list[str]], title: str) -> None:
        funclets = sum(1 for r in rvas if is_funclet(inv.name(r)))
        rvas = [r for r in rvas if not is_funclet(inv.name(r))]
        lines.append(f"## {title} ({len(rvas)}{f' + {funclets} SEH funclets' if funclets else ''})")
        lines.append("")
        if not rvas:
            lines.append("_none_")
            lines.append("")
            return
        lines.append("| RVA | name | size | sigs | callers |")
        lines.append("|---|---|---|---|---|")
        for r in rvas:
            lines.append(f"| {_h(r)} | {inv.name(r)} | {inv.size(r)} | {', '.join(sigs.get(r, [])) or '—'} | "
                         f"{_callers(inv, r, max_callers)} |")
        lines.append("")

    changed = [p for p in diff["pairs"] if p["class"] == "changed"]
    refs = [p for p in diff["pairs"] if p["class"] == "changed:refs"]
    refs_direct = [p for p in refs if int(p["new"], 16) not in induced]
    refs_induced = [p for p in refs if int(p["new"], 16) in induced]

    pair_rows(changed, "Changed (instruction shape differs)")
    pair_rows(refs_direct, "Changed references only — direct (all callees unchanged; a string/constant/table/global differs)")
    pair_rows(refs_induced, "Changed references only — callee-induced (a direct callee is itself changed)")
    side_rows(rec["new"], new, sig_new, f"New in {diff['new_build']} (no counterpart)")
    side_rows(rec["removed"], old, sig_old, f"Removed from {diff['old_build']} (no counterpart)")
    if rec.get("shape_pairs"):
        rows = [{"old": _h(o), "new": _h(n), "class": "changed:refs", "pass": "r"} for o, n in rec["shape_pairs"]]
        pair_rows(rows, "Leftovers paired by equal shape (refs differ; pairing by RVA order — indicative only)")

    if diff.get("validation"):
        v = diff["validation"]
        o = v["overall"]
        lines.append("## Matcher validation against the signature sweep")
        lines.append("")
        lines.append(f"{v['ground_truth']['scored_pairs']} ground-truth pairs; precision {o['precision']}, "
                     f"recall {o['recall']}; wrong pairs {v['wrong_total']}.")
        for w in v.get("wrong", []):
            lines.append(f"- pass {w['pass']}: {w['old']} {w['old_name']} → {w['new']} {w['new_name']} "
                         f"(truth {'/'.join(w['truth_names']) or 'none'} → {w['truth_new']})")
        lines.append("")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--diff", type=Path, required=True)
    ap.add_argument("--old-inv", type=Path, required=True)
    ap.add_argument("--new-inv", type=Path, required=True)
    ap.add_argument("--old-pescan", type=Path, help="pescan output of the old build (enables leftover reconciliation)")
    ap.add_argument("--new-pescan", type=Path, help="pescan output of the new build")
    ap.add_argument("--sig", type=Path, help="validate_signatures.sh --json output covering both builds")
    ap.add_argument("--out", type=Path, help="write the Markdown report here (default: stdout)")
    ap.add_argument("--max-callers", type=int, default=6)
    args = ap.parse_args(argv)
    if bool(args.old_pescan) != bool(args.new_pescan):
        ap.error("--old-pescan and --new-pescan go together")

    diff = _read(args.diff)
    old, new = Inv(args.old_inv), Inv(args.new_inv)
    sig = _read(args.sig) if args.sig else None
    sig_old = sig_names_by_function(old, sig, diff["old_build"])
    sig_new = sig_names_by_function(new, sig, diff["new_build"])
    text = render(diff, old, new, sig_old, sig_new, args.max_callers,
                  load_prints(args.old_pescan), load_prints(args.new_pescan))
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text + "\n", encoding="utf-8")
        print(f"report written to {args.out}")
    else:
        print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
