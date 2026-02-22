#!/usr/bin/env python3
"""
process_manga.py

One-command "do everything" script.

You pass exactly ONE argument: the path to a series folder (your "longest series").

What it does:
1) Ensures a series cover exists:
   - Prefers first-volume cover extraction (first image in first volume .cbz/.zip)
   - Falls back to local cover image in series folder
   - Otherwise downloads cover.jpg (MangaDex -> AniList -> Kitsu)
2) Scans the folder for volume archives (.cbz/.cbr/.cb7/.zip)
3) Shows a detailed PLAN (batch folders, renames, moves, cover actions)
4) On confirmation, executes ALL actions at once:
   - Splits into batch folders of 20 volumes each (SeriesName 1, SeriesName 2, ...)
   - Renames volumes to clean, consistent names
   - Creates cover_old.jpg in each batch (copied from the series' cover image)
   - Generates cover.jpg in each batch with the batch number placed DEAD-CENTER
   - If a batch already has cover.jpg, it is archived to cover_old_*.jpg first

Dependencies:
  pip install pillow requests

Example:
  python3 process_manga.py "/Volumes/Manga/Manga/local/One Piece"
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Sequence, Set, Tuple

import requests
from PIL import Image, ImageDraw, ImageFont


# -----------------------------
# Config
# -----------------------------

FILES_PER_FOLDER = 20
VOLUME_EXTS = {".cbz", ".cbr", ".cb7", ".zip"}
IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".webp"}

COVER_CANDIDATES = [
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "poster.jpg",
    "poster.png",
    "cover_old.jpg",
]

USER_AGENT = "manga-toolkit/1.1 (+https://example.invalid)"


# -----------------------------
# Small utils
# -----------------------------

def is_hidden_or_macos_junk(name: str) -> bool:
    return name.startswith(".") or name.startswith("._")


def natural_key_name(name: str) -> Tuple:
    parts = re.split(r"(\d+)", name.lower())
    return tuple(int(x) if x.isdigit() else x for x in parts)


def natural_key_path(p: Path) -> Tuple:
    return natural_key_name(p.name)


def ensure_dir(p: Path) -> None:
    p.mkdir(parents=True, exist_ok=True)


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


def unique_path_reserved(dest_dir: Path, filename: str, reserved: Set[str]) -> Path:
    candidate = dest_dir / filename
    if not candidate.exists() and candidate.name not in reserved:
        reserved.add(candidate.name)
        return candidate

    stem = Path(filename).stem
    ext = Path(filename).suffix
    i = 2
    while True:
        candidate = dest_dir / f"{stem} ({i}){ext}"
        if not candidate.exists() and candidate.name not in reserved:
            reserved.add(candidate.name)
            return candidate
        i += 1


def unique_cover_old_path(dest_dir: Path) -> Path:
    p = dest_dir / "cover_old.jpg"
    if not p.exists():
        return p
    i = 2
    while True:
        p2 = dest_dir / f"cover_old_{i}.jpg"
        if not p2.exists():
            return p2
        i += 1


def confirm(prompt: str) -> bool:
    ans = input(prompt).strip().lower()
    return ans in {"y", "yes"}


# -----------------------------
# Cover download (MangaDex -> AniList -> Kitsu)
# -----------------------------

@dataclass(frozen=True)
class CoverResult:
    source: str
    url: str


@dataclass(frozen=True)
class VolumeCoverResult:
    volume_file: Path
    image_entry: str
    output_file: Path


def _http_get_json(url: str, *, params=None, headers=None, timeout=20) -> dict:
    h = {"User-Agent": USER_AGENT, **(headers or {})}
    r = requests.get(url, params=params, headers=h, timeout=timeout)
    r.raise_for_status()
    return r.json()


def _http_post_json(url: str, *, payload: dict, headers=None, timeout=20) -> dict:
    h = {"User-Agent": USER_AGENT, "Content-Type": "application/json", **(headers or {})}
    r = requests.post(url, data=json.dumps(payload), headers=h, timeout=timeout)
    r.raise_for_status()
    return r.json()


def _download_file(url: str, out_path: Path, *, timeout=30) -> None:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    h = {"User-Agent": USER_AGENT, "Referer": "https://mangadex.org/"}
    with requests.get(url, headers=h, stream=True, timeout=timeout) as r:
        r.raise_for_status()
        with open(out_path, "wb") as f:
            for chunk in r.iter_content(chunk_size=1024 * 128):
                if chunk:
                    f.write(chunk)


def _best_title(attrs: dict) -> str:
    t = (attrs.get("title") or {})
    if not isinstance(t, dict) or not t:
        return ""
    return t.get("en") or next(iter(t.values()), "")


def _normalize_title(s: str) -> str:
    return re.sub(r"[^a-z0-9]+", "", s.lower())


def _parse_int_volume(vol: object) -> Optional[int]:
    if not isinstance(vol, str):
        return None
    m = re.fullmatch(r"\s*0*(\d+)(?:\.0+)?\s*", vol)
    if not m:
        return None
    return int(m.group(1))


def fetch_cover_mangadex(title: str, *, size: str = "best") -> Optional[CoverResult]:
    base = "https://api.mangadex.org"

    data = _http_get_json(f"{base}/manga", params={"title": title, "limit": 5})
    items = data.get("data") or []
    if not items:
        return None

    title_l = title.strip().lower()
    title_n = _normalize_title(title_l)

    def score(item: dict) -> int:
        attrs = item.get("attributes") or {}
        main = _best_title(attrs).strip().lower()
        main_n = _normalize_title(main)
        alts = attrs.get("altTitles") or []
        alt_vals = []
        alt_norms = []
        for a in alts:
            if isinstance(a, dict):
                vals = [str(v).strip().lower() for v in a.values()]
                alt_vals.extend(vals)
                alt_norms.extend([_normalize_title(v) for v in vals])

        if main_n == title_n:
            return 6
        if any(vn == title_n for vn in alt_norms):
            return 5
        if main == title_l:
            return 4
        if any(v == title_l for v in alt_vals):
            return 3
        if title_l in main:
            return 2
        if any(title_l in v for v in alt_vals):
            return 1
        return 1

    items.sort(key=score, reverse=True)
    manga = items[0]
    manga_id = manga.get("id")
    if not manga_id:
        return None

    # Strictly require the first-volume cover (v1/01/001 etc.) when available.
    cover_id: Optional[str] = None
    try:
        covers_resp = _http_get_json(
            f"{base}/cover",
            params={
                "manga[]": manga_id,
                "limit": 100,
                "order[createdAt]": "asc",
            },
        )
        covers = covers_resp.get("data") or []
        first_volume_covers = []
        for c in covers:
            attrs = c.get("attributes") or {}
            vol_num = _parse_int_volume(attrs.get("volume"))
            if vol_num == 1:
                first_volume_covers.append(c)
        if first_volume_covers:
            cover_id = first_volume_covers[0].get("id")
    except Exception:
        cover_id = None

    if not cover_id:
        # If MangaDex has no explicit volume-1 cover entry, skip MangaDex result.
        return None

    cover = _http_get_json(f"{base}/cover/{cover_id}")
    file_name = (((cover.get("data") or {}).get("attributes") or {}).get("fileName"))
    if not file_name:
        return None

    url = f"https://uploads.mangadex.org/covers/{manga_id}/{file_name}"
    if size == "512":
        url = f"{url}.512.jpg"
    elif size == "256":
        url = f"{url}.256.jpg"

    return CoverResult(source="mangadex", url=url)


def fetch_cover_anilist(title: str) -> Optional[CoverResult]:
    endpoint = "https://graphql.anilist.co"
    query = """
    query ($search: String) {
      Media(search: $search, type: MANGA) {
        id
        coverImage { extraLarge large }
      }
    }
    """
    resp = _http_post_json(endpoint, payload={"query": query, "variables": {"search": title}})
    media = (resp.get("data") or {}).get("Media")
    if not media:
        return None
    ci = media.get("coverImage") or {}
    url = ci.get("extraLarge") or ci.get("large")
    if not url:
        return None
    return CoverResult(source="anilist", url=url)


def fetch_cover_kitsu(title: str) -> Optional[CoverResult]:
    base = "https://kitsu.io/api/edge"
    data = _http_get_json(f"{base}/manga", params={"filter[text]": title, "page[limit]": 5})
    items = data.get("data") or []
    if not items:
        return None

    attrs = (items[0].get("attributes") or {})
    cover = attrs.get("coverImage") or {}
    url = cover.get("original") or cover.get("large") or cover.get("small") or cover.get("tiny")
    if not url:
        return None
    return CoverResult(source="kitsu", url=url)


def _zip_entry_is_image(entry_name: str) -> bool:
    p = Path(entry_name)
    if not p.suffix.lower() in IMAGE_EXTS:
        return False
    for part in p.parts:
        if part == "__MACOSX" or is_hidden_or_macos_junk(part):
            return False
    return True


def _first_image_entry_in_zip(volume_file: Path) -> Optional[str]:
    with zipfile.ZipFile(volume_file) as zf:
        entries = [
            info.filename for info in zf.infolist()
            if not info.is_dir() and _zip_entry_is_image(info.filename)
        ]
        if not entries:
            return None
        entries.sort(key=natural_key_name)
        return entries[0]


def find_first_volume_cover(series_dir: Path) -> Tuple[Optional[VolumeCoverResult], Optional[Exception]]:
    """
    Mihon-like local logic:
    choose the first volume and take the first image found in that archive.
    """
    try:
        vols = scan_volumes(series_dir)
        if not vols:
            return None, None

        first_vol = vols[0]
        if first_vol.suffix.lower() not in {".cbz", ".zip"}:
            return None, RuntimeError(
                f"first volume is {first_vol.suffix.lower()} (local extraction currently supports .cbz/.zip only)",
            )

        first_image = _first_image_entry_in_zip(first_vol)
        if not first_image:
            return None, RuntimeError(f"no image files found in first volume archive: {first_vol.name}")

        return VolumeCoverResult(
            volume_file=first_vol,
            image_entry=first_image,
            output_file=series_dir / "cover.jpg",
        ), None
    except Exception as e:
        return None, e


def write_volume_cover(result: VolumeCoverResult) -> Path:
    result.output_file.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(result.volume_file) as zf:
        with zf.open(result.image_entry) as src:
            with Image.open(src) as im:
                im.convert("RGB").save(result.output_file, format="JPEG", quality=95, subsampling=0)
    return result.output_file


def ensure_cover_jpg(series_dir: Path, selected_cover: Path) -> Path:
    cover_jpg = series_dir / "cover.jpg"
    if selected_cover.resolve() == cover_jpg.resolve():
        return cover_jpg

    with Image.open(selected_cover) as im:
        im.convert("RGB").save(cover_jpg, format="JPEG", quality=95, subsampling=0)
    return cover_jpg


def open_image(path: Path) -> None:
    # Use OS default viewer
    if sys.platform == "darwin":
        cmd = ["open", str(path)]
    elif sys.platform.startswith("win"):
        cmd = ["cmd", "/c", "start", "", str(path)]
    else:
        cmd = ["xdg-open", str(path)]

    subprocess.run(cmd, check=True)


def find_remote_cover(title: str) -> Tuple[Optional[CoverResult], Optional[Exception]]:
    fetchers = [
        ("mangadex", lambda: fetch_cover_mangadex(title, size="best")),
        ("anilist", lambda: fetch_cover_anilist(title)),
        ("kitsu", lambda: fetch_cover_kitsu(title)),
    ]

    last_err: Optional[Exception] = None
    for _, fn in fetchers:
        try:
            res = fn()
            if res:
                return res, None
        except Exception as e:
            last_err = e
            continue
    return None, last_err


def ensure_series_cover(series_dir: Path, title: str) -> Optional[Path]:
    """
    Return a cover image path for the series folder.
    If none exists locally, attempt to download cover.jpg into the folder.
    """
    first_vol_cover, first_vol_err = find_first_volume_cover(series_dir)
    if first_vol_cover is not None:
        try:
            out = write_volume_cover(first_vol_cover)
            print(
                f"[COVER] Extracted series cover from first volume: {out} "
                f"(source={first_vol_cover.volume_file.name}:{first_vol_cover.image_entry})",
            )
            return out
        except Exception as e:
            first_vol_err = e

    existing = choose_series_cover(series_dir)
    if existing is not None:
        return existing

    out_file = series_dir / "cover.jpg"
    res, last_err = find_remote_cover(title)
    if res:
        try:
            _download_file(res.url, out_file)
            print(f"[COVER] Downloaded series cover: {out_file} (source={res.source})")
            return out_file
        except Exception as e:
            last_err = e

    if first_vol_err:
        print(f"[WARN] Failed to extract first-volume cover. Last error: {first_vol_err}", file=sys.stderr)

    if last_err:
        print(f"[WARN] Failed to download series cover. Last error: {last_err}", file=sys.stderr)
    else:
        print("[WARN] Failed to download series cover (no results).", file=sys.stderr)
    return None


# -----------------------------
# Filename cleaning
# -----------------------------

def clean_volume_filename(src_name: str, pad_to_3: bool = True) -> str:
    p = Path(src_name)
    stem, ext = p.stem, p.suffix

    stem = re.sub(r"\s*\([^)]*\)", "", stem)
    stem = re.sub(r"\s{2,}", " ", stem).strip()

    stem = re.sub(r"(v\s*\d+)(?:_\d+)+", r"\1", stem, flags=re.IGNORECASE)

    m = re.search(r"\bv\s*0*(\d+)", stem, flags=re.IGNORECASE)
    if m:
        vol_num = int(m.group(1))
        title = stem[: m.start()].strip()
        title = re.sub(r"\s{2,}", " ", title).strip()

        vpart = f"v{vol_num:03d}" if pad_to_3 else f"v{vol_num}"

        if not title:
            return f"{vpart}{ext}"
        return f"{title} {vpart}{ext}"

    return f"{stem}{ext}"


# -----------------------------
# Cover generation (DEAD CENTER)
# -----------------------------

def pick_font(size: int) -> ImageFont.ImageFont:
    candidates = [
        "/System/Library/Fonts/Supplemental/Arial Black.ttf",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        "/System/Library/Fonts/Supplemental/Impact.ttf",
        "/System/Library/Fonts/Supplemental/Helvetica Bold.ttf",
        "/Library/Fonts/Arial Black.ttf",
        "/Library/Fonts/Arial Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    ]
    for fp in candidates:
        p = Path(fp)
        if p.exists():
            try:
                return ImageFont.truetype(str(p), size=size)
            except Exception:
                pass
    return ImageFont.load_default()


def fit_font_size(draw: ImageDraw.ImageDraw, text: str, w: int, h: int, margin_frac: float = 0.06) -> int:
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


def draw_dead_center_text(base_im: Image.Image, text: str, opacity: int = 255, scale: float = 0.90) -> Image.Image:
    im = base_im.convert("RGBA")
    w, h = im.size

    overlay = Image.new("RGBA", (w, h), (255, 255, 255, 0))
    draw = ImageDraw.Draw(overlay)

    max_size = fit_font_size(draw, text, w, h)
    font_size = max(10, int(max_size * scale))
    font = pick_font(font_size)

    cx, cy = w / 2, h / 2
    bbox = draw.textbbox((cx, cy), text, font=font, anchor="mm")
    bx0, by0, bx1, by1 = bbox
    bcx, bcy = (bx0 + bx1) / 2, (by0 + by1) / 2
    dx, dy = cx - bcx, cy - bcy

    draw.text((cx + dx, cy + dy), text, font=font, fill=(0, 0, 0, opacity), anchor="mm")
    return Image.alpha_composite(im, overlay).convert("RGB")


def choose_series_cover(series_dir: Path) -> Optional[Path]:
    for name in COVER_CANDIDATES:
        p = series_dir / name
        if p.exists() and p.is_file():
            return p
    imgs = [
        p for p in series_dir.iterdir()
        if p.is_file() and not is_hidden_or_macos_junk(p.name) and p.suffix.lower() in IMAGE_EXTS
    ]
    if imgs:
        imgs.sort(key=natural_key_path)
        return imgs[0]
    return None


def ensure_cover_old(batch_dir: Path, series_cover: Path) -> Path:
    primary = batch_dir / "cover_old.jpg"
    if primary.exists():
        return primary

    target = unique_cover_old_path(batch_dir)
    shutil.copy2(str(series_cover), str(target))
    return target


def archive_existing_cover_jpg(batch_dir: Path) -> Optional[Path]:
    cover = batch_dir / "cover.jpg"
    if not cover.exists():
        return None
    dst = unique_cover_old_path(batch_dir)
    cover.rename(dst)
    return dst


def write_numbered_cover(batch_dir: Path, number: int, series_cover: Path) -> None:
    ensure_dir(batch_dir)
    archive_existing_cover_jpg(batch_dir)
    base = ensure_cover_old(batch_dir, series_cover)

    with Image.open(base) as im0:
        rendered = draw_dead_center_text(im0, str(number), opacity=255, scale=0.90)
        rendered.save(batch_dir / "cover.jpg", format="JPEG", quality=95, subsampling=0)


# -----------------------------
# Planning + execution
# -----------------------------

@dataclass(frozen=True)
class FileMove:
    src: Path
    dst: Path
    dst_name: str


@dataclass(frozen=True)
class BatchPlan:
    batch_index: int
    batch_dir: Path
    moves: List[FileMove]
    will_make_cover: bool


def scan_volumes(series_dir: Path) -> List[Path]:
    vols = [
        p for p in series_dir.iterdir()
        if p.is_file() and not is_hidden_or_macos_junk(p.name) and p.suffix.lower() in VOLUME_EXTS
    ]
    vols.sort(key=natural_key_path)
    return vols


def chunk(lst: List[Path], size: int) -> List[List[Path]]:
    return [lst[i:i + size] for i in range(0, len(lst), size)]


def build_plan(series_dir: Path, series_cover: Optional[Path]) -> List[BatchPlan]:
    vols = scan_volumes(series_dir)
    if not vols:
        raise RuntimeError(f"No volume files found in: {series_dir}")

    groups = chunk(vols, FILES_PER_FOLDER)
    dest_parent = series_dir.parent

    plan: List[BatchPlan] = []
    for idx, group in enumerate(groups, start=1):
        batch_dir = dest_parent / f"{series_dir.name} {idx}"
        moves: List[FileMove] = []
        reserved_names: Set[str] = set()
        for p in group:
            cleaned = clean_volume_filename(p.name, pad_to_3=True)
            dst = unique_path_reserved(batch_dir, cleaned, reserved_names)
            moves.append(FileMove(src=p, dst=dst, dst_name=dst.name))

        plan.append(
            BatchPlan(
                batch_index=idx,
                batch_dir=batch_dir,
                moves=moves,
                will_make_cover=(series_cover is not None),
            )
        )

    return plan


def print_plan(series_dir: Path, plan: Sequence[BatchPlan], series_cover: Optional[Path]) -> None:
    vols_count = sum(len(b.moves) for b in plan)
    print("\n" + "=" * 98)
    print("[PLAN] Manga toolkit")
    print(f"[PLAN] Series folder: {series_dir}")
    print(f"[PLAN] Volumes found: {vols_count}")
    print(f"[PLAN] Batch size: {FILES_PER_FOLDER}")
    if series_cover:
        print(f"[PLAN] Series cover source: {series_cover.name}")
        print("[PLAN] Each batch will have:")
        print("       - cover_old.jpg (copied once from series cover, preserved)")
        print("       - cover.jpg (rendered with batch number DEAD-CENTER)")
        print("       - any existing cover.jpg will be archived to cover_old_*.jpg")
    else:
        print("[PLAN] Covers: skipped (no cover image found/downloaded)")
    print("=" * 98)

    for b in plan:
        start_idx = (b.batch_index - 1) * FILES_PER_FOLDER + 1
        end_idx = start_idx + len(b.moves) - 1
        print(f"\n{series_dir.name} {b.batch_index}  (volumes {start_idx}–{end_idx})")
        print(f"  [DIR] {b.batch_dir}")
        if series_cover:
            print(f"  [COVER] cover_old.jpg + cover.jpg (number {b.batch_index})")

        for i, mv in enumerate(b.moves, start=1):
            n = start_idx + i - 1
            rename_note = "" if mv.src.name == mv.dst_name else f"  (rename: {mv.src.name} -> {mv.dst_name})"
            print(f"  {n:>4}. {mv.src.name}{rename_note}")

    print("\n" + "=" * 98)


def execute(plan: Sequence[BatchPlan], series_cover: Optional[Path]) -> None:
    for b in plan:
        ensure_dir(b.batch_dir)

        print("\n" + "-" * 98)
        print(f"[DO] Batch {b.batch_index}: {b.batch_dir.name}")
        print("-" * 98)

        for i, mv in enumerate(b.moves, start=1):
            print(f"[MOVE] ({i}/{len(b.moves)}) {mv.src.name} -> {mv.dst_name}")
            ensure_dir(mv.dst.parent)
            shutil.move(str(mv.src), str(mv.dst))

        if series_cover:
            print(f"[COVER] Rendering cover.jpg (batch number {b.batch_index})")
            write_numbered_cover(b.batch_dir, b.batch_index, series_cover)

    print("\n[COMPLETE] Done.")


# -----------------------------
# Main
# -----------------------------

def main(argv: Optional[Sequence[str]] = None) -> int:
    argv = list(argv) if argv is not None else sys.argv[1:]
    parser = argparse.ArgumentParser(description="Clean and batch manga files with numbered covers.")
    parser.add_argument("series_dir", help="Path to the series folder")
    parser.add_argument(
        "--show-cover",
        action="store_true",
        help="Resolve the selected cover, ensure cover.jpg exists, open it, then exit.",
    )
    args = parser.parse_args(argv)

    series_dir = Path(args.series_dir).expanduser().resolve()
    if not series_dir.is_dir():
        print(f"[ERROR] Not a directory: {series_dir}", file=sys.stderr)
        return 2

    if args.show_cover:
        series_cover = ensure_series_cover(series_dir, title=series_dir.name)
        if series_cover is None:
            print("[COVER-CHECK] No cover found from local files or remote providers.", file=sys.stderr)
            return 1

        try:
            cover_jpg = ensure_cover_jpg(series_dir, series_cover)
            print(f"[COVER-CHECK] Opening: {cover_jpg}")
            open_image(cover_jpg)
            return 0
        except Exception as e:
            print(f"[COVER-CHECK] Failed to open cover image: {e}", file=sys.stderr)
            return 1

    # Ensure series cover exists (download if missing)
    series_cover = ensure_series_cover(series_dir, title=series_dir.name)

    try:
        plan = build_plan(series_dir, series_cover)
    except Exception as e:
        print(f"[ERROR] {e}", file=sys.stderr)
        return 2

    print_plan(series_dir, plan, series_cover)

    if not confirm("\nProceed and execute everything now? [y/N]: "):
        print("[SKIP] Aborted by user.")
        return 0

    execute(plan, series_cover)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
