# Evidence probe 4: authoritative Z-order of Progman's direct children.
# GetWindow(GW_CHILD) + GW_HWNDNEXT walks the real sibling Z chain (top to bottom),
# unlike EnumChildWindows which flattens the tree.
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class Z {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint owner);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  public static string C(IntPtr h){var sb=new StringBuilder(256);GetClassNameW(h,sb,256);return sb.ToString();}
  public static string T(IntPtr h){var sb=new StringBuilder(256);GetWindowTextW(h,sb,256);return sb.ToString();}
  public static uint P(IntPtr h){uint p;GetWindowThreadProcessId(h,out p);return p;}
}
'@
[void][Z]::SetProcessDPIAware()
function Row([IntPtr]$h,[string]$tag){
  if($h -eq [IntPtr]::Zero){return "$tag NULL"}
  $r=New-Object Z+RECT; [void][Z]::GetWindowRect($h,[ref]$r)
  $ex=[Z]::GetWindowLongPtr($h,-20).ToInt64()
  $st=[Z]::GetWindowLongPtr($h,-16).ToInt64()
  $t=[Z]::T($h); if($t.Length -gt 24){$t=$t.Substring(0,24)}
  $tr = ''
  if (($ex -band 0x20) -ne 0) { $tr = '[TRANSPARENT]' }
  '{0,-6} 0x{1:X8} {2,-30} pid={3,-6} vis={4,-5} ex=0x{5:X8}{6} style=0x{7:X8} ({8},{9})-({10},{11}) "{12}"' -f `
    $tag,$h.ToInt64(),[Z]::C($h),[Z]::P($h),[Z]::IsWindowVisible($h),$ex,$tr,$st,$r.left,$r.top,$r.right,$r.bottom,$t
}
$shell=[Z]::GetShellWindow()
'Progman (shell) = 0x{0:X8}' -f $shell.ToInt64()
'--- direct children of Progman, front (top) to back ---'
$i=0
$cur=[Z]::GetWindow($shell,5)   # GW_CHILD
while($cur -ne [IntPtr]::Zero -and $i -lt 40){
  Row $cur ("#{0}" -f $i)
  $cur=[Z]::GetWindow($cur,2)   # GW_HWNDNEXT
  $i++
}
'--- foreground ---'
Row ([Z]::GetForegroundWindow()) 'FG'
