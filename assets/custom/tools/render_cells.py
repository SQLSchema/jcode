"""Render a *.cells.json terminal dump (from the welcome_hero tests) to PNG.

Each cell is [symbol, fg, bg, bold]; fg/bg are "#rrggbb" or null.
Half-block glyphs are drawn as real pixel halves so the preview matches what a
truecolor terminal shows. Other glyphs are drawn with a monospace font.

Usage: python render_cells.py welcome_120x40.cells.json [out.png]
"""

import json
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

CW, CH = 9, 18
BG = (12, 12, 16)
FG = (204, 204, 204)


def hexc(v, default):
    if not v or not isinstance(v, str) or not v.startswith("#"):
        return default
    return tuple(int(v[i : i + 2], 16) for i in (1, 3, 5))


def main() -> None:
    src = Path(sys.argv[1])
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else src.with_suffix("").with_suffix(".png")
    d = json.loads(src.read_text(encoding="utf-8"))
    w, h = d["width"], d["height"]
    img = Image.new("RGB", (w * CW, h * CH), BG)
    dr = ImageDraw.Draw(img)
    try:
        font = ImageFont.truetype("consola.ttf", 15)
        bold = ImageFont.truetype("consolab.ttf", 15)
    except OSError:
        font = bold = ImageFont.load_default()
    for y, row in enumerate(d["rows"]):
        for x, cell in enumerate(row):
            sym, fg, bg, is_bold = (cell + [None, None, False])[:4]
            fgc = hexc(fg, FG)
            bgc = hexc(bg, BG)
            x0, y0 = x * CW, y * CH
            dr.rectangle([x0, y0, x0 + CW - 1, y0 + CH - 1], fill=bgc)
            if sym == "▀":
                dr.rectangle([x0, y0, x0 + CW - 1, y0 + CH // 2 - 1], fill=fgc)
            elif sym == "▄":
                dr.rectangle([x0, y0 + CH // 2, x0 + CW - 1, y0 + CH - 1], fill=fgc)
            elif sym == "█":
                dr.rectangle([x0, y0, x0 + CW - 1, y0 + CH - 1], fill=fgc)
            elif sym and sym.strip():
                dr.text((x0, y0 + 1), sym, fill=fgc, font=bold if is_bold else font)
    img.save(out)
    print(out)


if __name__ == "__main__":
    main()
