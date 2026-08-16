"""以 variant-v4/frame-1-sleep.jpg 为底图，edits 生成睁眼/起身/打哈欠（构图连续、风格固定）。

人设已修正（有腿 + 鲸鱼尾巴只是装饰），底图 v4 睡脸。
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
V4_DIR = ROOT / "assets" / "personas" / "wake-frames" / "variant-v4"
BASE = V4_DIR / "frame-1-sleep.jpg"
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


STYLE = (
    "保持参考图完全相同的场景、人物、构图、光线与画风"
    "（深夜程序员工作室，蓝紫色柔顺长发的鲸鱼娘女仆少女，"
    "深蓝白哥特女仆裙，有双腿，深蓝色星光纹理的鲸鱼尾巴只是装饰垂在椅侧，"
    "长发非常柔顺丝滑光泽细腻不凌乱，smooth silky hair，"
    "暖黄台灯与冷蓝夜景的电影感光影，低饱和深色）。"
)

FRAMES = [
    (
        "frame-2-eyes.jpg",
        STYLE + "角色仍趴在办公桌上，但双眼明显睁开、眼神清澈明亮，"
        "睫毛清晰瞳孔有神，刚醒来的朦胧清醒感，眼睛睁开是画面最突出的变化。",
    ),
    (
        "frame-3-situp.jpg",
        STYLE + "角色从桌上撑起上半身坐起来，半坐半靠，睡眼惺忪，"
        "双手撑桌，双腿坐在椅子上，鲸鱼尾巴仍垂在椅侧。",
    ),
    (
        "frame-4-yawn.jpg",
        STYLE + "角色坐直身体，双手抬起伸懒腰，张大嘴打哈欠，"
        "刚醒来的慵懒样子，双腿坐在椅子上，鲸鱼尾巴仍垂在椅侧。",
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
    ap.add_argument("--size", default="1659x948")
    ap.add_argument("--frames", default="2,3,4")
    args = ap.parse_args()

    key = get_env("CODEX_API_KEY")
    if not key:
        print("缺少 CODEX_API_KEY", file=sys.stderr)
        sys.exit(2)
    if not BASE.exists():
        print(f"缺少底图: {BASE}", file=sys.stderr)
        sys.exit(2)

    V4_DIR.mkdir(parents=True, exist_ok=True)
    wanted = {int(x) for x in args.frames.split(",")}
    ok = 0
    for idx, (name, prompt) in enumerate(FRAMES, start=2):
        if idx not in wanted:
            continue
        if edit_frame(args.model, prompt, V4_DIR / name, key, args.size):
            ok += 1
        time.sleep(1)
    print(f"完成: {ok}/{len(wanted)} 帧 → {V4_DIR}")


if __name__ == "__main__":
    main()
