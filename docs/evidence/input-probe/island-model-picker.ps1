# Read the wallpaper WebView's accessibility tree and report the model picker's real contents.
#
# The island's model <select> is a ComboBox in the accessibility tree, and its <option>s are its
# children, so this shows what the UI actually contains instead of what it was supposed to.
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class M {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string w);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static string T(IntPtr h){var sb=new StringBuilder(256);GetWindowTextW(h,sb,256);return sb.ToString();}
  public static uint P(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);return p;}
}
'@
[void][M]::SetProcessDPIAware()

$host_ = [M]::FindWindowExW([M]::GetShellWindow(), [IntPtr]::Zero, 'Tauri Window', 'DSH Wallpaper')
"wallpaper host 0x{0:X8} ('{1}')" -f $host_.ToInt64(), [M]::T($host_)
if ($host_ -eq [IntPtr]::Zero) { 'the wallpaper host was not found under Progman (is the wallpaper running?)'; exit 1 }
$render = [IntPtr]::Zero
$kids = New-Object System.Collections.ArrayList
$cb = [M+EnumProc]{ param($h, $l) [void]$kids.Add($h); return $true }
[void][M]::EnumChildWindows($host_, $cb, [IntPtr]::Zero)
foreach ($k in $kids) { if ([M]::C($k) -eq 'Chrome_RenderWidgetHostHWND') { $render = $k; break } }
"renderer 0x{0:X8}" -f $render.ToInt64()
if ($render -eq [IntPtr]::Zero) { 'no WebView renderer found'; exit 1 }

$root = [System.Windows.Automation.AutomationElement]::FromHandle($render)
$all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
"descendants: $($all.Count)"
'--- every element (type / name / rect); options are listed under each ComboBox ---'
foreach ($e in $all) {
  $r = $e.Current.BoundingRectangle
  $kind = $e.Current.ControlType.ProgrammaticName -replace 'ControlType\.', ''
  $name = $e.Current.Name
  if ($name.Length -gt 44) { $name = $name.Substring(0, 44) }
  '{0,-12} name="{1}" rect=({2},{3})-({4},{5}) enabled={6}' -f $kind, $name, [int]$r.Left, [int]$r.Top, [int]$r.Right, [int]$r.Bottom, $e.Current.IsEnabled
  if ($kind -eq 'ComboBox') {
    $items = $e.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    "    -> options: $($items.Count)"
    foreach ($i in $items) {
      '       {0} "{1}"' -f ($i.Current.ControlType.ProgrammaticName -replace 'ControlType\.', ''), $i.Current.Name
    }
  }
}