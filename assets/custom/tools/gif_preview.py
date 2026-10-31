"""Render GIF frames as half-block terminal previews to judge sizes.

Usage: python gif_preview.py rem.gif 40,52,64
"""

import sys
from pathlib import Path

from PIL import Image, ImageSequence

HERE = Path(__file__).resolve().parent.parent


def frames(path: Path) -> list[Image.Image]:
    im = Image.open(path)
    return [f.convert("RGBA") for f in ImageSequence.Iterator(im)]


def shrink(frame: Image.Image, width: int, bbox) -> Image.Image:
    frame = frame.crop(bbox)
    height = round(frame.height * width / frame.width)
    height += height % 2
    small = frame.resize((width, height), Image.Resampling.LANCZOS)
    alpha = small.getchannel("A").point(lambda a: 255 if a >= 128 else 0)
    small.putalpha(alpha)
    return small


def main() -> None:
    src = HERE / sys.argv[1]
    widths = [int(w) for w in sys.argv[2].split(",")]
    fs = frames(src)
    # union bbox across frames so the animation does not jitter
    box = None
    for f in fs:
        b = f.getchannel("A").getbbox()
        if b:
            box = b if box is None else (min(box[0], b[0]), min(box[1], b[1]), max(box[2], b[2]), max(box[3], b[3]))
    print("frames", len(fs), "bbox", box)
    scale = 6
    tiles = []
    for w in widths:
        small = shrink(fs[0], w, box)
        print(f"width {w}: {small.width} cols x {small.height // 2} rows")
        bg = Image.new("RGBA", small.size, (12, 12, 16, 255))
        bg.alpha_composite(small)
        # Each terminal cell is roughly 1:2, a half block is 1:1, so scale evenly.
        tiles.append(bg.resize((small.width * scale, small.height * scale), Image.Resampling.NEAREST))
    gap = 20
    sheet = Image.new("RGBA", (sum(t.width for t in tiles) + gap * (len(tiles) + 1), max(t.height for t in tiles) + 2 * gap), (12, 12, 16, 255))
    x = gap
    for t in tiles:
        sheet.alpha_composite(t, (x, gap))
        x += t.width + gap
    sheet.save(HERE / "gif_preview_sheet.png")


if __name__ == "__main__":
    main()
