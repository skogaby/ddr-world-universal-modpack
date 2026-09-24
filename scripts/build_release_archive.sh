#!/usr/bin/env bash
set -euo pipefail
# Build a release of the modpack: the Windows 7 builds of the hook DLL and the
# auto-updater exe, plus the runtime data files an operator needs.
#
# Output (all in release/, gitignored):
#   ddr-world-universal-modpack-YYYYMMDD.zip   the GitHub release asset (flat:
#                                              zip root = game folder; carries
#                                              the updater exe so installs
#                                              self-update)
#   ddr_world_hook_updater.exe                 bare copy for testers who do not
#                                              have the updater yet
#   install-update-YYYYMMDD.bat                tester one-click installer: runs
#                                              the updater with --from-zip on
#                                              THIS zip (same date), so private
#                                              builds get the same merges /
#                                              backups / rollback as a public
#                                              release. Send all three files.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Windows 7 compatible target. The default x86_64-pc-windows-msvc target
# imports ProcessPrng from bcryptprimitives.dll, which doesn't exist on Win7 and
# causes the loader to reject the binary with "The procedure entry point
# ProcessPrng could not be located". The x86_64-win7-windows-msvc target uses
# RtlGenRandom instead. It's a tier 3 target, so std must be built from source
# via -Z build-std. BOTH binaries below must use this recipe.
WIN7_TARGET="x86_64-win7-windows-msvc"
DLL="target/$WIN7_TARGET/release/ddr_world_hook.dll"
UPDATER="updater/target/$WIN7_TARGET/release/ddr_world_hook_updater.exe"

RELEASE_DIR="release"
STAMP="$(date +%Y%m%d)"
ARCHIVE_NAME="ddr-world-universal-modpack-$STAMP.zip"
ARCHIVE="$RELEASE_DIR/$ARCHIVE_NAME"
INSTALL_BAT="$RELEASE_DIR/install-update-$STAMP.bat"

# Tag the updater records in its manifest for a private build: the archive date
# plus the commit it was built from, so a tester's log names the exact build.
# The updater re-installs whenever tag OR zip digest differ, so two same-day
# builds still both install.
GIT_SHA="$(git rev-parse --short HEAD 2>/dev/null || echo nogit)"
GIT_DIRTY=""
if [[ -n "$(git status --porcelain --untracked-files=no 2>/dev/null)" ]]; then
    GIT_DIRTY="-dirty"
fi
BUILD_TAG="dev-$STAMP-$GIT_SHA$GIT_DIRTY"

# Fail the release if a binary would not load on Windows 7 (see above). Fails
# CLOSED when `strings` is unavailable: a missing tool must not pass silently.
assert_win7_safe() {
    local bin="$1"
    if ! command -v strings >/dev/null 2>&1; then
        echo "ERROR: 'strings' not found; cannot verify $bin is Windows 7 safe" >&2
        exit 1
    fi
    if strings "$bin" | grep -q ProcessPrng; then
        echo "ERROR: $bin imports ProcessPrng (bcryptprimitives.dll) and will not load on Windows 7." >&2
        echo "       It was not built with the $WIN7_TARGET recipe." >&2
        exit 1
    fi
}

# The tester installer. Every path is anchored on the .bat's own folder, so it
# works from a double-click regardless of the working directory; the updater
# then resolves the game folder from the exe's location and refuses anything
# that is not one. It pauses at the end because a double-clicked .bat shares its
# console with cmd.exe (the updater's own "alone on the console" wait never
# fires there) and the window would otherwise close before the output is read.
write_install_bat() {
    local out="$1" zip_name="$2" tag="$3"
    # CRLF line endings: written LF here, converted below.
    cat > "$out" <<EOF
@echo off
setlocal
cd /d "%~dp0"
echo DDR World Universal Modpack -- test build installer
echo   archive: $zip_name
echo   tag:     $tag
echo.
if not exist "%~dp0spice64.exe" if not exist "%~dp0ddr_world_hook.dll" (
    echo This folder does not look like the DDR World game folder: no spice64.exe or ddr_world_hook.dll here.
    echo Copy ddr_world_hook_updater.exe, $zip_name and this .bat into the folder that contains spice64.exe, then run this .bat again.
    goto :end
)
if not exist "%~dp0ddr_world_hook_updater.exe" (
    echo ddr_world_hook_updater.exe is missing. Copy it next to this .bat and run again.
    goto :end
)
if not exist "%~dp0$zip_name" (
    echo $zip_name is missing. Copy it next to this .bat and run again.
    goto :end
)
tasklist /FI "IMAGENAME eq spice64.exe" 2>nul | find /I "spice64.exe" >nul
if not errorlevel 1 (
    echo The game is running. Close it, then run this .bat again.
    goto :end
)
"%~dp0ddr_world_hook_updater.exe" --from-zip "%~dp0$zip_name" --tag "$tag"
set "RC=%errorlevel%"
echo.
if "%RC%"=="0" echo Done. Your settings and per-song offsets were kept; start the game normally.
if "%RC%"=="1" echo The update failed AND could not be fully rolled back. Check ddr_world_hook_updater.log and the .ddr_world_hook_updater\backup folder before starting the game.
echo.
echo NOTE: if your gamestart.bat runs ddr_world_hook_updater.exe automatically, the next launch will
echo replace this test build with the latest PUBLIC release. Remove that line while testing a private build.
:end
echo.
pause
endlocal
EOF
    # cmd.exe is happiest with CRLF.
    sed -i.bak 's/$/\r/' "$out" && rm -f "$out.bak"
}

echo "==> Building Windows 7 DLL"
cargo xwin build \
    --release \
    --target "$WIN7_TARGET" \
    -Z build-std=std,panic_abort

if [[ ! -f "$DLL" ]]; then
    echo "ERROR: expected build output not found: $DLL" >&2
    exit 1
fi

echo "==> Building Windows 7 updater"
(cd updater && cargo xwin build \
    --release \
    --target "$WIN7_TARGET" \
    -Z build-std=std,panic_abort)

if [[ ! -f "$UPDATER" ]]; then
    echo "ERROR: expected build output not found: $UPDATER" >&2
    exit 1
fi

echo "==> Verifying Windows 7 compatibility"
assert_win7_safe "$DLL"
assert_win7_safe "$UPDATER"

echo "==> Staging release contents"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

cp "$DLL" "$UPDATER" "$STAGE/"
cp mod-config.json judgement_offsets.csv README.md "$STAGE/"

# data_mods/ as checked in, minus macOS metadata. Nothing else needs excluding:
# runtime artifacts (_cache/, *.arc) never exist in a checkout (*.arc is
# gitignored; _cache/ is created on cabinets), and the committed *_ifs/
# directories (custom_options, custom_folders, custom_series,
# music_wheel_song_length) are intended shipping content generated by
# scripts/gen_option_labels.py and friends. The updater prunes files a later
# release drops, so what ships here is exactly what cabinets end up with.
rsync -a \
    --exclude '.DS_Store' \
    data_mods "$STAGE/"

# DDR SELECTION's A3 importer (operators run it once against their own A3
# install — the repo never ships A3 data). Lands in <game>/ddr_selection_import/
# so the scripts' default World path (the folder above them) is the game folder.
mkdir -p "$STAGE/ddr_selection_import"
cp scripts/ddr_selection/import_a3_assets.sh scripts/ddr_selection/a3_assets.manifest \
    "$STAGE/ddr_selection_import/"
# cmd.exe is happiest with CRLF.
sed 's/$/\r/' scripts/ddr_selection/import_a3_assets.bat \
    > "$STAGE/ddr_selection_import/import_a3_assets.bat"

echo "==> Creating $ARCHIVE"
mkdir -p "$RELEASE_DIR"
rm -f "$ARCHIVE" "$INSTALL_BAT"
(cd "$STAGE" && zip -r -q "$REPO_ROOT/$ARCHIVE" .)

echo "==> Writing tester files"
cp "$UPDATER" "$RELEASE_DIR/"
write_install_bat "$INSTALL_BAT" "$ARCHIVE_NAME" "$BUILD_TAG"

echo "Done: $RELEASE_DIR/"
echo "  $ARCHIVE_NAME  (GitHub release asset; $(unzip -l "$ARCHIVE" | tail -1 | awk '{print $2}') files)"
echo "  ddr_world_hook_updater.exe  (bare copy for testers)"
echo "  install-update-$STAMP.bat  (tester installer, tag $BUILD_TAG)"
unzip -l "$ARCHIVE" | grep -E 'ddr_world_hook(_updater\.exe|\.dll)$'
