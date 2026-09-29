$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Collections.Generic;
public class IconProbe {
  [DllImport("user32.dll")] public static extern bool EnumWindows(Proc cb, IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, Proc cb, IntPtr p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int m);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  public delegate bool Proc(IntPtr h, IntPtr p);
  public static List<IntPtr> Hosts = new List<IntPtr>();
  public static List<string> Rows = new List<string>();
  public static List<IntPtr> Lists = new List<IntPtr>();
  public const uint LVM_GETITEMCOUNT = 0x1004;
  public static string Cls(IntPtr h) { if (h == IntPtr.Zero) return "(null)"; var sb = new StringBuilder(256); GetClassName(h, sb, 256); return sb.ToString(); }
  public static bool Top(IntPtr h, IntPtr p) { var c = Cls(h); if (c == "Progman" || c == "WorkerW") Hosts.Add(h); return true; }
  public static bool Child(IntPtr h, IntPtr p) {
    var c = Cls(h);
    if (c == "SHELLDLL_DefView") {
      RECT r; GetWindowRect(h, out r);
      Rows.Add(string.Format("SHELLDLL_DefView h=0x{0:X} 父={1} visible={2} rect=({3},{4})-({5},{6})", h.ToInt64(), Cls(GetParent(h)), IsWindowVisible(h), r.L, r.T, r.R, r.B));
    }
    if (c == "SysListView32") {
      RECT r; GetWindowRect(h, out r);
      int n = (int)SendMessage(h, LVM_GETITEMCOUNT, IntPtr.Zero, IntPtr.Zero);
      Rows.Add(string.Format("SysListView32   h=0x{0:X} visible={1} rect=({2},{3})-({4},{5}) 列表自报数量={6}", h.ToInt64(), IsWindowVisible(h), r.L, r.T, r.R, r.B, n));
      Lists.Add(h);
    }
    return true;
  }
  public static string Chain(IntPtr h) {
    var names = new List<string>(); var cur = h;
    for (int i = 0; i < 14; i++) { if (cur == IntPtr.Zero) break; names.Add(Cls(cur)); var parent = GetParent(cur); if (parent == IntPtr.Zero || parent == cur) break; cur = parent; }
    return string.Join(" < ", names);
  }
}
"@

Write-Host "== 1) 桌面宿主与图标列表 =="
[IconProbe]::EnumWindows([IconProbe+Proc]{ param($h,$p) [IconProbe]::Top($h,$p) }, [IntPtr]::Zero) | Out-Null
"  找到宿主窗口（Progman/WorkerW）: $([IconProbe]::Hosts.Count) 个"
foreach ($w in [IconProbe]::Hosts) { [IconProbe]::EnumChildWindows($w, [IconProbe+Proc]{ param($h,$p) [IconProbe]::Child($h,$p) }, [IntPtr]::Zero) | Out-Null }
[IconProbe]::Rows | ForEach-Object { "  " + $_ }

$visibleList = $null
foreach ($l in [IconProbe]::Lists) { if ([IconProbe]::IsWindowVisible($l)) { $visibleList = $l } }
if ($visibleList) { "  可见的图标列表: h=0x$($visibleList.ToInt64().ToString('X'))  => 当前是【表桌面】" }
else { "  没有可见的图标列表  => 当前是【里桌面】（图标层被隐藏）" }

Write-Host "`n== 2) 用辅助功能读可见列表的图标矩形（不注入、不拦鼠标） =="
if (-not $visibleList) {
  "  里桌面状态下图标层不可见，辅助功能读到 0 项是正常的 —— 按方案，这一状态下不问 Explorer，改由壁纸自己处理空白双击。"
} else {
  $el = [System.Windows.Automation.AutomationElement]::FromHandle($visibleList)
  "  列表元素: Name='$($el.Current.Name)' Class='$($el.Current.ClassName)'"
  $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::ListItem)
  $items = $el.FindAll([System.Windows.Automation.TreeScope]::Children, $cond)
  "  辅助功能报告图标数: $($items.Count)"
  $rects = New-Object System.Collections.ArrayList
  for ($i = 0; $i -lt $items.Count; $i++) {
    $it = $items.Item($i)
    $r = $it.Current.BoundingRectangle
    [void]$rects.Add(@([int]$r.X, [int]$r.Y, [int]($r.X + $r.Width), [int]($r.Y + $r.Height), $it.Current.Name))
  }
  $show = [Math]::Min(6, $rects.Count)
  for ($i = 0; $i -lt $show; $i++) { $r = $rects[$i]; "    '{0}' ({1},{2})-({3},{4})" -f $r[4], $r[0], $r[1], $r[2], $r[3] }
  $zero = ($rects | Where-Object { $_[2] -le $_[0] -or $_[3] -le $_[1] }).Count
  "  尺寸为零的矩形: $zero 个（这类必须判成无法确认，不能算空白）"
  Write-Host "`n== 3) 图标点与空白点，两条判据分别怎么判 =="
  $iconPt = $null
  foreach ($r in $rects) { if ($r[2] -gt $r[0] -and $r[3] -gt $r[1]) { $iconPt = @([int](($r[0]+$r[2])/2), [int](($r[1]+$r[3])/2)); break } }
  $blankPt = $null
  for ($y = 60; $y -lt 1000 -and -not $blankPt; $y += 30) {
    for ($x = 60; $x -lt 1650; $x += 30) {
      $inside = $false
      foreach ($r in $rects) { if ($x -ge $r[0] -and $x -lt $r[2] -and $y -ge $r[1] -and $y -lt $r[3]) { $inside = $true; break } }
      if (-not $inside) { $blankPt = @($x, $y); break }
    }
  }
  function Probe-Point($name, $pt) {
    if (-not $pt) { Write-Host "  ${name}: 无可用点"; return }
    Write-Host "  ${name} ($($pt[0]),$($pt[1]))"
    $p = New-Object IconProbe+POINT; $p.x = $pt[0]; $p.y = $pt[1]
    "    Win32 命中链: " + [IconProbe]::Chain([IconProbe]::WindowFromPoint($p))
    $hit = [System.Windows.Automation.AutomationElement]::FromPoint([System.Windows.Point]::new($pt[0], $pt[1]))
    if (-not $hit) { "    UIA 命中: 空（按旧判据会当成空白）"; return }
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $cur = $hit; $parts = @(); $isItem = $false
    for ($d = 0; $d -lt 6 -and $cur; $d++) {
      $nm = if ($cur.Current.Name) { $cur.Current.Name.Substring(0, [Math]::Min(16, $cur.Current.Name.Length)) } else { '-' }
      $parts += ("{0}:{1}" -f $cur.Current.ControlType.ProgrammaticName.Replace('ControlType.',''), $nm)
      if ($cur.Current.ControlType -eq [System.Windows.Automation.ControlType]::ListItem) { $isItem = $true }
      $cur = $walker.GetParent($cur)
    }
    "    UIA 命中链: " + ($parts -join ' < ')
    "    六层内是否含 ListItem: $(if ($isItem) { '是（会判成图标）' } else { '否（会判成空白）' })"
  }
  Probe-Point '图标点' $iconPt
  Probe-Point '空白点' $blankPt
}
