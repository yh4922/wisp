# 启动 wisp.exe（可带参数），等待窗口出现后截图，然后结束该进程（开发验证用）
param([string]$Out, [string[]]$AppArgs = @(), [int]$Wait = 25, [switch]$Keep)
$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root 'target\debug\wisp.exe'
$psi = New-Object Diagnostics.ProcessStartInfo $exe
$psi.Arguments = ($AppArgs | ForEach-Object { '"' + $_ + '"' }) -join ' '
$psi.UseShellExecute = $true
$p = [Diagnostics.Process]::Start($psi)
"started pid $($p.Id)"
$deadline = (Get-Date).AddSeconds($Wait)
$win = $null
while ((Get-Date) -lt $deadline) {
  Start-Sleep 2
  $win = Get-Process wisp -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
  if ($win -and $win.MainWindowTitle) { break }
}
if (-not $win) { 'no window'; exit 1 }
Start-Sleep 5
$win.Refresh()
"window pid $($win.Id) title: $($win.MainWindowTitle)"
& (Join-Path $PSScriptRoot 'snap-window.ps1') -ProcessId $win.Id -Out $Out
if (-not $Keep) { Get-Process wisp -ErrorAction SilentlyContinue | Stop-Process -Force }
