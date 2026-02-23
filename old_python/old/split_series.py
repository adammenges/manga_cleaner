#!/usr/bin/env python3
"""
===============================================================================
USAGE
===============================================================================

Split one or more manga/comic folders into batches of 20 volumes each.

- MOVES files (does NOT copy).
- Cleans filenames while moving:
    * Removes all (...) segments
    * Ensures vXXX is always 3 digits (v007, v120, etc.)
- Creates batch folders next to the original:
      <SeriesName> 1
      <SeriesName> 2
      ...
- Each folder gets a generated cover.jpg with a large centered number (if a cover image exists).
- Prints a full execution plan (ALL files) before doing anything, INCLUDING cover generation.
- Asks for confirmation before proceeding.

-------------------------------------------------------------------------------
Install dependency:

    pip3 install pillow

Run:

    python3 split_series.py "/path/to/One Piece" "/path/to/Naruto"

===============================================================================
"""

from __future__ import annotations

import sys
import re
import shutil
from pathlib import Path
from typing import List, Tuple

from PIL import Image, ImageDraw, ImageFont


FILES_PER_FOLDER = 20
VOLUME_EXTS = {".cbz", ".cbr", ".cb7", ".zip"}
COVER_CANDIDATES = ["cover.jpg", "cover.jpeg", "cover.png", "poster.jpg", "poster.png"]

# Print ALL files in the execution plan (fully verbose output).
PLAN_MAX_ITEMS_PER_BATCH = None


# ============================
# Utilities
# ============================

def natural_key(p: Path) -> Tuple:
    parts = re.split(r"(\d+)", p.name.lower())
    return tuple(int(x) if x.isdigit() else x for x in parts)


def clean_volume_filename(src_name: str) -> str:
    """
    Example:
    Inuyasha v07 (2011) (VIZBIG Edition) (Digital).cbz
      -> Inuyasha v007.cbz

    Notes:
    - Removes all "(...)" segments from the stem.
    - Normalizes whitespace.
    - Normalizes the first numeric run after 'v' to 3 digits.
      (e.g., v71_1_1 -> v071; v07 -> v007)
    """
    p = Path(src_name)
    stem = p.stem
    ext = p.suffix

    # Remove parenthetical chunks
    stem = re.sub(r"\s*\([^)]*\)", "", stem)

    # Normalize whitespace
    stem = re.sub(r"\s{2,}", " ", stem).strip()

    # Normalize volume number (first numeric run after 'v')
    m = re.search(r"\bv\s*0*(\d+)", stem, flags=re.IGNORECASE)
    if m:
        vol_num = int(m.group(1))
        title = stem[: m.start()].strip()
        title = re.sub(r"\s{2,}", " ", title)

        # If no title is present (e.g., "v12.cbz"), avoid leading spaces.
        if not title:
            return f"v{vol_num:03d}{ext}"

        return f"{title} v{vol_num:03d}{ext}"

    return f"{stem}{ext}"


def unique_path(dest_dir: Path, filename: str) -> Path:
    candidate = dest_dir / filename
    if not candidate.exists():
        return candidate

    stem = Path(filename).stem
    ext = Path(filename).suffix
    i = 2
    while True:
        candidate = dest_dir / f"{stem} ({i}){ext}"
        if not candidate.exists():
            return candidate
        i += 1


def chunk(lst: List[Path], size: int) -> List[List[Path]]:
    return [lst[i:i + size] for i in range(0, len(lst), size)]


# ============================
# Cover generation
# ============================

def pick_cover(src_dir: Path) -> Path:
    for name in COVER_CANDIDATES:
        candidate = src_dir / name
        if candidate.exists():
            return candidate

    for p in sorted(src_dir.iterdir(), key=natural_key):
        if (
            p.is_file()
            and not p.name.startswith(".")
            and p.suffix.lower() in {".jpg", ".jpeg", ".png", ".webp"}
        ):
            return p

    raise FileNotFoundError(f"No cover image found in {src_dir}")


def load_font(size: int) -> ImageFont.ImageFont:
    candidates = [
        "/System/Library/Fonts/Supplemental/Arial Black.ttf",
        "/System/Library/Fonts/Supplemental/Impact.ttf",
        "/System/Library/Fonts/Supplemental/Helvetica Bold.ttf",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    ]
    for fp in candidates:
        if Path(fp).exists():
            try:
                return ImageFont.truetype(fp, size=size)
            except Exception:
                pass
    return ImageFont.load_default()


def make_numbered_cover(src_cover: Path, dest_cover: Path, n: int) -> None:
    im = Image.open(src_cover).convert("RGBA")
    w, h = im.size

    overlay = Image.new("RGBA", (w, h), (255, 255, 255, 0))
    draw = ImageDraw.Draw(overlay)

    text = str(n)
    cx, cy = w // 2, h // 2

    target_w = int(w * 0.85)
    target_h = int(h * 0.85)

    font_size = int(min(w, h) * 0.9)
    font = load_font(font_size)

    def measure(f: ImageFont.ImageFont) -> Tuple[int, int]:
        # Pillow compatibility: prefer textbbox, fall back to textsize.
        try:
            bbox = draw.textbbox((0, 0), text, font=f)
            return bbox[2] - bbox[0], bbox[3] - bbox[1]
        except Exception:
            try:
                return draw.textsize(text, font=f)  # type: ignore[attr-defined]
            except Exception:
                # Last-resort: approximate
                return int(getattr(f, "size", 0) * len(text)), int(getattr(f, "size", 0))

    tw, th = measure(font)
    while (tw > target_w or th > target_h) and font_size > 10:
        font_size = int(font_size * 0.92)
        font = load_font(font_size)
        tw, th = measure(font)

    # Center manually for broad Pillow compatibility.
    x = int(cx - tw / 2)
    y = int(cy - th / 2)
    draw.text((x, y), text, font=font, fill=(0, 0, 0, 200))

    out = Image.alpha_composite(im, overlay).convert("RGB")
    dest_cover.parent.mkdir(parents=True, exist_ok=True)
    out.save(dest_cover, quality=95)


# ============================
# Planning + Execution
# ============================

def scan_volumes(src: Path) -> List[Path]:
    vols = [
        p for p in src.iterdir()
        if (
            p.is_file()
            and not p.name.startswith(".")
            and p.suffix.lower() in VOLUME_EXTS
        )
    ]
    vols.sort(key=natural_key)
    return vols


def print_plan(series_name: str, dest_parent: Path, groups: List[List[Path]], cover_path: Path | None) -> None:
    print("\n" + "=" * 90)
    print(f"[PLAN] Series: {series_name}")
    print(f"[PLAN] Destination parent: {dest_parent}")
    print(f"[PLAN] Batch size: {FILES_PER_FOLDER}")
    if cover_path:
        print(f"[PLAN] Cover source: {cover_path.name}")
        print("[PLAN] Cover output: cover.jpg in each batch folder")
    else:
        print("[PLAN] Cover: (none) — will skip cover.jpg generation")
    print("=" * 90)

    for idx, group in enumerate(groups, start=1):
        start_num = (idx - 1) * FILES_PER_FOLDER + 1
        end_num = start_num + len(group) - 1
        batch_dir = dest_parent / f"{series_name} {idx}"

        print(f"\n{series_name} {idx}  (files {start_num}–{end_num})")
        print(f"  [DIR] {batch_dir}")

        if cover_path:
            print(f"  [COVER] {batch_dir / 'cover.jpg'}  (from {cover_path.name})")

        for j, p in enumerate(group, start=1):
            cleaned = clean_volume_filename(p.name)
            number = start_num + j - 1
            if cleaned != p.name:
                print(f"  {number:>4}. {p.name}  ->  {cleaned}")
            else:
                print(f"  {number:>4}. {p.name}")

    print("\n" + "=" * 90)


def confirm_plan() -> bool:
    while True:
        ans = input("\nProceed with this plan? [y/N]: ").strip().lower()
        if ans in {"y", "yes"}:
            return True
        if ans in {"", "n", "no"}:
            return False
        print("Please answer y or n.")


def process_series(src: Path) -> None:
    series_name = src.name

    # Write directly next to the original folder.
    # Example outputs:
    #   /path/to/One Piece 1
    #   /path/to/One Piece 2
    #   ...
    dest_parent = src.parent

    print(f"\n[INIT] Processing: {series_name}")
    print(f"[INIT] Source: {src}")
    print(f"[INIT] Destination parent: {dest_parent}")

    volumes = scan_volumes(src)
    if not volumes:
        print("[ERROR] No valid volume files found.")
        return

    # Cover is optional: splitting should still work without it.
    cover_path: Path | None
    try:
        cover_path = pick_cover(src)
    except FileNotFoundError:
        cover_path = None
        print("[WARN] No cover image found; will skip cover.jpg generation.")

    groups = chunk(volumes, FILES_PER_FOLDER)

    # Print plan INCLUDING cover outputs.
    print_plan(series_name, dest_parent, groups, cover_path)

    if not confirm_plan():
        print("[SKIP] Aborted by user.")
        return

    for idx, group in enumerate(groups, start=1):
        start_num = (idx - 1) * FILES_PER_FOLDER + 1
        end_num = start_num + len(group) - 1

        out_dir = dest_parent / f"{series_name} {idx}"
        print("\n" + "-" * 90)
        print(f"[BATCH] Creating {out_dir.name} (files {start_num}–{end_num})")
        print("-" * 90)

        out_dir.mkdir(parents=True, exist_ok=True)

        for i, p in enumerate(group, start=1):
            cleaned_name = clean_volume_filename(p.name)
            dest_path = unique_path(out_dir, cleaned_name)
            print(f"[MOVE] ({i}/{len(group)}) {p.name} -> {dest_path.name}")
            shutil.move(str(p), str(dest_path))

        if cover_path is not None:
            make_numbered_cover(cover_path, out_dir / "cover.jpg", idx)
            print(f"[COVER] Wrote {out_dir / 'cover.jpg'}")

        print(f"[DONE] {out_dir}")

    print(f"\n[COMPLETE] Finished: {series_name}")


def main():
    if len(sys.argv) < 2:
        print('Usage: python3 split_series.py "/path/to/Series1" "/path/to/Series2" ...')
        sys.exit(1)

    for path in sys.argv[1:]:
        src = Path(path).expanduser().resolve()
        if not src.exists() or not src.is_dir():
            print(f"[ERROR] Invalid folder: {src}")
            continue
        process_series(src)


if __name__ == "__main__":
    main()