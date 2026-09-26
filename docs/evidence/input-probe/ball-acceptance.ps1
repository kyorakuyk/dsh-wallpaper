# Acceptance probe for the floating ball window (increment 1).
# Read-only. Measures what the contract promises, instead of trusting that it "looks right":
#   1. the ball is a TOP-LEVEL window (no parent) - that is what makes the 表/里 toggle
#      monitor ignore clicks on it for free;
#   2. its ex-style carries TOOLWINDOW|NOACTIVATE and NOT TOPMOST / NOT TRANSPARENT;
#   3. its Z slot is above Progman (icons) and below every normal app window;
#   4. while hidden it is fully off-screen, so it cannot block any desktop point;
#   5. the cursor's bottom hot band makes it slide in, and a point inside its rect is
#      owned by the ball (so it gets real mouse input).
param(
  [string]$BallLabelClass = 'Tauri Window',
  [int]$HotBand = 6,
  [switch]$Watch
)

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class B {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static string T(IntPtr h){var sb=new StringBuilder(256);GetWindowTextW(h,sb,256);return sb.ToString();}
  public static uint P(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);return p;}
}
'@
[void][B]::SetProcessDPIAware()

function Flags([int64]$ex) {
  $names = @()
  if (($ex -band 0x8) -ne 0) { $names += 'TOPMOST' }
  if (($ex -band 0x20) -ne 0) { $names += 'TRANSPARENT' }
  if (($ex -band 0x80) -ne 0) { $names += 'TOOLWINDOW' }
  if (($ex -band 0x80000) -ne 0) { $names += 'LAYERED' }
  if (($ex -band 0x40000) -ne 0) { $names += 'APPWINDOW' }
  if (($ex -band 0x8000000) -ne 0) { $names += 'NOACTIVATE' }
  if ($names.Count -eq 0) { return '(none)' }
  return ($names -join '|')
}
function Rect([IntPtr]$h) {
  $r = New-Object B+RECT
  if (-not [B]::GetWindowRect($h, [ref]$r)) { return $null }
  return $r
}

# The ball is the visible top-level window of our process that is not the wallpaper host,
# not the settings window, and not a tray/event helper.
$ourPids = (Get-Process -Name 'dsh-wallpaper' -ErrorAction SilentlyContinue).Id
$candidates = New-Object System.Collections.ArrayList
$cb = [B+EnumProc]{
  param($h, $l)
  if ($ourPids -notcontains [int][B]::P($h)) { return $true }
  $title = [B]::T($h)
  $cls = [B]::C($h)
  # tray_icon_app / single-instance helpers are 22x22 or hidden
  if ($cls -eq 'tray_icon_app' -or $cls -eq 'com.dsh.wallpaper-sic' -or $cls -eq 'Tao Thread Event Target') { return $true }
  $r = Rect $h
  if ($null -eq $r) { return $true }
  [void]$candidates.Add([pscustomobject]@{ hwnd = $h; cls = $cls; title = $title; r = $r })
  return $true
}
[void][B]::EnumWindows($cb, [IntPtr]::Zero)

'=== candidate top-level windows owned by dsh-wallpaper ==='
foreach ($c in $candidates) {
  $ex = [B]::GetWindowLongPtr($c.hwnd, -20).ToInt64()
  $st = [B]::GetWindowLongPtr($c.hwnd, -16).ToInt64()
  $parent = [B]::GetParent($c.hwnd)
  $name = $c.cls; if ($c.title) { $name = "$($c.cls) ""$($c.title)""" }
  '  0x{0:X8} {1}' -f $c.hwnd.ToInt64(), $name
  '      size={0}x{1} at ({2},{3}) visible={4} parent={5}' -f `
    ($c.r.right - $c.r.left), ($c.r.bottom - $c.r.top), $c.r.left, $c.r.top, [B]::IsWindowVisible($c.hwnd), `
    $(if ($parent -eq [IntPtr]::Zero) { 'none (TOP-LEVEL - good)' } else { [B]::C($parent) })
  '      exstyle=0x{0:X8} -> {1}' -f $ex, (Flags $ex)
  '      style  =0x{0:X8} -> WS_CHILD={1} WS_POPUP={2} WS_VISIBLE={3}' -f $st, (($st -band 0x40000000) -ne 0), (($st -band 0x80000000) -ne 0), (($st -band 0x10000000) -ne 0)
  if ($c.title -eq 'DSH Wallpaper Ball' -or ($c.r.right - $c.r.left) -lt 400) {
    '      ^^ likely the ball'
  }
}

'=== Z slot relative to Progman (walk GW_HWNDPREV from the ball) ==='
$ball = $candidates | Where-Object { $_.title -eq 'DSH Wallpaper Ball' } | Select-Object -First 1
if ($null -eq $ball) {
  'could not identify the ball by title "DSH Wallpaper Ball"; set -BallLabelClass or check the builder title'
} else {
  $h = $ball.hwnd
  $i = 0
  $cur = $h
  while ($cur -ne [IntPtr]::Zero -and $i -lt 12) {
    $r = Rect $cur
    '  {0,-8} 0x{1:X8} {2,-26} pid={3,-6} visible={4,-6} ({5},{6})-({7},{8})' -f `
      $(if ($i -eq 0) { 'BALL' } else { 'above' }), $cur.ToInt64(), [B]::C($cur), [B]::P($cur), [B]::IsWindowVisible($cur), `
      $r.left, $r.top, $r.right, $r.bottom
    $cur = [B]::GetWindow($cur, 3)   # GW_HWNDPREV = the window above this one
    $i++
  }
  $shell = [B]::GetShellWindow()
  $r = Rect $shell
  '  Progman (shell) 0x{0:X8} ({1},{2})-({3},{4})' -f $shell.ToInt64(), $r.left, $r.top, $r.right, $r.bottom
}

'=== off-screen / hot band behaviour ==='
Add-Type -AssemblyName System.Windows.Forms -ErrorAction SilentlyContinue
$vc = Get-CimInstance Win32_VideoController | Select-Object -First 1
'primary screen: {0}x{1}' -f [int]$vc.CurrentHorizontalResolution, [int]$vc.CurrentVerticalResolution
if ($null -ne $ball) {
  $r = Rect $ball.hwnd
  $bottom = [int]$vc.CurrentVerticalResolution
  $off = $r.top -ge $bottom
  "ball rect y={0}..{1} vs screen bottom {2} -> hidden-off-screen={3}" -f $r.top, $r.bottom, $bottom, $off
}
if ($Watch) {
  '=== watching cursor vs ball for 20s (move the cursor to the bottom edge) ==='
  for ($i = 0; $i -lt 40; $i++) {
    $p = New-Object B+POINT
    [void][B]::GetCursorPos([ref]$p)
    $hit = [B]::WindowFromPoint($p)
    $r = Rect $ball.hwnd
    '{0:HH:mm:ss.f} cursor=({1},{2}) hit={3} owner={4} ball=({5},{6})-({7},{8})' -f `
      (Get-Date), $p.x, $p.y, [B]::C($hit), [B]::P($hit), $r.left, $r.top, $r.right, $r.bottom
    Start-Sleep -Milliseconds 500
  }
}
