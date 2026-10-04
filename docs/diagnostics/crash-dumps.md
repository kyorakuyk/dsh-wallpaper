# 崩溃之后去哪儿找证据：报告、全量转储、符号

这份文档只回答一件事：**应用崩了，现场在哪、怎么读、缺什么就读不出来。**

它对着两个东西写：

1. `wallpaper/src-tauri/src/crash_report.rs`（进程自己在崩溃现场写报告的模块，2026-10-05 起
   还写全量转储）；
2. `scripts/publish-local-nsis.ps1`（本机发布时把 PDB 留档的那一段）。

背景一句话：真机上出现过「壁纸莫名其妙自动退出」，从事件日志只能读到「出错模块 + 异常码 +
偏移」这种没有符号就没有意义的线索；更早的 0.2.0 / 0.3.0 / 0.4.0 崩溃，**当时的 PDB 已经不在
了，所以永远无法符号化**。这份文档与那两个改动合起来，就是为了让下一次不再重复这件事。

---

## 1. 一次崩溃会留下两处落点（同一套布局）

两处都在**同一个标识符目录**下，`logs` 与 `crashes` 平级：

| 东西 | 落点（完整版） | 落点（Lite 版） | 大小量级 |
| --- | --- | --- | --- |
| 崩溃报告（纯文本，人读） | `%LOCALAPPDATA%\com.dsh.wallpaper\logs\crash-<时间戳>.txt` | `%LOCALAPPDATA%\com.dsh.wallpaper.lite\logs\crash-<时间戳>.txt` | 几十 KB |
| 全量转储（minidump，调试器读） | `%LOCALAPPDATA%\com.dsh.wallpaper\crashes\crash-<时间戳>-<异常码>.dmp` | `%LOCALAPPDATA%\com.dsh.wallpaper.lite\crashes\...` | 几十到几百 MB |
| 应用日志（报告会带它的尾部） | `%LOCALAPPDATA%\com.dsh.wallpaper\logs\dsh-wallpaper.log` | `...\.lite\logs\dsh-wallpaper-lite.log` | 40 KB 轮换 |
| 发布留档的符号 | `dist\<版本>\dsh_wallpaper_<版本>.pdb` | `dist\<版本>\dsh_wallpaper_lite_<版本>.pdb` | 约 10 MB |

`<时间戳>` 是本地时间 `YYYYMMDD-HHMMSS-mmm`（带毫秒：同一毫秒里的第二份现场不会被覆盖，
会在名字后面加 `-2`）。`<异常码>` 是 8 位十六进制，例如 `c0000005`。

**报告与转储是同一次崩溃、同一个时间戳**：报告里 `dump.file` / `dump.path` 写明了这一份报告
配的是哪个转储文件。两者要一起看（见第 4 节）。

完整版与 Lite 版各有自己的目录，别在另一个目录里找。装的到底是哪一个：安装目录
`%LOCALAPPDATA%\dsh-wallpaper\` 下的 `dsh-wallpaper.exe` / `dsh-wallpaper-lite.exe`。

### 报告里有什么

纯文本，字段一眼看得懂：进程、线程、异常码与地址、出错线程的调用栈（返回地址）、**模块表
（基址 + 大小 + 基名）**、应用日志尾部 40 行。模块表是它最要紧的部分：有了基址才能把地址
换算成「模块名+偏移」，报告正文里也已经替人算好了。

### 转储里有什么

`MiniDumpWriteDump` + `MiniDumpWithFullMemory`（另有 `MiniDumpWithProcessThreadData` 与
`MiniDumpWithThreadInfo`），也就是**完整内存** + 所有线程的上下文。之所以要全量：之前那几次
是 2 MB 的迷你转储，**坏对象所在的内存页根本不在里面**，所以「那个指针为什么是垃圾」查不到。
全量转储把整块用户态内存都带上了，坏的那一页就在里面。

---

## 2. 什么时候写、什么时候不写、怎么关

- **原生异常**（访问违例、堆破坏、栈溢出这类 SEH 异常）才写转储，包括第一机会（VEH）与顶层
  未处理异常过滤器两条路。同一个现场只写一份（重入旗）。
- **Rust panic 不写全量转储**：没有异常记录、没有出错线程上下文，写出来只有基础部分，不值那
  几百 MB。panic 仍然有文本报告。
- **单测环境不写**（`cargo test` 里跳过真正落盘那一步），否则一轮测试要写几十 GB。

保留策略与开关：

| 想要什么 | 怎么做 |
| --- | --- |
| 不要转储 | 设环境变量 `DSH_WALLPAPER_NO_CRASH_DUMP=1`（`true` / `yes` / `on` 同效，大小写无所谓）。文本报告不受影响。 |
| 转储放到别的盘 | 设 `DSH_WALLPAPER_CRASH_DUMP_DIR=D:\dsh-crash`（安装时读一次；目录会被创建）。 |
| 改保留份数 | 改 `crash_report.rs` 里的 `KEEP_DUMP_FILES`（默认 3），重新构建。 |

**保留最近 3 份**，清理发生在**应用启动时**（`prune_dumps`）：按文件写入时间排序，删掉最旧的，
只留 3 份。所以磁盘峰值是 4 份（3 份旧的 + 正在写的这一份），不会无限涨。清理只删
`crash-*.dmp`；目录里别的文件（你自己放的、别的工具的）**一个都不动**，即使你把转储目录指到
一个放着别的东西的目录也一样。

失败时你看到什么：

- **没有 `.dmp` 文件**，但 `logs\crash-*.txt` 还在 —— 报告里的 `[dump]` 段会写明为什么
  （`dump.error` + 有时 `dump.error.code` 是 `GetLastError` 的十六进制值）。常见原因：目录权限、
  磁盘空间、被关掉了、这次是 panic。
- 写失败会把**半截文件删掉**：不会留一个看起来像转储、实际读不出东西的文件。
- 写转储失败**绝不影响崩溃本身的处理**：异常照常交回系统（`EXCEPTION_CONTINUE_SEARCH`），
  WER 与系统错误提示照旧，进程不会因为写转储再死一次。

环境变量是**启动时**读的：改完要重启应用才生效。应用启动日志里会有一行说明转储目录就绪与否。

---

## 3. 怎么读这两样东西

### 读文本报告

直接打开 `logs\crash-*.txt`。它本身就是给不看调试器的人写的：异常码、地址、`模块名+偏移`、
调用栈、模块表、日志尾部都在里面。事件日志里的「异常偏移」（例如 `0x353d17`）就是报告里的
`exception.address` 减去所属模块基址 —— 报告已经算好，对着核即可。

### 读全量转储（cdb.exe）

用 **WinDbg 的 MSIX 包自带**的 `cdb.exe` 就行，**不要**去装 Windows SDK。它在这里：

```
C:\Program Files\WindowsApps\Microsoft.WinDbg_<版本>_x64__8wekyb3d8bbwe\amd64\cdb.exe
```

`WindowsApps` 默认不让你直接浏览（访问被拒是正常的，文件本身是可读可执行的）。要拿到确切
路径，在 PowerShell 里问一句：

```powershell
Get-ChildItem 'C:\Program Files\WindowsApps' -Directory |
  Where-Object Name -like 'Microsoft.WinDbg*' | Select-Object -ExpandProperty FullName
```

本机（2026-10-05）实测是
`C:\Program Files\WindowsApps\Microsoft.WinDbg_1.2606.22001.0_x64__8wekyb3d8bbwe\amd64\cdb.exe`。
没有 WinDbg 时用 `winget install Microsoft.WinDbg` 装它（比装 SDK 小得多）。

打开一份转储：

```
& 'C:\Program Files\WindowsApps\Microsoft.WinDbg_1.2606.22001.0_x64__8wekyb3d8bbwe\amd64\cdb.exe' `
  -z "$env:LOCALAPPDATA\com.dsh.wallpaper\crashes\crash-20251005-101112-345-c0000005.dmp" `
  -y "$env:LOCALAPPDATA\com.dsh.wallpaper\crashes" `
  -c ".symfix; .reload; !analyze -v; k; q"
```

- `-z` 指定转储；`-y` 指定符号目录（**这一项最关键**，见第 5 节）；
- `!analyze -v` 是崩溃分析（异常码、出错模块、出错指令、可能的原因）；
- `k` 是出错线程的调用栈；`~*k` 是所有线程的栈；`dq`/`dps` 看内存。

符号路径也可以不写 `-y`，进去之后用命令设：

```
.sympath C:\Users\<你>\AppData\Local\com.dsh.wallpaper\crashes
.reload
```

`.symfix` 会把微软公共符号服务器加进符号路径，Windows 自己的 DLL（ntdll 等）就能符号化；
应用自己的 **PDB 不在公共服务器上**，只能本地给 —— 这就是第 5 节那件事。

> 转储很大时（几百 MB），报告里的 `dump.size` 会写明它多大。读的时候 `!analyze -v` 可能要几十秒，
> 属正常。

### 读不出来的时候，先把「报告 + 事件日志」看完

文本报告不依赖符号也不依赖调试器，它已经把「谁崩的、崩在哪、现场日志的最后 40 行」写清楚了。
转储是补「内存里那一页为什么坏」，两者是互补关系，不是替代关系。

---

## 4. 读一份现场：建议顺序

1. `logs\crash-<时间戳>.txt`：看 `exception.code` / `exception.address` 的 `模块名+偏移`；
   看 `[dump]` 段落确认转储在哪、多大；看 `log.tail` 看崩溃前应用在做什么。
2. 打开 `crashes\crash-<同一时间戳>-<异常码>.dmp`，用 `!analyze -v` 与 `k`。
3. 用第 5 节的符号路径把地址变成函数名；对照报告里的模块基址核一遍。
4. 时间对齐：报告里 `time.local` 与 `time.utc` 都给了，应用日志是 UTC。
5. 如果事件日志里也有这条崩溃：事件日志的「异常偏移」应与报告里同一个数。

---

## 5. 符号（PDB）：没有它，全量转储也读不出东西

把地址变成函数名要三样东西对齐：**同一个二进制**、**同一次构建的 PDB**、以及调试器知道去
哪里找它。PDB 里带 CodeView 标识（GUID + 时间戳），二进制里也存着同一份；对不上时调试器会
直接说找不到符号 —— 这种时候换一个「看起来差不多」的 PDB 是没用的。

所以：

- **本机构建**：`cargo` 会把符号留在 `wallpaper\src-tauri\target\release\`：
  - `dsh_wallpaper.pdb`（完整版）
  - `dsh_wallpaper_lite.pdb`（Lite 版）
  - 发布脚本 `scripts/publish-local-nsis.ps1` 会把它复制成
    `dist\<版本>\dsh_wallpaper_<版本>.pdb`（Lite 版是 `dsh_wallpaper_lite_<版本>.pdb`），
    并打印它的 SHA-256。**那份带版本号的副本就是要留档的东西**（`dist\` 不进仓库，
    见 `.gitignore`；请自己把它和安装器一起存档）。
  - 不想复制就加 `-SkipSymbols`；找不到 PDB 时脚本只告警、不拦发布。
- **发布出去**：安装包本身**不带符号**（那会把包变大），所以每次发版要**额外**把同一次构建的
  PDB 作为附件留档。CI 那部分目前是「建议补丁」（见第 7 节），尚未落地。
- **已经丢了的**：0.2.0 / 0.3.0 / 0.4.0 那几次崩溃就是这种状态 —— 转储在，PDB 不在，
  **永远无法符号化**。这种损失不可逆，所以第 6 节那些「顺手留档」的做法值得坚持。

读的时候把 `-y` 指到 PDB **所在目录**（不是文件本身）：

```
cdb.exe -z <转储> -y "dist\0.4.7" -c ".reload; !analyze -v; k; q"
```

源码行号（`<文件>:<行>`）需要 PDB 里带行号信息。当前发布配置
（`wallpaper/src-tauri/Cargo.toml` 的 `[profile.release]` 只有 `strip = true`）实测已经产出
约 10 MB 的 PDB，函数名与模块名够用；`strip = true` 只影响打进可执行文件的调试信息，
**不会让 PDB 消失**。如果在某个函数里还需要逐行信息，可以把它改成 `strip = true` +
`debug = 1`（本机实测：小程序的 PDB 从 1265664 字节变成 1273856 字节，
也就是几乎不变；安装包本身不带符号，不受影响）。这是可选升级，不在本次改动范围内。

---

## 6. 备选：WER 的 LocalDumps（机器级，需要管理员）

应用自己写转储是**首选**：不需要管理员、不需要改注册表、保留策略与开关都在应用里。
WER 的 `LocalDumps` 可以作为补充（它抓的是系统级的未处理异常，多一份保险）。要点：

- 它是 **HKLM** 下的机器级设置：
  `HKLM\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps`（全局）或
  `...\LocalDumps\dsh-wallpaper.exe`（只针对这一个程序）；
- `DumpFolder`（REG_EXPAND_SZ）、`DumpType=2`（**全量**；默认是 1 = 迷你）、`DumpCount`（份数上限）；
- **要管理员权限才能写**。没有管理员时请把下面的命令交给管理员执行。

**本机现状（2026-10-05 只读检查）**：

- `HKLM\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps` **存在**，里面已有
  若干**别的软件**的按程序子键（ASUS Armoury Crate、GlideX、NVIDIA 等，多为 `DumpType=1`
  的迷你转储）；
- **没有 `dsh-wallpaper.exe` 这一项**；
- `HKCU\...\LocalDumps` **不存在**（用户级通常没有这一项，也不用去建它）；
- 当前会话**不是管理员**，因此本次没有写任何注册表。

只读检查现状：

```powershell
Get-ChildItem 'HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps' -ErrorAction SilentlyContinue |
  Select-Object PSChildName
Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps' -ErrorAction SilentlyContinue
Test-Path 'HKCU:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps'
```

**供管理员直接粘贴**（会同时在系统默认位置 `%LOCALAPPDATA%\CrashDumps` 之外再存一份全量转储；
不想自己管清理就把 `DumpCount` 调小）：

```powershell
# 需要「以管理员身份运行」的 PowerShell
$dumps = Join-Path $env:LOCALAPPDATA 'dsh-wallpaper-crashes'
New-Item -ItemType Directory -Path $dumps -Force | Out-Null
$key = 'HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\dsh-wallpaper.exe'
New-Item -Path $key -Force | Out-Null
New-ItemProperty -Path $key -Name 'DumpFolder' -PropertyType ExpandString -Value $dumps -Force | Out-Null
New-ItemProperty -Path $key -Name 'DumpType'   -PropertyType DWord        -Value 2      -Force | Out-Null
New-ItemProperty -Path $key -Name 'DumpCount'  -PropertyType DWord        -Value 3      -Force | Out-Null
Get-ItemProperty -Path $key
```

撤销：

```powershell
# 同样需要管理员
Remove-Item -Path 'HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\dsh-wallpaper.exe' -Recurse -Force
```

Lite 版要做同样的事，把上面的 `dsh-wallpaper.exe` 换成 `dsh-wallpaper-lite.exe`（可执行文件
基名，不带路径）。关于 `%LOCALAPPDATA%`：WER 展开 `DumpFolder` 时用的是**崩溃进程自己**的
账户环境，所以普通用户跑的壁纸会落到那个用户的目录里 —— 但这一点**本次没有实测**（写 HKLM
需要管理员，本会话没有提权）。不想赌就把 `$dumps` 换成一个所有账户都写得进去的绝对路径，
例如 `C:\ProgramData\dsh-wallpaper-crashes`（记得给 Users 组写权限）。

---

## 7. PDB 留档：本机脚本已做，CI 还是建议

- **本机**（已落地）：`scripts/publish-local-nsis.ps1` 构建完之后把
  `target\release\dsh_wallpaper.pdb`（Lite 是 `dsh_wallpaper_lite.pdb`）复制到
  `dist\<版本>\` 下并打印 SHA-256；找不到只告警；`-SkipSymbols` 可跳过。
- **CI**（**尚未落地**，以下是建议补丁，请有发布权限的人评估后再加）：

  `package.yml` 的 `Collect package artifacts` 里已经复制安装器，加一行把 PDB 也收进来：

  ```powershell
  # 与安装器同批的符号：不给它留档，日后崩溃就只能看地址
  $pdb = Get-ChildItem -LiteralPath "$env:GITHUB_WORKSPACE\wallpaper\src-tauri\target\release" -Filter 'dsh_wallpaper*.pdb' -File | Select-Object -First 1
  if (-not $pdb) { throw '未找到符号文件 dsh_wallpaper*.pdb。' }
  $symbolVersion = (Get-Content -LiteralPath "$env:GITHUB_WORKSPACE\wallpaper\src-tauri\Cargo.toml" -Raw | Select-String -Pattern 'version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
  Copy-Item -LiteralPath $pdb.FullName -Destination "$env:GITHUB_WORKSPACE\ci-artifacts\nsis\$($pdb.BaseName)_$symbolVersion.pdb" -Force
  ```

  然后给 upload-artifact 那一步加一条（名字不与现有资产冲突：

  ```yaml
  - name: Upload the symbols
    uses: actions/upload-artifact@v4
    with:
      name: dsh-wallpaper-${{ matrix.edition }}-symbols
      path: ci-artifacts/nsis/*.pdb
      if-no-files-found: error
      retention-days: 90
  ```

  `package-full.yml` 走的是 MSIX 那套，要留档就把同样的两段加在它的
  `Verify and collect the full MSIX test package` 之后。**注意**：这两个工作流目前**标签触发已被
  冻结**（文件头写了 FREEZE），要动它们请先确认发布路径的现状。

---

## 8. 常见坑

- **报告在 `logs\`，转储在 `crashes\`**：只在其中一个目录里找，就会以为「什么都没留下」。
- **PDB 必须同批**：换了版本、重新构建过、换了机器编译，都不能拿来符号化这一份转储。
- **`dist\` 不进仓库**：PDB 与安装器都是构建产物，请自己归档（网盘 / Release 附件）。
- **`%LOCALAPPDATA%\CrashDumps` 里的迷你转储**：那是 WER 抓的，1.9 MB 上下，**没有坏页所在
  的内存**；能用的是应用自己写的 `crashes\crash-*.dmp`。
- **环境变量在启动时读**：改了要重启。
- **应用日志会轮换**：超过 40000 字节就删档重开，所以「崩溃前的日志」要靠报告自带的尾部。
- **本机没有管理员**：上面第 6 节的管理员命令要么请管理员跑，要么就只用应用自写的转储
  （推荐那条路，它本来就不需要管理员）。
