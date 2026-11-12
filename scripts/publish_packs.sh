#!/usr/bin/env bash
set -euo pipefail
# Publish the optional content packs: build every pack from the assets checkout,
# upload the pack zips the shelf does not have yet, then replace the shelf's
# catalog.json (design: .agents/planning/2026-11-11-optional-content-packs/design/
# detailed-design.md §4.3).
#
# The SHELF is one GitHub release, tagged `packs`, in the assets repository. It is
# file storage, not a version: every pack zip ever published sits there under a
# name that carries its content id, so a pack is uploaded exactly once per
# revision and a run that changes no model uploads nothing but the catalog.
# Modpack release builds download catalog.json from it and embed it.
#
# Order matters: zips first, catalog last — a published catalog never names a
# file that is not there.
#
# Usage: scripts/publish_packs.sh [--dry-run]
#   --dry-run   build and report what would be uploaded; upload nothing
#
# Environment:
#   DDR_WORLD_ASSETS   the assets checkout (default: the folder
#                      ddr-world-universal-modpack-assets beside this repository)
#   PACKS_REPO         OWNER/NAME of the assets repository
#                      (default: skogaby/ddr-world-universal-modpack-assets)
#
# Requires the GitHub CLI, logged in with push access to the assets repository
# (`brew install gh` / https://cli.github.com, then `gh auth login`), and Python 3.8+.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

PACKS_REPO="${PACKS_REPO:-skogaby/ddr-world-universal-modpack-assets}"
TAG="packs"
OUT="release/packs"
PUBLISHED="$OUT/published"
URL_BASE="https://github.com/$PACKS_REPO/releases/download/$TAG/"
ASSETS="${DDR_WORLD_ASSETS:-$REPO_ROOT/../ddr-world-universal-modpack-assets}"

DRY_RUN=""
for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        *) echo "usage: $0 [--dry-run]" >&2; exit 2 ;;
    esac
done

if ! command -v gh >/dev/null 2>&1; then
    echo "ERROR: the GitHub CLI (gh) is not installed. Install it (brew install gh, or" >&2
    echo "       https://cli.github.com) and run 'gh auth login'." >&2
    exit 1
fi
if ! gh auth status >/dev/null 2>&1; then
    echo "ERROR: gh is not logged in. Run 'gh auth login'." >&2
    exit 1
fi
if [[ ! -d "$ASSETS/dancers" && ! -d "$ASSETS/stages" ]]; then
    echo "ERROR: no assets checkout found. Clone $PACKS_REPO beside this repository or set DDR_WORLD_ASSETS." >&2
    exit 1
fi

# What is published should be what is in the assets repository's history.
if [[ -n "$(git -C "$ASSETS" status --porcelain 2>/dev/null)" ]]; then
    echo "WARNING: the assets checkout has uncommitted changes; the packs will contain them." >&2
fi
if ! git -C "$ASSETS" diff --quiet '@{upstream}' HEAD 2>/dev/null; then
    echo "WARNING: the assets checkout differs from its upstream branch (unpushed or unpulled commits)." >&2
fi

mkdir -p "$OUT" "$PUBLISHED"
SHELF="$OUT/shelf.json"
NAMES="$OUT/shelf-names.txt"

# The shelf's release id, or empty when the release does not exist yet.
release_id() {
    local id
    # On a 404 gh prints the error body to stdout: accept a numeric id only.
    id="$(gh api "repos/$PACKS_REPO/releases/tags/$TAG" --jq '.id' 2>/dev/null || true)"
    if [[ "$id" =~ ^[0-9]+$ ]]; then
        echo "$id"
    fi
}

# Write the shelf's assets to $SHELF as {"assets": [...]} and their names to
# $NAMES. The release's own asset list is paginated, so it is read page by page.
read_shelf() {
    local id="$1"
    if [[ -z "$id" ]]; then
        echo '{"assets": []}' > "$SHELF"
    else
        gh api --paginate "repos/$PACKS_REPO/releases/$id/assets?per_page=100" --jq '.[]' \
            | python3 -c 'import json, sys; print(json.dumps({"assets": [json.loads(l) for l in sys.stdin if l.strip()]}))' \
            > "$SHELF"
    fi
    python3 -c 'import json, sys; [print(a["name"]) for a in json.load(open(sys.argv[1]))["assets"]]' "$SHELF" > "$NAMES"
}

echo "==> Reading the shelf ($PACKS_REPO, release '$TAG')"
ID="$(release_id)"
if [[ -z "$ID" ]]; then
    echo "    the release does not exist yet"
fi
read_shelf "$ID"
echo "    $(grep -c . "$NAMES" || true) asset(s) on the shelf"

echo "==> Building packs"
DDR_WORLD_ASSETS="$ASSETS" python3 scripts/build_packs.py build \
    --out "$OUT" --existing "$NAMES" --url-base "$URL_BASE"

# The zips this build wrote are exactly the ones the shelf lacks.
UPLOADS=()
while IFS= read -r name; do
    [[ -n "$name" ]] && UPLOADS+=("$name")
done < <(python3 -c '
import json, sys
for p in json.load(open(sys.argv[1]))["packs"]:
    if p["zip_size"] is not None:
        print(p["asset"])
' "$OUT/catalog.json")

echo "==> ${#UPLOADS[@]} pack zip(s) to upload"
for name in ${UPLOADS[@]+"${UPLOADS[@]}"}; do
    echo "    $name"
done

if [[ -n "$DRY_RUN" ]]; then
    echo "Dry run: nothing uploaded. The catalog would be replaced after the uploads."
    exit 0
fi

if [[ -z "$ID" ]]; then
    echo "==> Creating the '$TAG' release"
    gh release create "$TAG" --repo "$PACKS_REPO" --latest=false \
        --title "Optional content packs" \
        --notes "File storage for the DDR World Universal Modpack's optional dancer and stage packs. The modpack's updater downloads the packs a user selects from here; a pack zip can also be unzipped by hand into the game folder. Each zip's name ends with its content id, and catalog.json lists the current revision of every pack. This is not a versioned release: files are added as packs change."
    ID="$(release_id)"
fi

for name in ${UPLOADS[@]+"${UPLOADS[@]}"}; do
    echo "==> Uploading $name"
    gh release upload "$TAG" "$OUT/$name" --repo "$PACKS_REPO"
done

echo "==> Writing the catalog"
read_shelf "$ID"
python3 scripts/build_packs.py catalog \
    --built "$OUT/catalog.json" --shelf "$SHELF" --out "$PUBLISHED/catalog.json"
gh release upload "$TAG" "$PUBLISHED/catalog.json" --repo "$PACKS_REPO" --clobber

echo "==> Checking the catalog is downloadable without credentials"
if curl -fsSL -o /dev/null "${URL_BASE}catalog.json"; then
    echo "    ok: ${URL_BASE}catalog.json"
else
    echo "WARNING: ${URL_BASE}catalog.json is not reachable anonymously." >&2
    echo "         If the assets repository is private, cabinets cannot download packs." >&2
fi

echo "Done: ${#UPLOADS[@]} zip(s) uploaded, catalog replaced."
