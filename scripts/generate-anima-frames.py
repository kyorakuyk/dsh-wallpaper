"""以 variant-anima/sleep.png 为底图，生成配套苏醒帧（睁眼/起身/打哈欠）。

着重强调：场景完全一致、风格完全一致（同一深夜场景、同一角色、同一光影）。
输出到 variant-anima/：frame-2-eyes.png / frame-3-situp.png / frame-4-yawn.png
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
ANIMA_DIR = ROOT / "assets" / "personas" / "wake-frames" / "variant-anima"
BASE = ANIMA_DIR / "sleep.png"
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


# 一致性强化前缀：所有帧共用
CONSISTENCY = (
    "严格保持参考图完全相同的场景、角色、构图、视角、光线与整体画风，"
    "只允许角色的动作和表情变化，其余元素（背景、灯光、桌面物品、服装、发丝走向）"
    "必须与参考图完全一致，不可重新布置场景、不可改变视角、不可改变画风。"
    "same scene, same character, same composition, same lighting, same art style, "
    "only change the character's pose and expression."
)

FRAMES = [
    (
        "frame-2-eyes.png",
        CONSISTENCY
        + "参考图中的角色仍然保持原有的姿势（趴着/坐着的位置不变），"
        "但双眼明显睁开，眼神清澈明亮，睫毛清晰，瞳孔有神，刚醒来的朦胧与清醒感，"
        "眼睛睁开是画面最突出的变化，其余一切与参考图保持一致。",
    ),
    (
        "frame-3-situp.png",
        CONSISTENCY
        + "参考图中的角色从当前姿势缓缓坐起身来，撑起上半身，半坐半靠，"
        "睡眼惺忪，姿态自然，场景中的背景、灯光、桌面物品、服装细节均与参考图完全一致，"
        "只是角色的身体姿态从趴伏变为坐起。",
    ),
    (
        "frame-4-yawn.png",
        CONSISTENCY
        + "参考图中的角色坐直身体，抬起一只手轻轻掩在嘴前，手指自然并拢、"
        "指节柔和弯曲、五指清晰分明、比例正常绝不扭曲，anatomy correct hand，"
        "闭上一只眼睛，另一只眼半睁，轻轻打一个含蓄的小哈欠，动作幅度小，"
        "表情慵懒温柔，场景与参考图完全一致。",
    ),
]


def edit_frame(model: str, prompt: str, out: Path, key: str, size: str) -> bool:
    files = [("image", (BASE.name, BASE.read_bytes(), "image/png"))]
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
    ap.add_argument("--size", default="1920x1024", help="匹配 sleep.png 尺寸")
    ap.add_argument("--frames", default="2,3,4")
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
        if edit_frame(args.model, prompt, ANIMA_DIR / name, key, args.size):
            ok += 1
        time.sleep(1)
    print(f"完成: {ok}/{len(wanted)} 帧 → {ANIMA_DIR}")


if __name__ == "__main__":
    main()
