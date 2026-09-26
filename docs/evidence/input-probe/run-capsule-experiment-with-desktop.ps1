# Orchestrator: expose the desktop for a few seconds, run the capsule click experiment,
# then restore every window to exactly the placement it had before.
#
# Restoration uses GetWindowPlacement/SetWindowPlacement, which preserves both the
# normal position and whether the window was maximized/minimized.
param(
  [int]$CapsuleX = 1280,
  [int]$CapsuleY = 1487,
  [string]$Payload = 'capsule-click-experiment.ps1',
  [switch]$Run
)

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class W {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string w);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool GetWindowPlacement(IntPtr h, ref WINDOWPLACEMENT p);
  [DllImport("user32.dll")] public static extern bool SetWindowPlacement(IntPtr h, ref WINDOWPLACEMENT p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  [StructLayout(LayoutKind.Sequential)] public struct WINDOWPLACEMENT {
    public int length; public int flags; public int showCmd;
    public POINT minPosition; public POINT maxPosition; public RECT normalPosition; }
  public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static uint P(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);return p;}
}
'@
[void][W]::SetProcessDPIAware()

$SW_MINIMIZE = 6
$MY_PID = $PID

function DefView {
  return [W]::FindWindowExW([W]::GetShellWindow(), [IntPtr]::Zero, 'SHELLDLL_DefView', $null)
}
function OwnerAt([int]$x, [int]$y) {
  $p = New-Object W+POINT; $p.x = $x; $p.y = $y
  $h = [W]::WindowFromPoint($p)
  return [W]::C($h)
}

# Which top-level, visible, foreign windows cover the probe points and must step aside?
$targets = New-Object System.Collections.ArrayList
$cb = [W+EnumProc]{
  param($h, $l)
  if (-not [W]::IsWindowVisible($h)) { return $true }
  $owner = [W]::P($h)
  if ($owner -eq 0 -or $owner -eq [uint32]$MY_PID) { return $true }
  $cls = [W]::C($h)
  if ($cls -in @('Progman','WorkerW','Shell_TrayWnd','Shell_SecondaryTrayWnd','Windows.UI.Core.CoreWindow')) { return $true }
  $own = $owner
  if ($pr = Get-Process -Id $own -ErrorAction SilentlyContinue) {
    if ($pr.ProcessName -like 'dsh-wallpaper*') { return $true }   # the subject under test
  }
  $r = New-Object W+RECT
  if (-not [W]::GetWindowRect($h, [ref]$r)) { return $true }
  $coversCapsule = ($r.left -le $CapsuleX) -and ($r.right -ge $CapsuleX) -and ($r.top -le $CapsuleY) -and ($r.bottom -ge $CapsuleY)
  $coversBlank = ($r.left -le 1250) -and ($r.right -ge 1250) -and ($r.top -le 900) -and ($r.bottom -ge 900)
  if ($coversCapsule -or $coversBlank) { [void]$targets.Add($h) }
  return $true
}
[void][W]::EnumWindows($cb, [IntPtr]::Zero)

"windows to step aside: $($targets.Count)"
$snapshot = @()
foreach ($h in $targets) {
  $wp = New-Object W+WINDOWPLACEMENT
  $wp.length = [System.Runtime.InteropServices.Marshal]::SizeOf($wp)
  [void][W]::GetWindowPlacement($h, [ref]$wp)
  $snapshot += [pscustomobject]@{ hwnd = $h; placement = $wp; cls = [W]::C($h); pid = [W]::P($h); showCmd = $wp.showCmd }
  "  0x{0:X8} {1,-30} pid={2,-6} showCmd={3}" -f $h.ToInt64(), [W]::C($h), [W]::P($h), $wp.showCmd
}

try {
  $cursor0 = New-Object W+POINT
  [void][W]::GetCursorPos([ref]$cursor0)
  foreach ($item in $snapshot) { [void][W]::ShowWindow($item.hwnd, $SW_MINIMIZE) }
  Start-Sleep -Milliseconds 900

  "after minimizing, owner at capsule point = $(OwnerAt $CapsuleX $CapsuleY)"
  "after minimizing, owner at blank point (1250,900) = $(OwnerAt 1250 900)"

  $exp = Join-Path $PSScriptRoot $Payload
  if (-not (Test-Path $exp)) { throw "payload not found: $exp" }
  $args1 = @('-NoProfile','-File', $exp)
  if ($Payload -eq 'capsule-click-experiment.ps1') {
    $args1 += @('-CapsuleX', $CapsuleX, '-CapsuleY', $CapsuleY)
    if ($Run) { $args1 += '-Run' }
  }
  & pwsh @args1
}
finally {
  '=== restoring windows ==='
  Start-Sleep -Milliseconds 400
  foreach ($item in $snapshot) {
    $wp = $item.placement
    [void][W]::SetWindowPlacement($item.hwnd, [ref]$wp)
    "  restored 0x{0:X8} {1}" -f $item.hwnd.ToInt64(), $item.cls
  }
  Start-Sleep -Milliseconds 600
  [void][W]::SetCursorPos($cursor0.x, $cursor0.y)
  "cursor restored to ($($cursor0.x),$($cursor0.y))"
  "final owner at capsule point = $(OwnerAt $CapsuleX $CapsuleY)"
}
