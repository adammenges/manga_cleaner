# Manga Toolkit

`process_manga.py` is a one-command tool that cleans and organizes a manga series folder.

You give it one folder path (usually your largest or "main" series folder), and it will:

1. Build a full preview plan.
2. Ask for your confirmation.
3. Apply all changes in one run.

The goal is to make messy folders consistent, readable, and ready to browse in your library.

## What This Script Does

When you run the script, it performs these steps:

1. Ensure a cover image exists for the series.

- It first tries Mihon-style local cover extraction:
  - first volume file in natural order
  - first image inside that volume (currently supported for `.cbz` / `.zip`)
- If that is unavailable, it uses a local cover file (`cover.jpg`, `cover_old.jpg`, `poster.jpg`, etc.).
- If no local cover can be used, it tries to download one from:
  - MangaDex
  - AniList
  - Kitsu
  - MangaDex is preferred first and now strictly targets volume 1 cover entries.

2. Scan volume archive files.

- Supported file types: `.cbz`, `.cbr`, `.cb7`, `.zip`
- Hidden and macOS junk files (like `._*`) are ignored.

3. Show you a detailed plan before making changes.

- Planned batch folders (`Series Name 1`, `Series Name 2`, ...)
- Per-file rename actions
- File move actions
- Cover image actions
- Volume ranges per batch

4. Execute only after you confirm with `y` or `yes`.

- Split volumes into folders of 20 files each
- Clean and normalize file names
- Create numbered batch covers
- Archive existing covers safely instead of overwriting

## Installation

Install required Python packages:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

## Usage

Run the script with exactly one argument: the series folder path.

```bash
python3 process_manga.py "/path/to/Your Series Folder"
```

Preview and open the selected cover image:

```bash
python3 process_manga.py --show-cover "/path/to/Your Series Folder"
```

Example:

```bash
python3 process_manga.py "/Volumes/Manga/Manga/local/One Piece"
```

## Filename Cleanup Rules

The script normalizes naming to keep files clean and sortable.

Examples:

```text
Naruto (CM) v55.cbz   -> Naruto v055.cbz
Naruto v71_1_1.cbz    -> Naruto v071.cbz
```

Rules:

- Remove parenthesized segments like `(CM)` or `(Digital)`.
- Normalize spacing and separators.
- Collapse variants like `v71_1_1` to `v71`.
- Zero-pad volume numbers to 3 digits (`v001`, `v045`, `v123`).

## Cover Behavior

### Series folder cover

The script prioritizes cover selection in this order:

1. First volume archive cover (first image from first volume file, Mihon-style).
2. Existing local cover files in the series folder.
3. Downloaded `cover.jpg` from MangaDex/AniList/Kitsu fallback.

### Batch folder covers

Each batch folder receives:

- `cover_old.jpg`: preserved base image
- `cover.jpg`: generated image with the batch number centered

If a batch folder already has `cover.jpg`, it is archived instead of replaced directly:

```text
cover.jpg -> cover_old_2.jpg
```

### Batch number placement

The batch number is rendered to be:

- Large and readable
- Scaled to fill the cover well
- Centered precisely (using glyph bounding-box centering)

## Output Example

Input:

```text
One Piece/
  One Piece v001.cbz
  One Piece v002.cbz
  ...
```

After processing:

```text
One Piece 1/
  One Piece v001.cbz
  ...
  One Piece v020.cbz
  cover_old.jpg
  cover.jpg

One Piece 2/
  One Piece v021.cbz
  ...
```

## Safety Model

This tool is designed to avoid destructive behavior:

- Prints a complete plan before changing anything
- Does nothing until you confirm
- Uses collision-safe renaming
- Archives existing covers instead of deleting them

## Requirements

- Python 3.9+
- `Pillow`
- `requests`

## Best For

This script is ideal for large, inconsistent manga folders that need:

- Cleaner file names
- Predictable folder grouping
- Consistent numbered covers
- A mostly hands-off workflow
