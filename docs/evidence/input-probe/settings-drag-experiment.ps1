# Does the settings window still move (titlebar drag) and resize (edge drag)?
#
# The non-client area is gone on purpose (see settings-window-frame-and-corners.md), so both
# interactions are now driven from the renderer and must be measured, not assumed. This script
# synthesizes a real drag in each direction and reports the window rectangle before/after.
#
# Reversible: after measuring, it drags the window back by the same deltas.
param(
  [int]$MoveByX = 140,
  [int]$MoveByY = 90,
  [int]$ResizeByX = 160,
  [int]$WaitSeconds = 420,
  [switch]$Run
)

Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;
public class G {
 public delegate bool EnumProc(IntPtr h, IntPtr l);
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint dx,uint dy,uint d,UIntPtr e);
 [StructLayout(LayoutKind.Sequential)] public struct RECT{public int left,top,right,bottom;}
 public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
 public static string T(IntPtr h){var sb=new StringBuilder(256);GetWindowTextW(h,sb,256);return sb.ToString();}
}
'@
[void][G]::SetProcessDPIAware()

$LEFTDOWN = 0x0002
$LEFTUP = 0x0004

function FindSettings {
  $script:win = [IntPtr]::Zero
  $cb = [G+EnumProc]{
    param($h, $l)
    if (-not [G]::IsWindowVisible($h)) { return $true }
    if ([G]::C($h) -eq 'Tauri Window' -and [G]::T($h) -like '*Settings*') { $script:win = $h }
    return $true
  }
  [void][G]::EnumWindows($cb, [IntPtr]::Zero)
  return $script:win
}
function Rect([IntPtr]$h) { $r = New-Object G+RECT; [void][G]::GetWindowRect($h, [ref]$r); return $r }
function Describe([IntPtr]$h) { $r = Rect $h; return "({0},{1})-({2},{3}) {4}x{5}" -f $r.left, $r.top, $r.right, $r.bottom, ($r.right - $r.left), ($r.bottom - $r.top) }

function DragFrom([int]$x, [int]$y, [int]$dx, [int]$dy) {
  [void][G]::SetCursorPos($x, $y)
  Start-Sleep -Milliseconds 150
  [G]::mouse_event($LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 150
  $steps = 10
  for ($i = 1; $i -le $steps; $i++) {
    [void][G]::SetCursorPos(($x + [int]($dx * $i / $steps)), ($y + [int]($dy * $i / $steps)))
    Start-Sleep -Milliseconds 35
  }
  Start-Sleep -Milliseconds 150
  [G]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 400
}

$win = FindSettings
if ($win -eq [IntPtr]::Zero) {
  "waiting up to ${WaitSeconds}s for the settings window (open 设置中心 now)..."
  $deadline = (Get-Date).AddSeconds($WaitSeconds)
  while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 400
    $win = FindSettings
    if ($win -ne [IntPtr]::Zero) { break }
  }
}
if ($win -eq [IntPtr]::Zero) { 'settings window never became visible - open 设置中心 and leave it open'; exit 2 }
[void][G]::SetForegroundWindow($win)
Start-Sleep -Milliseconds 600
$h = 0
$before = Rect $win
$width = $before.right - $before.left
"Heading: settings window 0x{0:X8}" -f $win.ToInt64()
"before      : $(Describe $win)"

$titleX = $before.left + [int]($width / 2)
$titleY = $before.top + 32
"titlebar point = ($titleX,$titleY)"

if (-not $Run) {
  'DRY RUN: pass -Run to synthesize the drags.'
  exit 0
}

'=== 1. move: drag the titlebar by (+{0},+{1}) ===' -f $MoveByX, $MoveByY
DragFrom $titleX $titleY $MoveByX $MoveByY
$afterMove = Rect $win
"after move  : $(Describe $win)"
$movedX = $afterMove.left - $before.left
$movedY = $afterMove.top - $before.top
"measured delta = ({0},{1})  expected ~({2},{3})  -> move {4}" -f $movedX, $movedY, $MoveByX, $MoveByY, $(if ([Math]::Abs($movedX - $MoveByX) -le 10 -and [Math]::Abs($movedY - $MoveByY) -le 10) { 'WORKS' } else { 'BROKEN' })

'=== 2. resize: drag the right edge by (+{0},0) ===' -f $ResizeByX
$midY = $afterMove.top + [int](($afterMove.bottom - $afterMove.top) / 2)
$edgeX = $afterMove.right - 3
"edge point = ($edgeX,$midY)"
DragFrom $edgeX $midY $ResizeByX 0
$afterResize = Rect $win
"after resize: $(Describe $win)"
$grewX = $afterResize.right - $afterMove.right
"measured width delta = {0}  expected ~{1}  -> resize {2}" -f $grewX, $ResizeByX, $(if ([Math]::Abs($grewX - $ResizeByX) -le 12) { 'WORKS' } else { 'BROKEN' })

'=== 3. restore the original placement (drag back and shrink back) ==='
$cur = Rect $win
$centerX = $cur.left + [int](($cur.right - $cur.left) / 2)
$centerY = $cur.top + 32
DragFrom $centerX $centerY ($before.left - $cur.left) ($before.top - $cur.top)
$cur2 = Rect $win
$edgeX2 = $cur2.right - 3
$midY2 = $cur2.top + [int](($cur2.bottom - $cur2.top) / 2)
DragFrom $edgeX2 $midY2 ($before.right - $cur2.right) 0
"restored    : $(Describe $win)"
