<#
.SYNOPSIS
    测「Windows 从登录到真正拉起 dsh-wallpaper 进程」这一段有多久（只读，幂等）。

.DESCRIPTION
    要判断「改用登录触发器计划任务能不能让壁纸更早出现」，必须先知道现在这一段有多长：

        [本次登录会话开始] --(A)--> [dsh-wallpaper 进程启动] --(B)--> [壁纸首帧可见]

    (A) 就是 Windows 延迟拉起进程的那一段，现有日志里**完全看不到**（日志里的
    `elapsed_ms` 一律从进程内部某个起点算起，起点在进程已经跑起来之后）。
    这个脚本把 (A) 单独量出来，(B) 从应用自己写下的启动时间点里取。

    本脚本只读。它不建计划任务、不写注册表、不改任何系统设置、不启动也不停止任何进程。
    可以反复跑，任何时候跑结果都只反映「现在能观测到的事实」。

.PARAMETER ProcessName
    要测量的进程名字（不带 .exe）。默认 dsh-wallpaper。

.PARAMETER LogPath
    应用写下的启动诊断文件。默认 %LOCALAPPDATA%\DSHWallpaper\startup-diagnostic.log。

.PARAMETER AppLogPath
    应用自身日志。默认 %LOCALAPPDATA%\com.dsh.wallpaper\logs\dsh-wallpaper.log。

.EXAMPLE
    pwsh -NoProfile -File scripts/measure-startup-gap.ps1

.EXAMPLE
    pwsh -NoProfile -File scripts/measure-startup-gap.ps1 | Tee-Object -FilePath .\output\startup-gap-01.txt

.NOTES
    怎么拿数据（实验步骤，照做即可）：

    1. 先把机器弄到「干净一次启动」的状态：注销再登录（或者重启）。
       不要只关掉壁纸再手动双击 —— 那样量出来的是「手动启动」，不是登录链路。
    2. 记一下这一次有没有别的启动项在抢资源（本脚本的「启动项与资源竞争」一节会列
       Run 键与登录时触发的计划任务；另外自己记一条当时的观察，例如
       「开着 Docker Desktop / Steam 正在更新 / 杀毒在扫盘」）。
    3. 等壁纸真的出现在桌面上（首帧可见，即桌面不再是黑/纯色）。
    4. 跑这个脚本，把**完整输出**留档：
           pwsh -NoProfile -File scripts/measure-startup-gap.ps1 > output\startup-gap-<日期>-<第几次>.txt
    5. 至少跑三次（三次完整注销/登录），并且最好包含「冷启动」与「热启动」各一次。
       单次数字不能当结论用：这一段受启动项并发、杀毒、磁盘状态影响很大。
    6. 三次输出放在一起比对，取中位数，再看离散度。离散度大就说明还要多测几次。

    每一步的**数据来源**与是否需要管理员：

    - 本次登录会话开始时间：优先用 `Win32_LogonSession`（LogonType=2，交互式登录）。
      本机实测**不需要管理员**就能拿到（CIM 的 `Win32_LogonSession` 默认可见），
      而且它正是「登录会话开始」的时刻（本机实测比 explorer 早 4 秒）。
      降级顺序：
        第一档 `Win32_LogonSession`（不需要管理员；拿不到就往下走）
        第二档 系统启动时间 `Win32_OperatingSystem.LastBootUpTime`（不需要管理员；
               它量的是「开机到进程启动」，比登录更早，只是上界，会把这写进输出）
        第三档 用户注册表配置单元 `%USERPROFILE%\NTUSER.DAT` 的 LastWriteTime
               （不需要管理员；那是配置单元最后一次被写的时间，登录时会写，但不保证
                就等于登录时刻 —— 只当作弱的参考）
      每一档都会在输出里写明「来源」与「是否需要管理员」，一条都拿不到就明确报「拿不到」，
      绝不静默跳过、也绝不拿别的数字顶上去冒充。
      说明：`query user` / `quser` 在本机不存在；安全日志（事件 4624）需要管理员，
      不作为唯一来源也不默认尝试。
    - 应用进程启动时间：`Win32_Process.CreationDate`（不需要管理员），
      与 `Get-Process.StartTime` 交叉核对。日志里的 `--desktop-repair` 辅助进程会被排除。
    - explorer.exe 启动时间：`Win32_Process` / `Get-Process`（不需要管理员）。
      它是「shell 何时就绪」的参照 —— 壁纸由 shell 之后的登录链路拉起，
      所以 explorer 之后到进程启动的这一段才是真正的「拉起延迟」。
    - 应用内部的各个时间点：`startup-diagnostic.log` 里带 `epoch_ms=` 的行
      （由 `wallpaper/src-tauri/src/startup_timeline.rs` 写下）。
#>
[CmdletBinding()]
param(
    [string]$ProcessName = 'dsh-wallpaper',
    [string]$LogPath = (Join-Path $env:LOCALAPPDATA 'DSHWallpaper\startup-diagnostic.log'),
    [string]$AppLogPath = (Join-Path $env:LOCALAPPDATA 'com.dsh.wallpaper\logs\dsh-wallpaper.log')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$Culture = [System.Globalization.CultureInfo]::InvariantCulture

function Write-Section([string]$Title) {
    Write-Output ''
    Write-Output ('=' * 72)
    Write-Output $Title
    Write-Output ('=' * 72)
}

function Format-Stamp([datetime]$Value) {
    if ($null -eq $Value) { return '拿不到' }
    return $Value.ToString('yyyy-MM-dd HH:mm:ss.fff', $Culture)
}

function Format-Gap([object]$Milliseconds) {
    if ($null -eq $Milliseconds) { return '拿不到' }
    return ('{0} 毫秒（{1:N3} 秒）' -f [long]$Milliseconds, ([double]$Milliseconds / 1000.0))
}

function Test-Administrator {
    # 只读判断：拿当前身份的令牌看它在管理员组里是「启用」还是「仅用于拒绝」。
    try {
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        $principal = New-Object Security.Principal.WindowsPrincipal -ArgumentList $identity
        return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    } catch {
        return $false
    }
}

$isAdmin = Test-Administrator

Write-Output "dsh-wallpaper 启动链路测量（只读，幂等）"
Write-Output ("运行时间：" + (Get-Date).ToString('yyyy-MM-dd HH:mm:ss', $Culture))
Write-Output ("当前身份：{0}（管理员：{1}）" -f [Security.Principal.WindowsIdentity]::GetCurrent().Name, $(if ($isAdmin) { '是' } else { '否 —— 依赖管理员的来源会被标记并降级' }))
Write-Output ("进程名：" + $ProcessName)

# ---------------------------------------------------------------- 本机登录会话
Write-Section '一、本次登录会话开始时间（这一段是「Windows 延迟拉起进程」的起点）'

$logonStart = $null
$logonSource = $null
$logonNeedsAdmin = $null
$logonNote = $null

try {
    $interactive = @(Get-CimInstance -ClassName Win32_LogonSession -ErrorAction Stop |
        Where-Object { $_.LogonType -eq 2 -and $_.StartTime -is [datetime] } |
        Sort-Object StartTime -Descending)
    if ($interactive.Count -gt 0) {
        $logonStart = $interactive[0].StartTime
        $logonSource = 'Win32_LogonSession（LogonType=2，交互式登录，取最后一次）'
        $logonNeedsAdmin = '不需要管理员'
        $logonNote = "本机可见的交互式登录会话共 $($interactive.Count) 个"
    } else {
        $logonNote = 'Win32_LogonSession 里没有任何 LogonType=2 且带 StartTime 的记录'
    }
} catch {
    $logonNote = "Win32_LogonSession 查询失败：$($_.Exception.Message)"
}

if ($null -eq $logonStart) {
    try {
        $boot = (Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop).LastBootUpTime
        if ($boot -is [datetime]) {
            $logonStart = $boot
            $logonSource = 'Win32_OperatingSystem.LastBootUpTime（降级：这是开机时刻，不是登录时刻）'
            $logonNeedsAdmin = '不需要管理员'
            $logonNote = '拿不到登录会话，只能用开机时刻兜底。它偏早，量出来的这一段是上界。'
        }
    } catch {
        $logonNote = "$logonNote；Win32_OperatingSystem 也失败：$($_.Exception.Message)"
    }
}

if ($null -eq $logonStart) {
    try {
        $hive = Get-Item -LiteralPath (Join-Path $env:USERPROFILE 'NTUSER.DAT') -Force -ErrorAction Stop
        $logonStart = $hive.LastWriteTime
        $logonSource = 'NTUSER.DAT 的 LastWriteTime（降级：配置单元最后一次被写的时间）'
        $logonNeedsAdmin = '不需要管理员'
        $logonNote = '前两档都拿不到。这一档不保证等于登录时刻，只当弱参考。'
    } catch {
        $logonNote = "$logonNote；NTUSER.DAT 也失败：$($_.Exception.Message)"
    }
}

if ($null -ne $logonStart) {
    Write-Output ("本次登录会话开始：" + (Format-Stamp $logonStart))
    Write-Output ("来源：" + $logonSource)
    Write-Output ("是否需要管理员：" + $logonNeedsAdmin)
    if ($logonNote) { Write-Output ("说明：" + $logonNote) }
} else {
    Write-Output '本次登录会话开始：拿不到（三档来源都失败）'
    Write-Output ("说明：" + $logonNote)
    Write-Output '结论：这一段本次量不出来；请勿用其它数字替代，先修好某一档来源再测。'
}

# ---------------------------------------------------------------- 进程启动时间
Write-Section '二、本应用进程的启动时间'

$candidates = @()
try {
    $candidates = @(Get-CimInstance -ClassName Win32_Process -Filter "Name = '$ProcessName.exe'" -ErrorAction Stop |
        Where-Object { $_.CommandLine -notmatch '--desktop-repair' })
} catch {
    Write-Output ("Win32_Process 查询失败：$($_.Exception.Message)")
}

$appProcess = $null
if ($candidates.Count -gt 0) {
    $appProcess = $candidates | Sort-Object CreationDate -Descending | Select-Object -First 1
}

$appStart = $null
$appPid = $null
if ($null -ne $appProcess) {
    $appStart = $appProcess.CreationDate
    $appPid = [int]$appProcess.ProcessId
    Write-Output ("进程启动（Win32_Process.CreationDate）：" + (Format-Stamp $appStart))
    Write-Output ("进程 id：$appPid")
    Write-Output ("命令行：" + $appProcess.CommandLine)
    Write-Output ("父进程 id：" + $appProcess.ParentProcessId)
    # 交叉核对：Get-Process 的 StartTime 也读一遍，两个来源差得离谱就当场说出来。
    try {
        $fromGetProcess = (Get-Process -Id $appPid -ErrorAction Stop).StartTime
        $delta = [math]::Abs(($fromGetProcess - $appStart).TotalMilliseconds)
        Write-Output ("Get-Process.StartTime 交叉核对：" + (Format-Stamp $fromGetProcess) + ("（与 CreationDate 相差 {0:N0} 毫秒）" -f $delta))
    } catch {
        Write-Output ("Get-Process.StartTime 取不到：" + $_.Exception.Message)
    }
    $excluded = @($candidates | Where-Object { [int]$_.ProcessId -ne $appPid })
    if ($excluded.Count -gt 0) {
        Write-Output ("同名的其它进程（未参与本次计算）：" + (($excluded | ForEach-Object { "$($_.ProcessId)" }) -join ', '))
    }
} else {
    Write-Output "进程启动：拿不到（当前没有正在运行的 $ProcessName.exe，或查询失败）"
    Write-Output '提示：这一段必须趁进程还活着的时候量。壁纸没在跑就先启动它再跑本脚本。'
}

# ---------------------------------------------------------------- explorer
Write-Section '三、explorer.exe（shell 何时就绪）'

$explorerStart = $null
try {
    $explorer = @(Get-Process -Name explorer -ErrorAction Stop) | Sort-Object StartTime | Select-Object -First 1
    if ($null -ne $explorer) {
        $explorerStart = $explorer.StartTime
        Write-Output ("explorer.exe 启动：" + (Format-Stamp $explorerStart) + ("（pid $($explorer.Id)，会话 $($explorer.SessionId)）"))
        $now = Get-Date
        Write-Output ("explorer.exe 距现在：" + (Format-Gap ($now - $explorerStart).TotalMilliseconds))
    }
} catch {
    Write-Output ("explorer.exe 启动：拿不到（" + $_.Exception.Message + '）')
}
if ($null -eq $explorerStart) { Write-Output '说明：拿不到 shell 时间就只能看登录到进程这一段，无法分开「Windows 登录流程」与「shell 之后的拉起」。' }

# ---------------------------------------------------------------- 差值
Write-Section '四、差值（这几行就是这次要的数据）'

if ($null -ne $logonStart -and $null -ne $appStart) {
    Write-Output ("【登录会话开始 -> 进程启动】" + (Format-Gap ($appStart - $logonStart).TotalMilliseconds))
    Write-Output ("    起点：" + (Format-Stamp $logonStart) + "（" + $logonSource + "）")
    Write-Output ("    终点：" + (Format-Stamp $appStart))
    if ($logonSource -notlike 'Win32_LogonSession*') {
        Write-Output '    注意：本行用的是降级来源，不是真正的登录时刻。'
    }
} else {
    Write-Output '【登录会话开始 -> 进程启动】拿不到（起点或终点缺一个）'
}

if ($null -ne $explorerStart -and $null -ne $appStart) {
    Write-Output ("【explorer 启动 -> 进程启动】" + (Format-Gap ($appStart - $explorerStart).TotalMilliseconds))
    Write-Output '    这一段才是「shell 就绪之后，登录链路又拖了多久才把壁纸拉起来」。'
} else {
    Write-Output '【explorer 启动 -> 进程启动】拿不到（起点或终点缺一个）'
}

if ($null -ne $logonStart -and $null -ne $explorerStart) {
    Write-Output ("【登录会话开始 -> explorer 启动】" + (Format-Gap ($explorerStart - $logonStart).TotalMilliseconds))
    Write-Output '    这一段是 Windows 自己的登录流程占掉的，与壁纸的启动机制无关。'
} else {
    Write-Output '【登录会话开始 -> explorer 启动】拿不到（起点或终点缺一个）'
}

if ($null -eq $logonStart) {
    Write-Output '本次登录会话开始时间：拿不到 —— 差值一与差值三都算不出来；下面第五节的应用内部时间点仍可读。'
}

# ---------------------------------------------------------------- 应用内部时间点
Write-Section '五、应用自己记下的启动时间点（最近一次启动）'

$runLookbackMs = 3 * 60 * 1000
if (-not (Test-Path -LiteralPath $LogPath)) {
    Write-Output ("找不到启动诊断文件：" + $LogPath)
    Write-Output '说明：这一档需要一份带 `epoch_ms=` 字段的日志。'
} else {
    Write-Output ("来源：" + $LogPath)
    $lines = @(Get-Content -LiteralPath $LogPath -ErrorAction SilentlyContinue)
    Write-Output ("文件行数：" + $lines.Count)

    $epochPattern = 'elapsed_ms=(\d+)\s+absolute=([\d\-]+\s[\d:.]+)\s+epoch_ms=(\d+)\s+event=(\S+)(?:\s+(.*))?$'
    $points = New-Object System.Collections.Generic.List[object]
    foreach ($line in $lines) {
        if ($line -match $epochPattern) {
            $points.Add([pscustomobject]@{
                ElapsedMs = [long]$Matches[1]
                Absolute  = $Matches[2]
                EpochMs   = [long]$Matches[3]
                Event     = $Matches[4]
                Text      = if ($Matches.Count -ge 6) { $Matches[5] } else { '' }
            })
        }
    }

    if ($points.Count -eq 0) {
        Write-Output '这份日志里没有带 epoch_ms 的时间点。'
        Write-Output '说明：正在跑的这份构建早于本次改动（启动时间点尚未编译进去）。'
        Write-Output '      改动要生效必须先重新构建并安装 —— 这一步由使用者自己做，脚本不做。'
    } else {
        Write-Output ("带 epoch_ms 的时间点共 " + $points.Count + ' 条')
        $newest = ($points | Sort-Object EpochMs -Descending | Select-Object -First 1).EpochMs
        $run = @($points | Where-Object { $_.EpochMs -ge ($newest - $runLookbackMs) } | Sort-Object EpochMs)
        Write-Output ''
        Write-Output ("最近一次启动：共 " + $run.Count + ' 条时间点，纪元毫秒 ' + $run[0].EpochMs + ' 到 ' + $run[-1].EpochMs)

        # 进程入口那一条的纪元毫秒，就是「应用自己认为的进程启动时刻」：
        # 用它可以把「登录 -> 进程启动」与「进程启动 -> 首帧可见」两段接成一条时间线。
        $entry = $run | Where-Object { $_.Event -eq 'process-entry' } | Select-Object -First 1
        $entryEpoch = $null
        if ($null -ne $entry) { $entryEpoch = $entry.EpochMs }

        if ($null -ne $appStart -and $null -ne $entry) {
            $windowsStamp = [datetimeoffset]::FromUnixTimeMilliseconds($entry.EpochMs).LocalDateTime
            Write-Output ("对齐：应用记的进程入口 = " + (Format-Stamp $windowsStamp) + ("；Windows 记的进程启动 = " + (Format-Stamp $appStart)))
            Write-Output ("    两者相差 " + (Format-Gap ([math]::Abs(($windowsStamp - $appStart).TotalMilliseconds))) + '（应在几十毫秒内：说明两边的时刻能对齐）')
        }

        $checkpointEvents = @(
            'process-entry',
            'native-cover-visible',
            'webview-visible',
            'host-ready-workerw',
            'update-reconcile-done',
            'harness-monitor-started',
            'floating-ball-ready'
        )
        Write-Output ''
        Write-Output '关心的启动时间点：'
        Write-Output ('{0,-24} {1,-14} {2,-24} {3}' -f '时间点（event）', '相对进程入口', '绝对时间', '中文说明')
        foreach ($name in $checkpointEvents) {
            $hit = $run | Where-Object { $_.Event -eq $name } | Select-Object -First 1
            if ($null -ne $hit) {
                Write-Output ('{0,-24} {1,-14} {2,-24} {3}' -f $name, ($hit.ElapsedMs.ToString() + ' ms'), $hit.Absolute, $hit.Text)
            } else {
                Write-Output ('{0,-24} {1,-14} {2,-24} {3}' -f $name, '未出现在', '这一次启动里', '无')
            }
        }

        # 从进程入口到首帧真正可见：这一段的终点优先取 webview-visible（WebView 自己画出来了），
        # 没有就退到 native-cover-visible（原生首帧层先盖住了桌面）。
        $firstVisible = $run | Where-Object { $_.Event -eq 'webview-visible' } | Select-Object -First 1
        $firstVisibleNote = '背景 WebView 已显示（首帧可见）'
        if ($null -eq $firstVisible) {
            $firstVisible = $run | Where-Object { $_.Event -eq 'native-cover-visible' } | Select-Object -First 1
            $firstVisibleNote = '原生首帧窗口已显示（WebView 首帧那一刻未记录）'
        }
        if ($null -ne $firstVisible -and $null -ne $entry) {
            Write-Output ''
            $visibleMs = [double]($firstVisible.ElapsedMs - $entry.ElapsedMs)
            Write-Output ("【进程启动 -> 壁纸首帧可见】" + (Format-Gap $visibleMs))
            Write-Output ("    终点取自 " + $firstVisible.Event + "：" + $firstVisibleNote)
            if ($null -ne $logonStart -and $null -ne $entryEpoch) {
                $entryLocal = [datetimeoffset]::FromUnixTimeMilliseconds($entryEpoch).LocalDateTime
                $logonToEntryMs = [double]($entryLocal - $logonStart).TotalMilliseconds
                $totalMs = $logonToEntryMs + $visibleMs
                Write-Output ("【登录会话开始 -> 壁纸首帧可见】" + (Format-Gap $totalMs) + '（= 第一段实测 + 第二段实测）')
                Write-Output '    这是在两台来源之间拼起来的时间线，不是一次连续测量，供参考。'
            } else {
                Write-Output '【登录会话开始 -> 壁纸首帧可见】算不出来（登录时刻或进程入口时刻缺一个）。'
            }
        } else {
            Write-Output ''
            Write-Output '【进程启动 -> 壁纸首帧可见】算不出来（这一次启动里没有首帧可见的时间点）。'
            Write-Output '    常见原因：壁纸还没画到首帧，或者最近一次启动被 64 KiB 的日志上限截掉了。'
            Write-Output '    先让壁纸真的显示出来，再跑这个脚本。'
        }

        Write-Output ''
        Write-Output '其余记录（含未纳入表格的事件，按时间顺序）：'
        foreach ($point in $run) {
            Write-Output ('  +{0,7} ms  {1}  {2}  {3}' -f $point.ElapsedMs, $point.Absolute, $point.Event, $point.Text)
        }
    }
}

# ---------------------------------------------------------------- 应用日志
Write-Section '六、应用日志里的同批时间点（交叉核对用）'

if (Test-Path -LiteralPath $AppLogPath) {
    $appLines = @(Get-Content -LiteralPath $AppLogPath -ErrorAction SilentlyContinue)
    Write-Output ("来源：" + $AppLogPath + ("（行数 " + $appLines.Count + "）"))
    $markedLines = @($appLines | Where-Object { $_ -match '启动时间点：' })
    if ($markedLines.Count -gt 0) {
        Write-Output '含绝对时间的启动时间点（应用日志的秒级前缀之外，消息里带毫秒）：'
        foreach ($markedLine in ($markedLines | Select-Object -Last 30)) { Write-Output ('  ' + $markedLine) }
    } else {
        Write-Output '这份日志里还没有「启动时间点：」的行。'
        Write-Output '说明：正在跑的构建早于本次改动，或这一次启动还没走到那些位置。'
    }
} else {
    Write-Output ("找不到应用日志：" + $AppLogPath)
}

# ---------------------------------------------------------------- 启动项
Write-Section '七、启动项与资源竞争（记录本次实验的「背景噪声」）'

Write-Output '当前用户 Run 键（HKCU，只读）：'
try {
    $runKey = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction Stop
    $properties = @($runKey.PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' })
    if ($properties.Count -eq 0) {
        Write-Output '  （空）'
    } else {
        foreach ($property in $properties) {
            $mark = ''
            if ($property.Name -like '*dsh*' -or ([string]$property.Value) -like '*dsh*') { $mark = '  <- 本应用' }
            Write-Output ("  {0} = {1}{2}" -f $property.Name, $property.Value, $mark)
        }
    }
} catch {
    Write-Output ('  读不到 Run 键：' + $_.Exception.Message)
}

Write-Output ''
Write-Output '登录时触发的计划任务（只读列举；本脚本不创建、不修改、不删除任何任务）：'
try {
    # 判断手法只用 `PSObject.TypeNames`（不碰可能为空的 CimClass 属性），
    # 否则在有触发器缺字段的任务上会当场抛错。
    $logonTasks = @()
    foreach ($task in @(Get-ScheduledTask -ErrorAction Stop)) {
        $hasLogonTrigger = $false
        foreach ($trigger in @($task.Triggers)) {
            if ($null -eq $trigger) { continue }
            foreach ($typeName in @($trigger.PSObject.TypeNames)) {
                if ($typeName -like '*MSFT_TaskLogonTrigger*') { $hasLogonTrigger = $true }
            }
        }
        if ($hasLogonTrigger) { $logonTasks += $task }
    }
    if ($logonTasks.Count -eq 0) {
        Write-Output '  （没有登录触发器任务）'
    } else {
        # 只看「能跟本应用抢资源」的那一部分：根目录下的自建任务，以及名字里带 dsh 的。
        # Windows 自带的 30 多个登录任务（Office / ASUS / 语言组件 ……）一律折叠成一行计数，
        # 否则每次实验的输出都被它们淹掉，反而看不见真正的噪声来源。
        $own = @($logonTasks | Where-Object { $_.TaskPath -eq '\' -or $_.TaskName -like '*dsh*' })
        $system = @($logonTasks | Where-Object { $own -notcontains $_ })
        Write-Output ("  登录触发器任务共 " + $logonTasks.Count + " 个；其中根目录自建或与本应用相关的有 " + $own.Count + ' 个：')
        if ($own.Count -eq 0) { Write-Output '    （没有）' }
        foreach ($task in $own) {
            Write-Output ("    {0}{1}  状态={2}" -f $task.TaskPath, $task.TaskName, $task.State)
        }
        Write-Output ("  其余 " + $system.Count + ' 个是 Windows 自带或系统组件的登录任务（只报数量，不逐个列出）。')
    }
} catch {
    Write-Output ('  列举计划任务失败（可能需要管理员，或 cmdlet 不可用）：' + $_.Exception.Message)
    Write-Output '  降级：这一段只是背景记录，不影响上面任何测量结果。'
}

Write-Output ''
Write-Output '本脚本没有做任何修改：只读进程、只读注册表、只读计划任务、只读日志。'
