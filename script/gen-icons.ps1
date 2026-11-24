# 从 assets/branding/wisp-logo.svg 生成各平台图标（需要本机 Microsoft Edge）
# 输出：assets/branding/png/wisp-<size>.png、crates/zed/resources 下的 app-icon*.png 与 windows/app-icon*.ico
$ErrorActionPreference = 'Continue'
$root = Split-Path $PSScriptRoot -Parent
$svg = Join-Path $root 'assets\branding\wisp-logo.svg'
$outDir = Join-Path $root 'assets\branding\png'
New-Item -ItemType Directory -Force $outDir | Out-Null

$edge = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe", "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") |
  Where-Object { Test-Path $_ } | Select-Object -First 1
$svgUri = ([Uri]$svg).AbsoluteUri
$sizes = 16, 24, 32, 48, 64, 128, 256, 512, 1024

# Edge 无头模式连续多次截图偶发卡死，所以只渲染一次 1024，其余尺寸用 System.Drawing 缩放
$master = Join-Path $outDir 'wisp-1024.png'
Remove-Item $master -ErrorAction SilentlyContinue
$html = Join-Path $env:TEMP 'wisp-icon-1024.html'
"<html><body style='margin:0;background:transparent'><img src='$svgUri' width='1024' height='1024' style='display:block'></body></html>" |
  Set-Content -Encoding UTF8 $html
$edgeArgs = @('--headless=new', '--disable-gpu', '--hide-scrollbars', '--force-device-scale-factor=1',
  '--default-background-color=00000000', "--user-data-dir=$env:TEMP\wisp-edge-profile",
  '--window-size=1024,1024', "--screenshot=$master", ([Uri]$html).AbsoluteUri)
$p = Start-Process $edge -ArgumentList $edgeArgs -PassThru -WindowStyle Hidden
if (-not $p.WaitForExit(60000)) { $p.Kill() }
if (-not (Test-Path $master)) { throw 'render failed' }

Add-Type -AssemblyName System.Drawing
$src = [Drawing.Image]::FromFile($master)
foreach ($s in $sizes | Where-Object { $_ -ne 1024 }) {
  $bmp = New-Object Drawing.Bitmap $s, $s, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [Drawing.Graphics]::FromImage($bmp)
  $g.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::HighQuality
  $g.Clear([Drawing.Color]::Transparent)
  $g.DrawImage($src, 0, 0, $s, $s)
  $g.Dispose()
  $bmp.Save((Join-Path $outDir "wisp-$s.png"), [Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}
$src.Dispose()

# 组装 ICO（PNG 压缩条目，Windows Vista+ 支持）
function Write-Ico([string]$path, [int[]]$icoSizes) {
  $entries = foreach ($s in $icoSizes) { , [IO.File]::ReadAllBytes((Join-Path $outDir "wisp-$s.png")) }
  $ms = New-Object IO.MemoryStream
  $bw = New-Object IO.BinaryWriter $ms
  $bw.Write([UInt16]0); $bw.Write([UInt16]1); $bw.Write([UInt16]$icoSizes.Count)
  $offset = 6 + 16 * $icoSizes.Count
  for ($i = 0; $i -lt $icoSizes.Count; $i++) {
    $s = $icoSizes[$i]; $b = if ($s -ge 256) { 0 } else { $s }
    $bw.Write([byte]$b); $bw.Write([byte]$b); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([UInt16]1); $bw.Write([UInt16]32)
    $bw.Write([UInt32]$entries[$i].Length); $bw.Write([UInt32]$offset)
    $offset += $entries[$i].Length
  }
  foreach ($e in $entries) { $bw.Write($e) }
  [IO.File]::WriteAllBytes($path, $ms.ToArray())
}

$res = Join-Path $root 'crates\zed\resources'
$ico = 16, 24, 32, 48, 64, 128, 256
foreach ($v in '', '-dev', '-nightly', '-preview') {
  Copy-Item (Join-Path $outDir 'wisp-512.png') (Join-Path $res "app-icon$v.png") -Force
  Copy-Item (Join-Path $outDir 'wisp-1024.png') (Join-Path $res "app-icon$v@2x.png") -Force
  Write-Ico (Join-Path $res "windows\app-icon$v.ico") $ico
}
Write-Host 'icons generated'
