import os
import re
import shutil

BASE_DIR = os.getcwd()

def clean_filename(name):
    # Remove parenthetical groups
    cleaned = re.sub(r"\s*\([^)]*\)", "", name)

    # Remove trailing underscore segments after volume (e.g., v71_1_1 → v71)
    cleaned = re.sub(r"(v\d+)(?:_\d+)+", r"\1", cleaned)
    
    # Remove extra spaces before file extension
    cleaned = re.sub(r"\s+(\.cbz)$", r"\1", cleaned)
    
    # Normalize multiple spaces
    cleaned = re.sub(r"\s{2,}", " ", cleaned)
    
    return cleaned.strip()

def get_unique_path(path):
    base, ext = os.path.splitext(path)
    counter = 1
    new_path = path
    while os.path.exists(new_path):
        new_path = f"{base}_{counter}{ext}"
        counter += 1
    return new_path

for root, dirs, files in os.walk(BASE_DIR):
    # if root == BASE_DIR:
    #     continue

    for file in files:
        # Skip macOS junk files and hidden files
        if file.startswith("._") or file.startswith("."):
            continue

        # Only process CBZ files
        if not file.lower().endswith(".cbz"):
            continue

        old_path = os.path.join(root, file)

        # Extra safety check
        if not os.path.exists(old_path):
            continue

        new_name = clean_filename(file)
        new_path = os.path.join(BASE_DIR, new_name)
        new_path = get_unique_path(new_path)

        shutil.move(old_path, new_path)
        print(f"Moved: {new_path}")

print("Done.")