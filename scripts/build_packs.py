#!/usr/bin/env python3
"""
build_packs — turn the assets repository's source folders into optional PACKS.

The ported dancers and stages live in the assets repository
(skogaby/ddr-world-universal-modpack-assets) as `dancers/<Source>/…` and
`stages/<Source>/…`. Each `<Source>` folder is one pack: a zip, rooted at the game
folder, that the updater downloads when a user selects it (design:
.agents/planning/2026-11-11-optional-content-packs/design/detailed-design.md §4.2).

A pack holds what a release ships for that source — every model folder as its ready
`.arc` (`scripts/pack_custom_models.py`), sidecars beside them — plus its MARKER,
`data_mods/custom_models/.packs/<pack id>.json`, which lists the pack's files with size
and hash. The updater recognises an installed pack by that marker, whether it installed
the pack itself or the zip was unpacked by hand.

Identity. A pack's CONTENT ID is a SHA-256 over its files (path and hash of each, in
byte-sorted path order; the marker excluded). The same models give the same id on every
build and every host; any change gives a new one. The asset name carries the first 12
hex digits, so "has this pack changed" is "is there an asset of this name already".
The zip's own bytes are NOT stable (deflate output varies between zlib builds), which is
why nothing identifies a pack by its zip.

Subcommands:
  build     pack every source, write the zips and catalog.json
  catalog   combine a built catalog.json with the shelf's asset listing into the
            catalog a modpack release embeds
  stage     write packed files + markers into a game-folder image (no zips)
  check     validate a catalog file
  base-image  finish a staged BASE release image: drop any source folder but
            `Custom`, embed the published catalog, enforce the size limit

The assets checkout is `$DDR_WORLD_ASSETS`, else the folder
`ddr-world-universal-modpack-assets` beside this repository; `--assets` overrides both.
"""

import argparse
import hashlib
import json
import os
import re
import sys
import urllib.error
import urllib.request
import zipfile
from pathlib import Path
from typing import Dict, Iterable, List, NamedTuple, Optional, Tuple

import pack_custom_models as P

REPO_ROOT = Path(__file__).resolve().parent.parent
ASSETS_REPO_NAME = "ddr-world-universal-modpack-assets"
URL_BASE = "https://github.com/skogaby/ddr-world-universal-modpack-assets/releases/download/packs/"
CATALOG_NAME = "catalog.json"
# The catalog's name inside a base release (and the game folder).
GAME_CATALOG_NAME = "ddr_world_hook_packs.json"
# The updater shipped with v1.3 / v1.4 performs the next update and refuses an
# archive of 2 GiB or more uncompressed: a base release must stay below it.
BASE_IMAGE_LIMIT = 2 * 1024 * 1024 * 1024
# The one source that ships in the base release (slug of its folder name).
BASE_SOURCE_SLUG = "custom"
CATALOG_SCHEMA = 1
MARKER_SCHEMA = 1

MODELS_BASE = "data_mods/custom_models"
MARKER_DIR = MODELS_BASE + "/.packs"
ASSET_PREFIX = "ddr-world-pack-"
GROUPS = {"dancers": "Dancers", "stages": "Stages"}
# The role of the arcs that count as one selectable model, per kind.
ITEM_ROLE = {"dancers": "body", "stages": "stage"}

ID_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
HEX64_RE = re.compile(r"^[0-9a-f]{64}$")
# Fixed entry timestamp: nothing about a pack zip depends on when it was built.
ZIP_DATE = (1980, 1, 1, 0, 0, 0)
CHUNK = 1 << 20


class BuildError(Exception):
    """A problem with the input that the operator has to fix."""


class Source(NamedTuple):
    """One source folder = one pack."""

    kind: str  # "dancers" | "stages"
    path: Path
    pack_id: str

    @property
    def title(self) -> str:
        return self.path.name

    @property
    def root(self) -> str:
        """Where the pack's files go, relative to the game folder."""
        return f"{MODELS_BASE}/{self.kind}/{self.path.name}"

    @property
    def marker(self) -> str:
        return f"{MARKER_DIR}/{self.pack_id}.json"


class Built(NamedTuple):
    source: Source
    files: Dict[str, Tuple[int, str]]  # game-relative path -> (size, sha256)
    items: int
    content_id: str

    @property
    def asset(self) -> str:
        return f"{ASSET_PREFIX}{self.source.pack_id}-{self.content_id[:12]}.zip"

    @property
    def installed_size(self) -> int:
        return sum(size for size, _ in self.files.values())


# ---------------------------------------------------------------------------
# Identity
# ---------------------------------------------------------------------------


def slug(name: str) -> str:
    """Lower-case ASCII letters and digits; every other run becomes one `-`."""
    out = re.sub(r"[^a-z0-9]+", "-", P.ascii_lower(name)).strip("-")
    return out


def pack_id(kind: str, source_name: str) -> str:
    s = slug(source_name)
    if not s:
        raise BuildError(f"{kind}/{source_name}: the folder name has no letters or digits")
    return f"{kind}-{s}"


def content_id(files: Dict[str, Tuple[int, str]]) -> str:
    h = hashlib.sha256()
    for path in sorted(files, key=lambda p: p.encode("utf-8")):
        h.update(path.encode("utf-8") + b"\n" + files[path][1].encode("ascii") + b"\n")
    return h.hexdigest()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(CHUNK), b""):
            h.update(chunk)
    return h.hexdigest()


# ---------------------------------------------------------------------------
# Sources
# ---------------------------------------------------------------------------


def default_assets_root() -> Path:
    override = os.environ.get("DDR_WORLD_ASSETS")
    if override:
        return Path(override).expanduser()
    return REPO_ROOT.parent / ASSETS_REPO_NAME


def find_sources(assets_root: Path) -> List[Source]:
    """Every source folder, dancers first, each kind sorted by name (case-insensitive)."""
    if not any((assets_root / k).is_dir() for k in P.KINDS):
        raise BuildError(
            f"{P.display(assets_root)} holds neither dancers/ nor stages/ -- check out the assets "
            "repository beside this one, set DDR_WORLD_ASSETS, or pass --assets"
        )
    sources: List[Source] = []
    for kind in P.KINDS:
        kind_root = assets_root / kind
        if not kind_root.is_dir():
            continue
        files, dirs = P._list(kind_root)
        if files:
            raise BuildError(
                f"{kind}/{files[0].name}: files directly under {kind}/ belong to no pack; "
                "move them into a source folder"
            )
        for d in sorted(dirs, key=lambda p: (p.name.casefold(), p.name)):
            if P.is_model_folder_name(d.name):
                raise BuildError(
                    f"{kind}/{d.name}: a model folder directly under {kind}/ belongs to no pack; "
                    "move it into a source folder"
                )
            sources.append(Source(kind, d, pack_id(kind, d.name)))
    seen: Dict[str, Source] = {}
    for s in sources:
        if s.pack_id in seen:
            raise BuildError(
                f"{s.kind}/{s.title} and {seen[s.pack_id].kind}/{seen[s.pack_id].title} "
                f"both yield the pack id {s.pack_id}; rename one"
            )
        seen[s.pack_id] = s
    return sources


def select(sources: List[Source], only: Optional[List[str]]) -> List[Source]:
    if not only:
        return sources
    known = {s.pack_id for s in sources}
    unknown = [i for i in only if i not in known]
    if unknown:
        raise BuildError(f"no such pack: {', '.join(unknown)}")
    return [s for s in sources if s.pack_id in only]


# ---------------------------------------------------------------------------
# Building one pack
# ---------------------------------------------------------------------------


def materialize(source: Source, image_root: Path) -> Built:
    """Write the pack's shipped form under `image_root` (a game-folder image): the
    source folder with every model folder replaced by its arc, exactly as
    `pack_custom_models.run` would leave it. The marker is NOT written here."""
    dest = image_root / source.root
    if dest.exists():
        raise BuildError(f"{P.display(dest)} already exists")
    kind_root = source.path.parent
    folders = [f for f in P.find_model_folders(kind_root) if source.path in f.parents]
    skip = {f.resolve() for f in folders}

    def copy_tree(src: Path, dst: Path) -> None:
        dst.mkdir(parents=True, exist_ok=True)
        files, dirs = P._list(src)
        for f in files:
            (dst / f.name).write_bytes(f.read_bytes())
        for d in dirs:
            if d.resolve() not in skip:
                copy_tree(d, dst / d.name)

    copy_tree(source.path, dest)
    for folder in folders:
        arc_path = (dest / folder.relative_to(source.path)).parent / (folder.name + ".arc")
        if arc_path.exists():
            raise BuildError(
                f"{source.kind}/{folder.relative_to(kind_root).as_posix()}: "
                f"{arc_path.name} already exists beside the model folder"
            )
        try:
            data, _ = P.pack_folder(folder)
        except (OSError, ValueError, RuntimeError) as e:
            raise BuildError(f"{source.kind}/{folder.relative_to(kind_root).as_posix()}: {e}")
        arc_path.parent.mkdir(parents=True, exist_ok=True)
        P._write_atomic(arc_path, data)

    files: Dict[str, Tuple[int, str]] = {}
    items = 0
    for dirpath, _dirnames, filenames in os.walk(dest):
        for name in filenames:
            path = Path(dirpath) / name
            rel = path.relative_to(image_root).as_posix()
            files[rel] = (path.stat().st_size, sha256_file(path))
            if P.classify_arc_name(name)[0] == ITEM_ROLE[source.kind]:
                items += 1
    if not files:
        raise BuildError(f"{source.kind}/{source.title}: the source folder is empty")
    return Built(source, files, items, content_id(files))


def marker_bytes(built: Built) -> bytes:
    doc = {
        "schema": MARKER_SCHEMA,
        "id": built.source.pack_id,
        "content_id": built.content_id,
        "files": {
            path: {"size": size, "sha256": sha}
            for path, (size, sha) in sorted(built.files.items(), key=lambda kv: kv[0].encode("utf-8"))
        },
    }
    return (json.dumps(doc, indent=1, ensure_ascii=False) + "\n").encode("utf-8")


def write_marker(built: Built, image_root: Path) -> None:
    path = image_root / built.source.marker
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(marker_bytes(built))


def write_zip(built: Built, image_root: Path, zip_path: Path) -> None:
    """Entries in byte-sorted path order, the marker last, fixed timestamps."""
    tmp = zip_path.with_name(zip_path.name + ".tmp")
    with zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:

        def info(name: str) -> zipfile.ZipInfo:
            zi = zipfile.ZipInfo(name, ZIP_DATE)
            zi.compress_type = zipfile.ZIP_DEFLATED
            zi.external_attr = 0o644 << 16
            return zi

        for rel in sorted(built.files, key=lambda p: p.encode("utf-8")):
            with open(image_root / rel, "rb") as src, zf.open(info(rel), "w") as dst:
                for chunk in iter(lambda: src.read(CHUNK), b""):
                    dst.write(chunk)
        zf.writestr(info(built.source.marker), marker_bytes(built), compresslevel=9)
    os.replace(tmp, zip_path)


def catalog_entry(built: Built, url_base: str, zip_path: Optional[Path]) -> dict:
    return {
        "id": built.source.pack_id,
        "group": GROUPS[built.source.kind],
        "title": built.source.title,
        "items": built.items,
        "root": built.source.root,
        "content_id": built.content_id,
        "asset": built.asset,
        "url": url_base + built.asset,
        "zip_size": zip_path.stat().st_size if zip_path else None,
        "zip_sha256": sha256_file(zip_path) if zip_path else None,
        "installed_size": built.installed_size,
    }


def write_json(path: Path, doc: dict) -> None:
    P._write_atomic(path, (json.dumps(doc, indent=2, ensure_ascii=False) + "\n").encode("utf-8"))


# ---------------------------------------------------------------------------
# Catalog validation
# ---------------------------------------------------------------------------


def clean_rel_path(path: str) -> bool:
    if not path or path.startswith("/") or "\\" in path or ":" in path:
        return False
    return all(part not in ("", ".", "..") for part in path.split("/"))


def catalog_problems(doc: object, allow_unbuilt: bool = False) -> List[str]:
    """Everything wrong with a catalog document (empty = usable). `allow_unbuilt`
    accepts null zip fields, as `build --existing` leaves for packs it skipped."""
    if not isinstance(doc, dict):
        return ["the catalog is not a JSON object"]
    problems: List[str] = []
    if doc.get("schema") != CATALOG_SCHEMA:
        problems.append(f"schema is {doc.get('schema')!r}, expected {CATALOG_SCHEMA}")
    packs = doc.get("packs")
    if not isinstance(packs, list):
        return problems + ["`packs` is not a list"]
    ids, roots = set(), set()
    for i, p in enumerate(packs):
        if not isinstance(p, dict):
            problems.append(f"packs[{i}] is not an object")
            continue
        pid = p.get("id")
        where = f"pack {pid!r}" if isinstance(pid, str) else f"packs[{i}]"

        def bad(what: str) -> None:
            problems.append(f"{where}: {what}")

        if not isinstance(pid, str) or not ID_RE.match(pid):
            bad("`id` must be lower-case letters, digits and single dashes")
            pid = None
        elif pid in ids:
            bad("duplicate id")
        else:
            ids.add(pid)
        for key in ("group", "title"):
            if not isinstance(p.get(key), str) or not p[key]:
                bad(f"`{key}` must be a non-empty string")
        if not isinstance(p.get("items"), int) or isinstance(p.get("items"), bool) or p["items"] < 0:
            bad("`items` must be a non-negative integer")
        root = p.get("root")
        if (
            not isinstance(root, str)
            or not clean_rel_path(root)
            or not any(root.startswith(f"{MODELS_BASE}/{k}/") for k in P.KINDS)
            or root.count("/") != 3
        ):
            bad(f"`root` must be a source folder under {MODELS_BASE}/dancers/ or .../stages/")
        elif root.casefold() in roots:
            bad("duplicate root")
        else:
            roots.add(root.casefold())
        cid = p.get("content_id")
        if not isinstance(cid, str) or not HEX64_RE.match(cid):
            bad("`content_id` must be 64 lower-case hex digits")
            cid = None
        asset = p.get("asset")
        if not isinstance(asset, str):
            bad("`asset` must be a string")
        elif pid and cid and asset != f"{ASSET_PREFIX}{pid}-{cid[:12]}.zip":
            bad(f"`asset` must be {ASSET_PREFIX}{pid}-{cid[:12]}.zip")
        url = p.get("url")
        if not isinstance(url, str) or not url.startswith("https://"):
            bad("`url` must be an https URL")
        elif isinstance(asset, str) and not url.endswith("/" + asset):
            bad("`url` must end with the asset name")
        size, sha = p.get("zip_size"), p.get("zip_sha256")
        if size is None and sha is None and allow_unbuilt:
            pass
        else:
            if not isinstance(size, int) or isinstance(size, bool) or size <= 0:
                bad("`zip_size` must be a positive integer")
            if not isinstance(sha, str) or not HEX64_RE.match(sha):
                bad("`zip_sha256` must be 64 lower-case hex digits")
        inst = p.get("installed_size")
        if not isinstance(inst, int) or isinstance(inst, bool) or inst <= 0:
            bad("`installed_size` must be a positive integer")
    return problems


# ---------------------------------------------------------------------------
# Subcommands
# ---------------------------------------------------------------------------


def human(n: int) -> str:
    return f"{n / (1 << 20):.1f} MB"


def cmd_build(assets: Path, out: Path, existing: Iterable[str], url_base: str, only: Optional[List[str]]) -> int:
    sources = select(find_sources(assets), only)
    existing = set(existing)
    out.mkdir(parents=True, exist_ok=True)
    work = out / ".work"
    if work.exists():
        P._rmtree(work)
    entries = []
    built_zips = skipped = 0
    total_zip = total_installed = 0
    print(f"building {len(sources)} pack(s) from {P.display(assets)} into {P.display(out)}")
    for source in sources:
        image = work / source.pack_id
        built = materialize(source, image)
        zip_path = out / built.asset
        if built.asset in existing:
            status, zip_for_entry = "already published", None
            skipped += 1
        else:
            if not zip_path.exists():
                write_zip(built, image, zip_path)
            status, zip_for_entry = human(zip_path.stat().st_size), zip_path
            total_zip += zip_path.stat().st_size
            built_zips += 1
        P._rmtree(image)
        total_installed += built.installed_size
        entries.append(catalog_entry(built, url_base, zip_for_entry))
        print(
            f"  {source.pack_id:<28} {built.items:>4} model(s) {len(built.files):>5} file(s) "
            f"{human(built.installed_size):>10} installed  {built.content_id[:12]}  {status}"
        )
    P._rmtree(work)
    catalog = {"schema": CATALOG_SCHEMA, "packs": entries}
    problems = catalog_problems(catalog, allow_unbuilt=True)
    if problems:
        raise BuildError("the catalog this build produced is invalid: " + "; ".join(problems))
    write_json(out / CATALOG_NAME, catalog)
    print(
        f"{built_zips} zip(s), {human(total_zip)}"
        + (f"; {skipped} pack(s) already published" if skipped else "")
        + f"; {human(total_installed)} installed in total; wrote {CATALOG_NAME}"
    )
    return 0


def cmd_stage(assets: Path, into: Path, only: Optional[List[str]]) -> int:
    sources = select(find_sources(assets), only)
    for source in sources:
        built = materialize(source, into)
        write_marker(built, into)
        print(f"  {source.pack_id:<28} {len(built.files):>5} file(s)  {human(built.installed_size)}")
    print(f"staged {len(sources)} pack(s) into {P.display(into)}")
    return 0


def cmd_check(catalog_path: Path, allow_unbuilt: bool) -> int:
    try:
        doc = json.loads(catalog_path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        print(f"error: cannot read {P.display(catalog_path)}: {e}", file=sys.stderr)
        return 1
    problems = catalog_problems(doc, allow_unbuilt)
    for p in problems:
        print(f"error: {p}", file=sys.stderr)
    if problems:
        return 1
    print(f"{P.display(catalog_path)}: {len(doc['packs'])} pack(s), valid")
    return 0


def published_catalog(built: dict, shelf: dict) -> dict:
    """The catalog a modpack release embeds: `built` (a `build` catalog) with every
    pack's zip size and digest taken from the shelf's asset of that name. `shelf` is
    the GitHub release object (`assets[].name/size/digest`). A rebuilt zip can differ
    byte for byte from the published one while holding identical files, so the
    published asset's values are the only ones a downloader can verify against."""
    problems = catalog_problems(built, allow_unbuilt=True)
    if problems:
        raise BuildError("the built catalog is invalid: " + "; ".join(problems))
    assets = {a.get("name"): a for a in shelf.get("assets", []) if isinstance(a, dict)}
    packs = []
    for p in built["packs"]:
        asset = assets.get(p["asset"])
        if asset is None:
            raise BuildError(f"pack {p['id']}: {p['asset']} is not on the shelf -- publish it first")
        if asset.get("state", "uploaded") != "uploaded":
            raise BuildError(f"pack {p['id']}: {p['asset']} is still uploading")
        digest = asset.get("digest") or ""
        sha = digest[len("sha256:"):] if digest.startswith("sha256:") else ""
        if not HEX64_RE.match(sha):
            raise BuildError(f"pack {p['id']}: the shelf publishes no sha256 digest for {p['asset']}")
        size = asset.get("size")
        if not isinstance(size, int) or size <= 0:
            raise BuildError(f"pack {p['id']}: the shelf reports no size for {p['asset']}")
        packs.append(dict(p, zip_size=size, zip_sha256=sha))
    doc = {"schema": CATALOG_SCHEMA, "packs": packs}
    problems = catalog_problems(doc)
    if problems:
        raise BuildError("the published catalog would be invalid: " + "; ".join(problems))
    return doc


def cmd_catalog(built_path: Path, shelf_path: Path, out: Path) -> int:
    try:
        built = json.loads(built_path.read_text(encoding="utf-8"))
        shelf = json.loads(shelf_path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise BuildError(f"cannot read the inputs: {e}")
    doc = published_catalog(built, shelf)
    write_json(out, doc)
    total = sum(p["zip_size"] for p in doc["packs"])
    print(f"wrote {P.display(out)}: {len(doc['packs'])} pack(s), {human(total)} to download in total")
    return 0


def fetch_url(url: str) -> bytes:
    """An anonymous GET — exactly what a cabinet's updater can do."""
    request = urllib.request.Request(url, headers={"User-Agent": "ddr-world-build-packs"})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read()


def prune_base_sources(image: Path) -> List[str]:
    """Remove everything under the image's dancers/ and stages/ except the
    `Custom` source: ported sources are packs, never base content. (The code
    repository ignores them, so a stray local copy or symlink can sit there.)"""
    removed = []
    for kind in P.KINDS:
        kind_root = image / MODELS_BASE / kind
        if not kind_root.is_dir():
            continue
        for entry in sorted(os.scandir(kind_root), key=lambda e: e.name):
            if entry.is_dir(follow_symlinks=False) and slug(entry.name) == BASE_SOURCE_SLUG:
                continue
            path = Path(entry.path)
            if entry.is_dir(follow_symlinks=False):
                P._rmtree(path)
            else:
                path.unlink()
            removed.append(f"{kind}/{entry.name}")
    return removed


def image_size(image: Path) -> int:
    return sum(
        (Path(d) / f).stat().st_size for d, _dirs, files in os.walk(image) for f in files
    )


def cmd_base_image(image: Path, catalog_file: Optional[Path], url: str, fetch=fetch_url) -> int:
    """Finish a staged base release image. `catalog_file` (PACKS_CATALOG)
    overrides the download for tester builds."""
    if not image.is_dir():
        raise BuildError(f"{P.display(image)} is not a folder")
    for name in prune_base_sources(image):
        print(f"WARNING: left out of the base release (ported sources ship as packs): {name}", file=sys.stderr)

    published: Optional[bytes]
    try:
        published = fetch(url)
    except (OSError, urllib.error.URLError, ValueError) as e:
        published, why = None, str(e)
    if catalog_file is not None:
        try:
            data = catalog_file.read_bytes()
        except OSError as e:
            raise BuildError(f"cannot read PACKS_CATALOG: {e}")
        source = P.display(catalog_file)
    elif published is not None:
        data, source = published, url
    else:
        raise BuildError(
            f"cannot download the pack catalog ({url}): {why}\n"
            "       Publish the packs first (scripts/publish_packs.sh), or set PACKS_CATALOG to a "
            "catalog file for a tester build."
        )
    try:
        doc = json.loads(data.decode("utf-8"))
    except ValueError as e:
        raise BuildError(f"the pack catalog from {source} is not JSON: {e}")
    problems = catalog_problems(doc)
    if problems:
        raise BuildError(f"the pack catalog from {source} is invalid: " + "; ".join(problems))
    (image / GAME_CATALOG_NAME).write_bytes(data)

    size = image_size(image)
    if size >= BASE_IMAGE_LIMIT:
        raise BuildError(
            f"the base release would be {human(size)} uncompressed; the updater already installed on "
            f"cabinets refuses {human(BASE_IMAGE_LIMIT)} or more"
        )
    total = sum(p["zip_size"] for p in doc["packs"])
    print(
        f"{GAME_CATALOG_NAME}: {len(doc['packs'])} pack(s), {human(total)} of optional downloads "
        f"(from {source}); base image {human(size)} uncompressed"
    )
    if catalog_file is not None:
        if published is None:
            print(
                f"WARNING: {url} cannot be downloaded without credentials ({why}).\n"
                "         Cabinets cannot fetch packs with this build until the packs are published "
                "and the assets repository is public.",
                file=sys.stderr,
            )
        elif published != data:
            print(
                "WARNING: the embedded catalog differs from the published one; packs it names may not "
                "be on the shelf.",
                file=sys.stderr,
            )
    return 0


def main(argv: Optional[List[str]] = None) -> int:
    ap = argparse.ArgumentParser(description="Build optional content packs from the assets repository.")
    sub = ap.add_subparsers(dest="cmd", required=True)

    def assets_arg(p: argparse.ArgumentParser) -> None:
        p.add_argument("--assets", type=Path, help="the assets checkout (default: see the module docs)")
        p.add_argument("--only", nargs="+", metavar="ID", help="only these pack ids")

    b = sub.add_parser("build", help="pack every source; write zips and catalog.json")
    assets_arg(b)
    b.add_argument("--out", type=Path, default=REPO_ROOT / "release" / "packs")
    b.add_argument(
        "--existing",
        type=Path,
        help="file of asset names already published (one per line); those zips are not written",
    )
    b.add_argument("--url-base", default=URL_BASE)

    s = sub.add_parser("stage", help="write packed files and markers into a game-folder image")
    assets_arg(s)
    s.add_argument("--into", type=Path, required=True)

    k = sub.add_parser("catalog", help="write the catalog a modpack release embeds")
    k.add_argument("--built", type=Path, required=True, help="catalog.json written by `build`")
    k.add_argument("--shelf", type=Path, required=True, help="the shelf release as JSON (GitHub REST release object)")
    k.add_argument("--out", type=Path, required=True)

    c = sub.add_parser("check", help="validate a catalog file")
    c.add_argument("catalog", type=Path)
    c.add_argument("--allow-unbuilt", action="store_true", help="accept packs without zip size and digest")

    i = sub.add_parser("base-image", help="finish a staged base release image")
    i.add_argument("image", type=Path, help="the staged game-folder image")
    i.add_argument("--url", default=URL_BASE + CATALOG_NAME, help="where the published catalog is")

    args = ap.parse_args(argv)
    try:
        if args.cmd == "build":
            existing: List[str] = []
            if args.existing:
                existing = [l.strip() for l in args.existing.read_text(encoding="utf-8").splitlines() if l.strip()]
            return cmd_build(
                (args.assets or default_assets_root()).resolve(),
                args.out.resolve(),
                existing,
                args.url_base,
                args.only,
            )
        if args.cmd == "stage":
            return cmd_stage((args.assets or default_assets_root()).resolve(), args.into.resolve(), args.only)
        if args.cmd == "base-image":
            override = os.environ.get("PACKS_CATALOG")
            return cmd_base_image(
                args.image.resolve(),
                Path(override).expanduser() if override else None,
                args.url,
            )
        if args.cmd == "catalog":
            return cmd_catalog(args.built, args.shelf, args.out)
        return cmd_check(args.catalog, args.allow_unbuilt)
    except BuildError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
