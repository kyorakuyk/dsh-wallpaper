"""生成「深海室内」主题背景壁纸（文生图，多候选）。

设计要点：深蓝色主色调、墙上鱼群游动的阴影、窗外海底景象、深海体积光。
输出：assets/personas/deepsea-bg/bg-cand1.png ~ bg-candN.png
"""
import argparse
import base64
import os
import sys
import winreg
from pathlib import Path

import requests

ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "assets" / "personas" / "deepsea-bg"
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
    "日系动漫插画风格，深海主题室内场景，16:9 横构图，高质量，电影感光影，"
    "high detail, anime illustration, cinematic lighting"
)

PROMPTS = [
    # 候选A：深海程序员工作室（呼应鲸鱼娘设定）
    "深夜的深海室内工作室：房间是深蓝色的海底小屋，主色调深蓝，"
    "墙上投映着鱼群游过的阴影，像海鱼在灯光下游过墙面，"
    "大窗户外面是海底景象——蓝色海水、游动的鱼群、摇曳的珊瑚和海草，"
    "一串串细小的气泡从窗外飘起，室内一盏暖黄色台灯发出柔和光线，"
    "书桌、书架、显示器构成一个温馨的深海工作角，"
    "暖光与深海冷蓝形成对比，静谧、梦幻、温馨。"
    + STYLE,
    # 候选B：海底列车站/穹顶舱（更梦幻）
    "深海玻璃穹顶舱室内：深蓝色为主色调，圆形大窗能看到外面幽蓝的海底——"
    "发光水母、鱼群、珊瑚礁，墙面上有鱼群游动的动态阴影，"
    "室内有沙发、植物、暖色落地灯，地板有轻微水波光影，"
    "氛围静谧梦幻，像住在海底世界。"
    + STYLE,
    # 候选C：深海书房（偏写实氛围）
    "深海底部的现代书房：深蓝色主色调，整面墙是落地玻璃，窗外是海底——"
    "蔚蓝海水、鱼群、海藻随水流摇曳、远处透下的阳光柱，"
    "室内暖黄灯光，木质书架和书桌，墙上挂着装饰，"
    "鱼影在墙面和天花板上缓缓游动，安静、深邃、温暖。"
    + STYLE,
]

CAND_NAMES = ["bg-cand1.png", "bg-cand2.png", "bg-cand3.png"]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=get_env("CODEX_IMAGE_MODEL") or "gpt-image-2")
    ap.add_argument("--size", default="1792x1024")
    ap.add_argument("--which", default="1,2,3")
    args = ap.parse_args()

    key = get_env("CODEX_API_KEY")
    if not key:
        print("缺少 CODEX_API_KEY", file=sys.stderr)
        sys.exit(2)

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    wanted = {int(x) for x in args.which.split(",")}
    headers = {"Authorization": f"Bearer {key}", "Content-Type": "application/json"}
    saved = 0
    for idx, (prompt, name) in enumerate(zip(PROMPTS, CAND_NAMES), start=1):
        if idx not in wanted:
            continue
        payload = {
            "model": args.model,
            "prompt": prompt,
            "n": 1,
            "size": args.size,
            "response_format": "b64_json",
        }
        print(f"生成候选{idx} ...")
        try:
            r = requests.post(f"{IMG_API_BASE}/images/generations", headers=headers, json=payload, timeout=300)
        except Exception as e:
            print(f"请求异常: {e}", file=sys.stderr)
            continue
        if r.status_code != 200:
            print(f"HTTP {r.status_code}: {r.text[:200]}", file=sys.stderr)
            continue
        out = OUT_DIR / name
        for item in r.json().get("data", []):
            b64 = item.get("b64_json")
            if b64:
                out.write_bytes(base64.b64decode(b64))
                print(f"候选{idx}: {out} ({out.stat().st_size:,} bytes)")
                saved += 1
            elif item.get("url"):
                img = requests.get(item["url"], timeout=120)
                out.write_bytes(img.content)
                print(f"候选{idx}(URL): {out}")
                saved += 1
    if not saved:
        print("全部失败", file=sys.stderr)
        sys.exit(5)
    print("完成")


if __name__ == "__main__":
    main()
