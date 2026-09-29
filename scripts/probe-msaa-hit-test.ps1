$ErrorActionPreference = 'Stop'
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Collections.Generic;
public class Msaa3 {
  [DllImport("oleacc.dll")] public static extern int AccessibleObjectFromWindow(IntPtr hwnd, uint idObject, ref Guid riid, [MarshalAs(UnmanagedType.Interface)] out object ppvObject);
  [DllImport("oleacc.dll")] public static extern int AccessibleObjectFromPoint(POINT pt, [MarshalAs(UnmanagedType.Interface)] out object ppvObject, [MarshalAs(UnmanagedType.Struct)] out object pvarChild);
  [DllImport("user32.dll")] public static extern bool EnumWindows(Proc cb, IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, Proc cb, IntPtr p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int m);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetDesktopWindow();
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  public delegate bool Proc(IntPtr h, IntPtr p);
  public static List<IntPtr> Hosts = new List<IntPtr>();
  public static List<IntPtr> Folders = new List<IntPtr>();
  public static string Cls(IntPtr h) { if (h == IntPtr.Zero) return "(null)"; var sb = new StringBuilder(256); GetClassName(h, sb, 256); return sb.ToString(); }
  public static bool Top(IntPtr h, IntPtr p) { var c = Cls(h); if (c == "Progman" || c == "WorkerW") Hosts.Add(h); return true; }
  public static bool Child(IntPtr h, IntPtr p) { if (Cls(h) == "SysListView32" && IsWindowVisible(h)) Folders.Add(h); return true; }
  public static bool Win32OnDesktop(int x, int y) {
    var pt = new POINT(); pt.x = x; pt.y = y;
    var cur = WindowFromPoint(pt);
    for (int i = 0; i < 16; i++) {
      if (cur == IntPtr.Zero) return false;
      var c = Cls(cur);
      if (cur == GetDesktopWindow()) return true;
      if (c == "Progman" || c == "WorkerW" || c == "SHELLDLL_DefView" || c == "SysListView32") return true;
      var parent = GetParent(cur);
      if (parent == IntPtr.Zero || parent == cur) return false;
      cur = parent;
    }
    return false;
  }
}
"@

[Msaa3]::EnumWindows([Msaa3+Proc]{ param($h,$p) [Msaa3]::Top($h,$p) }, [IntPtr]::Zero) | Out-Null
foreach ($w in [Msaa3]::Hosts) { [Msaa3]::EnumChildWindows($w, [Msaa3+Proc]{ param($h,$p) [Msaa3]::Child($h,$p) }, [IntPtr]::Zero) | Out-Null }
if ([Msaa3]::Folders.Count -eq 0) { Write-Host "当前不是表桌面，停止"; exit 0 }
$folder = [Msaa3]::Folders[0]
$iid = [Guid]'618736E0-3C3D-11CF-810C-00AA00389B71'
$root = $null
$null = [Msaa3]::AccessibleObjectFromWindow($folder, ([uint32]4294967292), [ref]$iid, [ref]$root)

# 取前若干子项矩形，各取一个"必然落在图标上"的点
$points = New-Object System.Collections.ArrayList
$count = [Math]::Min(3, $root.accChildCount)
for ($i = 1; $i -le $count; $i++) {
  $l = 0; $t = 0; $w = 0; $h = 0
  $root.accLocation([ref]$l, [ref]$t, [ref]$w, [ref]$h, $i)
  $name = $root.accName($i)
  [void]$points.Add(@([int]($l + $w / 2), [int]($t + $h / 2), "图标 $i '$name'"))
}
# 再取几个候选空白点
foreach ($b in @(@(200, 1000), @(1000, 1000), @(900, 400), @(1500, 950))) { [void]$points.Add(@($b[0], $b[1], '候选空白')) }

Write-Host "== 在 FolderView 上做 accHitTest（返回的是本对象内的子项编号） =="
foreach ($p in $points) {
  $x = $p[0]; $y = $p[1]; $label = $p[2]
  $hitResult = '<失败>'
  try {
    $r = $root.accHitTest($x, $y)
    if ($r -is [int]) { $hitResult = "子项编号 $r" } else { $hitResult = "返回了对象（跨元素）：" + $(try { $r.accName(0) } catch { '无名' }) }
  } catch { $hitResult = "异常 $($_.Exception.Message)" }
  $pt = New-Object Msaa3+POINT; $pt.x = $x; $pt.y = $y
  $onDesk = [Msaa3]::Win32OnDesktop($x, $y)
  "  ($x,$y) $label"
  "      accHitTest: $hitResult"
  "      窗口父链是否到桌面: $onDesk"
}