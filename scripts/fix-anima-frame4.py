"""优化 anima frame-4：更自然、更慵懒。以 variant-anima/frame-3-situp.png 为底图，
生成多候选供挑选（接口 n=1，逐次生成）。
"""
import argparse
import base64
import os
import sys
import winreg
from pathlib import Path

import requests

ROOT = Path(__file__).resolve().parents[1]
ANIMA_DIR = ROOT / "assets" / "personas" / "wake-frames" / "variant-anima"
BASE = ANIMA_DIR / "frame-3-situp.png"
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


CONSISTENCY = (
    "严格保持参考图完全相同的场景、角色、构图、视角、光线与整体画风，"
    "只允许角色的动作和表情变化，其余元素（背景、灯光、桌面物品、服装、发丝走向）"
    "必须与参考图完全一致。same scene, same character, same lighting, same art style."
)

# 慵懒版 frame-4：多候选
PROMPTS = [
    # 候选A：单手托腮，慵懒眯眼
    CONSISTENCY
    + "角色保持坐姿，身体微微放松后靠，一只手肘撑在桌面上，手背轻托着脸颊，"
    "另一只手自然垂下，眼睛半睁半闭，睡意朦胧，嘴角微微上扬，"
    "整个人散发着慵懒放松的晨起气息，动作自然不僵硬，手指自然放松不扭曲。"
    "relaxed, languid, sleepy morning mood, natural pose.",
    # 候选B：掩嘴轻呵，半眯眼
    CONSISTENCY
    + "角色保持坐姿，抬起一只手轻轻掩在嘴前，手背贴着嘴唇，手指自然并拢、"
    "指节柔和弯曲、五指清晰比例正常，anatomy correct hand，"
    "眼睛半眯着，慵懒地轻轻打了一个小小的哈欠，头微微低垂，"
    "神情放松迷糊，动作幅度小而自然，像还没完全醒来的样子。"
    "relaxed, drowsy, gentle yawn, natural fingers.",
    # 候选C：伸懒腰后放松，闭眼微笑
    CONSISTENCY
    + "角色保持坐姿，双手自然交叠放在腿上，身体微微后靠，"
    "眼睛半闭，轻轻打了一个慵懒的哈欠，表情放松温柔，"
    "像刚伸完懒腰后的松弛状态，头发柔顺垂落，整体氛围安静舒适，"
    "手指自然放松不扭曲，动作幅度小。relaxed, comfortable, drowsy morning."
]

CAND_NAMES = ["frame-4-yawn-candA.png", "frame-4-yawn-candB.png", "frame-4-yawn-candC.png"]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=get_env("CODEX_IMAGE_MODEL") or "gpt-image-2")
    ap.add_argument("--size", default="1920x1024")
    ap.add_argument("--which", default="A,B,C", help="要生成的候选")
    args = ap.parse_args()

    key = get_env("CODEX_API_KEY")
    if not key:
        print("缺少 CODEX_API_KEY", file=sys.stderr)
        sys.exit(2)
    if not BASE.exists():
        print(f"缺少底图: {BASE}", file=sys.stderr)
        sys.exit(2)

    wanted = {x.strip().upper() for x in args.which.split(",")}
    headers = {"Authorization": f"Bearer {key}"}
    saved = 0
    for label, prompt, name in zip("ABC", PROMPTS, CAND_NAMES):
        if label not in wanted:
            continue
        files = [("image", (BASE.name, BASE.read_bytes(), "image/png"))]
        data = [
            ("model", (None, args.model)),
            ("prompt", (None, prompt)),
            ("n", (None, "1")),
            ("size", (None, args.size)),
            ("response_format", (None, "b64_json")),
        ]
        print(f"生成候选{label} ...")
        try:
            r = requests.post(f"{IMG_API_BASE}/images/edits", headers=headers, data=data, files=files, timeout=300)
        except Exception as e:
            print(f"请求异常: {e}", file=sys.stderr)
            continue
        if r.status_code != 200:
            print(f"HTTP {r.status_code}: {r.text[:200]}", file=sys.stderr)
            continue
        out = ANIMA_DIR / name
        for item in r.json().get("data", []):
            b64 = item.get("b64_json")
            if b64:
                out.write_bytes(base64.b64decode(b64))
                print(f"候选{label}: {out} ({out.stat().st_size:,} bytes)")
                saved += 1
            elif item.get("url"):
                img = requests.get(item["url"], timeout=120)
                out.write_bytes(img.content)
                print(f"候选{label}(URL): {out}")
                saved += 1
    if not saved:
        print("全部失败", file=sys.stderr)
        sys.exit(5)
    print("完成")


if __name__ == "__main__":
    main()
