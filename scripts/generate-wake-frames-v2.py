"""以 frame-1-sleep-v3.jpg 为底图，生成配套苏醒帧（睁眼/起身/打哈欠），构图连续。

旧帧全部保留（留档），新帧命名为 *-v2.jpg。
"""
import argparse
import base64
import os
import sys
import time
import winreg
from pathlib import Path

import requests

ROOT = Path(__file__).resolve().parents[1]
FRAME_DIR = ROOT / "assets" / "personas" / "wake-frames"
BASE = FRAME_DIR / "frame-1-sleep-v3.jpg"
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


# 与 v3 同构图的连续帧：睁眼 → 起身 → 打哈欠
FRAMES = [
    (
        "frame-2-eyes-v2.jpg",
        "保持参考图完全相同的构图、场景、人物、背景、光线与画风（深夜程序员工作室，"
        "蓝紫色柔顺长发的鲸鱼娘人鱼女仆趴在办公桌上），但重新绘制："
        "角色仍然趴在桌上，但双眼明显睁开、眼神清晰明亮，透出刚刚醒来的朦胧与清醒感，"
        "眼睛变化是整幅画面最突出的改变，睫毛清晰，瞳孔有神，"
        "长发依然柔顺丝滑、光泽细腻、整齐铺满桌面，smooth silky hair, glossy hair。"
        "暖黄台灯与冷蓝夜景的电影感光影不变。",
    ),
    (
        "frame-3-situp-v2.jpg",
        "保持参考图完全相同的场景与画风（深夜程序员工作室，蓝紫色柔顺长发的鲸鱼娘人鱼女仆），"
        "但重新绘制：角色从桌上撑起上半身坐起来，半坐半靠，睡眼惺忪，"
        "长发柔顺丝滑地垂落，身后星光纹理的深蓝色人鱼尾巴仍垂在椅侧，"
        "smooth silky hair, glossy hair。暖黄台灯与冷蓝夜景的电影感光影不变。",
    ),
    (
        "frame-4-yawn-v2.jpg",
        "保持参考图完全相同的场景与画风（深夜程序员工作室，蓝紫色柔顺长发的鲸鱼娘人鱼女仆），"
        "但重新绘制：角色坐直身体，双手抬起伸懒腰，张大嘴打哈欠，刚醒来的慵懒样子，"
        "长发柔顺丝滑地垂落，smooth silky hair, glossy hair。"
        "暖黄台灯与冷蓝夜景的电影感光影不变。",
    ),
]


def edit_frame(model: str, prompt: str, out: Path, key: str, size: str) -> bool:
    files = [("image", (BASE.name, BASE.read_bytes(), "image/jpeg"))]
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
        r = requests.post(f"{IMG_API_BASE}/images/edits", headers=headers, data=data, files=files, timeout=300)
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
        u = item.get("url")
        if u:
            img = requests.get(u, timeout=120)
            out.write_bytes(img.content)
            print(f"    已保存(URL): {out}")
            return True
    print(f"    响应无图片: {str(data_json)[:300]}", file=sys.stderr)
    return False


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=get_env("CODEX_IMAGE_MODEL") or "gpt-image-2")
    ap.add_argument("--size", default="1659x948", help="匹配 v3 底图尺寸")
    ap.add_argument("--frames", default="2,3,4", help="要生成的帧序号")
    args = ap.parse_args()

    key = get_env("CODEX_API_KEY")
    if not key:
        print("缺少 CODEX_API_KEY", file=sys.stderr)
        sys.exit(2)
    if not BASE.exists():
        print(f"缺少底图: {BASE}", file=sys.stderr)
        sys.exit(2)

    wanted = {int(x) for x in args.frames.split(",")}
    ok = 0
    for idx, (name, prompt) in enumerate(FRAMES, start=2):
        if idx not in wanted:
            continue
        if edit_frame(args.model, prompt, FRAME_DIR / name, key, args.size):
            ok += 1
        time.sleep(1)
    print(f"完成: {ok}/{len(wanted)} 帧 → {FRAME_DIR}")


if __name__ == "__main__":
    main()
