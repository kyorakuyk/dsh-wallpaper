"""文生图生成第一帧（睡脸）：基于用户提供的完整场景描述 + deepseek 动漫拟人形象。

输出为独立文件名（frame-1-sleep-v2.jpg），不覆盖旧帧（留档）。
"""
import argparse
import base64
import json
import os
import sys
import winreg
from pathlib import Path

import requests

ROOT = Path(__file__).resolve().parents[1]
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


PROMPT = (
    "一幅高质量日系动漫插画，16:9 横构图，DeepSeek 鲸鱼娘的动漫拟人形象。"
    "\n"
    "深夜的程序员工作室，一位蓝紫色长发的人鱼女仆少女疲惫地趴在凌乱的办公桌上睡着了。"
    "她穿着深蓝色与白色的哥特女仆裙，带有白色蕾丝荷叶边、发饰和蝴蝶结。"
    "长发非常柔顺丝滑、光泽细腻、整齐地如丝绸瀑布般铺满桌面并垂落到地面，"
    "发丝根根分明、顺滑不凌乱，没有炸毛或散乱打结。"
    "身后是一条深蓝色、带有星光纹理的巨大人鱼尾巴，从椅子靠背的侧边垂落。"
    "她侧脸枕在交叠的手臂上，双眼闭合，神情安静疲惫。"
    "\n"
    "桌面上堆满了程序文档、手写笔记、设计草图、文件夹和便签纸，"
    "几台显示器显示代码编辑器和调试界面，旁边有一盏大型黑色台灯，发出温暖的橙黄色光线。"
    "桌上放着一杯咖啡和一个小型蓝色鲸鱼玩偶。背景是昏暗的办公室，窗外是深蓝色夜景，"
    "墙上贴着流程图和工作笔记。"
    "\n"
    "暖黄色台灯光与冷蓝色夜光形成强烈对比，电影感光影，低饱和深色配色，柔和体积光，"
    "丰富的桌面细节，精致服装褶皱，氛围宁静、疲惫、温馨，"
    "smooth silky hair, glossy hair, detailed hair strands, "
    "high detail, cinematic lighting, anime illustration, detailed background"
)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=get_env("CODEX_IMAGE_MODEL") or "gpt-image-2")
    ap.add_argument("--size", default="1792x1024", help="16:9 横构图优先")
    ap.add_argument("--out", default=str(OUT_DIR / "frame-1-sleep-v2.jpg"))
    args = ap.parse_args()

    key = get_env("CODEX_API_KEY")
    if not key:
        print("缺少 CODEX_API_KEY", file=sys.stderr)
        sys.exit(2)

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    payload = {
        "model": args.model,
        "prompt": PROMPT,
        "n": 1,
        "size": args.size,
        "response_format": "b64_json",
    }
    headers = {"Authorization": f"Bearer {key}", "Content-Type": "application/json"}
    print(f"文生图 {args.size} model={args.model} ...")
    try:
        r = requests.post(f"{IMG_API_BASE}/images/generations", headers=headers, json=payload, timeout=300)
    except Exception as e:
        print(f"请求异常: {e}", file=sys.stderr)
        sys.exit(3)

    if r.status_code != 200:
        print(f"HTTP {r.status_code}: {r.text[:400]}", file=sys.stderr)
        sys.exit(4)

    data = r.json()
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    saved = 0
    for item in data.get("data", []):
        b64 = item.get("b64_json")
        if b64:
            out.write_bytes(base64.b64decode(b64))
            print(f"已保存: {out} ({out.stat().st_size:,} bytes)")
            saved += 1
        elif item.get("url"):
            img = requests.get(item["url"], timeout=120)
            out.write_bytes(img.content)
            print(f"已保存(URL): {out}")
            saved += 1
    if not saved:
        print("响应无图片:", json.dumps(data)[:400], file=sys.stderr)
        sys.exit(5)
    print("完成")


if __name__ == "__main__":
    main()
