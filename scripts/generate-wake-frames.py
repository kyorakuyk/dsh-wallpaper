"""按原睡觉图构图，生成连续苏醒帧序列：睡脸 → 睁眼 → 起身 → 打哈欠。

方法：每帧以 assets/personas/睡觉.jpg 为 image 底图调用 images/edits，
prompt 强调保持构图/背景/角色/视角/画风完全一致，仅改变动作。

输出（assets/personas/wake-frames/）：
  frame-1-sleep.jpg   睡脸（闭眼熟睡，构图同原图）
  frame-2-eyes.jpg    睁眼（眼睛睁开，仍躺着）
  frame-3-situp.jpg   起身（坐起来）
  frame-4-yawn.jpg    打哈欠（张嘴伸懒腰）
"""
import argparse
import base64
import json
import os
import sys
import time
import winreg
from pathlib import Path

import requests

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "assets" / "personas" / "睡觉.jpg"
OUT_DIR = ROOT / "assets" / "personas" / "wake-frames"
IMG_API_BASE = "https://img-api.apinebula.ai/v1"


def get_env(name: str) -> str:
    try:
        reg = winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment")
        v, _ = winreg.QueryValueEx(reg, name)
        if v:
            return v
    except OSError:
        pass
    return os.environ.get(name, "")


# 每帧：文件名 + prompt（构图一致性是核心要求）
# 注意：frame-1 要求"元素相同但重新绘制"——保留原图场景元素（床/熟睡少女/夜晚卧室），
# 但作为全新绘制（画风参考蓝色成年立绘的二次元风格），避免与原图像素级一致（版权规避）。
FRAMES = [
    (
        "frame-1-sleep.jpg",
        "全新绘制一张二次元插画：同样的场景元素——夜晚卧室里，少女在床上熟睡，"
        "有床铺、枕头、被子、柔和的月光，角色闭眼安睡。"
        "重新绘制、重新上色，画风为精致二次元动漫插画风格（参考蓝色鲸鱼娘立绘的画风），"
        "不要复制任何现有图片的像素，构图与元素保留但画面是全新的创作。",
    ),
    (
        "frame-2-eyes.jpg",
        "保持参考图的构图、场景元素与画风完全一致（同一张床、同一位熟睡的少女、同样的夜晚卧室），"
        "但重新绘制：角色仍然躺着，眼睛睁开了，刚刚醒来，表情朦胧惺忪。"
        "画风为精致二次元插画，全新上色，不要复制像素。",
    ),
    (
        "frame-3-situp.jpg",
        "保持参考图的构图、场景元素与画风完全一致（同一张床、同一位少女、同样的夜晚卧室），"
        "但重新绘制：角色从床上坐起来，半坐半靠，头发微乱，刚醒来的样子。"
        "画风为精致二次元插画，全新上色，不要复制像素。",
    ),
    (
        "frame-4-yawn.jpg",
        "保持参考图的构图、场景元素与画风完全一致（同一张床、同一位少女、同样的夜晚卧室），"
        "但重新绘制：角色坐直身体，双手抬起伸懒腰，张大嘴打哈欠，刚醒来的慵懒样子。"
        "画风为精致二次元插画，全新上色，不要复制像素。",
    ),
]


def edit_frame(model: str, prompt: str, out: Path, key: str, url: str, size: str, base_img: Path) -> bool:
    files = [
        ("image", (base_img.name, base_img.read_bytes(), "image/jpeg")),
    ]
    data = [
        ("model", (None, model)),
        ("prompt", (None, prompt)),
        ("n", (None, "1")),
        ("size", (None, size)),
        ("response_format", (None, "b64_json")),
    ]
    headers = {"Authorization": f"Bearer {key}"}
    print(f"  生成 {out.name} ...")
    try:
        r = requests.post(f"{url}/images/edits", headers=headers, data=data, files=files, timeout=300)
    except Exception as e:
        print(f"    请求异常: {e}", file=sys.stderr)
        return False
    if r.status_code != 200:
        print(f"    HTTP {r.status_code}: {r.text[:300]}", file=sys.stderr)
        return False
    data_json = r.json()
    for item in data_json.get("data", []):
        b64 = item.get("b64_json")
        if b64:
            out.write_bytes(base64.b64decode(b64))
            print(f"    已保存: {out} ({out.stat().st_size:,} bytes)")
            return True
        url_img = item.get("url")
        if url_img:
            img = requests.get(url_img, timeout=120)
            out.write_bytes(img.content)
            print(f"    已保存(URL): {out}")
            return True
    print(f"    响应无图片: {json.dumps(data_json)[:300]}", file=sys.stderr)
    return False


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=get_env("CODEX_IMAGE_MODEL") or "gpt-image-2")
    ap.add_argument("--size", default="1536x1024", help="输出尺寸，默认匹配原图 3:2")
    ap.add_argument("--frames", default="1,2,3,4", help="要生成的帧，逗号分隔")
    args = ap.parse_args()

    key = get_env("CODEX_API_KEY")
    if not key:
        print("缺少 CODEX_API_KEY", file=sys.stderr)
        sys.exit(2)
    if not SRC.exists():
        print(f"缺少底图: {SRC}", file=sys.stderr)
        sys.exit(2)

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    wanted = {int(x) for x in args.frames.split(",")}
    ok = 0
    # 帧 1 底图 = 原睡觉图（元素参考）；后续帧底图 = 上一帧输出（保证序列连贯）。
    # 若上一帧已存在（部分生成场景），直接复用它，保证全局连贯。
    base = SRC
    prev = OUT_DIR / FRAMES[0][0]
    if prev.exists():
        base = prev
    for idx, (name, prompt) in enumerate(FRAMES, start=1):
        if idx not in wanted:
            if (OUT_DIR / name).exists():
                base = OUT_DIR / name  # 即使本次不生成，也推进底图
            continue
        if edit_frame(args.model, prompt, OUT_DIR / name, key, IMG_API_BASE, args.size, base):
            ok += 1
            base = OUT_DIR / name  # 后续帧基于刚生成的帧
        time.sleep(1)
    print(f"完成: {ok}/{len(wanted)} 帧成功 → {OUT_DIR}")


if __name__ == "__main__":
    main()
