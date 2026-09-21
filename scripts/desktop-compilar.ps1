# Compila UI + binario debug y lanza. Con target caliente son 1-2 min;
# en frio ~6-10 min. La UI se compila por ruta real (el junction rompe a vite)
# y se anula beforeBuildCommand solo durante el build (se restaura al final).
# Uso: .\scripts\desktop-compilar.ps1 [-TaskId 179A-2]
param([string]$TaskId = '179A-2')
$ErrorActionPreference = 'Stop'
$raiz = Split-Path -Parent $PSScriptRoot
$ui = Join-Path $raiz 'desktop\ui'
$tauriConf = Join-Path $raiz 'desktop\src-tauri\tauri.conf.json'
$tauriBin = Join-Path $ui 'node_modules\.bin\tauri.cmd'
$exe = 'C:\tmp\glory-target\glory-harness\debug\glory-harness-desktop.exe'

$lease = (sentinel lease issue --project-root $raiz --task-id $TaskId --command 'cargo' --ttl-ms 7200000 --json | Out-String | ConvertFrom-Json).path
$env:CARGO_TARGET_DIR = 'C:\tmp\glory-target\glory-harness'
$env:GLORY_QUALITY_GATE_LEASE = $lease
$env:CARGO_BUILD_JOBS = '4'
$env:PATH = "C:\Users\Owner\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin;$env:PATH"

if (-not (Test-Path -LiteralPath (Join-Path $ui 'node_modules'))) { npm --prefix $ui install --no-audit --no-fund }
npm --prefix $ui run build
if ($LASTEXITCODE -ne 0) { throw 'Fallo el build de la UI (tsc/vite)' }

$confOriginal = Get-Content -LiteralPath $tauriConf -Raw
Push-Location (Join-Path $raiz 'desktop')
try {
  (Get-Content -LiteralPath $tauriConf -Raw) -replace '"beforeBuildCommand"\s*:\s*"[^"]*"', '"beforeBuildCommand" : ""' |
    Set-Content -LiteralPath $tauriConf -NoNewline
  & $tauriBin build --debug --no-bundle
  if ($LASTEXITCODE -ne 0) { throw 'Fallo tauri build (mira la salida de arriba)' }
} finally {
  Set-Content -LiteralPath $tauriConf -Value $confOriginal -NoNewline
  Pop-Location
}
& (Join-Path $PSScriptRoot 'desktop-correr.ps1')
"Listo: $exe"
