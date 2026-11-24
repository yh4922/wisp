# 从 assets/branding/wisp-logo.png（1024×1024 透明底母版）生成各平台图标
# 输出：assets/branding/png/wisp-<size>.png、crates/zed/resources 下的 app-icon*.png 与 windows/app-icon*.ico
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$master = Join-Path $root 'assets\branding\wisp-logo.png'
$outDir = Join-Path $root 'assets\branding\png'
New-Item -ItemType Directory -Force $outDir | Out-Null
$sizes = 16, 24, 32, 48, 64, 128, 256, 512, 1024

Add-Type -AssemblyName System.Drawing

function Resize-Image([Drawing.Image]$img, [int]$s) {
  $bmp = New-Object Drawing.Bitmap $s, $s, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [Drawing.Graphics]::FromImage($bmp)
  $g.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.CompositingQuality = [Drawing.Drawing2D.CompositingQuality]::HighQuality
  $g.Clear([Drawing.Color]::Transparent)
  $g.DrawImage($img, 0, 0, $s, $s)
  $g.Dispose()
  return $bmp
}

$src = [Drawing.Image]::FromFile($master)
foreach ($s in $sizes) {
  # 大比例缩小时逐级减半，避免一次缩放产生锯齿和细节闪烁
  $cur = $src; $owned = $false
  while ($cur.Width -gt $s * 2) {
    $next = Resize-Image $cur ([int]($cur.Width / 2))
    if ($owned) { $cur.Dispose() }
    $cur = $next; $owned = $true
  }
  $out = Resize-Image $cur $s
  if ($owned) { $cur.Dispose() }
  $out.Save((Join-Path $outDir "wisp-$s.png"), [Drawing.Imaging.ImageFormat]::Png)
  $out.Dispose()
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
# 欢迎页 Logo（gpui img 从内嵌资源加载）
Copy-Item (Join-Path $outDir 'wisp-128.png') (Join-Path $root 'assets\images\wisp_logo.png') -Force
Write-Host 'icons generated'
