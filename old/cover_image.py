#!/usr/bin/env python3
"""
For each immediate subfolder whose name ends in a number (e.g., "One Piece 5"):

Base image selection:
- If cover_old.jpg exists, use THAT as the base image and (re)generate cover.jpg from it.
- Else, if cover.jpg exists, rename cover.jpg -> cover_old.jpg, then use cover_old.jpg as the base.

Output:
- Draw the folder's trailing number in HUGE text, completely black, EXACTLY centered.
- Text size is ~10% smaller than the maximum possible fit.
- Save as cover.jpg (overwriting if present).

Requires:
  pip install pillow
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


TRAILING_NUMBER_RE = re.compile(r"(\d+)\s*$")


def pick_font(font_size: int) -> ImageFont.FreeTypeFont:
    candidates = [
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        "/System/Library/Fonts/Supplemental/Helvetica Neue Bold.ttf",
        "/Library/Fonts/Arial Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    ]
    for p in candidates:
        fp = Path(p)
        if fp.exists():
            return ImageFont.truetype(str(fp), font_size)
    return ImageFont.load_default()


def unique_old_name(folder: Path) -> Path:
    base = folder / "cover_old.jpg"
    if not base.exists():
        return base
    i = 2
    while True:
        cand = folder / f"cover_old_{i}.jpg"
        if not cand.exists():
            return cand
        i += 1


def choose_base_image(folder: Path) -> Path | None:
    cover_old = folder / "cover_old.jpg"
    if cover_old.exists():
        return cover_old

    cover = folder / "cover.jpg"
    if cover.exists():
        dst = unique_old_name(folder)
        cover.rename(dst)
        return dst

    return None


def max_font_size_for_text(draw: ImageDraw.ImageDraw, text: str, w: int, h: int, margin_frac: float = 0.06) -> int:
    max_w = int(w * (1 - 2 * margin_frac))
    max_h = int(h * (1 - 2 * margin_frac))

    lo, hi = 10, max(w, h) * 5
    best = lo
    while lo <= hi:
        mid = (lo + hi) // 2
        font = pick_font(mid)
        bbox = draw.textbbox((0, 0), text, font=font, anchor="lt")
        tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
        if tw <= max_w and th <= max_h:
            best = mid
            lo = mid + 1
        else:
            hi = mid - 1
    return best


def draw_centered_text(im: Image.Image, text: str) -> Image.Image:
    im = im.convert("RGB")
    w, h = im.size
    draw = ImageDraw.Draw(im)

    max_size = max_font_size_for_text(draw, text, w, h)

    # Make text ~10% smaller than max fit
    font_size = max(1, int(max_size * 0.9))
    font = pick_font(font_size)

    draw.text(
        (w / 2, h / 2),
        text,
        font=font,
        fill=(0, 0, 0),
        anchor="mm",
    )

    return im


def process_folder(folder: Path) -> None:
    m = TRAILING_NUMBER_RE.search(folder.name)
    if not m:
        return

    num = m.group(1)
    base = choose_base_image(folder)
    if base is None:
        return

    out_cover = folder / "cover.jpg"

    with Image.open(base) as im0:
        im = draw_centered_text(im0, num)
        im.save(out_cover, format="JPEG", quality=95, subsampling=0)


def main() -> int:
    root = Path(sys.argv[1]).expanduser().resolve() if len(sys.argv) > 1 else Path.cwd()

    if not root.exists() or not root.is_dir():
        print(f"Not a directory: {root}", file=sys.stderr)
        return 2

    for child in sorted(root.iterdir()):
        if child.is_dir() and TRAILING_NUMBER_RE.search(child.name):
            process_folder(child)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())