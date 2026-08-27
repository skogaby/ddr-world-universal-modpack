#!/usr/bin/env python3
"""Scan DLLs for AOB uniqueness across game builds.

Usage:
  scripts/aob_check.py --build NAME=/path/to/gamemdx.dll [--build ...] \\
                       --pattern NAME="48 89 5C ? ? 57" [--pattern ...]

Or from a JSON spec file:
  scripts/aob_check.py --spec patterns.json

A "clean" signature produces exactly one match in every build. The tool prints
a per-pattern, per-build result line and returns non-zero if any pattern
missed, duplicated, or referenced a missing file.

JSON spec format:
  {
    "builds":   { "20260324": "/path/to/dll", "20250805": "/path/to/dll" },
    "patterns": { "metadata_insert": "48 89 5C 24 10 ..." }
  }
"""
from __future__ import annotations
import argparse
import json
import re
import sys
from pathlib import Path


def parse_aob(pat: str) -> re.Pattern:
    """Convert space-separated hex-byte AOB to a compiled bytes regex.

    `?` or `??` at a byte position becomes a single-byte regex wildcard (`.`).
    Literal bytes are emitted as `re.escape(bytes([n]))`.
    """
    tokens = pat.replace(",", " ").split()
    out = b""
    for t in tokens:
        if t in ("?", "??"):
            out += b"."
        else:
            out += re.escape(bytes([int(t, 16)]))
    return re.compile(out, re.DOTALL)


def scan(path: Path, pattern: re.Pattern) -> list[int]:
    return [m.start() for m in pattern.finditer(path.read_bytes())]


def parse_kv_pair(raw: str, kind: str) -> tuple[str, str]:
    if "=" not in raw:
        raise SystemExit(f"--{kind} expects NAME=VALUE, got {raw!r}")
    name, value = raw.split("=", 1)
    name = name.strip()
    value = value.strip()
    if not name or not value:
        raise SystemExit(f"--{kind} has an empty name or value: {raw!r}")
    return name, value


def load_spec(path: Path) -> tuple[dict[str, str], dict[str, str]]:
    spec = json.loads(path.read_text())
    builds = spec.get("builds") or {}
    patterns = spec.get("patterns") or {}
    if not isinstance(builds, dict) or not isinstance(patterns, dict):
        raise SystemExit(f"{path}: 'builds' and 'patterns' must be objects")
    return builds, patterns


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--build",
        action="append",
        default=[],
        metavar="NAME=PATH",
        help="Build to scan (repeatable). Example: --build 20260324=/path/gamemdx.dll",
    )
    parser.add_argument(
        "--pattern",
        action="append",
        default=[],
        metavar="NAME=AOB",
        help="AOB pattern to check (repeatable). Example: --pattern foo='48 89 ? 24'",
    )
    parser.add_argument(
        "--spec",
        type=Path,
        help="JSON spec with 'builds' and 'patterns' objects. "
        "Builds/patterns here can be extended by repeated --build/--pattern flags.",
    )
    args = parser.parse_args()

    builds: dict[str, str] = {}
    patterns: dict[str, str] = {}

    if args.spec:
        builds, patterns = load_spec(args.spec)

    for raw in args.build:
        name, value = parse_kv_pair(raw, "build")
        builds[name] = value
    for raw in args.pattern:
        name, value = parse_kv_pair(raw, "pattern")
        patterns[name] = value

    if not builds:
        raise SystemExit("At least one --build (or --spec with builds) is required.")
    if not patterns:
        raise SystemExit("At least one --pattern (or --spec with patterns) is required.")

    overall_ok = True
    print(f"Scanning {len(patterns)} signature(s) across {len(builds)} build(s)\n")

    for name, pat_str in patterns.items():
        print(f"── {name} ──────────────────────────────────────────────")
        compiled = parse_aob(pat_str)
        for build, path_str in builds.items():
            path = Path(path_str).expanduser()
            if not path.exists():
                print(f"  [{build}]  ERROR — file not found at {path}")
                overall_ok = False
                continue
            offsets = scan(path, compiled)
            count = len(offsets)
            if count == 1:
                marker = "OK "
            elif count == 0:
                marker = "MISS"
                overall_ok = False
            else:
                marker = "DUP "
                overall_ok = False
            pretty = ", ".join(f"0x{o:X}" for o in offsets[:5])
            if count > 5:
                pretty += f", ... (+{count - 5} more)"
            print(f"  [{build}]  {marker}  count={count}  offsets=[{pretty}]")
        print()

    print("─────────────────────────────────────────────────")
    if overall_ok:
        print("ALL PATTERNS UNIQUE across all builds.")
        return 0
    print("FAIL — at least one pattern missed, duplicated, or referenced a missing file.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
