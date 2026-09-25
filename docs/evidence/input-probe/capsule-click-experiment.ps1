# Experiment: is a click inside the capsule's published region classified as wallpaper UI
# or as blank desktop? Determines why the island/capsule clicks fall through to the
# 表/里 desktop toggle.
#
# Fully automated and reversible: synthesizes real left clicks with SendInput at points
# that contain no desktop icons, and watches SHELLDLL_DefView visibility, which is the
# exact state the app flips when it enters/leaves the inner desktop.
#
# Requires: desktop exposed (no maximized window over the probe points).
param(
  [switch]$Run,                 # without -Run this only reports state (dry)
  [int]$CapsuleX = 1280,
  [int]$CapsuleY = 1487,
  [int]$BelowOffset = 20
)

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class E {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string w);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern bool GetForegroundWindow(out IntPtr h);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static uint P(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);return p;}
  public static IntPtr Fg(){var sb=new StringBuilder(256);GetClassNameW(GetForegroundWindowRaw(),sb,256);return IntPtr.Zero;}
  [DllImport("user32.dll", EntryPoint="GetForegroundWindow")] public static extern IntPtr GetForegroundWindowRaw();
}
'@
[void][E]::SetProcessDPIAware()

function DefView {
  $shell = [E]::GetShellWindow()
  if ($shell -eq [IntPtr]::Zero) { return [IntPtr]::Zero }
  return [E]::FindWindowExW($shell, [IntPtr]::Zero, 'SHELLDLL_DefView', $null)
}
function State([string]$tag) {
  $dv = DefView
  $p = New-Object E+POINT; $p.x = $CapsuleX; $p.y = $CapsuleY
  $hit = [E]::WindowFromPoint($p)
  $fg = [E]::GetForegroundWindowRaw()
  '{0,-22} DefView=0x{1:X8} visible={2,-6} | at ({3},{4}) hit=0x{5:X8} {6,-24} pid={7,-6} | fg={8} {9}' -f `
    $tag, $dv.ToInt64(), [E]::IsWindowVisible($dv), $CapsuleX, $CapsuleY, $hit.ToInt64(),
    [E]::C($hit), [E]::P($hit), [E]::C($fg), [E]::P($fg)
}
function StrayLightMods {
  # report whether any hotkey modifier is held, so a synthesized click cannot be misread
  $held = @()
  foreach ($k in 'Control','Alt','Shift','LWin','RWin') {
    if ([System.Windows.Forms.Control]::ModifierKeys -band 0) { }
  }
  return $held
}
function DoubleClickAt([int]$x, [int]$y) {
  [void][E]::SetCursorPos($x, $y)
  Start-Sleep -Milliseconds 60
  for ($i = 0; $i -lt 2; $i++) {
    [E]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)   # LEFTDOWN
    Start-Sleep -Milliseconds 30
    [E]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)   # LEFTUP
    Start-Sleep -Milliseconds 90
  }
  Start-Sleep -Milliseconds 400
}

'=== baseline ==='
State 'baseline'
$dvBase = DefView
$visibleBefore = [E]::IsWindowVisible($dvBase)
if (-not $visibleBefore) {
  '!! SHELLDLL_DefView is hidden: the app is already in the INNER desktop. Run this with the outer desktop active.'
}
$p = New-Object E+POINT; $p.x = $CapsuleX; $p.y = $CapsuleY; [void][E]::GetCursorPos([ref]$p)
$hitAtCapsule = [E]::WindowFromPoint($p)

if (-not $Run) {
  'DRY RUN. Re-run with -Run to synthesize the clicks.'
  exit 0
}

'=== A: double click INSIDE the capsule region (expect: suppressed, DefView stays visible) ==='
DoubleClickAt $CapsuleX $CapsuleY
State 'after capsule dbl'

'=== B: double click just BELOW the capsule (expect: classified blank -> toggle) ==='
$belowY = $CapsuleY + $BelowOffset
DoubleClickAt $CapsuleX $belowY
State "after below dbl (y=$belowY)"

'=== C: restore to the outer desktop if we flipped ==='
if (-not [E]::IsWindowVisible((DefView))) {
  'inner desktop detected; double clicking blank desktop to return'
  DoubleClickAt 600 400
  State 'after restore dbl'
} else {
  'still on the outer desktop; nothing to restore'
}
State 'final'
