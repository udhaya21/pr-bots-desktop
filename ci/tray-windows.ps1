# Starts the tray app, waits for the window to close and 60s of idle, then records the memory of the
# app and its WebView2 children and copies the app's phase0 report. Usage: tray-windows.ps1 <out-dir>
param([string]$Out)
$ErrorActionPreference = 'Stop'
$data = Join-Path $env:APPDATA 'dev.prbots.phase0'
$status = Join-Path $data 'phase0-status'

$app = Start-Process -FilePath .\target\release\prbots-phase0.exe -PassThru
foreach ($i in 1..120) {
  if ((Test-Path $status) -and (Get-Content $status) -eq 'done') { break }
  if ($app.HasExited) { throw "tray app exited with $($app.ExitCode)" }
  Start-Sleep 1
}
if (-not ((Test-Path $status) -and (Get-Content $status) -eq 'done')) { throw 'no report after 120s' }
Start-Sleep 60

$all = Get-CimInstance Win32_Process
function Tree($id) { $id; $all | Where-Object ParentProcessId -eq $id | ForEach-Object { Tree $_.ProcessId } }
$rows = Tree $app.Id | ForEach-Object {
  $p = Get-Process -Id $_ -ErrorAction SilentlyContinue
  if ($p) { [pscustomobject]@{ Pid = $p.Id; Name = $p.ProcessName; WorkingSetMB = [math]::Round($p.WorkingSet64 / 1MB, 1); PrivateMB = [math]::Round($p.PrivateMemorySize64 / 1MB, 1) } }
}
$rows | Format-Table | Out-String | Tee-Object -FilePath (Join-Path $Out 'memory.txt')
$total = "total private {0:N1} MB, working set {1:N1} MB" -f ($rows | Measure-Object PrivateMB -Sum).Sum, ($rows | Measure-Object WorkingSetMB -Sum).Sum
$total | Tee-Object -FilePath (Join-Path $Out 'memory.txt') -Append
$total | Out-File -Append $env:GITHUB_STEP_SUMMARY

Copy-Item (Join-Path $data 'phase0-report.json') (Join-Path $Out 'phase0-tray.json')
Stop-Process -Id $app.Id
