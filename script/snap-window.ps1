# 截取指定进程主窗口（开发验证用）
# 用法：.\script\snap-window.ps1 -ProcessId <pid> -Out <png>
param([int]$ProcessId, [string]$Out)
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class WispWin {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
'@
[WispWin]::SetProcessDPIAware() | Out-Null
Add-Type -AssemblyName System.Windows.Forms
$p = Get-Process -Id $ProcessId
$h = $p.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { throw 'no main window' }
if (-not [WispWin]::IsZoomed($h)) { [WispWin]::ShowWindow($h, 9) | Out-Null }
[WispWin]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 800
$r = New-Object WispWin+RECT
[WispWin]::GetWindowRect($h, [ref]$r) | Out-Null
$wa = [Windows.Forms.Screen]::FromHandle($h).WorkingArea
"maximized=$([WispWin]::IsZoomed($h)) GetWindowRect=($($r.Left),$($r.Top),$($r.Right),$($r.Bottom)) workarea=($($wa.Left),$($wa.Top),$($wa.Right),$($wa.Bottom))"
# Use the DWM extended frame bounds (excludes the invisible resize border / maximize overhang).
$f = New-Object WispWin+RECT
if ([WispWin]::DwmGetWindowAttribute($h, 9, [ref]$f, 16) -eq 0) { "frame=($($f.Left),$($f.Top),$($f.Right),$($f.Bottom))"; $r = $f }
$w = $r.Right - $r.Left; $hgt = $r.Bottom - $r.Top
$bmp = New-Object Drawing.Bitmap $w, $hgt
$g = [Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
$g.Dispose()
$bmp.Save($Out, [Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
"saved $Out ${w}x$hgt"
