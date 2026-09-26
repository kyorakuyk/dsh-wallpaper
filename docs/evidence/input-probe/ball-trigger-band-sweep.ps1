# Why does the ball refuse to pop? Sweep the cursor down the bottom edge and report,
# for each y, which window owns that point and whether its parent chain reaches the desktop.
# This reproduces the app's own gate (`cursor_on_desktop_surface_via_label`) from outside.
# Read-only apart from moving the cursor; the cursor is restored.
param(
  [int]$X = 1280,
  [int[]]$Ys = @(1560, 1580, 1588, 1592, 1594, 1596, 1598, 1599, 1600),
  [int]$SettleMs = 300
)

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class S {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("user32.dll")] public static extern IntPtr GetDesktopWindow();
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static string T(IntPtr h){var sb=new StringBuilder(256);GetWindowTextW(h,sb,256);return sb.ToString();}
  public static uint P_(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);return p;}
}
'@
[void][S]::SetProcessDPIAware()

# Class names that make a point belong to "the desktop surface", mirroring the app's rule
# (it compares against the wallpaper host, the desktop window, and desktop shell classes).
function ReachesDesktop([IntPtr]$h) {
  $desktop = [S]::GetDesktopWindow()
  $shell = [S]::GetShellWindow()
  $current = $h
  for ($i = 0; $i -lt 16; $i++) {
    if ($current -eq [IntPtr]::Zero) { return 'no' }
    $cls = [S]::C($current)
    if ($current -eq $desktop) { return "yes($cls)" }
    if ($current -eq $shell) { return "yes(Progman shell)" }
    if ($cls -in @('Progman','WorkerW','SHELLDLL_DefView','SysListView32')) { return "yes($cls)" }
    if ($cls -eq 'Shell_TrayWnd') { return 'no(Shell_TrayWnd parent chain)' }
    $parent = [S]::GetParent($current)
    if ($parent -eq [IntPtr]::Zero -or $parent -eq $current) { return "no(top-level $cls)" }
    $current = $parent
  }
  return 'no(depth)'
}
function BallRect {
  $mine = @(Get-Process -Name 'dsh-wallpaper' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  $script:ball = [IntPtr]::Zero
  $cb = [S+EnumProc]{
    param($h, $l)
    if ($mine -notcontains [int][S]::P_($h)) { return $true }
    if ([S]::T($h) -eq 'DSH Wallpaper Ball') { $script:ball = $h }
    return $true
  }
  [void][S]::EnumWindows($cb, [IntPtr]::Zero)
  if ($script:ball -eq [IntPtr]::Zero) { return $null }
  $r = New-Object S+RECT
  [void][S]::GetWindowRect($script:ball, [ref]$r)
  return [pscustomobject]@{ hwnd = $script:ball; r = $r }
}

$tray = @(Get-Process -Name explorer -ErrorAction SilentlyContinue)
$cursor0 = New-Object S+POINT
[void][S]::GetCursorPos([ref]$cursor0)
"cursor before = ($($cursor0.x),$($cursor0.y))"
' y     owner class              pid     desktop-chain            ball rect / popped'
try {
  foreach ($y in $Ys) {
    [void][S]::SetCursorPos($X, $y)
    Start-Sleep -Milliseconds $SettleMs
    $p = New-Object S+POINT; $p.x = $X; $p.y = $y
    $owner = [S]::WindowFromPoint($p)
    $b = BallRect
    $popped = $false
    $rectText = 'no ball window'
    if ($null -ne $b) {
      $popped = $b.r.top -lt 1600
      $rectText = "({0},{1})-({2},{3}) popped={4}" -f $b.r.left, $b.r.top, $b.r.right, $b.r.bottom, $popped
    }
    '{0,-6} {1,-24} {2,-7} {3,-24} {4}' -f $y, [S]::C($owner), [S]::P_($owner), (ReachesDesktop $owner), $rectText
  }
} finally {
  [void][S]::SetCursorPos($cursor0.x, $cursor0.y)
  "cursor restored to ($($cursor0.x),$($cursor0.y))"
}
'--- taskbar geometry (auto-hide keeps it mostly below the screen) ---'
$cb2 = [S+EnumProc]{
  param($h, $l)
  $cls = [S]::C($h)
  if ($cls -eq 'Shell_TrayWnd') {
    $r = New-Object S+RECT
    [void][S]::GetWindowRect($h, [ref]$r)
    '  Shell_TrayWnd visible={0} rect=({1},{2})-({3},{4})' -f [S]::IsWindowVisible($h), $r.left, $r.top, $r.right, $r.bottom
  }
  return $true
}
[void][S]::EnumWindows($cb2, [IntPtr]::Zero)
