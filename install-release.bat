@echo off
REM Instala el release de Glory Harness desktop (039A-1 F6) desde el target
REM de compilacion en C:\tmp (nunca dentro del arbol). Uso:
REM   install-release.bat [rama]
REM Por defecto instala lo compilado en la rama actual de este repo.
setlocal EnableExtensions

set "BRAMA=%~1"
if "%BRAMA%"=="" (
  for /f "delims=" %%b in ('git branch --show-current 2^>nul') do set "BRAMA=%%b"
)
if "%BRAMA%"=="" set "BRAMA=main"

set "SRC=C:\tmp\glory-target\glory-harness\release\glory-harness-desktop.exe"
if not exist "%SRC%" (
  echo [install-release] no hay release: %SRC%
  echo Compila primero con tauri build (CARGO_TARGET_DIR=C:\tmp\glory-target\glory-harness).
  exit /b 1
)

set "DEST=%LOCALAPPDATA%\GloryHarness"
if not exist "%DEST%" mkdir "%DEST%" || exit /b 1
copy /y "%SRC%" "%DEST%\glory-harness-desktop.exe" || exit /b 1
"%DEST%\glory-harness-desktop.exe" --version >nul 2>&1
echo [install-release] instalado en %DEST%\glory-harness-desktop.exe
endlocal
