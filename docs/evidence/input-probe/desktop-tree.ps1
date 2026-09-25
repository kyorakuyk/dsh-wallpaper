# Evidence probe 2: recursive walk of the desktop shell tree + all WebView/Tauri windows.
# Read-only. Answers: which HWND paints the wallpaper, where it sits relative to the
# icon layer, and which HWND a click at a given point would hit.
param(
  [int[]]$ProbePid = @(),
  [int[]]$ProbeX = @(),
  [int[]]$ProbeY = @()
)

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class D {
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
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x; public int y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  public static string ClassOf(IntPtr h) { var sb = new StringBuilder(256); GetClassNameW(h, sb, 256); return sb.ToString(); }
  public static string TextOf(IntPtr h) { var sb = new StringBuilder(256); GetWindowTextW(h, sb, 256); return sb.ToString(); }
  public static uint OwnerOf(IntPtr h) { uint owner; GetWindowThreadProcessId(h, out owner); return owner; }
}
'@
[void][D]::SetProcessDPIAware()

function Line([IntPtr]$h, [string]$indent) {
  if ($h -eq [IntPtr]::Zero) { return "${indent}NULL" }
  $r = New-Object D+RECT
  [void][D]::GetWindowRect($h, [ref]$r)
  $txt = [D]::TextOf($h)
  if ($txt.Length -gt 28) { $txt = $txt.Substring(0, 28) }
  '{0}0x{1:X8} {2,-26} pid={3,-6} ex=0x{4:X8} style=0x{5:X8} vis={6,-5} ({7},{8})-({9},{10}) "{11}"' -f `
    $indent, $h.ToInt64(), [D]::ClassOf($h), [D]::OwnerOf($h),
    [D]::GetWindowLongPtr($h, -20).ToInt64(), [D]::GetWindowLongPtr($h, -16).ToInt64(),
    [D]::IsWindowVisible($h), $r.left, $r.top, $r.right, $r.bottom, $txt
}

$shell = [D]::GetShellWindow()
'=== shell tree (Progman) root=0x{0:X8} ===' -f $shell.ToInt64()
Line $shell ''
$all = New-Object System.Collections.ArrayList
$keep = [D+EnumProc]{ param($h, $l) [void]$all.Add($h); return $true }
[void][D]::EnumChildWindows($shell, $keep, [IntPtr]::Zero)
foreach ($h in $all) { Line $h '  ' }

'=== top-level windows (all) matching probe pids / tauri / webview classes ==='
$tops = New-Object System.Collections.ArrayList
$grab = [D+EnumProc]{ param($h, $l) [void]$tops.Add($h); return $true }
[void][D]::EnumWindows($grab, [IntPtr]::Zero)
foreach ($h in $tops) {
  $cls = [D]::ClassOf($h)
  $owner = [D]::OwnerOf($h)
  $interesting = ($ProbePid -contains [int]$owner) -or
                 $cls -like 'TAURI*' -or $cls -like 'Chrome_*' -or $cls -eq 'WorkerW' -or
                 $cls -eq 'Progman' -or $cls -eq 'Shell_TrayWnd' -or $cls -eq 'Tauri Window'
  if (-not $interesting) { continue }
  Line $h ''
  $kids = New-Object System.Collections.ArrayList
  $sub = [D+EnumProc]{ param($h2, $l) [void]$kids.Add($h2); return $true }
  [void][D]::EnumChildWindows($h, $sub, [IntPtr]::Zero)
  foreach ($k in $kids) {
    Line $k '  '
    $grand = New-Object System.Collections.ArrayList
    $sub2 = [D+EnumProc]{ param($h3, $l) [void]$grand.Add($h3); return $true }
    [void][D]::EnumChildWindows($k, $sub2, [IntPtr]::Zero)
    foreach ($g in $grand) { Line $g '    ' }
  }
}

'=== WindowFromPoint (physical pixels) ==='
for ($i = 0; $i -lt $ProbeX.Count; $i++) {
  $p = New-Object D+POINT
  $p.x = $ProbeX[$i]; $p.y = $ProbeY[$i]
  $hit = [D]::WindowFromPoint($p)
  'point ({0},{1}) ->' -f $p.x, $p.y
  Line $hit '  '
  if ($hit -ne [IntPtr]::Zero) {
    $ch = [D]::RealChildWindowFromPoint($hit, $p)
    if ($ch -ne $hit -and $ch -ne [IntPtr]::Zero) { Line $ch '    child: ' }
    $pa = [D]::GetParent($hit)
    if ($pa -ne [IntPtr]::Zero) { Line $pa '    parent:' }
  }
}
