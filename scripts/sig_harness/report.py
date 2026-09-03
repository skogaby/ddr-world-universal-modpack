#!/usr/bin/env python3
"""Join per-build signature-harness logs against the crate's consumer graph.

Invoked by scripts/validate_signatures.sh. Reads:
  --logs DIR   one `<build>.log` per build (harness stdout)
  --src  DIR   the crate's src/ (consumer graph is derived live from source)

Consumer graph:
  * `fn required_signatures(&self) -> &[&str] { &["a", "b"] }` in a mod
    => HARD requirement: ModRegistry skips the whole mod if any is missing.
  * `require_address("x")` => PANIC if missing (init-time); flagged loudly.
  * `get_address("x")` / `get_all_matches("x")` => soft consumer (fail-open
    or mod-side gate) — still reported so the maintainer can confirm the
    fallback path is actually graceful.

Signature names are matched literally, so version alternates (`foo`,
`foo_v1`, `foo_v2`) show up as separate rows; the "alternates" column
groups them by stem so a `foo` miss covered by a `foo_v1` hit is visible.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

# `[+] name (any annotation) @ +0x1234`, `[+] name = 0x1790`,
# `[+] a (derived) @ +0x1; b @ +0x2` — one or more name/value pairs per line.
PLUS_LINE_RE = re.compile(r"\[\+\]\s+(.*)")
PLUS_PAIR_RE = re.compile(r"([A-Za-z0-9_*]+)(?:\s+\((?:[^()]|\([^()]*\))*\))?\s*(?:@|=)\s*\+?0x([0-9A-Fa-f]+)")
MINUS_RE = re.compile(r"\[-\]\s+(\S+?)\s+(?:--|—)\s*(.*)")
SUMMARY_RE = re.compile(r"### summary found=(\d+) total=(\d+) missing=(.*)")
CRASH_RE = re.compile(r"### harness-crashed")


def parse_log(path: Path) -> dict:
    resolved: dict[str, int] = {}
    missing: dict[str, str] = {}
    warns: list[str] = []
    crashed = False
    summary = None
    unparsed: list[str] = []
    for line in path.read_text(errors="replace").splitlines():
        if CRASH_RE.search(line):
            crashed = True
        m = PLUS_LINE_RE.search(line)
        if m:
            pairs = PLUS_PAIR_RE.findall(m.group(1))
            if not pairs:
                unparsed.append(line.strip())
            for name, val in pairs:
                resolved[name] = int(val, 16)
            continue
        m = MINUS_RE.search(line)
        if m:
            name = m.group(1)
            missing.setdefault(name, m.group(2).strip())
            continue
        m = SUMMARY_RE.search(line)
        if m:
            summary = (int(m.group(1)), int(m.group(2)))
            continue
        if "[WARN]" in line:
            warns.append(line.strip())
    return {
        "resolved": resolved,
        "missing": missing,
        "warns": warns,
        "crashed": crashed,
        "summary": summary,
        "unparsed": unparsed,
    }


# ── consumer graph ──────────────────────────────────────────────────────────

REQ_RE = re.compile(r"fn required_signatures\s*\([^)]*\)\s*->\s*&\[&str\]\s*\{\s*&\[(.*?)\]", re.S)
ID_RE = re.compile(r"fn id\s*\(&self\)\s*->\s*&'?\w*\s*str\s*\{\s*\"([^\"]+)\"")
CALL_RE = re.compile(r"\b(get_address|require_address|get_all_matches)\(\s*\"([a-zA-Z0-9_]+)\"")
STR_RE = re.compile(r"\"([a-zA-Z0-9_]+)\"")


def unit_for(path: Path, src: Path) -> str:
    rel = path.relative_to(src).parts
    if len(rel) >= 2 and rel[0] in ("mods", "services"):
        kind = "mod" if rel[0] == "mods" else "svc"
        name = rel[1][:-3] if rel[1].endswith(".rs") else rel[1]
        return f"{kind}:{name}"
    return "core:" + "/".join(rel)


def build_consumer_graph(src: Path) -> tuple[dict, dict]:
    """Returns (sig -> {unit -> set(kinds)}, unit -> mod id or None)."""
    graph: dict[str, dict[str, set[str]]] = defaultdict(lambda: defaultdict(set))
    ids: dict[str, str] = {}
    for path in sorted(src.rglob("*.rs")):
        if path.name == "signatures.rs" and path.parent.name == "core":
            continue
        if path.name == "mod_trait.rs":  # doc-comment examples only
            continue
        text = path.read_text(errors="replace")
        unit = unit_for(path, src)
        m = ID_RE.search(text)
        if m and unit not in ids:
            ids[unit] = m.group(1)
        for m in REQ_RE.finditer(text):
            for sig in STR_RE.findall(m.group(1)):
                graph[sig][unit].add("required")
        for m in CALL_RE.finditer(text):
            kind = {"get_address": "get", "require_address": "REQUIRE",
                    "get_all_matches": "get_all"}[m.group(1)]
            graph[sig := m.group(2)][unit].add(kind)
    return graph, ids


def stem(name: str) -> str:
    return re.sub(r"_v\d+$", "", name)


# Version alternates whose names don't share a `_vN` stem. A miss of one
# member is by design when another member (or the derived name it feeds)
# resolves on that build. Keep this list in step with signatures.rs.
ALT_GROUPS: list[set[str]] = [
    {"series_label_lookup_inlined", "series_label_lookup_standalone"},
    {"textlayer_bind_anchor", "textlayer_bind_direct", "textlayer_bind"},
    {"hud_layout_builder", "hud_layout_builder_style_cluster"},
]


# ── report ──────────────────────────────────────────────────────────────────

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--logs", required=True, type=Path)
    ap.add_argument("--src", required=True, type=Path)
    ap.add_argument("--json", type=Path)
    args = ap.parse_args()

    logs = sorted(args.logs.glob("*.log"))
    if not logs:
        print("no harness logs found", file=sys.stderr)
        return 2
    builds = {p.stem: parse_log(p) for p in logs}
    build_names = sorted(builds)

    graph, ids = build_consumer_graph(args.src)

    all_names: set[str] = set()
    for b in builds.values():
        all_names |= set(b["resolved"]) | set(b["missing"])

    # Names that resolve somewhere but not everywhere.
    gaps: dict[str, list[str]] = {}
    never: list[str] = []
    for name in sorted(all_names):
        hits = [b for b in build_names if name in builds[b]["resolved"]]
        if not hits:
            never.append(name)
        elif len(hits) != len(build_names):
            gaps[name] = [b for b in build_names if b not in hits]

    # Alternates: stems where at least one variant resolves per build.
    by_stem: dict[str, list[str]] = defaultdict(list)
    for name in all_names:
        by_stem[stem(name)].append(name)

    def covered_by_alt(name: str, build: str) -> str | None:
        for alt in by_stem[stem(name)]:
            if alt != name and alt in builds[build]["resolved"]:
                return alt
        for group in ALT_GROUPS:
            if name in group:
                for alt in sorted(group):
                    if alt != name and alt in builds[build]["resolved"]:
                        return alt
        return None

    w = max(8, max(len(b) for b in build_names))
    print()
    print("=" * 78)
    print("CROSS-BUILD SIGNATURE SWEEP")
    print("=" * 78)
    for b in build_names:
        info = builds[b]
        s = info["summary"]
        base = f"{s[0]}/{s[1]} base signatures" if s else "no summary line"
        derived = len([n for n in info["resolved"] if n not in (info.get("_base") or {})])
        flag = "  ** HARNESS CRASHED **" if info["crashed"] else ""
        print(f"  {b:<{w}}  {base}; {len(info['resolved'])} names resolved incl. derived; "
              f"{len(info['missing'])} reported missing{flag}")

    for b in build_names:
        for line in builds[b]["unparsed"]:
            print(f"  [unparsed {b}] {line}")

    print()
    print("-" * 78)
    print("PER-SIGNATURE GAPS  (resolves on some builds, not on others)")
    print("-" * 78)
    if not gaps:
        print("  none — every name that resolves anywhere resolves everywhere")
    hard_hits: list[tuple[str, str, str]] = []
    for name, missing_on in gaps.items():
        alts = {b: covered_by_alt(name, b) for b in missing_on}
        alt_txt = ", ".join(f"{b}->{a}" for b, a in alts.items() if a)
        cons = graph.get(name, {})
        cons_txt = "; ".join(
            f"{u}[{'/'.join(sorted(k))}]" for u, k in sorted(cons.items())
        ) or "(no direct consumer — derivation input or dead)"
        print(f"\n  {name}")
        print(f"    missing on : {', '.join(missing_on)}")
        if alt_txt:
            print(f"    alternates : {alt_txt}")
        for b in missing_on:
            reason = builds[b]["missing"].get(name)
            if reason:
                print(f"    reason[{b}]: {reason}")
        print(f"    consumers  : {cons_txt}")
        for u, k in cons.items():
            if "required" in k or "REQUIRE" in k:
                for b in missing_on:
                    if not alts.get(b):
                        hard_hits.append((u, name, b))

    print()
    print("-" * 78)
    print("PER-CONSUMER IMPACT  (mods/services losing an address on some build)")
    print("-" * 78)
    impact: dict[str, dict[str, list[tuple[str, str]]]] = defaultdict(lambda: defaultdict(list))
    for name, missing_on in gaps.items():
        for u, kinds in graph.get(name, {}).items():
            for b in missing_on:
                alt = covered_by_alt(name, b)
                impact[u][b].append((name, "/".join(sorted(kinds)) + (f" alt={alt}" if alt else "")))
    if not impact:
        print("  none")
    for u in sorted(impact):
        label = u + (f" (id `{ids[u]}`)" if u in ids else "")
        print(f"\n  {label}")
        for b in sorted(impact[u]):
            for name, k in impact[u][b]:
                sev = "HARD" if ("required" in k or "REQUIRE" in k) and "alt=" not in k else "soft"
                print(f"    {b:<{w}}  {sev:<4}  {name}  [{k}]")

    print()
    print("-" * 78)
    print("NEVER RESOLVES ON ANY BUILD  (dead pattern, or host-unresolvable)")
    print("-" * 78)
    if not never:
        print("  none")
    for name in never:
        cons = graph.get(name, {})
        cons_txt = "; ".join(f"{u}[{'/'.join(sorted(k))}]" for u, k in sorted(cons.items())) or "-"
        reasons = {builds[b]["missing"].get(name, "?") for b in build_names}
        print(f"  {name}")
        print(f"    consumers: {cons_txt}")
        for r in sorted(reasons):
            print(f"    reason   : {r}")

    # Consumers referencing names that no build ever emits (typo / renamed).
    print()
    print("-" * 78)
    print("CONSUMER REFERENCES TO UNKNOWN NAMES  (never logged by any build)")
    print("-" * 78)
    unknown = sorted(n for n in graph if n not in all_names)
    if not unknown:
        print("  none")
    for name in unknown:
        cons_txt = "; ".join(f"{u}[{'/'.join(sorted(k))}]" for u, k in sorted(graph[name].items()))
        print(f"  {name}: {cons_txt}")

    if args.json:
        out = {
            "builds": {
                b: {
                    "resolved": {n: hex(o) for n, o in info["resolved"].items()},
                    "missing": info["missing"],
                    "crashed": info["crashed"],
                    "warns": info["warns"],
                }
                for b, info in builds.items()
            },
            "gaps": gaps,
            "never": never,
            "unknown_consumer_refs": unknown,
            "consumers": {n: {u: sorted(k) for u, k in c.items()} for n, c in graph.items()},
            "mod_ids": ids,
        }
        args.json.write_text(json.dumps(out, indent=2, sort_keys=True))
        print(f"\n[json written to {args.json}]")

    print()
    if hard_hits:
        print(f"RESULT: {len(hard_hits)} HARD gap(s) "
              "(required_signatures / require_address with no resolving alternate):")
        for u, name, b in hard_hits:
            print(f"  {u}  needs  {name}  — missing on {b}")
    crashed = [b for b in build_names if builds[b]["crashed"]]
    if crashed:
        print(f"RESULT: harness crashed on: {', '.join(crashed)}")
    uncovered = {
        name: [b for b in missing_on if not covered_by_alt(name, b)]
        for name, missing_on in gaps.items()
    }
    uncovered = {n: bs for n, bs in uncovered.items() if bs}
    covered = len(gaps) - len(uncovered)
    ok = not uncovered and not crashed
    if ok:
        print(f"RESULT: ALL GREEN ({covered} gap(s), every one covered by a resolving alternate)")
    else:
        print(f"RESULT: {len(uncovered)} name(s) with UNCOVERED cross-build gaps "
              f"({covered} further gap(s) covered by alternates):")
        for name, bs in sorted(uncovered.items()):
            print(f"  {name}: missing on {', '.join(bs)}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
