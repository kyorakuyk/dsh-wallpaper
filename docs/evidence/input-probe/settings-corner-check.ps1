# Verification for the settings window's rounded corners.
#
# Waits for the settings window to appear, then records evidence instead of opinions:
#   - the DWM corner preference actually stored on the window,
#   - its style bits (DWM only rounds windows that still carry a frame),
#   - a screenshot of the window plus zoomed crops of its top-left/top-right corners,
#     so the curve can be inspected by eye,
#   - the pixel colours along the top-left diagonal: a rounded corner shows the desktop
#     for the first few pixels, a square one shows the panel colour immediately.
param(
  [int]$WaitSeconds = 300,
  [string]$OutDir = 'D:\Family\DeepSeekHarness\plugins\dsh-wallpaper\artifacts\settings-corner',
  [string]$TitleMatch = 'Settings'
)

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class C {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out int value, int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  public static string Cls(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static string Txt(IntPtr h){var sb=new StringBuilder(256);GetWindowTextW(h,sb,256);return sb.ToString();}
}
'@
[void][C]::SetProcessDPIAware()

$DWMWA_WINDOW_CORNER_PREFERENCE = 33

function FindSettings {
  $script:found = [IntPtr]::Zero
  $cb = [C+EnumProc]{
    param($h, $l)
    if (-not [C]::IsWindowVisible($h)) { return $true }
    if ([C]::Cls($h) -ne 'Tauri Window') { return $true }
    if ([C]::Txt($h) -like "*$TitleMatch*") { $script:found = $h }
    return $true
  }
  [void][C]::EnumWindows($cb, [IntPtr]::Zero)
  return $script:found
}

'waiting for the settings window (open 设置中心 now)...'
$deadline = (Get-Date).AddSeconds($WaitSeconds)
$h = [IntPtr]::Zero
while ((Get-Date) -lt $deadline) {
  $h = FindSettings
  if ($h -ne [IntPtr]::Zero) { break }
  Start-Sleep -Milliseconds 400
}
if ($h -eq [IntPtr]::Zero) { "settings window did not appear within $WaitSeconds s"; exit 2 }
"found settings window 0x{0:X8} title='{1}'" -f $h.ToInt64(), [C]::Txt($h)

[void][C]::SetForegroundWindow($h)
Start-Sleep -Milliseconds 800

$r = New-Object C+RECT
[void][C]::GetWindowRect($h, [ref]$r)
$style = [C]::GetWindowLongPtr($h, -16).ToInt64()
$ex = [C]::GetWindowLongPtr($h, -20).ToInt64()
$corner = -1
$hr = [C]::DwmGetWindowAttribute($h, $DWMWA_WINDOW_CORNER_PREFERENCE, [ref]$corner, 4)
"rect = ({0},{1})-({2},{3})  size={4}x{5}" -f $r.left, $r.top, $r.right, $r.bottom, ($r.right - $r.left), ($r.bottom - $r.top)
"style=0x{0:X8}  WS_THICKFRAME={1}  WS_CAPTION={2}  exstyle=0x{3:X8}" -f `
  $style, (($style -band 0x00040000) -ne 0), (($style -band 0x00C00000) -ne 0), $ex
if ($hr -eq 0) {
  $name = switch ($corner) { 0 { 'DEFAULT' } 1 { 'DONOTROUND' } 2 { 'ROUND' } 3 { 'ROUNDSMALL' } default { '?' } }
  "DWM corner preference = $corner ($name)"
} else { "DwmGetWindowAttribute failed hr=0x$('{0:X}' -f $hr)" }

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$w = $r.right - $r.left
$ht = $r.bottom - $r.top
$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.left, $r.top, 0, 0, (New-Object System.Drawing.Size($w, $ht)))
$g.Dispose()
$full = Join-Path $OutDir 'settings-window.png'
$bmp.Save($full, [System.Drawing.Imaging.ImageFormat]::Png)
"saved $full"

# Zoomed corner crops: the top-left and top-right 80x80 regions, scaled 4x nearest-neighbour
# so a curve is unmistakable.
function Crop-Zoom([int]$x, [int]$y, [int]$size, [string]$path) {
  $crop = New-Object System.Drawing.Bitmap($size, $size)
  $cg = [System.Drawing.Graphics]::FromImage($crop)
  $cg.DrawImage($bmp, (New-Object System.Drawing.Rectangle(0, 0, $size, $size)),
                (New-Object System.Drawing.Rectangle($x, $y, $size, $size)),
                [System.Drawing.GraphicsUnit]::Pixel)
  $cg.Dispose()
  $zoom = New-Object System.Drawing.Bitmap(($size * 4), ($size * 4))
  $zg = [System.Drawing.Graphics]::FromImage($zoom)
  $zg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
  $zg.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
  $zg.DrawImage($crop, 0, 0, ($size * 4), ($size * 4))
  $zg.Dispose()
  $zoom.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $crop.Dispose(); $zoom.Dispose()
  "saved $path (zoom 4x of ${size}x${size} at $x,$y)"
}
Crop-Zoom 0 0 80 (Join-Path $OutDir 'corner-top-left.png')
Crop-Zoom ($w - 80) 0 80 (Join-Path $OutDir 'corner-top-right.png')
Crop-Zoom 0 ($ht - 80) 80 (Join-Path $OutDir 'corner-bottom-left.png')

# A capture with the surroundings included: comparing the pixel just outside the window with the
# pixel just inside tells apart "the window paints a frame there" from "we are seeing the backdrop".
$pad = 26
$pw = $w + 2 * $pad
$ph = $ht + 2 * $pad
$padded = New-Object System.Drawing.Bitmap($pw, $ph)
$pg = [System.Drawing.Graphics]::FromImage($padded)
$pg.CopyFromScreen(($r.left - $pad), ($r.top - $pad), 0, 0, (New-Object System.Drawing.Size($pw, $ph)))
$pg.Dispose()
$paddedPath = Join-Path $OutDir 'settings-with-surroundings.png'
$padded.Save($paddedPath, [System.Drawing.Imaging.ImageFormat]::Png)
"saved $paddedPath (window left edge is at x=$pad)"
$midY = [int]($ph / 2)
$row = @()
foreach ($x in @(2, 10, 18, 22, ($pad - 2), ($pad - 1), $pad, ($pad + 1), ($pad + 2), ($pad + 4), ($pad + 8), ($pad + 12), ($pad + 20), ($pad + 40), ($pad + 90))) {
  $c = $padded.GetPixel($x, $midY)
  $row += ('{0}:#{1:X2}{2:X2}{3:X2}' -f $x, $c.R, $c.G, $c.B)
}
'mid-row outside -> inside: ' + ($row -join '  ')
$midX = [int]($pw / 2)
$col = @()
foreach ($y in @(2, 10, 18, ($pad - 2), $pad, ($pad + 1), ($pad + 2), ($pad + 4), ($pad + 8), ($pad + 12), ($pad + 20))) {
  $c = $padded.GetPixel($midX, $y)
  $col += ('{0}:#{1:X2}{2:X2}{3:X2}' -f $y, $c.R, $c.G, $c.B)
}
'mid-col outside -> inside: ' + ($col -join '  ')
$padded.Dispose()

'--- diagonal pixel colours from the window top-left corner (0 = window pixel) ---'
$samples = @()
foreach ($d in 0, 1, 2, 3, 4, 6, 8, 10, 14, 20) {
  $px = $bmp.GetPixel([Math]::Min($d, $w - 1), [Math]::Min($d, $ht - 1))
  $samples += ('d{0}=#{1:X2}{2:X2}{3:X2}' -f $d, $px.R, $px.G, $px.B)
}
($samples -join '  ')
'--- same for the top edge (y=0) walking right ---'
$samples = @()
foreach ($x in 0, 1, 2, 3, 4, 6, 8, 10, 14, 20) {
  $px = $bmp.GetPixel([Math]::Min($x, $w - 1), 0)
  $samples += ('x{0}=#{1:X2}{2:X2}{3:X2}' -f $x, $px.R, $px.G, $px.B)
}
($samples -join '  ')
'--- reference: a pixel well inside the panel ---'
$inside = $bmp.GetPixel([int]($w / 2), [int]($ht / 2))
'centre=#{0:X2}{1:X2}{2:X2}' -f $inside.R, $inside.G, $inside.B
$bmp.Dispose()
'--- verdict hint ---'
'If the first diagonal pixels differ from the centre colour, the corner is rounded (the desktop shows through).'
'If they equal the centre colour immediately, the corner is still square.'
