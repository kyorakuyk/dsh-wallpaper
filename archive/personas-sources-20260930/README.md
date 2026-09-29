# 归档：素材（2026-09-30）

用户决定：**仓库里只留产品真正看得见的东西**——四张立绘 + 四张帧动画；其余"挪个窝存"，不删。
这里保存的是从仓库主体挪出来的素材，路径与原位置一一对应。

| 归档内容 | 原位置 | 为什么 |
| --- | --- | --- |
| wallpaper/public/personas/wake-frames/variant-v4/** | 同左 | 代码与构建**零引用**（只有一份计划文档提过它） |
| wallpaper/public/personas/wake-frames/frame-{1-sleep,2-eyes,3-situp,4-yawn}.jpg | 同左 | 零引用；真正在播的是 ariant-anima/ 里那四张（WakeScene.tsx 的 DEFAULT_WAKE_FRAMES） |

**产品仍在用的（留在原位，不要动）**：personas/portrait-{blue,black}-{child,adult}.png（四张立绘）、
personas/wake-frames/variant-anima/{sleep.png,frame-2-eyes.png,frame-3-yawn.webp,frame-4-awake.webp}（四张帧动画）、
以及 personas/{sleep.jpg,wake.jpg}。

## 还没挪的：`assets/`（约 81.7 MB 的原始素材）

它**不能直接挪**：`scripts/publish-local-msix.ps1` 有 9 处读它（打包时核对立绘）。
要挪就得**同时把那 9 处指向归档路径或 `wallpaper/public`**，改完跑一次打包验证——那一步单独做。

恢复办法：`git mv` 回原路径即可（本次全部用 `git mv`，历史都在）。