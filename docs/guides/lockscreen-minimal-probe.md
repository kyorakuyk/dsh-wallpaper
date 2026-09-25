# 最小锁屏诊断包

此诊断包用于判断 `TrySetLockScreenImageAsync` 的拒绝是否由 dsh-wallpaper
本身引起。它不启动 Tauri、WebView、托盘、恢复事务或项目配置；只运行一次
官方 WinRT 调用。

## 它会做什么

1. 读取 `IsSupported` 和当前锁屏图片 URI。
2. 若当前图片是本地文件，复制一份到诊断目录作为独立备份。
3. 读取包内 `sleep.png`。
4. 调用一次 `TrySetLockScreenImageAsync`，并记录布尔返回值或 HRESULT。

报告位置：

`%LOCALAPPDATA%\\Packages\\com.dsh.wallpaper.lockscreenprobe_*\\LocalCache\\Local\\DSHWallpaperLockScreenProbe\\report.json`

该报告只包含 API 状态和本地路径；不包含 Cookie、聊天内容或凭据。

## 验收判断

- `trySetReturned: true`：主应用的调用上下文或事务逻辑有问题。
- `trySetReturned: false`：该 Windows 环境拒绝此官方 API，问题不在 Tauri/WebView。
- 存在 `error`：根据其中的 WinRT/HRESULT 继续定位。
