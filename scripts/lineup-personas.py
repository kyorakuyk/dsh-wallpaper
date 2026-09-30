"""把四张立绘按固定顺序**等高、脚底对齐**拼成一张大图（透明背景）。

顺序：蓝色成年 → 蓝色幼年 → 红（黑红）幼年 → 红（黑红）成年
—— 也就是"一排四形态"那种公告/商店页排法。

尺寸不均（三张 1024×1536、蓝幼 1086×1448），所以按**最高的一张**等比缩放其余几张，
再按脚底对齐拼起来；不放大、不裁剪，因此不会有额外的模糊。

用法：
    python scripts/lineup-personas.py                 # 输出 docs/media/personas-lineup.png
    python scripts/lineup-personas.py 路径\\自定义.png  # 指定输出
"""

from pathlib import Path
import sys

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "wallpaper" / "public" / "personas"

# 顺序是有意的：先看的两种是同一只鲸鱼娘的成年与幼年（蓝），再是红（黑红）的幼年与成年。
ORDER = [
    "portrait-blue-adult.png",
    "portrait-blue-child.png",
    "portrait-black-child.png",
    "portrait-black-adult.png",
]


def main() -> int:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "docs" / "media" / "personas-lineup.png"

    missing = [name for name in ORDER if not (SRC / name).is_file()]
    if missing:
        print("缺少立绘：" + "、".join(missing))
        return 1

    opened = [(name, Image.open(SRC / name).convert("RGBA")) for name in ORDER]
    target_height = max(image.height for _, image in opened)

    frames = []
    for name, image in opened:
        scale = target_height / image.height
        frames.append(image.resize((round(image.width * scale), target_height), Image.Resampling.LANCZOS))

    width = sum(frame.width for frame in frames)
    canvas = Image.new("RGBA", (width, target_height), (0, 0, 0, 0))
    x = 0
    for frame in frames:
        # 等高之后脚底自然对齐（y = 0），横着依次放过去，不留缝。
        canvas.paste(frame, (x, 0), frame)
        x += frame.width

    out.parent.mkdir(parents=True, exist_ok=True)
    canvas.save(out)
    print(f"{out}  {canvas.width}x{canvas.height}  " + " | ".join(ORDER))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
