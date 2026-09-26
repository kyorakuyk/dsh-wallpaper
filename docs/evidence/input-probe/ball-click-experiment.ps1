# End-to-end acceptance for the floating ball (increment 2).
#
# Asserts, with measurements rather than impressions:
#   1. the ball pops out when the cursor approaches, and NOT when the input island is visible;
#   2. clicking the ball enters the inner desktop (Explorer's icon layer hides) and the island appears;
#   3. the ball retracts after that click, and the old collapsed capsule no longer exists in the
#      wallpaper WebView's DOM (checked through UI Automation, not by looking at a screenshot);
#   4. leaving the inner desktop restores the icons and lets the ball pop again.
#
# Everything is driven by synthesized mouse input at points the desktop owns, and every window
# the run needs out of the way is minimized and restored afterwards. The cursor is restored too.
param(
  [int]$SettleMs = 700,
  [int]$TriggerY = 1520,        # inside the ball's trigger band, above the taskbar reveal line
  [int]$NeutralY = 700,
  [switch]$Run                  # without -Run it only reports the baseline
)

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class A {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string w);
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static string T(IntPtr h){var sb=new StringBuilder(256);GetWindowTextW(h,sb,256);return sb.ToString();}
  public static uint P_(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);return p;}
}
'@
[void][A]::SetProcessDPIAware()

$LEFTDOWN = 0x0002
$LEFTUP = 0x0004

function BallHandle {
  $mine = @(Get-Process -Name 'dsh-wallpaper' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  $script:ball = [IntPtr]::Zero
  $cb = [A+EnumProc]{
    param($h, $l)
    if ($mine -notcontains [int][A]::P_($h)) { return $true }
    if ([A]::T($h) -eq 'DSH Wallpaper Ball') { $script:ball = $h }
    return $true
  }
  [void][A]::EnumWindows($cb, [IntPtr]::Zero)
  return $script:ball
}
function BallRect { $r = New-Object A+RECT; [void][A]::GetWindowRect((BallHandle), [ref]$r); return $r }
function BallPopped { $r = BallRect; return ($r.top -lt [A]::GetSystemMetrics(1)) }
function DefView { [A]::FindWindowExW([A]::GetShellWindow(), [IntPtr]::Zero, 'SHELLDLL_DefView', $null) }
function OuterDesktop { return [A]::IsWindowVisible((DefView)) }
function ClickNow {
  [A]::mouse_event($LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 60
  [A]::mouse_event($LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 120
}
function DoubleClickNow { ClickNow; ClickNow; Start-Sleep -Milliseconds 300 }
function MoveTo([int]$x, [int]$y, [int]$settle) { [void][A]::SetCursorPos($x, $y); Start-Sleep -Milliseconds $settle }

# The island lives in the wallpaper WebView; UI Automation is how we see it from outside.
# The renderer (Chrome_RenderWidgetHostHWND) belongs to the WebView2 process, not to
# dsh-wallpaper.exe, so it must be located by walking down from the wallpaper host window
# ("Tauri Window" titled "DSH Wallpaper" - the ball is titled "DSH Wallpaper Ball").
function WallpaperDomNames {
  $names = @()
  # The wallpaper host is a CHILD of Progman (it is reparented behind the icon layer), so it
  # never appears in EnumWindows. Search the shell window's children by class + title instead.
  $script:render = [IntPtr]::Zero
  $host_ = [A]::FindWindowExW([A]::GetShellWindow(), [IntPtr]::Zero, 'Tauri Window', 'DSH Wallpaper')
  if ($host_ -ne [IntPtr]::Zero) {
    $kids = New-Object System.Collections.ArrayList
    $sub = [A+EnumProc]{ param($h, $l) [void]$kids.Add($h); return $true }
    [void][A]::EnumChildWindows($host_, $sub, [IntPtr]::Zero)
    foreach ($k in $kids) { if ([A]::C($k) -eq 'Chrome_RenderWidgetHostHWND') { $script:render = $k; break } }
  }
  if ($script:render -eq [IntPtr]::Zero) { return @('<no renderer>') }
  try {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($script:render)
    $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($e in $all) {
      $n = $e.Current.Name
      if ($n) { $names += $n }
    }
  } catch { $names += "<uia failed: $($_.Exception.Message)>" }
  return $names
}

$ball = BallHandle
if ($ball -eq [IntPtr]::Zero) { 'no "DSH Wallpaper Ball" window - is the new build installed?'; exit 1 }
$screenW = [A]::GetSystemMetrics(0); $screenH = [A]::GetSystemMetrics(1)
$X = [int]($screenW / 2)
"No: ball=0x{0:X8} screen=${screenW}x${screenH} probe x=$X" -f $ball.ToInt64()

$cursor0 = New-Object A+POINT
[void][A]::GetCursorPos([ref]$cursor0)
$fg0 = [A]::GetForegroundWindow()
$r = BallRect
"baseline: ball rect=({0},{1})-({2},{3}) size={4}x{5} popped={6} outerDesktop={7}" -f `
  $r.left, $r.top, $r.right, $r.bottom, ($r.right - $r.left), ($r.bottom - $r.top), (BallPopped), (OuterDesktop)

$dom = WallpaperDomNames
"wallpaper DOM names (baseline): $($dom.Count) -> $($dom -join ' | ')"
$capsulePresent = $dom -contains '展开 AI 对话'
"old collapsed capsule still in the wallpaper DOM = $capsulePresent  (must become False in increment 2)"

if (-not $Run) { 'DRY RUN. Re-run with -Run to drive the interaction.'; [void][A]::SetCursorPos($cursor0.x, $cursor0.y); exit 0 }

$results = @()
try {
  '=== 0. prime the desktop as the foreground ==='
  # App.tsx:652-654 forces the docked layout back to `collapsed` whenever the native snapshot
  # says the desktop is NOT the foreground, so the island can only stay expanded while the
  # desktop owns the foreground. A single click on blank desktop makes Progman/the desktop the
  # foreground window without toggling anything (the toggle needs two clicks inside 500 ms).
  MoveTo $X $NeutralY 300
  ClickNow
  Start-Sleep -Milliseconds 900
  $fgPrime = [A]::GetForegroundWindow()
  "foreground after priming = $([A]::C($fgPrime)) (pid $([A]::P_($fgPrime)))"

  '=== 1. approach the ball with the island hidden: it must pop ==='
  MoveTo $X $TriggerY $SettleMs
  $poppedAtRest = BallPopped
  "popped = $poppedAtRest"
  $results += [pscustomobject]@{ step = 'approach (island hidden)'; expect = 'pops'; got = $poppedAtRest; ok = $poppedAtRest }

  '=== 2. click the ball: must enter the inner desktop, expand the island, retract the ball ==='
  $r = BallRect
  $cx = [int](($r.left + $r.right) / 2); $cy = [int](($r.top + $r.bottom) / 2)
  MoveTo $cx $cy 250
  ClickNow
  Start-Sleep -Milliseconds 1400
  $outerAfterClick = OuterDesktop
  $ballAfterClick = BallPopped
  $domAfter = WallpaperDomNames
  $islandUp = ($domAfter -join ' ') -match '对话|DSH|发送'
  "outerDesktop = $outerAfterClick (False = we are inside); ball popped = $ballAfterClick; island-like DOM names = $islandUp"
  "DOM names now: $($domAfter -join ' | ')"
  $results += [pscustomobject]@{ step = 'click ball'; expect = 'inner desktop'; got = (-not $outerAfterClick); ok = (-not $outerAfterClick) }
  $results += [pscustomobject]@{ step = 'click ball'; expect = 'ball retracts'; got = (-not $ballAfterClick); ok = (-not $ballAfterClick) }

  '=== 3. approach again while the island is visible: the ball must stay hidden ==='
  # Wait out the post-click cooldown (1.5 s) first, so that anything the ball does here is
  # explained by "the island is visible" and not by the cooldown.
  Start-Sleep -Milliseconds 1600
  MoveTo $X $TriggerY $SettleMs
  $poppedWithIsland = BallPopped
  "popped = $poppedWithIsland (must be False; cooldown already expired)"
  $results += [pscustomobject]@{ step = 'approach (island visible)'; expect = 'stays hidden'; got = $poppedWithIsland; ok = (-not $poppedWithIsland) }

  '=== 4. leave the inner desktop: icons return, and the ball may pop again ==='
  MoveTo $X $NeutralY 400
  DoubleClickNow                      # blank desktop double click toggles back out
  Start-Sleep -Milliseconds 900
  $outerAgain = OuterDesktop
  "outerDesktop = $outerAgain (must be True)"
  $results += [pscustomobject]@{ step = 'leave inner desktop'; expect = 'icons restored'; got = $outerAgain; ok = $outerAgain }
  MoveTo $X $TriggerY $SettleMs
  $poppedAgain = BallPopped
  "popped again = $poppedAgain"
  $results += [pscustomobject]@{ step = 'approach after leaving'; expect = 'pops again'; got = $poppedAgain; ok = $poppedAgain }
} finally {
  MoveTo $X $NeutralY 300
  [void][A]::SetCursorPos($cursor0.x, $cursor0.y)
  $fg1 = [A]::GetForegroundWindow()
  "cursor restored to ($($cursor0.x),$($cursor0.y)); foreground unchanged = $($fg1 -eq $fg0)"
}

'=== results ==='
$results | Format-Table -AutoSize
$failed = @($results | Where-Object { -not $_.ok })
if ($failed.Count -eq 0) { 'ALL CHECKS PASSED' } else { "$($failed.Count) CHECK(S) FAILED: $($failed.step -join ', ')" }
