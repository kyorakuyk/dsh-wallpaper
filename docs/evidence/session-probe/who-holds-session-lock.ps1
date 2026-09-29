# 谁在占着某个会话？—— 只读探针
#
# 背景（见 docs/evidence/shell-background-start-window.md 八点二节）：DSH 在 Windows 上用**命名内核
# 信号量**表达"这个会话正在被某个宿主写"。名字是
#
#     Local\dsh-session-lock-<sha256(resolve(path).toLowerCase())>
#
# 其中 `path` 是**会话目录下的锁路径**（Windows 上并没有这个文件，名字是从这条路径算出来的），
# 初值 1：持有者 WaitForSingleObject 之后变 0，第二个人零超时等待就会超时（那座宿主对外报
# `session/writer-held`，界面那句"当前会话已被占用"）。
#
# 于是"谁在排斥"这个问题可以这样回答：
#   - OpenExisting 失败                       => 内核里没有这个对象，没人占（持有者句柄一关，对象就没了）
#   - OpenExisting 成功 + WaitOne(0) 成功     => 对象在、计数是 1，只是有人开着句柄没人占
#                                               （我们**立刻** Release 还回去）
#   - OpenExisting 成功 + WaitOne(0) 超时     => **计数是 0，有人正占着**
#
# 三条踩过的坑，写在这里免得下一个人再踩：
#   1. 会话是**两层**目录：`sessions/<工作目录编码>/<会话 id>/`。只探一层会一个都找不到。
#   2. 目录的 mtime **不代表**没人在写：v4 起日志叫 `session.v4.jsonl.zstd`，旧目录里可能还躺着
#      一个几天前的 `session.jsonl.zstd`。看 `*.v4.jsonl.zstd` 的 mtime 才是"最近有没有在写"。
#   3. 这个探针**不能**指出持有者是哪个进程：句柄枚举要管理员权限。把窗口定到会话上之后，
#      持有者就是那台正在服务该会话的宿主 —— 用 `Get-NetTCPConnection` 找端口属主即可对上。
#
# 用法：pwsh -NoProfile -ExecutionPolicy Bypass -File .\who-holds-session-lock.ps1 [-Filter <会话名子串>]
#
# 注意 `-Filter` 这个参数名不是随便起的：PowerShell 的变量名**不区分大小写**，所以一旦把它写成
# `$Session`，它就和循环变量 `$session` 是同一个变量 —— 第一轮迭代就把它赋成目录对象，过滤条件
# 于是把每一项都跳过，脚本安静地报"0 个会话"。（这个坑当场踩过一次，记在这里。）
param([string]$Filter)

$ErrorActionPreference = 'Stop'

$home2 = if ($env:DSH_HOME) { $env:DSH_HOME } else { Join-Path $env:USERPROFILE '.dsh' }
$sessions = Join-Path $home2 'sessions'
if (-not (Test-Path -LiteralPath $sessions)) { throw "找不到会话目录：$sessions" }

function Get-Sha256Hex([string]$text) {
  $sha = [System.Security.Cryptography.SHA256]::Create()
  try { ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($text)))).Replace('-', '').ToLowerInvariant() }
  finally { $sha.Dispose() }
}

# 'nobody' | 'open-not-held' | 'held'
function Get-LeaseState([string]$lockPath) {
  $resolved = [IO.Path]::GetFullPath($lockPath).ToLowerInvariant()
  $name = 'Local\dsh-session-lock-' + (Get-Sha256Hex $resolved)
  try { $semaphore = [System.Threading.Semaphore]::OpenExisting($name) }
  catch [System.Threading.WaitHandleCannotBeOpenedException] { return 'nobody' }
  try {
    if ($semaphore.WaitOne(0)) { $semaphore.Release() | Out-Null; return 'open-not-held' }
    return 'held'
  } finally { $semaphore.Dispose() }
}

$rows = @()
foreach ($group in (Get-ChildItem -LiteralPath $sessions -Directory -ErrorAction SilentlyContinue)) {
  foreach ($sessionDir in (Get-ChildItem -LiteralPath $group.FullName -Directory -ErrorAction SilentlyContinue)) {
    if ($Filter -and $sessionDir.Name -notlike "*$Filter*") { continue }
    $log = Get-ChildItem -LiteralPath $sessionDir.FullName -Filter '*.jsonl.zstd' -ErrorAction SilentlyContinue |
      Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $rows += [pscustomobject]@{
      Lease = Get-LeaseState (Join-Path $sessionDir.FullName 'session.lock')
      Group = $group.Name
      Session = $sessionDir.Name
      LastWrite = if ($log) { $log.LastWriteTime } else { $sessionDir.LastWriteTime }
      Log = if ($log) { $log.Name } else { '(no log)' }
    }
  }
}

Write-Output ("会话 {0} 个，其中被占 {1} 个" -f $rows.Count, ($rows | Where-Object Lease -eq 'held').Count)
$rows | Sort-Object @{ Expression = { $_.Lease -eq 'held' }; Descending = $true }, LastWrite -Descending |
  ForEach-Object { "  {0,-14} {1,19:yyyy-MM-dd HH:mm:ss}  [{2}] {3}  ({4})" -f $_.Lease, $_.LastWrite, $_.Group, $_.Session, $_.Log }

if (-not $Filter) {
  Write-Output ''
  Write-Output '上面所有 held 的那些，就是此刻握着写句柄的会话。持有者是"正在服务它的那台宿主"，'
  Write-Output '用 Get-NetTCPConnection -State Listen 找 19387 / 3099 / 3080 的属主即可对上。'
}
