# Experiment: does the floating ball pop out, take mouse input, and never steal focus?
#
# Sweeps the cursor downwards through the ball's resting place instead of only touching the
# screen's very bottom edge: on this machine the auto-hide taskbar rises and claims the whole
# bottom band (measured), so "approach" has to mean "approach where the ball appears".
# For each probe y it records who owns the point, whether that point counts as the desktop
# surface, and whether the ball actually came on screen. Reversible: the cursor is restored.
#
# Requires: the desktop exposed at the probe points (a maximised app would make the ball
# refuse by design, and the measurement would be meaningless).
param(
  [int]$X = 0,                 # 0 = centre of the screen
  # 1600 is deliberately absent: y == screen height is outside the screen, WindowFromPoint
  # returns NULL there, which measures nothing.
  [int[]]$Ys = @(1596, 1580, 1550, 1530, 1500, 1470, 1440, 1400),
  [int]$SettleMs = 700,
  [int]$NeutralY = 700
)

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class P {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string w);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("user32.dll")] public static extern IntPtr GetDesktopWindow();
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static string T(IntPtr h){var sb=new StringBuilder(256);GetWindowTextW(h,sb,256);return sb.ToString();}
  public static uint P_(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);return p;}
}
'@
[void][P]::SetProcessDPIAware()

function BallHandle {
  $mine = @(Get-Process -Name 'dsh-wallpaper' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  $script:ball = [IntPtr]::Zero
  $cb = [P+EnumProc]{
    param($h, $l)
    if ($mine -notcontains [int][P]::P_($h)) { return $true }
    if ([P]::T($h) -eq 'DSH Wallpaper Ball') { $script:ball = $h }
    return $true
  }
  [void][P]::EnumWindows($cb, [IntPtr]::Zero)
  return $script:ball
}
function RectOf([IntPtr]$h) { $r = New-Object P+RECT; [void][P]::GetWindowRect($h, [ref]$r); return $r }
function Describe([IntPtr]$h) {
  if ($h -eq [IntPtr]::Zero) { return 'NULL' }
  return ('{0} (pid {1})' -f [P]::C($h), [P]::P_($h))
}
# Mirrors the app's own gate: the chain under the cursor must reach the shell/desktop.
function DesktopVerdict([IntPtr]$h) {
  $desktop = [P]::GetDesktopWindow(); $shell = [P]::GetShellWindow(); $cur = $h
  for ($i = 0; $i -lt 16; $i++) {
    if ($cur -eq [IntPtr]::Zero) { return 'no' }
    if ($cur -eq $desktop -or $cur -eq $shell) { return 'YES' }
    $cls = [P]::C($cur)
    if ($cls -in @('Progman','WorkerW','SHELLDLL_DefView','SysListView32')) { return 'YES' }
    if ($cls -eq 'Shell_TrayWnd') { return 'no(taskbar)' }
    $parent = [P]::GetParent($cur)
    if ($parent -eq [IntPtr]::Zero -or $parent -eq $cur) { return "no($cls)" }
    $cur = $parent
  }
  return 'no(depth)'
}

$ball = BallHandle
if ($ball -eq [IntPtr]::Zero) { 'no window titled "DSH Wallpaper Ball" - is the new build installed?'; exit 1 }
$screenW = [P]::GetSystemMetrics(0); $screenH = [P]::GetSystemMetrics(1)
if ($X -eq 0) { $X = [int]($screenW / 2) }
"ball hwnd = 0x{0:X8}; screen = ${screenW}x${screenH}; probe x = $X" -f $ball.ToInt64()
$r0 = RectOf $ball
"baseline rect = ({0},{1})-({2},{3}); fully off screen = {4}; parent = {5}" -f `
  $r0.left, $r0.top, $r0.right, $r0.bottom, ($r0.top -ge $screenH), `
  $(if ([P]::GetParent($ball) -eq [IntPtr]::Zero) { 'none (top-level)' } else { [P]::C([P]::GetParent($ball)) })

$cursor0 = New-Object P+POINT
[void][P]::GetCursorPos([ref]$cursor0)
$fgBefore = [P]::GetForegroundWindow()
"cursor before = ($($cursor0.x),$($cursor0.y)); foreground before = $(Describe $fgBefore)"
$poppedAt = $null

' y     owner                       desktop?      ball rect                              popped'
try {
  foreach ($y in $Ys) {
    [void][P]::SetCursorPos($X, $y)
    Start-Sleep -Milliseconds $SettleMs
    $pt = New-Object P+POINT; $pt.x = $X; $pt.y = $y
    $owner = [P]::WindowFromPoint($pt)
    $r = RectOf $ball
    $popped = $r.top -lt $screenH
    if ($popped -and $null -eq $poppedAt) { $poppedAt = [pscustomobject]@{ y = $y; rect = $r } }
    '{0,-6} {1,-27} {2,-13} ({3},{4})-({5},{6})   {7}' -f $y, (Describe $owner), (DesktopVerdict $owner), $r.left, $r.top, $r.right, $r.bottom, $popped
    # retract before the next probe so each y is measured from the hidden state
    [void][P]::SetCursorPos($X, $NeutralY)
    Start-Sleep -Milliseconds 900
  }

  if ($null -eq $poppedAt) {
    "NO POP-OUT at any probed y (x=$X). Check the log for the gate verdict."
  } else {
    "POP-OUT first happened at y=$($poppedAt.y), rect=({0},{1})-({2},{3})" -f $poppedAt.rect.left, $poppedAt.rect.top, $poppedAt.rect.right, $poppedAt.rect.bottom
    # re-create that state and assert the input properties
    [void][P]::SetCursorPos($X, $poppedAt.y)
    Start-Sleep -Milliseconds 700
    $final = RectOf $ball
    $cx = [int](($final.left + $final.right) / 2)
    $cy = [int](($final.top + $final.bottom) / 2)
    $pt = New-Object P+POINT; $pt.x = $cx; $pt.y = $cy
    $owner = [P]::WindowFromPoint($pt)
    # The WebView2 renderer is a child window in ANOTHER process, so the ball's own window
    # never owns the point directly: walk the ancestor chain, exactly like the app does.
    $walk = $owner
    $isOurs = $false
    for ($i = 0; $i -lt 16 -and $walk -ne [IntPtr]::Zero; $i++) {
      if ($walk -eq $ball) { $isOurs = $true; break }
      $parent = [P]::GetParent($walk)
      if ($parent -eq [IntPtr]::Zero -or $parent -eq $walk) { break }
      $walk = $parent
    }
    "hit test at ball centre ($cx,$cy) -> $(Describe $owner); reaches the ball window = $isOurs"
    "gap between ball bottom and screen bottom = $($screenH - $final.bottom) px"
    $fgDuring = [P]::GetForegroundWindow()
    "foreground during = $(Describe $fgDuring); unchanged = $($fgDuring -eq $fgBefore)"
  }
} finally {
  [void][P]::SetCursorPos($cursor0.x, $cursor0.y)
  "cursor restored to ($($cursor0.x),$($cursor0.y))"
}
