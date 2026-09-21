# Atajo rapido: (re)lanza el desktop ya compilado en segundos, sin compilar.
# Uso: .\scripts\desktop-correr.ps1
$ErrorActionPreference = 'Stop'
$exe = 'C:\tmp\glory-target\glory-harness\debug\glory-harness-desktop.exe'
if (-not (Test-Path -LiteralPath $exe)) {
  Write-Error "No hay binario. Compila primero con .\scripts\desktop-compilar.ps1"
}
Get-CimInstance Win32_Process -Filter "Name='glory-harness-desktop.exe'" -ErrorAction SilentlyContinue |
  ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
$pidLanzado = (Invoke-CimMethod Win32_Process -MethodName Create -Arguments @{ CommandLine = "`"$exe`"" }).ProcessId
"Desktop lanzado (PID $pidLanzado). Reiniciar = volver a correr este script."
