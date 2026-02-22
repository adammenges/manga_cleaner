Manga Toolkit

One-command script to organize a long manga series folder.

You pass one argument: the path to your longest series folder.
It shows a full plan, asks for confirmation, then does everything in one pass.

⸻

What It Does 1. Ensures a series cover exists
• Uses existing cover.jpg, cover*old.jpg, poster.jpg, etc.
• If none exist, downloads cover.jpg from:
• MangaDex
• AniList
• Kitsu 2. Scans volume archives
• Supports: .cbz, .cbr, .cb7, .zip
• Ignores hidden/macOS junk files 3. Shows a detailed plan
• Batch folders (SeriesName 1, SeriesName 2, …)
• Per-file renames
• Cover actions
• Volume ranges per batch 4. On confirmation (y) executes everything
• Splits into batches of 20 volumes
• Cleans filenames
• Creates:
• cover_old.jpg (base image, preserved)
• cover.jpg (batch number placed dead-center)
• Archives existing cover.jpg → cover_old*\*.jpg

⸻

Installation

pip install pillow requests

⸻

Usage

python3 manga_toolkit.py "/path/to/Your Longest Series Folder"

Example:

python3 manga_toolkit.py "/Volumes/Manga/Manga/local/One Piece"

⸻

Filename Cleanup Rules

Examples:

Naruto (CM) v55.cbz → Naruto v055.cbz
Naruto v71_1_1.cbz → Naruto v071.cbz

Rules:
• Removes ( … ) segments
• Normalizes spacing
• Collapses v71_1_1 → v71
• Pads volumes to 3 digits (v001, v045, v123)

⸻

Cover Behavior

Series Folder

If no cover exists:
• Downloads cover.jpg automatically.

Batch Folders

Each batch folder gets:

cover_old.jpg ← preserved base image
cover.jpg ← rendered with batch number

If a batch already contains cover.jpg, it is archived:

cover.jpg → cover_old_2.jpg

Number Placement

The batch number is:
• Scaled to fill the image
• Placed exactly dead-center
• Centered using glyph bounding box correction (not naive anchor centering)

⸻

Output Structure Example

Starting folder:

One Piece/
One Piece v001.cbz
One Piece v002.cbz
...

After running:

One Piece 1/
One Piece v001.cbz
...
One Piece v020.cbz
cover_old.jpg
cover.jpg

One Piece 2/
One Piece v021.cbz
...

⸻

Safety Model
• Always prints a full plan first.
• Nothing happens until you confirm.
• Uses collision-safe renaming.
• Archives old covers instead of deleting them.

⸻

Requirements
• Python 3.9+
• Pillow
• requests

⸻

Designed For

Large, messy manga folders that need:
• Clean naming
• Logical batch grouping
• Consistent numbered covers
• Zero manual intervention
