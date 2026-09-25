# Evidence probe 3: read the wallpaper WebView's accessibility tree through UI Automation.
# Zero-build: this is the same channel the 表/里 toggle uses to decide "blank desktop",
# and it exposes real DOM element names + physical bounding rectangles.
param(
  [Parameter(Mandatory=$true)][string]$Hwnd,   # hex string, e.g. 0x000B0BFA
  [int]$MaxElements = 400,
  [string[]]$AtPoint = @()                     # "x,y" physical points to classify
)

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

$h = [IntPtr]([Convert]::ToInt64($Hwnd, 16))
$root = [System.Windows.Automation.AutomationElement]::FromHandle($h)
if ($null -eq $root) { 'FromHandle returned null'; exit 1 }
"root: name='$($root.Current.Name)' class='$($root.Current.ClassName)' type=$($root.Current.ControlType.ProgrammaticName)"
"root rect: $($root.Current.BoundingRectangle)"

$all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                     [System.Windows.Automation.Condition]::TrueCondition)
"descendant elements: $($all.Count)"

$count = 0
foreach ($e in $all) {
  if ($count -ge $MaxElements) { break }
  $count++
  $r = $e.Current.BoundingRectangle
  if ($r.IsEmpty) { continue }
  $kind = $e.Current.ControlType.ProgrammaticName -replace 'ControlType\.', ''
  $name = $e.Current.Name
  if ($name.Length -gt 40) { $name = $name.Substring(0, 40) }
  '{0,-12} name="{1}" rect=({2},{3})-({4},{5}) size={6}x{7}' -f `
    $kind, $name, [int]$r.Left, [int]$r.Top, [int]$r.Right, [int]$r.Bottom, [int]$r.Width, [int]$r.Height
}

if ($AtPoint.Count -gt 0) {
  '--- UIA ElementFromPoint classification (what the blank-desktop check sees) ---'
  foreach ($spec in $AtPoint) {
    $parts = $spec.Split(',')
    $pt = New-Object System.Windows.Point([double]$parts[0], [double]$parts[1])
    $el = [System.Windows.Automation.AutomationElement]::FromPoint($pt)
    if ($null -eq $el) { "($spec) -> no element"; continue }
    $r = $el.Current.BoundingRectangle
    $kind = $el.Current.ControlType.ProgrammaticName -replace 'ControlType\.', ''
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $chain = @()
    $cur = $el
    for ($i = 0; $i -lt 4 -and $null -ne $cur; $i++) {
      $ct = $cur.Current.ControlType.ProgrammaticName -replace 'ControlType\.', ''
      $chain += "$ct[$($cur.Current.Name)]"
      $cur = $walker.GetParent($cur)
    }
    "($spec) -> $kind name='$($el.Current.Name)' class='$($el.Current.ClassName)' rect=($([int]$r.Left),$([int]$r.Top))-($([int]$r.Right),$([int]$r.Bottom))"
    "         chain: $($chain -join ' < ')"
  }
}
