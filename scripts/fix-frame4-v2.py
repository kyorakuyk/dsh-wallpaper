"""修正 frame-4 v2：以 variant-v4/frame-3-situp.jpg 为底图，
手遮嘴轻打哈欠，重点修复手指扭曲，生成多候选供挑选。
"""
import argparse
import base64
import os
import sys
import winreg
from pathlib import Path

import requests

ROOT = Path(__file__).resolve().parents[1]
V4_DIR = ROOT / "assets" / "personas" / "wake-frames" / "variant-v4"
BASE = V4_DIR / "frame-3-situp.jpg"
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


PROMPT = (
    "保持参考图完全相同的场景、人物、构图、坐姿、光线与画风"
    "（深夜程序员工作室，蓝紫色柔顺长发的鲸鱼娘女仆少女，深蓝白哥特女仆裙，"
    "有双腿，深蓝色星光纹理的鲸鱼尾巴只是装饰垂在椅侧，"
    "长发非常柔顺丝滑光泽细腻不凌乱，smooth silky hair，"
    "暖黄台灯与冷蓝夜景的电影感光影，低饱和深色）。"
    "角色保持坐姿不变，双腿自然不扭曲。"
    "她抬起一只手，手背轻轻掩在嘴巴前，手指自然并拢、指节柔和弯曲、"
    "五指清晰分明、比例正常、绝不扭曲或畸形，anatomy correct hand, natural fingers。"
    "闭上一只眼睛，另一只眼半睁，表情慵懒温柔，"
    "轻轻打一个含蓄的小哈欠，动作幅度小，宁静可爱。"
)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=get_env("CODEX_IMAGE_MODEL") or "gpt-image-2")
    ap.add_argument("--size", default="1659x948")
    ap.add_argument("--n", type=int, default=3, help="候选数量")
    args = ap.parse_args()

    key = get_env("CODEX_API_KEY")
    if not key:
        print("缺少 CODEX_API_KEY", file=sys.stderr)
        sys.exit(2)
    if not BASE.exists():
        print(f"缺少底图: {BASE}", file=sys.stderr)
        sys.exit(2)

    files = [("image", (BASE.name, BASE.read_bytes(), "image/jpeg"))]
    headers = {"Authorization": f"Bearer {key}"}
    saved = 0
    for i in range(1, args.n + 1):
        data = [
            ("model", (None, args.model)),
            ("prompt", (None, PROMPT)),
            ("n", (None, "1")),  # 接口仅支持 n=1，逐次生成
            ("size", (None, args.size)),
            ("response_format", (None, "b64_json")),
        ]
        print(f"生成候选 {i}/{args.n} ...")
        try:
            r = requests.post(f"{IMG_API_BASE}/images/edits", headers=headers, data=data, files=files, timeout=300)
        except Exception as e:
            print(f"请求异常: {e}", file=sys.stderr)
            continue
        if r.status_code != 200:
            print(f"HTTP {r.status_code}: {r.text[:200]}", file=sys.stderr)
            continue
        data_json = r.json()
        out = V4_DIR / f"frame-4-yawn-cand{i}.jpg"
        for item in data_json.get("data", []):
            b64 = item.get("b64_json")
            if b64:
                out.write_bytes(base64.b64decode(b64))
                print(f"候选{i}: {out} ({out.stat().st_size:,} bytes)")
                saved += 1
            elif item.get("url"):
                img = requests.get(item["url"], timeout=120)
                out.write_bytes(img.content)
                print(f"候选{i}(URL): {out}")
                saved += 1
    if not saved:
        print("全部失败", file=sys.stderr)
        sys.exit(5)
    print("完成")


if __name__ == "__main__":
    main()
