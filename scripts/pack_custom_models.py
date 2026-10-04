#!/usr/bin/env python3
"""
pack_custom_models — pre-pack Background Dancers MODEL FOLDERS into ready arcs.

The DLL accepts two forms of custom content under `data_mods/custom_models/`
(`src/mods/background_dancers/custom_content.rs` module docs):

  * a MODEL FOLDER (`pl_<key>/`, `pl_<key>_<part>/`, `mapset_<key>/`) — the
    authoring form. Every boot the scanner lists its whole tree (~0.4 ms per
    directory under CrossOver), and the first boot packs it into
    `data_mods/_cache/custom_models/<name>-<hash8>.arc` — a second full copy of
    the content on the cabinet.
  * a ready `<folder>.arc` beside it — the shipping form: one directory entry,
    no cache copy, no first-boot pack, and one file for the updater instead of
    the folder's dozens.

This tool turns the first form into the second, producing the arc the DLL
would have packed: members mapped exactly like
`custom_content::folder_member_path`, written exactly like
`core::arc::ArcArchive::to_bytes` (uncompressed, members in byte order, 64-byte
alignment). Sidecars (`chara_resources.rlist[.txt]`, `map_resources...`) and
every other non-model file stay where they are.

Model folders are packed only where the DLL's scanner looks for them
(`custom_scan::walk_kind_root`): directly in `dancers/` / `stages/`, in a
folder under it, or in a folder two levels under it — never inside another
model folder, never under a dot-directory. `mapset_*_g/` (never loaded) is left
alone.

Equivalence note: the planner reads a folder's members in walk order and an
arc's in header (sorted) order. Within every category it looks at (body model,
`motion/*.anm` clips, stage part models, `*.camanm` clips) the two orders agree
as long as a stage's camera clips sit in ONE directory (all shipped content:
`mapset_<key>/camera/`). Clips spread over several sub-directories would be
listed by basename in the arc, by path in the folder.

Usage:
  pack_custom_models.py SRC --out DST     write a packed MIRROR of SRC to DST
  pack_custom_models.py ROOT --in-place   replace ROOT's model folders by arcs
  (SRC / ROOT = a custom_models base: the folder holding dancers/ and stages/)

  --dry-run   report what would be packed, write nothing
  --force     allow --in-place inside a git work tree (the repo keeps the
              authoring folders; packing them in place deletes tracked files)

The release build packs its STAGED copy in place
(scripts/build_release_archive.sh on macOS/Linux,
scripts/build_release_distribution.bat on Windows); an operator can do the same
to an install's data_mods/custom_models (the DLL then prunes the orphaned cache
arcs itself).
"""

import argparse
import os
import shutil
import stat
import struct
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional, Tuple

SUMMARY = "Pre-pack Background Dancers model folders into ready arcs."

ARC_MAGIC = 0x19751120
ARC_VERSION = 1
ARC_COMPRESSION_NONE = 0
ARC_ALIGN = 64

KINDS = ("dancers", "stages")
PART_PREFIXES = ("head", "hips", "chest", "forearm", "face")
KEY_CHARS = frozenset("abcdefghijklmnopqrstuvwxyz0123456789_")


# ---------------------------------------------------------------------------
# Name rules (mirror custom_content.rs / sources.rs)
# ---------------------------------------------------------------------------


def ascii_lower(s: str) -> str:
    """Rust's `to_ascii_lowercase`: only A-Z change."""
    return "".join(chr(ord(c) + 32) if "A" <= c <= "Z" else c for c in s)


def valid_key(key: str) -> bool:
    return bool(key) and all(c in KEY_CHARS for c in key)


def _is_part_suffix(p: str) -> bool:
    for prefix in PART_PREFIXES:
        if p.startswith(prefix):
            digits = p[len(prefix):]
            return len(digits) == 2 and all("0" <= c <= "9" for c in digits)
    return False


def classify_arc_name(name: str) -> Tuple[str, Optional[str]]:
    """`(role, key)`, role in body|part|stage|gold|shadow|other."""
    lower = ascii_lower(name)
    if not lower.endswith(".arc"):
        return ("other", None)
    stem = lower[: -len(".arc")]
    if stem.startswith("pl_"):
        rest = stem[len("pl_"):]
        if rest == "shadow00":
            return ("shadow", None)
        if "_" in rest:
            k, p = rest.rsplit("_", 1)
            if _is_part_suffix(p):
                return ("part", k) if valid_key(k) else ("other", None)
        return ("body", rest) if valid_key(rest) else ("other", None)
    if stem.startswith("mapset_"):
        rest = stem[len("mapset_"):]
        if rest.endswith("_g"):
            k = rest[: -len("_g")]
            return ("gold", k) if valid_key(k) else ("other", None)
        return ("stage", rest) if valid_key(rest) else ("other", None)
    return ("other", None)


def classify_folder_name(name: str) -> Tuple[str, Optional[str]]:
    return classify_arc_name(name + ".arc")


def is_model_folder_name(name: str) -> bool:
    return classify_folder_name(name)[0] in ("body", "part", "stage", "gold")


def folder_member_path(role: str, key: Optional[str], folder: str, rel: str) -> str:
    """`custom_content::folder_member_path`."""
    rel = rel.replace("\\", "/").lstrip("/")
    if ascii_lower(rel[:5]) == "data/":
        return rel
    if role in ("body", "part"):
        return f"data/chara/{folder}/{rel}"
    if role in ("stage", "gold"):
        base = rel.rsplit("/", 1)[-1]
        if ascii_lower(base).endswith(".camanm"):
            return f"data/camera/{key}/{base}"
        return f"data/map/{rel}"
    return f"data/{rel}"


# ---------------------------------------------------------------------------
# Walking (mirror custom_scan.rs)
# ---------------------------------------------------------------------------


def _list(dir_path: Path) -> Tuple[List[Path], List[Path]]:
    """Files and dirs of one listing; dot-entries and symlinks skipped; sorted."""
    files, dirs = [], []
    try:
        entries = list(os.scandir(dir_path))
    except OSError:
        return files, dirs
    for e in entries:
        if e.name.startswith("."):
            continue
        if e.is_dir(follow_symlinks=False):
            dirs.append(Path(e.path))
        elif e.is_file(follow_symlinks=False):
            files.append(Path(e.path))
    files.sort()
    dirs.sort()
    return files, dirs


def find_model_folders(kind_root: Path) -> List[Path]:
    """Every model folder the scanner packs (body / part / stage), walk order."""
    found: List[Path] = []
    _, top = _list(kind_root)
    for d in top:
        if is_model_folder_name(d.name):
            found.append(d)
            continue
        _, sub = _list(d)
        for s in sub:
            if is_model_folder_name(s.name):
                found.append(s)
                continue
            _, inner = _list(s)
            found.extend(i for i in inner if is_model_folder_name(i.name))
    return [f for f in found if classify_folder_name(f.name)[0] != "gold"]


def folder_members(folder: Path) -> Dict[str, Path]:
    """member path -> source file. Two files mapping to one member: the later
    in (rel, path) order wins, as the DLL's BTreeMap insert does."""
    rels: List[Tuple[str, Path]] = []

    def rec(cur: Path) -> None:
        files, dirs = _list(cur)
        for f in files:
            rels.append((f.relative_to(folder).as_posix(), f))
        for d in dirs:
            rec(d)

    rec(folder)
    rels.sort(key=lambda t: (t[0], str(t[1])))
    role, key = classify_folder_name(folder.name)
    members: Dict[str, Path] = {}
    for rel, path in rels:
        members[folder_member_path(role, key, folder.name, rel)] = path
    return members


# ---------------------------------------------------------------------------
# ARC codec (mirror core::arc)
# ---------------------------------------------------------------------------


def _align(n: int) -> int:
    return (n + ARC_ALIGN - 1) // ARC_ALIGN * ARC_ALIGN


def arc_bytes(entries: Dict[str, bytes]) -> bytes:
    """`ArcArchive::to_bytes`: uncompressed, byte-ordered names, 64-byte aligned."""
    names = sorted(entries, key=lambda n: n.encode("utf-8"))
    count = len(names)
    str_cursor = 16 + count * 16
    str_offsets = []
    for n in names:
        str_offsets.append(str_cursor)
        str_cursor += len(n.encode("utf-8")) + 1
    data_start = _align(str_cursor)
    data_offsets = []
    cursor = data_start
    for n in names:
        data_offsets.append(cursor)
        cursor = _align(cursor + len(entries[n]))
    if cursor > 0xFFFFFFFF:
        raise ValueError("the arc would exceed 4 GiB")
    out = bytearray()
    out += struct.pack("<IIII", ARC_MAGIC, ARC_VERSION, count, ARC_COMPRESSION_NONE)
    for i, n in enumerate(names):
        size = len(entries[n])
        out += struct.pack("<IIII", str_offsets[i], data_offsets[i], size, size)
    for n in names:
        out += n.encode("utf-8") + b"\0"
    out += b"\0" * (data_start - len(out))
    for n in names:
        data = entries[n]
        out += data
        out += b"\0" * (_align(len(data)) - len(data))
    return bytes(out)


def arc_entries(data: bytes) -> List[Tuple[str, int, int]]:
    """`(path, data offset, size)` per cue (uncompressed arcs)."""
    magic, _version, count, _comp = struct.unpack_from("<IIII", data, 0)
    if magic != ARC_MAGIC:
        raise ValueError("not an arc")
    out = []
    for i in range(count):
        str_off, data_off, unpacked, packed = struct.unpack_from("<IIII", data, 16 + i * 16)
        if unpacked != packed:
            raise ValueError("compressed member")
        end = data.index(b"\0", str_off)
        out.append((data[str_off:end].decode("utf-8"), data_off, unpacked))
    return out


def pack_folder(folder: Path) -> Tuple[bytes, int]:
    """The arc for one model folder + its member count (self-checked)."""
    members = folder_members(folder)
    if not members:
        raise ValueError("model folder is empty")
    entries = {m: p.read_bytes() for m, p in members.items()}
    data = arc_bytes(entries)
    back = arc_entries(data)
    if [p for p, _, _ in back] != sorted(entries, key=lambda n: n.encode("utf-8")):
        raise RuntimeError("self-check failed: member table mismatch")
    for path, off, size in back:
        if data[off:off + size] != entries[path]:
            raise RuntimeError(f"self-check failed: payload of {path}")
    return data, len(entries)


# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------


def _inside_git_work_tree(path: Path) -> bool:
    try:
        r = subprocess.run(
            ["git", "-C", str(path), "rev-parse", "--is-inside-work-tree"],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        return False
    return r.returncode == 0 and r.stdout.strip() == "true"


def display(p: Path) -> str:
    """`~`-relative (log lines never spell out a home directory)."""
    s = str(p)
    home = os.path.expanduser("~")
    return "~" + s[len(home):] if s == home or s.startswith(home + os.sep) else s


def _write_atomic(path: Path, data: bytes) -> None:
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_bytes(data)
    os.replace(tmp, path)


def _rmtree(path: Path) -> None:
    """`shutil.rmtree` that clears a read-only attribute and retries (Windows
    refuses to delete read-only files; a copied tree can carry the flag)."""

    def retry(func, p, _exc):
        os.chmod(p, stat.S_IWRITE)
        func(p)

    if sys.version_info >= (3, 12):
        shutil.rmtree(path, onexc=retry)
    else:
        shutil.rmtree(path, onerror=retry)


def run(root: Path, out: Optional[Path], dry_run: bool) -> int:
    """Pack `root` in place (`out` None) or into the mirror `out`."""
    if not any((root / k).is_dir() for k in KINDS):
        print(f"error: {display(root)} holds neither dancers/ nor stages/", file=sys.stderr)
        return 2
    folders = [f for k in KINDS if (root / k).is_dir() for f in find_model_folders(root / k)]
    if out is not None and not dry_run:
        if out.exists() and any(out.iterdir()):
            print(f"error: {display(out)} exists and is not empty", file=sys.stderr)
            return 2
        skip = {f.resolve() for f in folders}
        shutil.copytree(
            root,
            out,
            dirs_exist_ok=True,
            ignore=lambda d, names: [
                n for n in names if n.startswith(".") or (Path(d) / n).resolve() in skip
            ],
        )

    packed = failed = 0
    total = 0
    for folder in folders:
        rel = folder.relative_to(root)
        dest_dir = (out / rel).parent if out is not None else folder.parent
        arc_path = dest_dir / (folder.name + ".arc")
        clash = (folder.parent / arc_path.name).exists() or (out is not None and arc_path.exists())
        if clash:
            print(f"  SKIP {rel.as_posix()}: {arc_path.name} already exists beside it", file=sys.stderr)
            failed += 1
            continue
        if dry_run:
            print(f"  would pack {rel.as_posix()} ({len(folder_members(folder))} file(s)) -> {arc_path.name}")
            packed += 1
            continue
        try:
            data, _ = pack_folder(folder)
        except (OSError, ValueError, RuntimeError) as e:
            print(f"  FAIL {rel.as_posix()}: {e}", file=sys.stderr)
            failed += 1
            continue
        _write_atomic(arc_path, data)
        if out is None:
            _rmtree(folder)
        packed += 1
        total += len(data)
    where = "" if dry_run else f" into {display(out if out is not None else root)}"
    print(
        f"{'would pack' if dry_run else 'packed'} {packed} model folder(s)"
        + ("" if dry_run else f" ({total / (1 << 20):.1f} MiB of arcs)")
        + where
        + (f"; {failed} skipped/failed" if failed else "")
    )
    return 1 if failed else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=SUMMARY)
    ap.add_argument("root", type=Path, help="a custom_models base (holds dancers/ and/or stages/)")
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--out", type=Path, help="write a packed mirror here (must be empty or absent)")
    mode.add_argument("--in-place", action="store_true", help="replace ROOT's model folders by arcs")
    ap.add_argument("--dry-run", action="store_true", help="report only, write nothing")
    ap.add_argument("--force", action="store_true", help="allow --in-place inside a git work tree")
    args = ap.parse_args()
    root = args.root.resolve()
    if args.in_place and not args.dry_run and not args.force and _inside_git_work_tree(root):
        print(
            f"error: {display(root)} is inside a git work tree -- the repo keeps the authoring "
            "folders; pack a copy (--out) or pass --force",
            file=sys.stderr,
        )
        return 2
    return run(root, args.out.resolve() if args.out else None, args.dry_run)


if __name__ == "__main__":
    sys.exit(main())
