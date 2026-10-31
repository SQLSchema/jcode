"""Build the welcome-screen assets embedded by crates/jcode-tui.

Inputs (assets/custom):
    rem-*.piskel    Piskel project (single layer, N frames)
    logo.txt        block-letter logo, lines may carry a leading "# " prefix

Outputs (crates/jcode-tui/src/tui/welcome_assets):
    sprite_<w>.png  horizontal sprite sheet, frames of width <w>, RGBA with
                    hard alpha (0 or 255); height is even so it maps to whole
                    half-block rows
    sprite.json     {"fps": .., "frames": .., "sizes": [{"width","height"}]}
    logo.txt        cleaned logo (prefix stripped, trailing blanks removed,
                    the blank gap between words collapsed to one line)

Usage: python tools/build_welcome_assets.py <file.piskel>
"""

import base64
import io
import json
import sys
from pathlib import Path

from PIL import Image

CUSTOM = Path(__file__).resolve().parent.parent
REPO = CUSTOM.parent.parent
OUT = REPO / "crates" / "jcode-tui" / "src" / "tui" / "welcome_assets"
WIDTHS = [64, 48, 36, 24]


def load_piskel(path: Path) -> tuple[list[Image.Image], int]:
    data = json.loads(path.read_text(encoding="utf-8"))["piskel"]
    w, h = data["width"], data["height"]
    layer = json.loads(data["layers"][0])
    chunk = layer["chunks"][0]
    sheet = Image.open(io.BytesIO(base64.b64decode(chunk["base64PNG"].split(",", 1)[1]))).convert("RGBA")
    frames = []
    for row in chunk["layout"]:
        for idx in row:
            frames.append(sheet.crop((idx * w, 0, idx * w + w, h)))
    return frames, int(data.get("fps", 12))


def union_bbox(frames: list[Image.Image]):
    box = None
    for f in frames:
        b = f.getchannel("A").getbbox()
        if not b:
            continue
        box = b if box is None else (min(box[0], b[0]), min(box[1], b[1]), max(box[2], b[2]), max(box[3], b[3]))
    return box


def scale(frame: Image.Image, width: int, height: int) -> Image.Image:
    # Premultiply-ish: resize RGB and alpha separately so transparent pixels do
    # not bleed dark fringes into the edge colors.
    rgba = frame.resize((width, height), Image.Resampling.LANCZOS)
    alpha = rgba.getchannel("A").point(lambda a: 255 if a >= 120 else 0)
    rgba.putalpha(alpha)
    return rgba


def clean_logo(text: str) -> str:
    lines = []
    for line in text.splitlines():
        if line.startswith("#"):
            line = line[1:]
        lines.append(line.rstrip())
    # trim leading/trailing blank lines
    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()
    # collapse runs of blank lines to a single blank line
    out: list[str] = []
    for line in lines:
        if not line.strip() and out and not out[-1].strip():
            continue
        out.append(line)
    # strip common leading indentation
    indent = min(len(l) - len(l.lstrip(" ")) for l in out if l.strip())
    out = [l[indent:] if l.strip() else "" for l in out]
    return "\n".join(out) + "\n"


def main() -> None:
    piskel = Path(sys.argv[1])
    frames, fps = load_piskel(piskel)
    box = union_bbox(frames)
    frames = [f.crop(box) for f in frames]
    src_w, src_h = frames[0].size
    OUT.mkdir(parents=True, exist_ok=True)

    sizes = []
    for w in WIDTHS:
        h = round(src_h * w / src_w)
        h += h % 2
        sheet = Image.new("RGBA", (w * len(frames), h), (0, 0, 0, 0))
        for i, f in enumerate(frames):
            sheet.alpha_composite(scale(f, w, h), (i * w, 0))
        sheet.save(OUT / f"sprite_{w}.png", optimize=True)
        sizes.append({"width": w, "height": h})
        print(f"sprite_{w}.png: {len(frames)} frames, {w}x{h}px = {w} cols x {h // 2} rows")

    meta = {"fps": fps, "frames": len(frames), "sizes": sizes}
    (OUT / "sprite.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")

    logo = clean_logo((CUSTOM / "logo.txt").read_text(encoding="utf-8"))
    (OUT / "logo.txt").write_text(logo, encoding="utf-8", newline="\n")
    lines = logo.splitlines()
    print(f"logo.txt: {len(lines)} rows x {max(len(l) for l in lines)} cols")


if __name__ == "__main__":
    main()
