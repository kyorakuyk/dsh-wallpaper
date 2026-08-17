"""调用 OpenAI 兼容 images/edits：参照黑红幼年 → 生成黑红成年鲸鱼娘立绘。

输入参照：
  assets/personas/黑红幼年.jpg   形象基底（黑红配色幼年形态）
  assets/personas/蓝色成年.jpg   成年风格参照（体态/构图）
输出：
  assets/personas/黑红成年.jpg
用法：
  python scripts/generate-black-adult.py [--model gpt-image-2]
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


def get_env(name: str) -> str:
    """优先用户级注册表（setx 写入的最新值），回退进程环境"""
    try:
        reg = winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment")
        v, _ = winreg.QueryValueEx(reg, name)
        if v:
            return v
    except OSError:
        pass
    return os.environ.get(name, "")


# 图像编辑接口固定使用 img-api 域名（主接口 apinebula.com TLS 不可达）；
# CODEX_API_URL 若指向旧域名则忽略。
IMG_API_BASE = "https://img-api.apinebula.ai/v1"


REF_CHILD = ROOT / "assets" / "personas" / "黑红幼年.jpg"
REF_BLUE_ADULT = ROOT / "assets" / "personas" / "蓝色成年.jpg"
OUT = ROOT / "assets" / "personas" / "黑红成年.jpg"

PROMPT = (
    "把图中的角色改成年成年的版本：保持黑红配色主题与角色特征不变，"
    "将体型、气质、发型改为成年女性（成熟优雅，身材修长），"
    "黑色长发搭配红色挑染，黑色为主红色点缀的连衣裙，黑色鱼尾，"
    "红色发饰，整体风格参照另一张参考图的成年体态。"
    "竖构图全身立绘，纯白色背景，无文字，正面站立，画风一致。"
)


def encode_b64(path: Path) -> str:
    return base64.b64encode(path.read_bytes()).decode()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=get_env("CODEX_IMAGE_MODEL") or "gpt-image-2")
    ap.add_argument("--out", default=str(OUT))
    args = ap.parse_args()

    key = get_env("CODEX_API_KEY")
    url = IMG_API_BASE  # 固定用图像接口域名
    if not key:
        print("缺少 CODEX_API_KEY", file=sys.stderr)
        sys.exit(2)

    if not REF_CHILD.exists() or not REF_BLUE_ADULT.exists():
        print(f"参照图缺失: {REF_CHILD.exists()=} {REF_BLUE_ADULT.exists()=}", file=sys.stderr)
        sys.exit(2)

    # images/edits: multipart/form-data（image 必须为首字段，部分网关要求）
    # 部分实现支持多图：image(主图) + mask 或第二 image 作风格参考。
    # 先尝试双图（image=黑红幼年, mask=蓝色成年），失败则回退单图+prompt 描述。
    fields = [
        ("model", (None, args.model)),
        ("prompt", (None, PROMPT)),
        ("n", (None, "1")),
        ("size", (None, "1024x1792")),
        ("response_format", (None, "b64_json")),
    ]
    files = [
        ("image", ("black-child.jpg", REF_CHILD.read_bytes(), "image/jpeg")),
    ]

    headers = {"Authorization": f"Bearer {key}"}

    def try_send(extra_files):
        data = list(fields)
        fl = list(files) + extra_files
        return requests.post(
            f"{url}/images/edits",
            headers=headers,
            data=data,
            files=fl,
            timeout=300,
        )

    r = None
    # 尝试 1：双图（黑红幼年 + 蓝色成年风格）
    try:
        r = try_send([("image", ("blue-adult.jpg", REF_BLUE_ADULT.read_bytes(), "image/jpeg"))])
        if r.status_code != 200:
            print(f"双图 HTTP {r.status_code}: {r.text[:300]}", file=sys.stderr)
            r = None
    except Exception as e:
        print(f"双图请求异常: {e}", file=sys.stderr)

    # 尝试 2：单图 + prompt 描述成年风格
    if r is None:
        print("回退单图模式...")
        try:
            r = try_send([])
        except Exception as e:
            print(f"单图请求异常: {e}", file=sys.stderr)
            sys.exit(3)

    if r.status_code != 200:
        print(f"HTTP {r.status_code}: {r.text[:600]}", file=sys.stderr)
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
        print("响应无图片:", json.dumps(data)[:500], file=sys.stderr)
        sys.exit(5)
    print("完成 → 下一步：抠图接入 black-adult 形态")


if __name__ == "__main__":
    main()
