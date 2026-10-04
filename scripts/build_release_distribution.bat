@echo off
rem build_release_distribution.bat -- the Windows counterpart of
rem scripts/build_release_archive.sh, minus the archive: builds the Windows 7
rem hook DLL and auto-updater and stages a ready-to-copy game-folder image in
rem release\ (gitignored):
rem
rem   release\ddr_world_hook.dll             Windows 7 build of the hook
rem   release\ddr_world_hook_updater.exe     Windows 7 build of the updater
rem   release\mod-config.json, judgement_offsets.csv, README.md
rem   release\data_mods\                     data_mods as checked in, with every
rem                                          Background Dancers MODEL FOLDER
rem                                          pre-packed into its ready .arc
rem   release\ddr_selection_import\          the A3 importer (.sh/.manifest LF,
rem                                          .bat CRLF)
rem
rem Copy the CONTENTS of release\ into the game's contents\ folder (anything
rem else release\ may hold -- the macOS script's .zip / install-update .bat --
rem is not part of the image). No .zip and no tester install .bat are made:
rem those need the archive. The checkout itself is never modified; the model
rem folders stay loose in the repo and are packed only in release\.
rem
rem Usage: scripts\build_release_distribution.bat [--no-pause]   (any cwd)
rem
rem Re-runnable: release\data_mods is deleted, copied fresh from the checkout
rem and packed again on every run; the other staged items are replaced.
rem
rem Requires, on PATH:
rem   - Rust via rustup (the repo's rust-toolchain.toml selects nightly with
rem     rust-src; x86_64-win7-windows-msvc is a tier-3 target, so std is built
rem     from source with -Z build-std -- BOTH binaries use this recipe)
rem   - the MSVC linker (Visual Studio Build Tools, "Desktop development with C++")
rem   - Python 3.8+ (py launcher or python.exe)
rem Keep the checkout path short (or enable Windows long paths): the deepest
rem staged file is release\data_mods\ + 136 characters.

setlocal EnableExtensions
rem Pause at the end when launched by double-click (cmd /c) so the output can
rem be read; `--no-pause` turns that off (cmd /c from PowerShell looks alike).
set "PAUSE_AT_END="
for %%A in (%cmdcmdline%) do if /i "%%~A"=="/c" set "PAUSE_AT_END=1"
if /i "%~1"=="--no-pause" set "PAUSE_AT_END="

rem Everything below is repo-relative (so no absolute path, which could hold a
rem ")", ever lands inside a ( ... ) block).
cd /d "%~dp0.." || goto :fail
set "WIN7_TARGET=x86_64-win7-windows-msvc"
set "DLL=target\%WIN7_TARGET%\release\ddr_world_hook.dll"
set "UPDATER=updater\target\%WIN7_TARGET%\release\ddr_world_hook_updater.exe"
set "RELEASE_DIR=release"
set "PYTHONIOENCODING=utf-8"
set "PYTHONDONTWRITEBYTECODE=1"

rem ---- prerequisites ---------------------------------------------------------
where cargo >nul 2>nul || (echo ERROR: cargo not found on PATH -- install Rust with rustup. & goto :fail)
set "PY="
py -3 -c "import sys; sys.exit(0 if sys.version_info >= (3, 8) else 1)" >nul 2>nul && set "PY=py -3"
if not defined PY python -c "import sys; sys.exit(0 if sys.version_info >= (3, 8) else 1)" >nul 2>nul && set "PY=python"
if not defined PY (echo ERROR: Python 3.8+ not found -- install it from python.org ^(the py launcher^). & goto :fail)

rem ---- build -----------------------------------------------------------------
echo ==^> Building Windows 7 DLL
cargo build --release --target %WIN7_TARGET% -Z build-std=std,panic_abort
if errorlevel 1 goto :fail
if not exist "%DLL%" (echo ERROR: expected build output not found: %DLL% & goto :fail)

echo ==^> Building Windows 7 updater
pushd updater || goto :fail
cargo build --release --target %WIN7_TARGET% -Z build-std=std,panic_abort
set "RC=%errorlevel%"
popd
if not "%RC%"=="0" goto :fail
if not exist "%UPDATER%" (echo ERROR: expected build output not found: %UPDATER% & goto :fail)

echo ==^> Verifying Windows 7 compatibility
call :assert_win7_safe "%DLL%" || goto :fail
call :assert_win7_safe "%UPDATER%" || goto :fail

rem ---- stage -----------------------------------------------------------------
echo ==^> Staging release contents into release\
if not exist "%RELEASE_DIR%" mkdir "%RELEASE_DIR%" || goto :fail
for %%F in ("%DLL%" "%UPDATER%" mod-config.json judgement_offsets.csv README.md) do (
    copy /Y "%%~F" "%RELEASE_DIR%\" >nul || goto :fail
)

rem data_mods as checked in (the .sh's rsync), minus OS metadata and any
rem machine-owned _cache\ (never shipping content); the previous run's staged
rem copy is deleted first (read-only flags cleared).
echo ==^> Copying data_mods
%PY% -c "import os, shutil, stat, sys; src, dst = sys.argv[1:3]; h = lambda f, p, e: (os.chmod(p, stat.S_IWRITE), f(p)); kw = {'onexc': h} if sys.version_info >= (3, 12) else {'onerror': h}; os.path.isdir(dst) and shutil.rmtree(dst, **kw); shutil.copytree(src, dst, ignore=shutil.ignore_patterns('.DS_Store', 'Thumbs.db', 'desktop.ini', '_cache'))" data_mods "%RELEASE_DIR%\data_mods"
if errorlevel 1 (echo ERROR: could not copy data_mods into release\. & goto :fail)

rem Background Dancers custom content ships PRE-PACKED (see the .sh): each
rem model folder of the STAGED copy becomes the ready .arc the DLL would
rem otherwise pack on every cabinet's first boot. --force: release\ sits inside
rem the git work tree (ignored), which the packer otherwise refuses.
if exist "%RELEASE_DIR%\data_mods\custom_models" (
    echo ==^> Pre-packing custom models
    %PY% scripts\pack_custom_models.py --in-place --force "%RELEASE_DIR%\data_mods\custom_models"
    if errorlevel 1 goto :fail
)

rem DDR SELECTION's A3 importer. The .sh and the manifest it reads with
rem `while read` must be LF; cmd.exe wants the .bat CRLF -- normalised here
rem whatever line endings this checkout uses.
set "IMPORT_DIR=%RELEASE_DIR%\ddr_selection_import"
if exist "%IMPORT_DIR%" rmdir /s /q "%IMPORT_DIR%"
mkdir "%IMPORT_DIR%" || goto :fail
call :to_lf "scripts\ddr_selection\import_a3_assets.sh" "%IMPORT_DIR%\import_a3_assets.sh" || goto :fail
call :to_lf "scripts\ddr_selection\a3_assets.manifest" "%IMPORT_DIR%\a3_assets.manifest" || goto :fail
call :to_crlf "scripts\ddr_selection\import_a3_assets.bat" "%IMPORT_DIR%\import_a3_assets.bat" || goto :fail

rem ---- summary ---------------------------------------------------------------
echo.
echo Done: release\ holds the game-folder image -- copy its contents into the game's contents\ folder.
echo   ddr_world_hook.dll, ddr_world_hook_updater.exe  (Windows 7 builds)
%PY% -c "import os, sys; n = [f for _, _, fs in os.walk(sys.argv[1]) for f in fs]; a = sum(f.lower().endswith('.arc') for f in n); print('  data_mods\\  {} files, {} custom model arcs'.format(len(n), a))" "%RELEASE_DIR%\data_mods"
echo   ddr_selection_import\
if defined PAUSE_AT_END pause
endlocal
exit /b 0

:fail
echo.
echo BUILD FAILED.
if defined PAUSE_AT_END pause
endlocal
exit /b 1

rem ---- helpers ---------------------------------------------------------------

rem Fail if a binary would not load on Windows 7: any ProcessPrng reference
rem (bcryptprimitives.dll) means it was not built with the win7 recipe. Fails
rem CLOSED: an unreadable file (Python exits 1) is an error too.
:assert_win7_safe
%PY% -c "import sys; sys.exit(3 if b'ProcessPrng' in open(sys.argv[1], 'rb').read() else 0)" "%~1"
if errorlevel 3 (
    echo ERROR: %~1 imports ProcessPrng ^(bcryptprimitives.dll^) and will not load on Windows 7.
    echo        It was not built with the %WIN7_TARGET% recipe.
    exit /b 1
)
if errorlevel 1 (
    echo ERROR: could not read %~1 to verify it is Windows 7 safe.
    exit /b 1
)
exit /b 0

rem Copy %1 to %2 with LF line endings (bytes otherwise untouched).
:to_lf
%PY% -c "import sys; d = open(sys.argv[1], 'rb').read().replace(b'\r\n', b'\n'); open(sys.argv[2], 'wb').write(d)" "%~1" "%~2"
if errorlevel 1 (echo ERROR: could not write %~2 & exit /b 1)
exit /b 0

rem Copy %1 to %2 with CRLF line endings (bytes otherwise untouched).
:to_crlf
%PY% -c "import sys; d = open(sys.argv[1], 'rb').read().replace(b'\r\n', b'\n').replace(b'\n', b'\r\n'); open(sys.argv[2], 'wb').write(d)" "%~1" "%~2"
if errorlevel 1 (echo ERROR: could not write %~2 & exit /b 1)
exit /b 0
