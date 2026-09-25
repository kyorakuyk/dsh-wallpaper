# Experiment: is a click inside the capsule's published region classified as wallpaper UI
# or as blank desktop? Determines why island/capsule clicks fall through to the 表/里 toggle.
#
# Fully automated and reversible: synthesizes real left clicks at points chosen to contain
# no desktop icons (verified with the same UI Automation channel the app uses), and watches
# SHELLDLL_DefView visibility - the exact state the app flips when it enters/leaves the
# inner desktop.
#
# Requires: desktop exposed (no maximized window over the probe points).
param(
  [switch]$Run,                 # without -Run this only reports state (dry)
  [int]$CapsuleX = 1280,
  [int]$CapsuleY = 1487,
  [int]$BelowOffset = 58        # capsule rect measured (1138,1454)-(1423,1521): +58 clears it
)

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class E {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string w);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static uint P(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);return p;}
}
'@
[void][E]::SetProcessDPIAware()

$LEFTDOWN = 0x0002
$LEFTUP   = 0x0004

function DefView {
  $shell = [E]::GetShellWindow()
  if ($shell -eq [IntPtr]::Zero) { return [IntPtr]::Zero }
  return [E]::FindWindowExW($shell, [IntPtr]::Zero, 'SHELLDLL_DefView', $null)
}

function State([string]$tag) {
  $dv = DefView
  $p = New-Object E+POINT; $p.x = $CapsuleX; $p.y = $CapsuleY
  $hit = [E]::WindowFromPoint($p)
  $fg = [E]::GetForegroundWindow()
  $inner = -not [E]::IsWindowVisible($dv)
  $desk = 'OUTER'
  if ($inner) { $desk = 'INNER' }
  # Write-Host, not the pipeline: a function that both prints and returns would hand
  # `if (State ...)` an array, which is truthy even when the desktop is OUTER.
  Write-Host ('{0,-20} DefView=0x{1:X8} visible={2,-6} desktop={3,-6} | at ({4},{5}) hit={6,-24} pid={7,-6} | fg={8} pid={9}' -f `
    $tag, $dv.ToInt64(), [E]::IsWindowVisible($dv), $desk, $CapsuleX, $CapsuleY,
    [E]::C($hit), [E]::P($hit), [E]::C($fg), [E]::P($fg))
  return $inner
}

# Same criterion the app's monitor uses: walk 4 parent levels looking for a desktop icon
# (ListItem). No ListItem anywhere in that chain means the point is blank desktop.
function Test-Blank([int]$x, [int]$y) {
  $pt = New-Object System.Windows.Point([double]$x, [double]$y)
  $el = [System.Windows.Automation.AutomationElement]::FromPoint($pt)
  if ($null -eq $el) { return $false }
  $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
  $cur = $el
  for ($i = 0; $i -lt 4 -and $null -ne $cur; $i++) {
    if ($cur.Current.ControlType -eq [System.Windows.Automation.ControlType]::ListItem) { return $false }
    $cur = $walker.GetParent($cur)
  }
  return $true
}

function DoubleClickAt([int]$x, [int]$y) {
  [void][E]::SetCursorPos($x, $y)
  Start-Sleep -Milliseconds 60
  for ($i = 0; $i -lt 2; $i++) {
    [E]::mouse_event($LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [E]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 70
  }
  Start-Sleep -Milliseconds 450
}

'=== baseline ==='
if (State 'baseline') {
  '!! already in the INNER desktop; aborting so the state is not confused.'
  exit 2
}
$p = New-Object E+POINT; $p.x = $CapsuleX; $p.y = $CapsuleY
$hitAtCapsule = [E]::WindowFromPoint($p)
$capsuleClass = [E]::C($hitAtCapsule)
"point over the capsule is owned by: $capsuleClass (pid $([E]::P($hitAtCapsule)))"
if ($capsuleClass -ne 'SysListView32' -and $capsuleClass -ne 'SHELLDLL_DefView') {
  "!! the desktop is NOT exposed at ($CapsuleX,$CapsuleY): $capsuleClass is covering it. Measurement would be meaningless."
  exit 3
}

if (-not $Run) {
  'DRY RUN (desktop looks exposed). Re-run with -Run to synthesize the clicks.'
  exit 0
}

'=== probes ==='
# The capsule's measured rect is (1138,1454)-(1423,1521). Probe A is inside it, probe B is
# below its bottom edge (outside every published region), probe C is far-empty desktop.
$probes = @(
  [pscustomobject]@{ label = 'A inside capsule region';        x = $CapsuleX; y = $CapsuleY },
  [pscustomobject]@{ label = 'B below capsule (no region)';    x = $CapsuleX; y = $CapsuleY + $BelowOffset },
  [pscustomobject]@{ label = 'C empty desktop (no region)';    x = 1250;      y = 900 }
)

$results = @()
foreach ($probe in $probes) {
  $px = $probe.x; $py = $probe.y
  $ppt = New-Object E+POINT; $ppt.x = $px; $ppt.y = $py
  $owner = [E]::WindowFromPoint($ppt)
  $blank = Test-Blank $px $py
  Write-Host ("--- {0} at ({1},{2}) | click owner={3} pid={4} | UIA says blank desktop={5}" -f `
    $probe.label, $px, $py, [E]::C($owner), [E]::P($owner), $blank)
  if ([E]::C($owner) -ne 'SysListView32' -and [E]::C($owner) -ne 'SHELLDLL_DefView') {
    Write-Host '    desktop not exposed here; skipping this probe'
    continue
  }
  $before = -not [E]::IsWindowVisible((DefView))
  DoubleClickAt $px $py
  $after = -not [E]::IsWindowVisible((DefView))
  [void](State ("  after " + $probe.label))
  $results += [pscustomobject]@{ label = $probe.label; x = $px; y = $py; blank = $blank; toggled = ($before -ne $after); nowInner = $after }
  if ($after) {
    Write-Host '    toggled to INNER; returning to OUTER before the next probe'
    foreach ($cand in @(@(1250, 900), @(700, 1180), @(1500, 700), @(640, 1320), @(1300, 620))) {
      if (Test-Blank $cand[0] $cand[1]) {
        DoubleClickAt $cand[0] $cand[1]
        if ([E]::IsWindowVisible((DefView))) { Write-Host "    restored via ($($cand[0]),$($cand[1]))"; break }
      }
    }
    if (-not [E]::IsWindowVisible((DefView))) { Write-Host '    !! could not return to OUTER'; break }
  }
}

if (-not [E]::IsWindowVisible((DefView))) {
  Write-Host '=== final restore attempt ==='
  foreach ($cand in @(@(1250, 900), @(700, 1180), @(1500, 700), @(640, 1320), @(1300, 620))) {
    if (Test-Blank $cand[0] $cand[1]) {
      DoubleClickAt $cand[0] $cand[1]
      if ([E]::IsWindowVisible((DefView))) { break }
    }
  }
}
[void](State 'final')

'=== verdict ==='
$inside = $results | Where-Object { $_.label -like 'A*' }
$outside = $results | Where-Object { $_.label -notlike 'A*' }
if ($inside) {
  if ($inside.toggled) { 'A toggled -> a click INSIDE the capsule region was treated as blank desktop (region miss).' }
  else { 'A suppressed -> the published region list DOES contain the capsule, and the hit test agrees with its measured rect.' }
}
foreach ($r in $outside) {
  if ($r.toggled) { "$($r.label): toggled -> clicks outside every region become desktop double clicks, as the model predicts." }
  else { "$($r.label): did NOT toggle (UIA blank=$($r.blank)) -> the toggle monitor has a second blocker beyond the region test." }
}
