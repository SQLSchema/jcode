"""Remove floating pixel specks from the welcome sprite sheets.

Keeps only pixels connected (4-neighbour) to the largest blob in each frame,
plus any other blob big enough to be real detail. Writes back in place and
reports what it removed. Run from the repo root.
"""
from collections import deque
from pathlib import Path

from PIL import Image

ASSETS = Path("crates/jcode-tui/src/tui/welcome_assets")
MIN_KEEP = {64: 24, 48: 14, 36: 9, 24: 5}


def components(px, w, h):
    seen = [[False] * w for _ in range(h)]
    comps = []
    for y in range(h):
        for x in range(w):
            if px[x, y][3] == 0 or seen[y][x]:
                continue
            q = deque([(x, y)])
            seen[y][x] = True
            comp = []
            while q:
                a, b = q.popleft()
                comp.append((a, b))
                for na, nb in ((a + 1, b), (a - 1, b), (a, b + 1), (a, b - 1)):
                    if 0 <= na < w and 0 <= nb < h and not seen[nb][na] and px[na, nb][3]:
                        seen[nb][na] = True
                        q.append((na, nb))
            comps.append(comp)
    return comps


for size in (64, 48, 36, 24):
    path = ASSETS / f"sprite_{size}.png"
    sheet = Image.open(path).convert("RGBA")
    h = sheet.height
    frames = sheet.width // size
    removed_total = 0
    for f in range(frames):
        frame = sheet.crop((f * size, 0, f * size + size, h))
        px = frame.load()
        comps = sorted(components(px, size, h), key=len, reverse=True)
        for comp in comps[1:]:
            if len(comp) < MIN_KEEP[size]:
                for x, y in comp:
                    px[x, y] = (0, 0, 0, 0)
                removed_total += len(comp)
        sheet.paste(frame, (f * size, 0))
    sheet.save(path, optimize=True)
    print(f"sprite_{size}.png: removed {removed_total} stray pixels across {frames} frames")
