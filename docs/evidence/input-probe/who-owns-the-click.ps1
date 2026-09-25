# Evidence probe: which HWND actually owns a click at a given screen point?
# Read-only measurement: enumerates the desktop wallpaper window stack and asks
# WindowFromPoint what a real click would hit. No window is modified.
param(
  [int]$ProbePid = 0,
  [int[]]$ProbeX = @(),
  [int[]]$ProbeY = @()
)

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class W {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumProc cb, IntPtr p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint owner);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr RealChildWindowFromPoint(IntPtr h, POINT p);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string win);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string cls, string win);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x; public int y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  public static string ClassOf(IntPtr h) { var sb = new StringBuilder(256); GetClassNameW(h, sb, 256); return sb.ToString(); }
  public static string TextOf(IntPtr h) { var sb = new StringBuilder(256); GetWindowTextW(h, sb, 256); return sb.ToString(); }
  public static uint OwnerOf(IntPtr h) { uint owner; GetWindowThreadProcessId(h, out owner); return owner; }
}
'@

[void][W]::SetProcessDPIAware()

function Describe([IntPtr]$h) {
  if ($h -eq [IntPtr]::Zero) { return '  NULL' }
  $owner = [W]::OwnerOf($h)
  $cls = [W]::ClassOf($h)
  $txt = [W]::TextOf($h)
  $ex  = [W]::GetWindowLongPtr($h, -20).ToInt64()
  $root = [W]::GetAncestor($h, 2)
  $r = New-Object W+RECT
  [void][W]::GetWindowRect($h, [ref]$r)
  $vis = [W]::IsWindowVisible($h)
  '  0x{0:X8} cls={1,-24} pid={2,-6} ex=0x{3:X8} vis={4,-5} rect=({5},{6})-({7},{8}) root=0x{9:X8} title="{10}"' -f `
    $h.ToInt64(), $cls, $owner, $ex, $vis, $r.left, $r.top, $r.right, $r.bottom, $root.ToInt64(), $txt
}

$progman = [W]::FindWindowW('Progman', $null)
'=== Progman ==='
Describe $progman
$defview = if ($progman -ne [IntPtr]::Zero) { [W]::FindWindowExW($progman, [IntPtr]::Zero, 'SHELLDLL_DefView', $null) } else { [IntPtr]::Zero }
'SHELLDLL_DefView under Progman:'
Describe $defview

'=== children of Progman (Z order, front to back) ==='
if ($progman -ne [IntPtr]::Zero) {
  $cb = [W+EnumProc]{ param($h, $l) Describe $h; return $true }
  [void][W]::EnumChildWindows($progman, $cb, [IntPtr]::Zero)
}

'=== top-level WorkerW windows and their children ==='
$workerList = New-Object System.Collections.ArrayList
$cb2 = [W+EnumProc]{
  param($h, $l)
  if ([W]::ClassOf($h) -eq 'WorkerW') { [void]$workerList.Add($h) }
  return $true
}
[void][W]::EnumWindows($cb2, [IntPtr]::Zero)
'SET_WALLPAPER workerw has a SHELLDLL_DefView child; the others are wallpaper hosts.'
foreach ($w in $workerList) {
  'WorkerW 0x{0:X8} pid={1}' -f $w.ToInt64(), [W]::OwnerOf($w)
  $cb3 = [W+EnumProc]{ param($h, $l) Describe $h; return $true }
  [void][W]::EnumChildWindows($w, $cb3, [IntPtr]::Zero)
}

'=== top-level windows owned by pid {0} ===' -f $ProbePid
$own = New-Object System.Collections.ArrayList
$cb4 = [W+EnumProc]{
  param($h, $l)
  if ([W]::OwnerOf($h) -eq [uint32]$ProbePid) { [void]$own.Add($h) }
  return $true
}
[void][W]::EnumWindows($cb4, [IntPtr]::Zero)
foreach ($h in $own) {
  Describe $h
  '  children:'
  $cb5 = [W+EnumProc]{ param($h2, $l) Describe $h2; return $true }
  [void][W]::EnumChildWindows($h, $cb5, [IntPtr]::Zero)
}

if ($ProbeX.Count -gt 0) {
  '=== WindowFromPoint: what a real click at that physical screen point hits ==='
  for ($i = 0; $i -lt $ProbeX.Count; $i++) {
    $p = New-Object W+POINT
    $p.x = $ProbeX[$i]; $p.y = $ProbeY[$i]
    $hit = [W]::WindowFromPoint($p)
    'point ({0},{1}):' -f $p.x, $p.y
    Describe $hit
    if ($hit -ne [IntPtr]::Zero) {
      $child = [W]::RealChildWindowFromPoint($hit, $p)
      if ($child -ne $hit -and $child -ne [IntPtr]::Zero) {
        '  real child at point:'
        Describe $child
      }
      $parent = [W]::GetParent($hit)
      if ($parent -ne [IntPtr]::Zero) {
        '  parent:'
        Describe $parent
      }
    }
  }
}
