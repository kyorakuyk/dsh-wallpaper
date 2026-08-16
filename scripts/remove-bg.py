"""白底立绘抠图 v2：背景基准色采样 + 收紧阈值 + 肤色保护 + 窄过渡带

v1 问题修复：
- 背景基准：纯白(255) → 四角均值（实际背景是暖灰，如 248.8,246.5,247.3）
- 阈值：28 → TOL（按到背景基准距离），避免误删浅色裙子/高光
- 过渡带：[28,70] → [TOL, TOL+10]，不再把皮肤高光半透明化
- 肤色保护：暖色像素（R 明显 > G > B）强制保留，防止皮肤被抠
"""
import sys
from pathlib import Path
from collections import deque
import numpy as np
from PIL import Image, ImageFilter

SRC_DIR = Path(r"C:\DeepSeekHarness\plugins\dsh-wallpaper\assets\personas")
OUT_DIR = Path(r"C:\DeepSeekHarness\plugins\dsh-wallpaper\wallpaper\public\personas")

TASKS = {
    "蓝色幼年.jpg": "portrait-blue-child.png",
    "蓝色成年.jpg": "portrait-blue-adult.png",
    "黑红幼年.jpg": "portrait-black-child.png",
    "黑红成年.jpg": "portrait-black-adult.png",
}

# 到背景基准的距离阈值：背景本身约 0-12，人物浅色约 20+
TOL = 16
# 边缘羽化过渡带宽度
BAND = 10
# 肤色保护：R>=200 且 (R-G)>=10 且 (R-B)>=16 的暖色像素永不删除
SKIN_R_MIN = 195
SKIN_RG_MIN = 10
SKIN_RB_MIN = 16


def background_ref(rgb: np.ndarray) -> np.ndarray:
    """四角区域均值作为背景基准色"""
    h, w = rgb.shape[:2]
    corners = np.concatenate([
        rgb[:10, :10].reshape(-1, 3),
        rgb[:10, -10:].reshape(-1, 3),
        rgb[-10:, :10].reshape(-1, 3),
        rgb[-10:, -10:].reshape(-1, 3),
    ])
    return corners.mean(axis=0)


def dist_to_ref(rgb: np.ndarray, ref: np.ndarray) -> np.ndarray:
    return np.sqrt(((rgb.astype(np.float32) - ref.astype(np.float32)) ** 2).sum(axis=-1))


def remove_white_background(src: Path, dst: Path) -> None:
    im = Image.open(src).convert("RGBA")
    arr = np.array(im)
    rgb = arr[..., :3].astype(np.float32)
    ref = background_ref(rgb)
    dist = dist_to_ref(rgb, ref)

    # 1) 背景候选：距基准 <= TOL
    bg_mask = dist <= TOL

    # 2) 肤色保护：暖色像素强制排除出背景（皮肤高光虽接近白，但偏暖）
    R, G, B = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    skin = (R >= SKIN_R_MIN) & ((R - G) >= SKIN_RG_MIN) & ((R - B) >= SKIN_RB_MIN)
    bg_mask = bg_mask & ~skin

    # 3) 连通域：只删「从边缘可达」的背景（内部白色/高光保留）
    h, w = bg_mask.shape
    visited = np.zeros_like(bg_mask)
    q = deque()
    for x in range(w):
        for y in (0, h - 1):
            if bg_mask[y, x]:
                q.append((y, x)); visited[y, x] = True
    for y in range(h):
        for x in (0, w - 1):
            if bg_mask[y, x]:
                q.append((y, x)); visited[y, x] = True
    while q:
        y, x = q.popleft()
        for dy, dx in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            ny, nx = y + dy, x + dx
            if 0 <= ny < h and 0 <= nx < w and not visited[ny, nx] and bg_mask[ny, nx]:
                visited[ny, nx] = True
                q.append((ny, nx))

    alpha = np.where(visited, 0, 255).astype(np.uint8)

    # 4) 窄过渡带 [TOL, TOL+BAND]：仅对「紧邻已删区域」的像素羽化，避免远处浅色被压半透明
    band = (dist > TOL) & (dist <= TOL + BAND)
    # 膨胀已删区域 1px，只处理边缘一圈
    dilated = visited.copy()
    for y in range(1, h - 1):
        for x in range(1, w - 1):
            if visited[y, x]:
                dilated[y - 1:y + 2, x - 1:x + 2] = True
    band &= dilated
    if band.any():
        t = (dist[band] - TOL) / BAND
        alpha[band] = (140 + (255 - 140) * t).astype(np.uint8)

    arr[..., 3] = alpha
    out = Image.fromarray(arr)
    out.putalpha(out.getchannel("A").filter(ImageFilter.GaussianBlur(0.6)))
    out.save(dst, "PNG")

    # 统计报告
    deleted = (alpha < 16).sum()
    semi = ((alpha >= 16) & (alpha < 245)).sum()
    total = alpha.size
    skin_deleted = ((alpha < 16) & skin).sum()
    print(f"OK: {src.name} -> {dst.name}  ({out.size[0]}x{out.size[1]})")
    print(f"    背景基准色: {ref.round(1)} | 透明 {deleted/total:.1%} 半透明 {semi/total:.1%} 保留 {100-deleted/total*100:.1f}%")
    print(f"    被删像素中肤色占比: {skin_deleted/max(deleted,1)*100:.2f}%")


if __name__ == "__main__":
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for src_name, dst_name in TASKS.items():
        src = SRC_DIR / src_name
        if not src.exists():
            print(f"SKIP: {src_name} 不存在")
            continue
        remove_white_background(src, OUT_DIR / dst_name)
    print("完成")
