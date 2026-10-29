#!/usr/bin/env python3
"""build_diff.py — map the functions of one build of a module onto another (W-107).

Implements the matcher of M1 brief §4.3. Inputs are the two raw Ghidra
inventories and their pescan outputs (both transient, outside the repo); the
mapping goes to /tmp/rustolution/ by default. Nothing binary-derived is written
under the repo: the report is stdout, the mapping is a transient JSON file.

  python3 scripts/build_diff.py --old-inv A --old-pescan B --new-inv C --new-pescan D
      [--truth sig_all.json [--old-build 20260825 --new-build 20260915]]
      [--out PATH] [--minhash-threshold 0.80]

Only non-external, non-thunk functions take part; a function must also have a
pescan fingerprint. Functions are identified by RVA (address - image_base).

Passes, each over still-unmatched functions only (brief §4.3):
  1  unique fp_deep on both sides                 -> unchanged
  2  unique fp_shape on both sides                -> changed:refs if fp_deep differs, else unchanged
  3  propagation from matched pairs to a fixed point, over callees (sorted by
     RVA, unmatched only) and callers, all-or-nothing per group: equal counts
     and every i-th pair feature-compatible -> label by the same rule as pass 2
  4  mutual-best MinHash similarity >= threshold, LSH candidates -> changed, low_confidence
  5  leftovers: new (new side), removed (old side)
  6  callee-changed: an unchanged pair that reaches, through the new call graph,
     a function that is not unchanged or is new (SCC condensation, no recursion)

Validation (--truth): a signature name resolved in both builds names one
function per build. Ground-truth pairs are scored against the matcher's pairs;
see validate() for the exact denominators.

Output JSON (format rustolution-build-diff/1). The build labels are
"old_build" / "new_build": the brief's "old"/"new" would collide with the "new"
list of unmatched new functions.
"""
from __future__ import annotations

import argparse
import bisect
import json
import re
import sys
import time
from collections import Counter, defaultdict, deque
from pathlib import Path
from typing import Any

FORMAT = "rustolution-build-diff/1"
PESCAN_FORMAT = "rustolution-pescan/1"
DEFAULT_THRESHOLD = 0.80
BANDS = 16
ROWS = 4
MINHASH_COMPAT = 0.5
TRANSIENT = Path("/tmp/rustolution")
WRONG_SHOWN = 20
CLASSES = ("unchanged", "callee-changed", "changed:refs", "changed")


# ---------------------------------------------------------------- loading

class Side:
    """One build: participating functions, call graph and body index (RVAs)."""

    def __init__(self, build: str, funcs: dict[int, dict], callees: dict[int, list[int]],
                 ranges: list[tuple[int, int, int]], unfingerprinted: list[int]):
        self.build = build
        self.funcs = funcs
        self.callees = callees
        self.callers: dict[int, list[int]] = defaultdict(list)
        for r in sorted(callees):
            for c in callees[r]:
                self.callers[c].append(r)
        self.ranges = sorted(ranges)
        self.starts = [s for s, _, _ in self.ranges]
        self.max_len = max((e - s for s, e, _ in self.ranges), default=0)
        self.unfingerprinted = sorted(unfingerprinted)

    def containing(self, site: int) -> tuple[int, int] | None:
        """(function RVA, RVA where its body range starts) of the innermost body
        containing `site`, or None. Offset = site - start: 0 is an entry site."""
        i = bisect.bisect_right(self.starts, site) - 1
        while i >= 0 and self.starts[i] >= site - self.max_len:
            s, e, r = self.ranges[i]
            if s <= site <= e:
                return r, s
            i -= 1
        return None


def _read_json(path: Path) -> Any:
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def _hex(rva: int) -> str:
    return "0x%x" % rva


def _build_label(build: str) -> str:
    m = re.search(r"(\d{8})", build or "")
    return m.group(1) if m else (build or "unknown")


def load_side(inv_path: Path, pescan_path: Path) -> Side:
    inv = _read_json(inv_path)
    pe = _read_json(pescan_path)
    if pe.get("format") != PESCAN_FORMAT:
        raise ValueError(f"{pescan_path.name}: format {pe.get('format')!r}, expected {PESCAN_FORMAT!r}")
    base = int(inv["image_base"], 16)
    pfuncs = pe.get("functions", {})
    funcs: dict[int, dict] = {}
    raw_callees: dict[int, list[int]] = {}
    ranges: list[tuple[int, int, int]] = []
    unfingerprinted: list[int] = []
    for f in inv["functions"]:
        if f.get("external"):
            continue
        rva = int(f["address"], 16) - base
        for start, end in f.get("body") or []:
            ranges.append((int(start, 16) - base, int(end, 16) - base, rva))
        if f.get("thunk"):
            continue
        p = pfuncs.get(_hex(rva))
        if p is None:
            unfingerprinted.append(rva)
            continue
        if rva in funcs:
            raise ValueError(f"{inv_path.name}: duplicate function at {_hex(rva)}")
        feats = p["features"]
        funcs[rva] = {
            "name": f.get("name", ""),
            "fp_shape": p["fp_shape"],
            "fp_deep": p["fp_deep"],
            "insns": feats["insns"],
            "calls": feats["calls"],
            "minhash": tuple(feats["minhash"]),
        }
        raw_callees[rva] = [int(c, 16) - base for c in f.get("callees") or []]
    # Callees that are not participants (thunks, externals, indirect-call
    # constants the exporter recorded as callees) take no part in matching.
    callees = {r: sorted({c for c in raw if c in funcs}) for r, raw in raw_callees.items()}
    return Side(_build_label(inv.get("build", "")), funcs, callees, ranges, unfingerprinted)


# ---------------------------------------------------------------- features

def jaccard(a: tuple, b: tuple) -> float:
    """MinHash Jaccard estimate: the fraction of equal positions."""
    return sum(1 for x, y in zip(a, b) if x == y) / len(a)


def compatible(fo: dict, fn: dict) -> bool:
    a, b = fo["insns"], fn["insns"]
    if abs(a - b) > max(3, 0.25 * max(a, b)):
        return False
    if abs(fo["calls"] - fn["calls"]) > 2:
        return False
    return jaccard(fo["minhash"], fn["minhash"]) >= MINHASH_COMPAT


def _label(fo: dict, fn: dict) -> str:
    if fo["fp_deep"] == fn["fp_deep"]:
        return "unchanged"
    if fo["fp_shape"] == fn["fp_shape"]:
        return "changed:refs"
    return "changed"


# ---------------------------------------------------------------- matching

class _Matcher:
    def __init__(self, old: Side, new: Side, threshold: float):
        self.old, self.new, self.thr = old, new, threshold
        self.o2n: dict[int, int] = {}
        self.n2o: dict[int, int] = {}
        # keyed by new RVA: (old RVA, class, pass, low_confidence)
        self.rec: dict[int, tuple[int, str, int, bool]] = {}

    def add(self, o: int, n: int, pass_no: int, cls: str | None = None, low: bool = False) -> None:
        assert o not in self.o2n and n not in self.n2o
        if cls is None:
            cls = _label(self.old.funcs[o], self.new.funcs[n])
        self.o2n[o] = n
        self.n2o[n] = o
        self.rec[n] = (o, cls, pass_no, low)


def _unique_pass(m: _Matcher, field: str, pass_no: int) -> None:
    um_o = [r for r in sorted(m.old.funcs) if r not in m.o2n]
    um_n = [r for r in sorted(m.new.funcs) if r not in m.n2o]
    co = Counter(m.old.funcs[r][field] for r in um_o)
    cn = Counter(m.new.funcs[r][field] for r in um_n)
    idx = {m.old.funcs[r][field]: r for r in um_o if co[m.old.funcs[r][field]] == 1}
    for n in um_n:
        v = m.new.funcs[n][field]
        if cn[v] == 1 and v in idx:
            m.add(idx[v], n, pass_no)


def _propagate(m: _Matcher) -> None:
    """Worklist to the fixed point. A pair is re-examined only when a neighbour
    of its own (callee or caller) on either side has just been matched, because
    that is the only way its unmatched neighbourhood can change."""
    queue: deque[int] = deque(sorted(m.rec))
    queued = set(queue)
    while queue:
        n = queue.popleft()
        queued.discard(n)
        o = m.rec[n][0]
        for nbr_old, nbr_new in ((m.old.callees, m.new.callees), (m.old.callers, m.new.callers)):
            oc = sorted(c for c in nbr_old.get(o, ()) if c not in m.o2n)
            nc = sorted(c for c in nbr_new.get(n, ()) if c not in m.n2o)
            if not oc or len(oc) != len(nc):
                continue
            if all(compatible(m.old.funcs[a], m.new.funcs[b]) for a, b in zip(oc, nc)):
                for a, b in zip(oc, nc):
                    m.add(a, b, 3)
                    _requeue_around(m, a, b, queue, queued)


def _requeue_around(m: _Matcher, a: int, b: int, queue: deque, queued: set) -> None:
    """A new pair (a, b) changes the unmatched neighbourhoods of matched pairs
    whose old side neighbours a, or whose new side neighbours b. Re-examine them,
    and the new pair itself."""
    touched: list[int] = [b]
    for x in [*m.old.callees.get(a, ()), *m.old.callers.get(a, ())]:
        if x in m.o2n:
            touched.append(m.o2n[x])
    for y in [*m.new.callees.get(b, ()), *m.new.callers.get(b, ())]:
        if y in m.rec:
            touched.append(y)
    for t in touched:
        if t not in queued:
            queued.add(t)
            queue.append(t)


def _lsh_pass(m: _Matcher) -> None:
    """Pass 4 over functions whose full MinHash is unique within their own build.

    A signature shared by several functions of one build (4,828 two-instruction
    stubs share one on 20260825) has no unique best match, and pairing one of
    them is a guess among identical bodies: the measured pass-4 pairs were all at
    similarity 1.0 and all duplicates. Restricting to build-unique signatures
    removes that, and makes the pass-4 label `changed` exact: a pair cannot be
    fp_shape- or fp_deep-equal without a duplicate on one side, which is
    excluded. Within the eligible set, groups by signature stay singletons, so
    the candidate set stays small and every pair is an unambiguous mutual best.
    """
    count_o = Counter(f["minhash"] for f in m.old.funcs.values())
    count_n = Counter(f["minhash"] for f in m.new.funcs.values())
    um_o = sorted(r for r in m.old.funcs
                  if r not in m.o2n and count_o[m.old.funcs[r]["minhash"]] == 1)
    um_n = sorted(r for r in m.new.funcs
                  if r not in m.n2o and count_n[m.new.funcs[r]["minhash"]] == 1)
    og: dict[tuple, list[int]] = defaultdict(list)
    ng: dict[tuple, list[int]] = defaultdict(list)
    for o in um_o:
        og[m.old.funcs[o]["minhash"]].append(o)
    for n in um_n:
        ng[m.new.funcs[n]["minhash"]].append(n)
    buckets: dict[tuple, list[tuple]] = defaultdict(list)
    for mh in og:
        for b in range(BANDS):
            buckets[(b, mh[b * ROWS:(b + 1) * ROWS])].append(mh)
    cands: set[tuple[tuple, tuple]] = set()
    for mh_n in ng:
        for b in range(BANDS):
            for mh_o in buckets.get((b, mh_n[b * ROWS:(b + 1) * ROWS]), ()):
                cands.add((mh_o, mh_n))
    sim = {(a, b): jaccard(a, b) for a, b in cands}
    # per group: the best similarity and every candidate group that reaches it
    best_for_o: dict[tuple, list[tuple]] = defaultdict(list)
    best_for_n: dict[tuple, list[tuple]] = defaultdict(list)
    top_o: dict[tuple, float] = {}
    top_n: dict[tuple, float] = {}
    for (a, b), s_ in sim.items():
        top_o[a] = max(top_o.get(a, -1.0), s_)
        top_n[b] = max(top_n.get(b, -1.0), s_)
    for (a, b), s_ in sim.items():
        if s_ == top_o[a]:
            best_for_o[a].append(b)
        if s_ == top_n[b]:
            best_for_n[b].append(a)
    for (a, b) in sorted(sim):
        s_ = sim[(a, b)]
        if s_ < m.thr or len(og[a]) != 1 or len(ng[b]) != 1:
            continue
        if best_for_o[a] == [b] and best_for_n[b] == [a]:
            m.add(og[a][0], ng[b][0], 4, cls="changed", low=True)


def _tarjan(nodes: list[int], succ: dict[int, list[int]]) -> list[list[int]]:
    """Strongly connected components, iterative; sinks first (reverse topological)."""
    index: dict[int, int] = {}
    low: dict[int, int] = {}
    on_stack: set[int] = set()
    stack: list[int] = []
    comps: list[list[int]] = []
    counter = 0
    for root in nodes:
        if root in index:
            continue
        index[root] = low[root] = counter
        counter += 1
        stack.append(root)
        on_stack.add(root)
        work = [(root, 0)]
        while work:
            v, i = work[-1]
            out = succ.get(v, [])
            if i < len(out):
                work[-1] = (v, i + 1)
                w = out[i]
                if w not in index:
                    index[w] = low[w] = counter
                    counter += 1
                    stack.append(w)
                    on_stack.add(w)
                    work.append((w, 0))
                elif w in on_stack:
                    low[v] = min(low[v], index[w])
            else:
                work.pop()
                if work:
                    u = work[-1][0]
                    low[u] = min(low[u], low[v])
                if low[v] == index[v]:
                    comp = []
                    while True:
                        w = stack.pop()
                        on_stack.discard(w)
                        comp.append(w)
                        if w == v:
                            break
                    comps.append(sorted(comp))
    return comps


def _callee_changed(m: _Matcher) -> None:
    """Mark unchanged pairs that reach a non-unchanged or new function (new call graph)."""
    nodes = sorted(m.new.funcs)
    succ = m.new.callees
    comps = _tarjan(nodes, succ)
    comp_of = {v: i for i, comp in enumerate(comps) for v in comp}
    base_bad = {n: (n not in m.n2o) or m.rec[n][1] != "unchanged" for n in nodes}
    closure = [False] * len(comps)
    flagged: set[int] = set()
    for i, comp in enumerate(comps):
        has_bad = any(base_bad[v] for v in comp)
        ext = any(closure[comp_of[w]] for v in comp for w in succ.get(v, ()) if comp_of[w] != i)
        closure[i] = has_bad or ext
        if ext or (len(comp) > 1 and has_bad):
            flagged.update(v for v in comp if v in m.rec and m.rec[v][1] == "unchanged")
    for n in flagged:
        o, _, p, low = m.rec[n]
        m.rec[n] = (o, "callee-changed", p, low)


def match(old: Side, new: Side, threshold: float = DEFAULT_THRESHOLD) -> dict[str, Any]:
    m = _Matcher(old, new, threshold)
    _unique_pass(m, "fp_deep", 1)
    _unique_pass(m, "fp_shape", 2)
    _propagate(m)
    _lsh_pass(m)
    new_only = [n for n in sorted(new.funcs) if n not in m.n2o]
    removed = [o for o in sorted(old.funcs) if o not in m.o2n]
    _callee_changed(m)
    pairs = [
        {"old": _hex(o), "new": _hex(n), "class": cls, "pass": p, **({"low_confidence": True} if low else {})}
        for n, (o, cls, p, low) in sorted(m.rec.items())
    ]
    return {"pairs": pairs, "new": [_hex(n) for n in new_only], "removed": [_hex(o) for o in removed],
            "_rec": m.rec}


# ---------------------------------------------------------------- validation

def _ground_truth(old: Side, new: Side, sig: dict, old_label: str, new_label: str) -> dict[str, Any]:
    builds = sig.get("builds", {})
    for lab in (old_label, new_label):
        if lab not in builds:
            raise ValueError(f"truth file has no build {lab!r}; builds: {sorted(builds)}")
    res_o = builds[old_label].get("resolved", {})
    res_n = builds[new_label].get("resolved", {})
    names = sorted(set(res_o) & set(res_n))
    dropped = 0
    pairs: dict[tuple[int, int], list[str]] = defaultdict(list)
    mid_pairs: dict[tuple[int, int], bool] = {}
    for name in names:
        so = int(res_o[name], 16)
        sn = int(res_n[name], 16)
        co = old.containing(so)
        cn = new.containing(sn)
        if co is None or cn is None:
            dropped += 1
            continue
        o, n = co[0], cn[0]
        mid = (so - co[1] > 0) or (sn - cn[1] > 0)
        pairs[(o, n)].append(name)
        mid_pairs[(o, n)] = mid_pairs.get((o, n), False) or mid
    by_old: dict[int, set[int]] = defaultdict(set)
    by_new: dict[int, set[int]] = defaultdict(set)
    for o, n in pairs:
        by_old[o].add(n)
        by_new[n].add(o)
    conflicts = [(o, n) for (o, n) in pairs if len(by_old[o]) > 1 or len(by_new[n]) > 1]
    excl_old: set[int] = set()
    excl_new: set[int] = set()
    for o, n in conflicts:
        excl_old.add(o)
        excl_new.add(n)
    scorable: dict[tuple[int, int], list[str]] = {}
    unscorable = 0
    for (o, n), ns in sorted(pairs.items()):
        if (o, n) in conflicts:
            continue
        if o not in old.funcs or n not in new.funcs:
            unscorable += 1
            excl_old.add(o)
            excl_new.add(n)
            continue
        scorable[(o, n)] = ns
    return {
        "mid_function": {k: v for k, v in mid_pairs.items() if k in scorable},
        "names_in_both": len(names),
        "dropped_no_function": dropped,
        "conflict_pairs": len(conflicts),
        "conflict_names": sorted({nm for k in conflicts for nm in pairs[k]}),
        "unscorable": unscorable,
        "scorable": scorable,
        "excl_old": excl_old,
        "excl_new": excl_new,
    }


def _rate(num: int, den: int) -> float | None:
    return round(num / den, 4) if den else None


def validate(rec: dict[int, tuple[int, str, int, bool]], old: Side, new: Side, sig: dict,
             old_label: str, new_label: str) -> dict[str, Any]:
    gt = _ground_truth(old, new, sig, old_label, new_label)
    truth = gt["scorable"]
    tf_old = {o for o, _ in truth}
    tf_new = {n for _, n in truth}
    truth_partner = {o: n for (o, n) in truth}
    truth_names = {k: v for k, v in truth.items()}
    per_pass: dict[int, Counter] = defaultdict(Counter)
    wrong: list[dict[str, Any]] = []
    found_by: dict[tuple[int, int], int] = {}
    for n, (o, _cls, p, _low) in rec.items():
        if o in gt["excl_old"] or n in gt["excl_new"]:
            continue
        if o not in tf_old and n not in tf_new:
            continue
        per_pass[p]["pairs"] += 1
        if (o, n) in truth:
            per_pass[p]["correct"] += 1
            found_by[(o, n)] = p
        else:
            wrong.append({
                "pass": p, "old": _hex(o), "new": _hex(n),
                "old_name": old.funcs[o]["name"], "new_name": new.funcs[n]["name"],
                "truth_new": _hex(truth_partner[o]) if o in truth_partner else None,
                "truth_names": truth_names[(o, truth_partner[o])] if o in truth_partner else [],
            })
    wrong.sort(key=lambda w: (w["pass"], w["new"]))
    total_truth = len(truth)
    for p in found_by.values():
        per_pass[p]["found"] += 1
    found_mid = sum(1 for k in found_by if gt["mid_function"].get(k))
    scored_mid = sum(1 for v in gt["mid_function"].values() if v)
    passes: dict[str, dict[str, Any]] = {}
    for p in (1, 2, 3, 4):
        c = per_pass[p]
        passes[str(p)] = {"pairs": c["pairs"], "correct": c["correct"],
                          "precision": _rate(c["correct"], c["pairs"]),
                          "found": c["found"], "recall": _rate(c["found"], total_truth)}
    tot_pairs = sum(per_pass[p]["pairs"] for p in per_pass)
    tot_correct = sum(per_pass[p]["correct"] for p in per_pass)
    tot_found = len(found_by)
    return {
        "ground_truth": {
            "names_in_both_builds": gt["names_in_both"],
            "dropped_no_function": gt["dropped_no_function"],
            "conflict_pairs_excluded": gt["conflict_pairs"],
            "conflict_names": gt["conflict_names"],
            "unscorable_pairs_excluded": gt["unscorable"],
            "scored_pairs": total_truth,
            "scored_mid_function": scored_mid,
        },
        "per_pass": passes,
        "overall": {"pairs": tot_pairs, "correct": tot_correct,
                    "precision": _rate(tot_correct, tot_pairs),
                    "found": tot_found, "recall": _rate(tot_found, total_truth)},
        "recall_entry_sites": _rate(tot_found - found_mid, total_truth - scored_mid),
        "recall_mid_function_sites": _rate(found_mid, scored_mid),
        "wrong_total": len(wrong),
        "wrong": wrong[:WRONG_SHOWN],
    }


# ---------------------------------------------------------------- output

def _fmt(x: float | None) -> str:
    return "n/a" if x is None else f"{x:.4f}"


def render(res: dict[str, Any], old: Side, new: Side, threshold: float,
           validation: dict[str, Any] | None, runtime: float) -> str:
    cls_counts = Counter(p["class"] for p in res["pairs"])
    low = sum(1 for p in res["pairs"] if p.get("low_confidence"))
    pass_counts = Counter(p["pass"] for p in res["pairs"])
    m_old = len(res["pairs"])
    lines = [
        f"build_diff: {old.build} -> {new.build}  (minhash threshold {threshold:.2f})",
        f"functions (participating): old {len(old.funcs)}  new {len(new.funcs)}"
        f"  (no fingerprint: old {len(old.unfingerprinted)}, new {len(new.unfingerprinted)})",
        "classes:",
    ]
    for c in CLASSES:
        extra = f"  (low_confidence {low})" if c == "changed" else ""
        lines.append(f"  {c:<15} {cls_counts.get(c, 0):>6}{extra}")
    lines.append(f"new: {len(res['new'])}   removed: {len(res['removed'])}")
    lines.append(f"matched share: old {m_old}/{len(old.funcs)} ({_fmt(m_old / len(old.funcs))})"
                 f"   new {m_old}/{len(new.funcs)} ({_fmt(m_old / len(new.funcs))})")
    lines.append("pairs by pass: " + "  ".join(f"{p}: {pass_counts.get(p, 0)}" for p in (1, 2, 3, 4)))
    if validation is not None:
        gt = validation["ground_truth"]
        lines.append(
            f"validation: {gt['scored_pairs']} scored ground-truth pairs "
            f"(names in both builds {gt['names_in_both_builds']}; dropped {gt['dropped_no_function']}, "
            f"conflicts excluded {gt['conflict_pairs_excluded']}, unscorable {gt['unscorable_pairs_excluded']})")
        lines.append(f"  {'pass':<6}{'pairs':>7}{'correct':>9}{'precision':>11}{'found':>7}{'recall':>9}")
        for p in ("1", "2", "3", "4"):
            r = validation["per_pass"][p]
            lines.append(f"  {p:<6}{r['pairs']:>7}{r['correct']:>9}{_fmt(r['precision']):>11}"
                         f"{r['found']:>7}{_fmt(r['recall']):>9}")
        o = validation["overall"]
        lines.append(f"  {'all':<6}{o['pairs']:>7}{o['correct']:>9}{_fmt(o['precision']):>11}"
                     f"{o['found']:>7}{_fmt(o['recall']):>9}")
        lines.append(f"  recall by site: entry {_fmt(validation['recall_entry_sites'])}"
                     f"  mid-function {_fmt(validation['recall_mid_function_sites'])}"
                     f" ({gt['scored_mid_function']} mid-function truth pairs)")
        lines.append(f"wrong pairs ({min(WRONG_SHOWN, validation['wrong_total'])} of {validation['wrong_total']}):")
        for w in validation["wrong"]:
            truth = (f"truth {'/'.join(w['truth_names'])} -> {w['truth_new']}"
                     if w["truth_new"] else "truth: none for old side")
            lines.append(f"  pass {w['pass']}  {w['old']} {w['old_name']} -> {w['new']} {w['new_name']}  ({truth})")
    lines.append(f"runtime: {runtime:.2f} s")
    return "\n".join(lines) + "\n"


def run(args: argparse.Namespace) -> dict[str, Any]:
    t0 = time.perf_counter()
    old = load_side(args.old_inv, args.old_pescan)
    new = load_side(args.new_inv, args.new_pescan)
    res = match(old, new, args.minhash_threshold)
    rec = res.pop("_rec")
    old_label = args.old_build or old.build
    new_label = args.new_build or new.build
    validation = None
    if args.truth is not None:
        validation = validate(rec, old, new, _read_json(args.truth), old_label, new_label)
    out = {
        "format": FORMAT,
        "old_build": old_label,
        "new_build": new_label,
        "pairs": res["pairs"],
        "new": res["new"],
        "removed": res["removed"],
        "summary": {
            "old_functions": len(old.funcs),
            "new_functions": len(new.funcs),
            "pairs": len(res["pairs"]),
            "classes": {c: sum(1 for p in res["pairs"] if p["class"] == c) for c in CLASSES},
            "pairs_by_pass": {str(p): sum(1 for q in res["pairs"] if q["pass"] == p) for p in (1, 2, 3, 4)},
            "low_confidence": sum(1 for p in res["pairs"] if p.get("low_confidence")),
            "new": len(res["new"]),
            "removed": len(res["removed"]),
            "matched_share_old": _rate(len(res["pairs"]), len(old.funcs)),
            "matched_share_new": _rate(len(res["pairs"]), len(new.funcs)),
            "minhash_threshold": args.minhash_threshold,
        },
        "validation": validation,
    }
    out_path = args.out or TRANSIENT / f"diff_{old_label}__{new_label}.json"
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump(out, fh, indent=1, sort_keys=False)
        fh.write("\n")
    runtime = time.perf_counter() - t0
    sys.stdout.write(render(res, old, new, args.minhash_threshold, validation, runtime))
    sys.stdout.write(f"mapping written to {out_path}\n")
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Map functions between two builds of a module (M1 §4.3).")
    ap.add_argument("--old-inv", type=Path, required=True)
    ap.add_argument("--old-pescan", type=Path, required=True)
    ap.add_argument("--new-inv", type=Path, required=True)
    ap.add_argument("--new-pescan", type=Path, required=True)
    ap.add_argument("--truth", type=Path, help="signature sweep JSON (ground truth)")
    ap.add_argument("--old-build", help="build key in the truth file (default: the 8-digit id)")
    ap.add_argument("--new-build", help="build key in the truth file (default: the 8-digit id)")
    ap.add_argument("--out", type=Path, help=f"mapping output (default {TRANSIENT}/diff_<old>__<new>.json)")
    ap.add_argument("--minhash-threshold", type=float, default=DEFAULT_THRESHOLD)
    args = ap.parse_args(argv)
    try:
        run(args)
    except (ValueError, KeyError, OSError, json.JSONDecodeError) as exc:
        print(f"build_diff.py: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
