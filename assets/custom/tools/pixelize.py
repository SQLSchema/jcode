"""Turn a normal illustration into small terminal-ready pixel art.

Usage:
    python pixelize.py source.png --crop 600,20,1300,720 --widths 32,40,48,56

Writes, next to this script's parent folder:
    logo_<w>.png          real-size pixel art (RGBA, transparent background)
    preview_<w>.png       8x nearest-neighbour preview on a black "terminal"
    preview_sheet.png     all previews side by side

Each terminal cell shows 2 vertical pixels via the half block glyph, so a
48x48 image becomes 48 columns x 24 rows on screen.
"""

import argparse
from collections import deque
from pathlib import Path

from PIL import Image, ImageEnhance

HERE = Path(__file__).resolve().parent.parent  # assets/custom


def remove_background(im: Image.Image, threshold: int = 232) -> Image.Image:
    """Flood fill near-white pixels connected to the border into transparency.

    Only the border-connected region is removed, so white clothing that is
    enclosed by line art survives.
    """
    im = im.convert("RGBA")
    w, h = im.size
    px = im.load()

    def is_bg(x: int, y: int) -> bool:
        r, g, b, _ = px[x, y]
        return r >= threshold and g >= threshold and b >= threshold

    seen = bytearray(w * h)
    queue: deque[tuple[int, int]] = deque()
    for x in range(w):
        queue.append((x, 0))
        queue.append((x, h - 1))
    for y in range(h):
        queue.append((0, y))
        queue.append((w - 1, y))

    while queue:
        x, y = queue.popleft()
        idx = y * w + x
        if seen[idx]:
            continue
        seen[idx] = 1
        if not is_bg(x, y):
            continue
        px[x, y] = (0, 0, 0, 0)
        if x > 0:
            queue.append((x - 1, y))
        if x < w - 1:
            queue.append((x + 1, y))
        if y > 0:
            queue.append((x, y - 1))
        if y < h - 1:
            queue.append((x, y + 1))
    return im


def pixelize(im: Image.Image, width: int, colors: int) -> Image.Image:
    height = round(im.height * width / im.width)
    height += height % 2  # whole terminal rows
    small = im.resize((width, height), Image.Resampling.BOX)

    # Hard alpha edges: terminals cannot blend partially transparent cells.
    alpha = small.getchannel("A").point(lambda a: 255 if a >= 110 else 0)

    rgb = small.convert("RGB")
    rgb = ImageEnhance.Color(rgb).enhance(1.25)
    rgb = ImageEnhance.Contrast(rgb).enhance(1.1)
    rgb = rgb.quantize(colors=colors, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    out = rgb.convert("RGBA")
    out.putalpha(alpha)
    return out


def preview(im: Image.Image, scale: int = 8) -> Image.Image:
    bg = Image.new("RGBA", im.size, (12, 12, 16, 255))
    bg.alpha_composite(im)
    return bg.resize((im.width * scale, im.height * scale), Image.Resampling.NEAREST)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("source")
    ap.add_argument("--crop", default="", help="left,top,right,bottom")
    ap.add_argument("--widths", default="32,40,48,56")
    ap.add_argument("--colors", type=int, default=24)
    args = ap.parse_args()

    im = Image.open(args.source)
    if args.crop:
        im = im.crop(tuple(int(v) for v in args.crop.split(",")))
    im = remove_background(im)
    bbox = im.getchannel("A").getbbox()
    if bbox:
        im = im.crop(bbox)
    im.save(HERE / "cutout.png")

    previews = []
    for w in (int(v) for v in args.widths.split(",")):
        art = pixelize(im, w, args.colors)
        art.save(HERE / f"logo_{w}.png")
        p = preview(art)
        p.save(HERE / f"preview_{w}.png")
        previews.append((w, p))
        print(f"width {w}: {art.width}x{art.height}px -> {art.width} cols x {art.height // 2} rows")

    gap = 24
    sheet_w = sum(p.width for _, p in previews) + gap * (len(previews) + 1)
    sheet_h = max(p.height for _, p in previews) + gap * 2
    sheet = Image.new("RGBA", (sheet_w, sheet_h), (12, 12, 16, 255))
    x = gap
    for _, p in previews:
        sheet.alpha_composite(p, (x, gap))
        x += p.width + gap
    sheet.save(HERE / "preview_sheet.png")


if __name__ == "__main__":
    main()
