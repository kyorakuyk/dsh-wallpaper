<#
.SYNOPSIS
  只读诊断：壁纸读不到 bridge token 时，看它到底卡在"安全形状"的哪一条。

.DESCRIPTION
  用在那台报 `DSH bridge token 文件不安全` 的机器上。**只读**，不修改、不删除任何东西。

    pwsh -NoProfile -ExecutionPolicy Bypass -File .\bridge-token-acl.ps1

  背景：壁纸读令牌时会要求文件同时满足七条（见 wallpaper/src-tauri/src/chat.rs 的
  `bridge_token_acl_is_private`）：所有者是当前用户、DACL 存在且**已保护**（不继承）、
  **恰好一条** ACE、是允许型、无继承标志、权限是**完全控制**、且授予的是当前用户。

  这个脚本把其中能读到的都打出来，并顺带打印各档案里装的桥版本 —— 因为写这个文件的正是桥
  （`dsh-wallpaper-bridge`），版本不对（或那不是我们发布的那一份）会让形状对不上。
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Continue'

$home_ = if ($env:DSH_HOME) { $env:DSH_HOME } else { Join-Path $env:USERPROFILE '.dsh' }
$token = Join-Path $home_ 'wallpaper\bridge-token'
$profiles = Join-Path $home_ 'profiles'

Write-Host "== 1) 令牌文件 =="
Write-Host ("  路径      : " + $token)
if (-not (Test-Path -LiteralPath $token)) {
  Write-Host "  不存在 —— 这说明是另一条错误（“未找到 … 请先安装并启动 dsh-wallpaper-bridge”），"
  Write-Host "  而不是 ACL 那条。先确认桥装了、宿主起来了。"
} else {
  $item = Get-Item -LiteralPath $token -Force
  Write-Host ("  大小      : " + $item.Length + " 字节（上限 512）")
  Write-Host ("  属性      : " + $item.Attributes + "（不该有 ReparsePoint/Directory）")
  try {
    $acl = Get-Acl -LiteralPath $token
    Write-Host ("  所有者    : " + $acl.Owner + "   （要求 = 当前用户 " + [System.Security.Principal.WindowsIdentity]::GetCurrent().Name + "）")
    Write-Host ("  是否受保护: " + $acl.AreAccessRulesProtected + "   （要求 = True，即不继承父目录）")
    Write-Host ("  ACE 数量  : " + $acl.Access.Count + "   （要求 = 1）")
    foreach ($ace in $acl.Access) {
      Write-Host ("    - " + $ace.IdentityReference + "  " + $ace.AccessControlType + "  " + $ace.FileSystemRights + "  inherited=" + $ace.IsInherited)
    }
  } catch {
    Write-Host ("  读 ACL 失败: " + $_.Exception.Message)
  }
  Write-Host "  --- icacls 原样（便于对比） ---"
  icacls $token 2>&1 | ForEach-Object { Write-Host ("    " + $_) }
  Write-Host "  --- 所在卷的文件系统（FAT/exFAT 没有真正的 ACL，会被判不安全） ---"
  $root = (Split-Path -Qualifier $token) + '\'
  fsutil fsinfo volumeinfo $root 2>&1 | Select-String -Pattern 'File System Name|文件系统名' | ForEach-Object { Write-Host ("    " + $_.Line.Trim()) }
}

Write-Host ""
Write-Host "== 2) 各档案里装的桥版本（写这个文件的就是它） =="
if (Test-Path -LiteralPath $profiles) {
  Get-ChildItem -LiteralPath $profiles -Directory | ForEach-Object {
    $pkg = Join-Path $_.FullName 'node_modules\dsh-wallpaper-bridge\package.json'
    if (Test-Path -LiteralPath $pkg) {
      $version = (Get-Content -LiteralPath $pkg -Raw | ConvertFrom-Json).version
      Write-Host ("  " + $_.Name.PadRight(10) + " " + $version + "   （发布版应为 0.1.5）")
    } else {
      Write-Host ("  " + $_.Name.PadRight(10) + " (未安装桥)")
    }
  }
} else {
  Write-Host "  没有 profiles 目录"
}

Write-Host ""
Write-Host "== 3) 宿主里跑的是哪一份桥 =="
$tokenText = if (Test-Path -LiteralPath $token) { (Get-Content -LiteralPath $token -Raw).Trim() } else { '' }
if ($tokenText) {
  foreach ($port in 3080, 3099, 19387) {
    try {
      $status = Invoke-WebRequest -Uri "http://127.0.0.1:$port/api/wallpaper/v1/status" -Headers @{ Authorization = "Bearer $tokenText" } -TimeoutSec 3 -UseBasicParsing
      $json = $status.Content | ConvertFrom-Json
      Write-Host ("  端口 " + $port + "：bridgeVersion=" + $json.bridgeVersion + "  bridgeBuild=" + $json.bridgeBuild)
    } catch {
      Write-Host ("  端口 " + $port + "：没有应答")
    }
  }
} else {
  Write-Host "  读不到令牌内容，跳过"
}

Write-Host ""
Write-Host "== 4) 怎么修（按可能性排序） =="
Write-Host "  a) 删掉那个令牌文件再让桥重写：关掉壁纸与宿主，删除上面那个路径，然后重启 DSH 宿主与壁纸。"
Write-Host "     它由桥按"当前用户 + 完全控制 + 不继承"重新创建，多数情况这一下就好。"
Write-Host "  b) 若版本不是 0.1.5：dsh plugin --profile web add dsh-wallpaper-bridge@0.1.5"
Write-Host "  c) 若所在卷是 FAT/exFAT/网络盘：ACL 无从谈起，把 DSH_HOME 放回 NTFS 本地盘。"
Write-Host "  d) 若所有者在管理员组：那个文件是提权进程写的，删掉重写（见 a）。"
