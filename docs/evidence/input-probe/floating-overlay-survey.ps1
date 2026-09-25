# Survey: how do floating balls / desktop overlays actually set up their windows?
# Reads top-level windows that carry the layered/no-activate/toolwindow/topmost style mix
# and reports what those flags imply for input. Read-only.
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class F {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static string T(IntPtr h){var sb=new StringBuilder(256);GetWindowTextW(h,sb,256);return sb.ToString();}
  public static uint P(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);return p;}
}
'@
[void][F]::SetProcessDPIAware()

$EX = @{ 0x8='TOPMOST'; 0x20='TRANSPARENT'; 0x80='TOOLWINDOW'; 0x80000='LAYERED';
         0x8000000='NOACTIVATE'; 0x40000='APPWINDOW'; 0x200000='NOREDIRECTIONBITMAP' }

$rows = New-Object System.Collections.ArrayList
$cb = [F+EnumProc]{
  param($h, $l)
  if (-not [F]::IsWindowVisible($h)) { return $true }
  $ex = [F]::GetWindowLongPtr($h, -20).ToInt64()
  # floating-overlay signature: layered or no-activate, and not a normal app frame
  if ((($ex -band 0x80000) -eq 0) -and (($ex -band 0x8000000) -eq 0)) { return $true }
  $r = New-Object F+RECT
  if (-not [F]::GetWindowRect($h, [ref]$r)) { return $true }
  $w = $r.right - $r.left; $ht = $r.bottom - $r.top
  if ($w -le 0 -or $ht -le 0 -or $w -gt 1400 -or $ht -gt 900) { return $true }
  $flags = @()
  foreach ($k in $EX.Keys) { if (($ex -band $k) -ne 0) { $flags += $EX[$k] } }
  $parent = [F]::GetParent($h)
  $owner = [F]::P($h)
  $proc = (Get-Process -Id $owner -ErrorAction SilentlyContinue).ProcessName
  [void]$rows.Add([pscustomobject]@{
    hwnd = ('0x{0:X8}' -f $h.ToInt64()); cls = [F]::C($h); title = [F]::T($h)
    pid = $owner; proc = $proc; size = "${w}x${ht}"; pos = "($($r.left),$($r.top))"
    parent = $(if ($parent -eq [IntPtr]::Zero) { 'none(top-level)' } else { [F]::C($parent) })
    flags = ($flags -join '|') })
  return $true
}
[void][F]::EnumWindows($cb, [IntPtr]::Zero)
$rows | Sort-Object proc, hwnd | Format-Table -AutoSize -Wrap