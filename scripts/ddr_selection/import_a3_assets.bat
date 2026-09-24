@echo off
rem DDR SELECTION - copy the DDR A3 files the feature needs into a DDR World
rem install's LayeredFS mod folder. Same contract as import_a3_assets.sh.
rem
rem Usage:
rem   import_a3_assets.bat "<A3 install>" ["<World install>"]
rem
rem Each path is the folder that CONTAINS data\. <World install> defaults to
rem the folder above this script's folder (the release ships the script in
rem <game>\ddr_selection_import\). Files go to
rem   <World install>\data_mods\ddr_selection_a3\<path relative to data\>
rem - nothing under World's own data\ is touched.
rem
rem a3_assets.manifest (next to this script) lists the files: "always" entries
rem are copied every run (today only arc\bm2d\dance_combo0005_v0.arc, whose
rem World copy is blanked); "missing" entries only when World has no copy of
rem its own (a stock World install has them all).

setlocal EnableExtensions EnableDelayedExpansion

set "SCRIPT_DIR=%~dp0"
if "%~1"=="" goto :usage
if not "%~3"=="" goto :usage
set "A3_ROOT=%~f1"
if "%~2"=="" (
  for %%P in ("%SCRIPT_DIR%..") do set "WORLD_ROOT=%%~fP"
) else (
  set "WORLD_ROOT=%~f2"
)
set "MANIFEST=%SCRIPT_DIR%a3_assets.manifest"

if not exist "%MANIFEST%" (
  echo error: a3_assets.manifest not found next to this script
  exit /b 2
)
if not exist "%A3_ROOT%\data\" (
  echo error: "%A3_ROOT%" has no data\ folder ^(A3 install^)
  exit /b 2
)
if not exist "%WORLD_ROOT%\data\" (
  echo error: "%WORLD_ROOT%" has no data\ folder ^(World install^)
  exit /b 2
)

set "DEST=%WORLD_ROOT%\data_mods\ddr_selection_a3"
set /a COPIED=0, FAILED=0
rem (the "%%B" guard skips blank lines, which Wine's cmd hands to for /f)
for /f "usebackq eol=# tokens=1,2" %%A in ("%MANIFEST%") do (
  if not "%%B"=="" (
    set "MODE=%%A"
    set "REL=%%B"
    set "REL=!REL:/=\!"
    call :entry
  )
)

echo done: %COPIED% file^(s^) copied to data_mods\ddr_selection_a3\, %FAILED% missing from the A3 install
if %FAILED% GTR 0 exit /b 1
exit /b 0

:entry
if /i "%MODE%"=="missing" if exist "%WORLD_ROOT%\data\%REL%" goto :eof
if not exist "%A3_ROOT%\data\%REL%" (
  echo   MISSING IN A3  %REL%
  set /a FAILED+=1
  goto :eof
)
for %%D in ("%DEST%\%REL%") do if not exist "%%~dpD" mkdir "%%~dpD"
copy /y /b "%A3_ROOT%\data\%REL%" "%DEST%\%REL%" >nul
echo   copied  %REL%
set /a COPIED+=1
goto :eof

:usage
echo usage: import_a3_assets.bat "<A3 install>" ["<World install>"]  (each = the folder that contains data\)
exit /b 2
