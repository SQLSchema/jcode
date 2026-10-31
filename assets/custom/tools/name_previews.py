"""Render candidate product names in the welcome-logo block style.

Writes previews/names/<name>.txt (logo text, same style as logo.txt) and a
combined PNG preview with the blue->lavender gradient, so names can be judged
in context. Usage: python name_previews.py NAME [NAME ...]
"""
import sys
from pathlib import Path

import pyfiglet
from PIL import Image, ImageDraw, ImageFont

OUT = Path(__file__).resolve().parent.parent / "previews" / "names"
FONT = "dos_rebel"
FROM, TO = (0x3D, 0x5A, 0xFE), (0xB4, 0xC0, 0xFF)
CW, CH = 9, 18


def render(name: str) -> list[str]:
    fig = pyfiglet.Figlet(font=FONT, width=400)
    lines = []
    for word in name.split():
        rows = [r.rstrip() for r in fig.renderText(word.upper()).splitlines()]
        while rows and not rows[-1].strip():
            rows.pop()
        if lines:
            lines.append("")
        lines.extend(rows)
    return lines


def lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def to_png(lines: list[str], path: Path):
    width = max((len(l) for l in lines), default=1)
    img = Image.new("RGB", ((width + 4) * CW, (len(lines) + 2) * CH), (12, 12, 16))
    draw = ImageDraw.Draw(img)
    for y, line in enumerate(lines):
        for x, ch in enumerate(line):
            if ch == " ":
                continue
            color = lerp(FROM, TO, x / max(width - 1, 1))
            if ch in "░▒▓":
                color = tuple(int(c * 0.5) for c in color)
            x0, y0 = (x + 2) * CW, (y + 1) * CH
            draw.rectangle([x0, y0, x0 + CW - 1, y0 + CH - 1], fill=color)
    img.save(path)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    sheet_parts = []
    for name in sys.argv[1:]:
        lines = render(name)
        slug = name.lower().replace(" ", "-")
        (OUT / f"{slug}.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
        png = OUT / f"{slug}.png"
        to_png(lines, png)
        width = max(len(l) for l in lines)
        print(f"{name}: {len(lines)} rows x {width} cols -> {png.name}")
        sheet_parts.append(Image.open(png))
    if sheet_parts:
        w = max(p.width for p in sheet_parts)
        h = sum(p.height for p in sheet_parts)
        sheet = Image.new("RGB", (w, h), (12, 12, 16))
        y = 0
        for p in sheet_parts:
            sheet.paste(p, (0, y))
            y += p.height
        sheet.save(OUT / "_sheet.png")


if __name__ == "__main__":
    main()
