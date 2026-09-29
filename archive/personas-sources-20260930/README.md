# 归档：素材（2026-09-30）

用户决定：**仓库里只留产品真正在用的东西**；不用的"挪个窝存"，不删。这里保存的就是那些从主体挪出来的
素材，内部路径与原位置一一对应（`git mv`，历史可查，恢复就是挪回去）。

## 归档内容

| 归档内容 | 原位置 | 为什么判定"没在用" |
| --- | --- | --- |
| `assets/personas/**`（除下面保留的四个） | 同左 | 唯一读 `assets/` 的是 `scripts/publish-local-msix.ps1`，它只读四个文件；其余是原始件与工作副本 |
| `wallpaper/public/personas/wake-frames/variant-v4/**`（8 张） | 同左 | 代码、构建、打包**零引用** |
| `wallpaper/public/personas/wake-frames/frame-{1-sleep,2-eyes,3-situp,4-yawn}.jpg` | 同左 | 零引用；真正在播的是 `variant-anima/` 那四张 |

## 原地保留（**在用，不要动**）

* `assets/personas/{蓝幼,蓝熟,黑红幼,黑红熟}.png` —— `publish-local-msix.ps1` 打包时用它们核对包里的四张立绘；
* `wallpaper/public/personas/portrait-{blue,black}-{child,adult}.png` —— 产品里的**四张立绘**；
* `wallpaper/public/personas/wake-frames/variant-anima/{sleep.png,frame-2-eyes.png,frame-3-yawn.webp,frame-4-awake.webp}` —— 产品里的**四张帧动画**（`WakeScene.tsx` 的 `DEFAULT_WAKE_FRAMES`）；
* `wallpaper/public/personas/{sleep.jpg,wake.jpg}` —— 睡眠与苏醒两张场景图。