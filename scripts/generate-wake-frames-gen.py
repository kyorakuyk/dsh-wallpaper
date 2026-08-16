"""文生图生成配套苏醒帧（睁眼/起身/打哈欠）——edits 保真度过高导致动作变化不明显，
改用文生图 + 完整场景描述，动作变化更清晰。"""

import argparse
import base64
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


SCENE = (
    "深夜的程序员工作室，一位蓝紫色长发的人鱼女仆少女，穿着深蓝色与白色的哥特女仆裙"
    "（白色蕾丝荷叶边、发饰和蝴蝶结），身后是一条深蓝色、带有星光纹理的巨大人鱼尾巴，"
    "从椅子靠背的侧边垂落。桌面上堆满程序文档、手写笔记、设计草图、文件夹和便签纸，"
    "几台显示器显示代码编辑器和调试界面，旁边有一盏大型黑色台灯发出温暖的橙黄色光线，"
    "桌上放着一杯咖啡和一个小型蓝色鲸鱼玩偶。背景是昏暗的办公室，窗外是深蓝色夜景，"
    "墙上贴着流程图和工作笔记。暖黄台灯与冷蓝夜景强烈对比，电影感光影，低饱和深色配色。"
    "长发非常柔顺丝滑、光泽细腻、整齐如丝绸瀑布般铺满桌面，发丝根根分明不凌乱，"
    "smooth silky hair, glossy hair, high detail, cinematic lighting, anime illustration, "
    "DeepSeek 鲸鱼娘动漫拟人形象"
)

# 动作描述：睁眼 → 起身 → 打哈欠（构图继承：深夜工作室、趴桌/桌旁、同一角色）
FRAMES = [
    (
        "frame-2-eyes-v2.jpg",
        "她侧脸枕在交叠的手臂上趴在办公桌上，双眼明显睁开、眼神清澈明亮，"
        "睫毛清晰，瞳孔有神，透出刚醒来的朦胧与清醒，眼睛睁开是画面最突出的变化。"
        + SCENE,
    ),
    (
        "frame-3-situp-v2.jpg",
        "她从桌上撑起上半身坐起来，半坐半靠，睡眼惺忪，长发柔顺丝滑地垂落，"
        "身后的深蓝色星光鱼尾仍垂在椅侧。"
        + SCENE,
    ),
    (
        "frame-4-yawn-v2.jpg",
        "她坐直身体，双手抬起伸懒腰，张大嘴打哈欠，刚醒来的慵懒样子，"
        "长发柔顺丝滑地垂落。"
        + SCENE,
    ),
]


def gen(model: str, prompt: str, out: Path, key: str, size: str) -> bool:
    payload = {
        "model": model,
        "prompt": prompt,
        "n": 1,
        "size": size,
        "response_format": "b64_json",
    }
    headers = {"Authorization": f"Bearer {key}", "Content-Type": "application/json"}
    print(f"  生成 {out.name} ...")
    try:
        r = requests.post(f"{IMG_API_BASE}/images/generations", headers=headers, json=payload, timeout=300)
    except Exception as e:
        print(f"    请求异常: {e}", file=sys.stderr)
        return False
    if r.status_code != 200:
        print(f"    HTTP {r.status_code}: {r.text[:300]}", file=sys.stderr)
        return False
    data = r.json()
    for item in data.get("data", []):
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
    print(f"    响应无图片: {str(data)[:300]}", file=sys.stderr)
    return False


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=get_env("CODEX_IMAGE_MODEL") or "gpt-image-2")
    ap.add_argument("--size", default="1792x1024")
    ap.add_argument("--frames", default="2,3,4")
    args = ap.parse_args()

    key = get_env("CODEX_API_KEY")
    if not key:
        print("缺少 CODEX_API_KEY", file=sys.stderr)
        sys.exit(2)

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    wanted = {int(x) for x in args.frames.split(",")}
    ok = 0
    for idx, (name, prompt) in enumerate(FRAMES, start=2):
        if idx not in wanted:
            continue
        if gen(args.model, prompt, OUT_DIR / name, key, args.size):
            ok += 1
    print(f"完成: {ok}/{len(wanted)} 帧 → {OUT_DIR}")


if __name__ == "__main__":
    main()
