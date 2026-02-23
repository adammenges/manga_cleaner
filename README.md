# Manga Cleaner

`manga_cleaner_native` is now the primary app for this project: a fully native Rust desktop app built with Iced.

## Primary Usage (Rust + Cargo)

### 1. Install Rust (macOS)

```bash
curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"
```

### 2. Build the app

```bash
cargo build --release
```

### 3. Run the native desktop app (recommended)

```bash
cargo run --release --bin manga_cleaner_native
```

Optional: start with a prefilled series folder path.

```bash
cargo run --release --bin manga_cleaner_native -- "/path/to/Your Series Folder"
```

### 4. Run the Rust CLI

```bash
cargo run --release --bin process_manga_rs -- "/path/to/Your Series Folder"
```

Common CLI options:

```bash
# Preview plan only (no file changes)
cargo run --release --bin process_manga_rs -- --dry-run "/path/to/Your Series Folder"

# Execute without prompt
cargo run --release --bin process_manga_rs -- --yes "/path/to/Your Series Folder"

# Resolve + open selected cover
cargo run --release --bin process_manga_rs -- --show-cover "/path/to/Your Series Folder"
```

## What the App Does

Given one series folder, Manga Cleaner will:

1. Resolve a series cover image.
2. Scan volume archives (`.cbz`, `.cbr`, `.cb7`, `.zip`).
3. Build and show a full execution plan.
4. Process volumes into batches of 20.
5. Normalize filenames.
6. Generate numbered batch covers.

### Cover resolution order

1. First image in first volume archive (`.cbz`/`.zip`) if available.
2. Existing local cover files in the series folder.
3. Remote fallback (`MangaDex -> AniList -> Kitsu`).

### Filename normalization

Examples:

```text
Naruto (CM) v55.cbz   -> Naruto v055.cbz
Naruto v71_1_1.cbz    -> Naruto v071.cbz
```

Rules:

- Remove parenthesized suffixes like `(CM)` or `(Digital)`.
- Collapse patterns like `v71_1_1` to `v71`.
- Zero-pad volume numbers to 3 digits (`v001`, `v045`, `v123`).

### Batch cover behavior

Each output folder receives:

- `cover_old.jpg` (preserved base)
- `cover.jpg` (generated number overlay)

If `cover.jpg` already exists, it is archived first (for example `cover_old_2.jpg`).

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

- Prints a complete plan before changing files.
- Supports dry-run mode.
- Uses collision-safe naming.
- Archives existing covers instead of deleting.

---

## Legacy Python Workflow (Old)

The Python tools are kept for compatibility, but are no longer the primary path.

### Install Python dependencies

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

### Old CLI

```bash
python3 process_manga.py "/path/to/Your Series Folder"
```

Useful options:

```bash
python3 process_manga.py --dry-run "/path/to/Your Series Folder"
python3 process_manga.py --yes "/path/to/Your Series Folder"
python3 process_manga.py --show-cover "/path/to/Your Series Folder"
```

### Old browser UI

```bash
python3 process_manga_ui.py
```

Optional with prefilled folder:

```bash
python3 process_manga_ui.py "/path/to/Your Series Folder"
```
