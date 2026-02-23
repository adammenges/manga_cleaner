use std::{
    cmp::Reverse,
    collections::HashSet,
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    time::Duration,
};

use ab_glyph::{FontArc, PxScale};
use anyhow::{anyhow, bail, Context, Result};
use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageReader, Rgba, RgbaImage};
use imageproc::drawing::{draw_text_mut, text_size};
use natord::compare_ignore_case;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use zip::ZipArchive;

pub const FILES_PER_FOLDER: usize = 20;
pub const VOLUME_EXTS: &[&str] = &[".cbz", ".cbr", ".cb7", ".zip"];
pub const IMAGE_EXTS: &[&str] = &[".jpg", ".jpeg", ".png", ".webp", ".bmp", ".gif"];

pub const COVER_CANDIDATES: &[&str] = &[
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "poster.jpg",
    "poster.png",
    "cover_old.jpg",
];

pub const USER_AGENT: &str = "manga-toolkit-rust/1.0 (+https://example.invalid)";

static PARENS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*\([^)]*\)").expect("valid regex"));
static MULTI_SPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s{2,}").expect("valid regex"));
static V_UNDERSCORE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(v\s*\d+)(?:_\d+)+").expect("valid regex"));
static VOLUME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bv\s*0*(\d+)").expect("valid regex"));
static NON_ALNUM_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[^a-z0-9]+").expect("valid regex"));
static INT_VOLUME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*0*(\d+)(?:\.0+)?\s*$").expect("valid regex"));

#[derive(Debug, Clone)]
pub struct CoverResult {
    pub source: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct VolumeCoverResult {
    pub volume_file: PathBuf,
    pub image_entry: String,
    pub output_file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct FileMove {
    pub src: PathBuf,
    pub dst: PathBuf,
    pub dst_name: String,
}

#[derive(Debug, Clone)]
pub struct BatchPlan {
    pub batch_index: usize,
    pub batch_dir: PathBuf,
    pub moves: Vec<FileMove>,
    pub will_make_cover: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    ShowCover,
    Preview,
    Process,
}

impl UiAction {
    pub fn label(self) -> &'static str {
        match self {
            UiAction::ShowCover => "Show Cover",
            UiAction::Preview => "Show Plan",
            UiAction::Process => "Commit + Process",
        }
    }

    pub fn action_title(self) -> &'static str {
        match self {
            UiAction::ShowCover => "SHOW COVER",
            UiAction::Preview => "SHOW PLAN",
            UiAction::Process => "COMMIT + PROCESS",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActionOutput {
    pub action: UiAction,
    pub cover_path: Option<PathBuf>,
}

pub fn is_hidden_or_macos_junk(name: &str) -> bool {
    name.starts_with('.') || name.starts_with("._")
}

fn has_known_ext(path: &Path, exts: &[&str]) -> bool {
    let lower = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    exts.iter().any(|ext| lower.ends_with(ext))
}

fn file_name_text(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn natural_sort_paths(paths: &mut [PathBuf]) {
    paths.sort_by(|a, b| compare_ignore_case(&file_name_text(a), &file_name_text(b)));
}

fn natural_sort_strings(values: &mut [String]) {
    values.sort_by(|a, b| compare_ignore_case(a, b));
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory: {}", path.display()))
}

pub fn unique_path(dest_dir: &Path, filename: &str) -> PathBuf {
    let candidate = dest_dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }

    let src = Path::new(filename);
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| filename.to_string());
    let ext = src
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    let mut idx = 2;
    loop {
        let candidate = dest_dir.join(format!("{stem} ({idx}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
        idx += 1;
    }
}

pub fn unique_path_reserved(
    dest_dir: &Path,
    filename: &str,
    reserved: &mut HashSet<String>,
) -> PathBuf {
    let candidate = dest_dir.join(filename);
    if !candidate.exists() && !reserved.contains(filename) {
        reserved.insert(filename.to_string());
        return candidate;
    }

    let src = Path::new(filename);
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| filename.to_string());
    let ext = src
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    let mut idx = 2;
    loop {
        let name = format!("{stem} ({idx}){ext}");
        let candidate = dest_dir.join(&name);
        if !candidate.exists() && !reserved.contains(&name) {
            reserved.insert(name);
            return candidate;
        }
        idx += 1;
    }
}

pub fn unique_cover_old_path(dest_dir: &Path) -> PathBuf {
    let first = dest_dir.join("cover_old.jpg");
    if !first.exists() {
        return first;
    }

    let mut idx = 2;
    loop {
        let candidate = dest_dir.join(format!("cover_old_{idx}.jpg"));
        if !candidate.exists() {
            return candidate;
        }
        idx += 1;
    }
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" || path.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            let suffix = path.strip_prefix("~/").unwrap_or("");
            return PathBuf::from(home).join(suffix);
        }
    }
    PathBuf::from(path)
}

pub fn resolve_series_dir(path: &str) -> Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("Folder path is empty.");
    }

    let resolved = expand_tilde(trimmed)
        .canonicalize()
        .with_context(|| format!("failed to resolve path: {trimmed}"))?;

    if !resolved.is_dir() {
        bail!("Not a valid folder: {}", resolved.display());
    }

    Ok(resolved)
}

pub fn clean_volume_filename(src_name: &str, pad_to_3: bool) -> String {
    let p = Path::new(src_name);
    let stem_raw = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| src_name.to_string());
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    let mut stem = PARENS_RE.replace_all(&stem_raw, "").into_owned();
    stem = MULTI_SPACE_RE.replace_all(stem.trim(), " ").into_owned();
    stem = V_UNDERSCORE_RE.replace_all(&stem, "$1").into_owned();

    if let Some(caps) = VOLUME_RE.captures(&stem) {
        if let Some(vol_match) = caps.get(1) {
            if let Ok(vol_num) = vol_match.as_str().parse::<u32>() {
                let whole = caps.get(0).map(|m| m.start()).unwrap_or(0);
                let mut title = stem[..whole].trim().to_string();
                title = MULTI_SPACE_RE.replace_all(title.trim(), " ").into_owned();

                let vpart = if pad_to_3 {
                    format!("v{vol_num:03}")
                } else {
                    format!("v{vol_num}")
                };

                if title.is_empty() {
                    return format!("{vpart}{ext}");
                }
                return format!("{title} {vpart}{ext}");
            }
        }
    }

    format!("{stem}{ext}")
}

pub fn scan_volumes(series_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut volumes = Vec::new();
    for entry in fs::read_dir(series_dir)
        .with_context(|| format!("failed to read directory: {}", series_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = file_name_text(&path);
        if is_hidden_or_macos_junk(&name) {
            continue;
        }
        if has_known_ext(&path, VOLUME_EXTS) {
            volumes.push(path);
        }
    }
    natural_sort_paths(&mut volumes);
    Ok(volumes)
}

fn chunk_paths(paths: &[PathBuf], size: usize) -> Vec<Vec<PathBuf>> {
    if paths.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut index = 0;
    while index < paths.len() {
        let end = usize::min(index + size, paths.len());
        chunks.push(paths[index..end].to_vec());
        index = end;
    }
    chunks
}

pub fn build_plan(series_dir: &Path, series_cover: Option<&Path>) -> Result<Vec<BatchPlan>> {
    let volumes = scan_volumes(series_dir)?;
    if volumes.is_empty() {
        bail!("No volume files found in: {}", series_dir.display());
    }

    let groups = chunk_paths(&volumes, FILES_PER_FOLDER);
    let parent = series_dir
        .parent()
        .ok_or_else(|| anyhow!("Series folder has no parent: {}", series_dir.display()))?;

    let mut plan = Vec::new();
    let series_name = file_name_text(series_dir);
    for (idx, group) in groups.iter().enumerate() {
        let batch_index = idx + 1;
        let batch_dir = parent.join(format!("{series_name} {batch_index}"));
        let mut moves = Vec::new();
        let mut reserved = HashSet::new();

        for src in group {
            let src_name = file_name_text(src);
            let cleaned = clean_volume_filename(&src_name, true);
            let dst = unique_path_reserved(&batch_dir, &cleaned, &mut reserved);
            let dst_name = file_name_text(&dst);
            moves.push(FileMove {
                src: src.clone(),
                dst,
                dst_name,
            });
        }

        plan.push(BatchPlan {
            batch_index,
            batch_dir,
            moves,
            will_make_cover: series_cover.is_some(),
        });
    }

    Ok(plan)
}

pub fn format_plan(series_dir: &Path, plan: &[BatchPlan], series_cover: Option<&Path>) -> String {
    let mut out = String::new();
    let vols_count: usize = plan.iter().map(|b| b.moves.len()).sum();
    let series_name = file_name_text(series_dir);

    out.push('\n');
    out.push_str(&"=".repeat(98));
    out.push('\n');
    out.push_str("[PLAN] Manga toolkit (Rust)\n");
    out.push_str(&format!("[PLAN] Series folder: {}\n", series_dir.display()));
    out.push_str(&format!("[PLAN] Volumes found: {vols_count}\n"));
    out.push_str(&format!("[PLAN] Batch size: {FILES_PER_FOLDER}\n"));

    if let Some(cover) = series_cover {
        out.push_str(&format!(
            "[PLAN] Series cover source: {}\n",
            cover.display()
        ));
        out.push_str("[PLAN] Each batch will have:\n");
        out.push_str("       - cover_old.jpg (copied once from series cover, preserved)\n");
        out.push_str("       - cover.jpg (rendered with batch number DEAD-CENTER)\n");
        out.push_str("       - any existing cover.jpg archived to cover_old_*.jpg\n");
    } else {
        out.push_str("[PLAN] Covers: skipped (no cover image found/downloaded)\n");
    }

    out.push_str(&"=".repeat(98));
    out.push('\n');

    for batch in plan {
        let start_idx = (batch.batch_index - 1) * FILES_PER_FOLDER + 1;
        let end_idx = start_idx + batch.moves.len() - 1;

        out.push('\n');
        out.push_str(&format!(
            "{} {}  (volumes {}-{})\n",
            series_name, batch.batch_index, start_idx, end_idx
        ));
        out.push_str(&format!("  [DIR] {}\n", batch.batch_dir.display()));
        if series_cover.is_some() {
            out.push_str(&format!(
                "  [COVER] cover_old.jpg + cover.jpg (number {})\n",
                batch.batch_index
            ));
        }

        for (i, mv) in batch.moves.iter().enumerate() {
            let n = start_idx + i;
            if file_name_text(&mv.src) == mv.dst_name {
                out.push_str(&format!("  {n:>4}. {}\n", file_name_text(&mv.src)));
            } else {
                out.push_str(&format!(
                    "  {n:>4}. {}  (rename: {} -> {})\n",
                    file_name_text(&mv.src),
                    file_name_text(&mv.src),
                    mv.dst_name
                ));
            }
        }
    }

    out.push('\n');
    out.push_str(&"=".repeat(98));
    out.push('\n');

    out
}

fn move_file(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        ensure_dir(parent)?;
    }

    match fs::rename(src, dst) {
        Ok(_) => Ok(()),
        Err(err) => {
            if err.raw_os_error() == Some(libc::EXDEV) {
                fs::copy(src, dst).with_context(|| {
                    format!(
                        "cross-device copy failed from {} to {}",
                        src.display(),
                        dst.display()
                    )
                })?;
                fs::remove_file(src)
                    .with_context(|| format!("failed to remove source file: {}", src.display()))?;
                Ok(())
            } else {
                Err(err).with_context(|| {
                    format!(
                        "failed to move file from {} to {}",
                        src.display(),
                        dst.display()
                    )
                })
            }
        }
    }
}

fn http_client(timeout_secs: u64) -> Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .context("failed to initialize HTTP client")
}

fn http_get_json(url: &str, params: &[(&str, String)], timeout_secs: u64) -> Result<Value> {
    let client = http_client(timeout_secs)?;
    let mut req = client.get(url);
    if !params.is_empty() {
        req = req.query(params);
    }

    let resp = req
        .send()
        .with_context(|| format!("request failed: {url}"))?
        .error_for_status()
        .with_context(|| format!("request returned error status: {url}"))?;

    resp.json().context("failed to decode JSON response")
}

fn http_post_json(url: &str, payload: &Value, timeout_secs: u64) -> Result<Value> {
    let client = http_client(timeout_secs)?;
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(payload)
        .send()
        .with_context(|| format!("request failed: {url}"))?
        .error_for_status()
        .with_context(|| format!("request returned error status: {url}"))?;

    resp.json().context("failed to decode JSON response")
}

fn download_file(url: &str, out_path: &Path, timeout_secs: u64) -> Result<()> {
    if let Some(parent) = out_path.parent() {
        ensure_dir(parent)?;
    }

    let client = http_client(timeout_secs)?;
    let mut resp = client
        .get(url)
        .header("Referer", "https://mangadex.org/")
        .send()
        .with_context(|| format!("request failed: {url}"))?
        .error_for_status()
        .with_context(|| format!("request returned error status: {url}"))?;

    let mut out = fs::File::create(out_path)
        .with_context(|| format!("failed to create output file: {}", out_path.display()))?;
    io::copy(&mut resp, &mut out).with_context(|| {
        format!(
            "failed while writing downloaded data to {}",
            out_path.display()
        )
    })?;
    Ok(())
}

fn best_title(attrs: &Value) -> String {
    let Some(title_obj) = attrs.get("title").and_then(Value::as_object) else {
        return String::new();
    };

    if let Some(en) = title_obj.get("en").and_then(Value::as_str) {
        return en.to_string();
    }

    title_obj
        .values()
        .find_map(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn normalize_title(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    NON_ALNUM_RE.replace_all(&lower, "").into_owned()
}

fn parse_int_volume(vol: &Value) -> Option<u32> {
    let s = vol.as_str()?;
    let caps = INT_VOLUME_RE.captures(s)?;
    caps.get(1)?.as_str().parse::<u32>().ok()
}

fn score_mangadex_item(item: &Value, title_l: &str, title_n: &str) -> i32 {
    let attrs = item.get("attributes").unwrap_or(&Value::Null);
    let main = best_title(attrs).trim().to_ascii_lowercase();
    let main_n = normalize_title(&main);

    let mut alt_values = Vec::new();
    let mut alt_norms = Vec::new();

    if let Some(alts) = attrs.get("altTitles").and_then(Value::as_array) {
        for alt in alts {
            if let Some(obj) = alt.as_object() {
                for value in obj.values() {
                    if let Some(text) = value.as_str() {
                        let lowered = text.trim().to_ascii_lowercase();
                        alt_norms.push(normalize_title(&lowered));
                        alt_values.push(lowered);
                    }
                }
            }
        }
    }

    if main_n == title_n {
        return 6;
    }
    if alt_norms.iter().any(|v| v == title_n) {
        return 5;
    }
    if main == title_l {
        return 4;
    }
    if alt_values.iter().any(|v| v == title_l) {
        return 3;
    }
    if main.contains(title_l) {
        return 2;
    }
    if alt_values.iter().any(|v| v.contains(title_l)) {
        return 1;
    }
    1
}

pub fn fetch_cover_mangadex(title: &str, size: &str) -> Result<Option<CoverResult>> {
    let base = "https://api.mangadex.org";

    let data = http_get_json(
        &format!("{base}/manga"),
        &[("title", title.to_string()), ("limit", "5".to_string())],
        20,
    )?;

    let mut items = data
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if items.is_empty() {
        return Ok(None);
    }

    let title_l = title.trim().to_ascii_lowercase();
    let title_n = normalize_title(&title_l);

    items.sort_by_key(|item| Reverse(score_mangadex_item(item, &title_l, &title_n)));

    let manga_id = match items
        .first()
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
    {
        Some(id) => id.to_string(),
        None => return Ok(None),
    };

    let cover_id = match http_get_json(
        &format!("{base}/cover"),
        &[
            ("manga[]", manga_id.clone()),
            ("limit", "100".to_string()),
            ("order[createdAt]", "asc".to_string()),
        ],
        20,
    ) {
        Ok(covers_resp) => {
            let mut first_volume_cover: Option<String> = None;
            if let Some(covers) = covers_resp.get("data").and_then(Value::as_array) {
                for cover in covers {
                    let attrs = cover.get("attributes").unwrap_or(&Value::Null);
                    if parse_int_volume(attrs.get("volume").unwrap_or(&Value::Null)) == Some(1) {
                        first_volume_cover =
                            cover.get("id").and_then(Value::as_str).map(str::to_string);
                        if first_volume_cover.is_some() {
                            break;
                        }
                    }
                }
            }
            first_volume_cover
        }
        Err(_) => None,
    };

    let Some(cover_id) = cover_id else {
        return Ok(None);
    };

    let cover = http_get_json(&format!("{base}/cover/{cover_id}"), &[], 20)?;
    let file_name = match cover
        .pointer("/data/attributes/fileName")
        .and_then(Value::as_str)
    {
        Some(name) => name,
        None => return Ok(None),
    };

    let mut url = format!("https://uploads.mangadex.org/covers/{manga_id}/{file_name}");
    if size == "512" {
        url.push_str(".512.jpg");
    } else if size == "256" {
        url.push_str(".256.jpg");
    }

    Ok(Some(CoverResult {
        source: "mangadex".to_string(),
        url,
    }))
}

pub fn fetch_cover_anilist(title: &str) -> Result<Option<CoverResult>> {
    let endpoint = "https://graphql.anilist.co";
    let query = r#"
    query ($search: String) {
      Media(search: $search, type: MANGA) {
        id
        coverImage { extraLarge large }
      }
    }
    "#;

    let payload = json!({
        "query": query,
        "variables": {
            "search": title,
        }
    });

    let resp = http_post_json(endpoint, &payload, 20)?;
    let media = resp.pointer("/data/Media").unwrap_or(&Value::Null);
    if media.is_null() {
        return Ok(None);
    }

    let url = media
        .pointer("/coverImage/extraLarge")
        .and_then(Value::as_str)
        .or_else(|| media.pointer("/coverImage/large").and_then(Value::as_str));

    let Some(url) = url else {
        return Ok(None);
    };

    Ok(Some(CoverResult {
        source: "anilist".to_string(),
        url: url.to_string(),
    }))
}

pub fn fetch_cover_kitsu(title: &str) -> Result<Option<CoverResult>> {
    let base = "https://kitsu.io/api/edge";
    let data = http_get_json(
        &format!("{base}/manga"),
        &[
            ("filter[text]", title.to_string()),
            ("page[limit]", "5".to_string()),
        ],
        20,
    )?;

    let items = data
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let Some(first) = items.first() else {
        return Ok(None);
    };

    let url = first
        .pointer("/attributes/coverImage/original")
        .and_then(Value::as_str)
        .or_else(|| {
            first
                .pointer("/attributes/coverImage/large")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            first
                .pointer("/attributes/coverImage/small")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            first
                .pointer("/attributes/coverImage/tiny")
                .and_then(Value::as_str)
        });

    let Some(url) = url else {
        return Ok(None);
    };

    Ok(Some(CoverResult {
        source: "kitsu".to_string(),
        url: url.to_string(),
    }))
}

pub fn find_remote_cover(title: &str) -> (Option<CoverResult>, Option<String>) {
    let mut last_err: Option<String> = None;

    match fetch_cover_mangadex(title, "best") {
        Ok(Some(cover)) => return (Some(cover), None),
        Ok(None) => {}
        Err(err) => last_err = Some(err.to_string()),
    }

    match fetch_cover_anilist(title) {
        Ok(Some(cover)) => return (Some(cover), None),
        Ok(None) => {}
        Err(err) => last_err = Some(err.to_string()),
    }

    match fetch_cover_kitsu(title) {
        Ok(Some(cover)) => return (Some(cover), None),
        Ok(None) => {}
        Err(err) => last_err = Some(err.to_string()),
    }

    (None, last_err)
}

fn zip_entry_is_image(entry_name: &str) -> bool {
    let lower = entry_name.to_ascii_lowercase();
    if !IMAGE_EXTS.iter().any(|ext| lower.ends_with(ext)) {
        return false;
    }

    for component in Path::new(entry_name).components() {
        if let Component::Normal(part) = component {
            let part_str = part.to_string_lossy();
            if part_str == "__MACOSX" || is_hidden_or_macos_junk(&part_str) {
                return false;
            }
        }
    }

    true
}

fn first_image_entry_in_zip(volume_file: &Path) -> Result<Option<String>> {
    let file = fs::File::open(volume_file)
        .with_context(|| format!("failed to open archive: {}", volume_file.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read archive: {}", volume_file.display()))?;

    let mut entries = Vec::new();
    for idx in 0..archive.len() {
        let entry = archive.by_index(idx)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if zip_entry_is_image(&name) {
            entries.push(name);
        }
    }

    if entries.is_empty() {
        return Ok(None);
    }

    natural_sort_strings(&mut entries);
    Ok(entries.into_iter().next())
}

fn find_first_volume_cover_inner(series_dir: &Path) -> Result<Option<VolumeCoverResult>> {
    let volumes = scan_volumes(series_dir)?;
    if volumes.is_empty() {
        return Ok(None);
    }

    let first_volume = volumes[0].clone();
    if !has_known_ext(&first_volume, &[".cbz", ".zip"]) {
        let ext = first_volume
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
            .unwrap_or_else(|| "(none)".to_string());
        bail!(
            "first volume is {} (local extraction currently supports .cbz/.zip only)",
            ext
        );
    }

    let first_image = first_image_entry_in_zip(&first_volume)?.ok_or_else(|| {
        anyhow!(
            "no image files found in first volume archive: {}",
            file_name_text(&first_volume)
        )
    })?;

    Ok(Some(VolumeCoverResult {
        volume_file: first_volume,
        image_entry: first_image,
        output_file: series_dir.join("cover.jpg"),
    }))
}

pub fn find_first_volume_cover(series_dir: &Path) -> (Option<VolumeCoverResult>, Option<String>) {
    match find_first_volume_cover_inner(series_dir) {
        Ok(result) => (result, None),
        Err(err) => (None, Some(err.to_string())),
    }
}

fn save_jpeg(image: &DynamicImage, out_path: &Path) -> Result<()> {
    if let Some(parent) = out_path.parent() {
        ensure_dir(parent)?;
    }

    let rgb = image.to_rgb8();
    let rendered = DynamicImage::ImageRgb8(rgb);

    let mut out = fs::File::create(out_path)
        .with_context(|| format!("failed to create image file: {}", out_path.display()))?;
    let mut encoder = JpegEncoder::new_with_quality(&mut out, 95);
    encoder
        .encode_image(&rendered)
        .with_context(|| format!("failed to encode JPEG: {}", out_path.display()))?;
    Ok(())
}

pub fn write_volume_cover(result: &VolumeCoverResult) -> Result<PathBuf> {
    if let Some(parent) = result.output_file.parent() {
        ensure_dir(parent)?;
    }

    let file = fs::File::open(&result.volume_file)
        .with_context(|| format!("failed to open archive: {}", result.volume_file.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read archive: {}", result.volume_file.display()))?;
    let mut entry = archive
        .by_name(&result.image_entry)
        .with_context(|| format!("missing image entry in archive: {}", result.image_entry))?;

    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .context("failed to read image from archive")?;

    let image = image::load_from_memory(&bytes).context("failed to decode image from archive")?;
    save_jpeg(&image, &result.output_file)?;
    Ok(result.output_file.clone())
}

pub fn ensure_cover_jpg(series_dir: &Path, selected_cover: &Path) -> Result<PathBuf> {
    let cover_jpg = series_dir.join("cover.jpg");
    let selected_resolved = selected_cover
        .canonicalize()
        .unwrap_or_else(|_| selected_cover.to_path_buf());
    let cover_resolved = cover_jpg
        .canonicalize()
        .unwrap_or_else(|_| cover_jpg.clone());

    if selected_resolved == cover_resolved {
        return Ok(cover_jpg);
    }

    let image = ImageReader::open(selected_cover)
        .with_context(|| format!("failed to open image: {}", selected_cover.display()))?
        .decode()
        .context("failed to decode selected cover image")?;

    save_jpeg(&image, &cover_jpg)?;
    Ok(cover_jpg)
}

pub fn open_image(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut cmd = Command::new("open");
        cmd.arg(path);
        cmd
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg("start").arg("").arg(path);
        cmd
    };

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut command = {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(path);
        cmd
    };

    let status = command
        .status()
        .with_context(|| format!("failed to launch image viewer for {}", path.display()))?;

    if !status.success() {
        bail!("image viewer exited with status: {status}");
    }

    Ok(())
}

pub fn choose_series_cover(series_dir: &Path) -> Result<Option<PathBuf>> {
    for name in COVER_CANDIDATES {
        let candidate = series_dir.join(name);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }

    let mut images = Vec::new();
    for entry in fs::read_dir(series_dir)
        .with_context(|| format!("failed to read directory: {}", series_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = file_name_text(&path);
        if is_hidden_or_macos_junk(&name) {
            continue;
        }
        if has_known_ext(&path, IMAGE_EXTS) {
            images.push(path);
        }
    }

    if images.is_empty() {
        return Ok(None);
    }

    natural_sort_paths(&mut images);
    Ok(images.into_iter().next())
}

pub fn ensure_series_cover(
    series_dir: &Path,
    title: &str,
    log: &mut dyn FnMut(String),
) -> Result<Option<PathBuf>> {
    let (first_vol_cover, mut first_vol_err) = find_first_volume_cover(series_dir);

    if let Some(cover) = first_vol_cover {
        match write_volume_cover(&cover) {
            Ok(out) => {
                log(format!(
                    "[COVER] Extracted series cover from first volume: {} (source={}:{})",
                    out.display(),
                    file_name_text(&cover.volume_file),
                    cover.image_entry
                ));
                return Ok(Some(out));
            }
            Err(err) => {
                first_vol_err = Some(err.to_string());
            }
        }
    }

    if let Some(existing) = choose_series_cover(series_dir)? {
        return Ok(Some(existing));
    }

    let out_file = series_dir.join("cover.jpg");
    let (remote_cover, mut last_err) = find_remote_cover(title);
    if let Some(result) = remote_cover {
        match download_file(&result.url, &out_file, 30) {
            Ok(_) => {
                log(format!(
                    "[COVER] Downloaded series cover: {} (source={})",
                    out_file.display(),
                    result.source
                ));
                return Ok(Some(out_file));
            }
            Err(err) => {
                last_err = Some(err.to_string());
            }
        }
    }

    if let Some(err) = first_vol_err {
        log(format!(
            "[WARN] Failed to extract first-volume cover. Last error: {err}"
        ));
    }

    if let Some(err) = last_err {
        log(format!(
            "[WARN] Failed to download series cover. Last error: {err}"
        ));
    } else {
        log("[WARN] Failed to download series cover (no results).".to_string());
    }

    Ok(None)
}

fn pick_font() -> Result<FontArc> {
    let candidates = [
        "/System/Library/Fonts/Supplemental/Arial Black.ttf",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        "/System/Library/Fonts/Supplemental/Impact.ttf",
        "/System/Library/Fonts/Supplemental/Helvetica Bold.ttf",
        "/Library/Fonts/Arial Black.ttf",
        "/Library/Fonts/Arial Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    ];

    for candidate in candidates {
        let path = Path::new(candidate);
        if !path.exists() {
            continue;
        }

        let bytes = fs::read(path)
            .with_context(|| format!("failed to read font file: {}", path.display()))?;
        if let Ok(font) = FontArc::try_from_vec(bytes) {
            return Ok(font);
        }
    }

    bail!("unable to find a usable font for cover rendering")
}

fn fit_font_size(font: &FontArc, text: &str, w: u32, h: u32, margin_frac: f32) -> u32 {
    let max_w = ((w as f32) * (1.0 - 2.0 * margin_frac)).max(1.0) as u32;
    let max_h = ((h as f32) * (1.0 - 2.0 * margin_frac)).max(1.0) as u32;

    let mut lo: u32 = 10;
    let mut hi: u32 = w.max(h).saturating_mul(5).max(10);
    let mut best = lo;

    while lo <= hi {
        let mid = (lo + hi) / 2;
        let scale = PxScale::from(mid as f32);
        let (tw, th) = text_size(scale, font, text);

        if tw <= max_w && th <= max_h {
            best = mid;
            lo = mid.saturating_add(1);
        } else {
            if mid == 0 {
                break;
            }
            hi = mid.saturating_sub(1);
        }
    }

    best
}

fn alpha_bbox(image: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = image.dimensions();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;

    for (x, y, px) in image.enumerate_pixels() {
        if px.0[3] == 0 {
            continue;
        }
        found = true;
        if x < min_x {
            min_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if x > max_x {
            max_x = x;
        }
        if y > max_y {
            max_y = y;
        }
    }

    if !found {
        return None;
    }

    Some((min_x, min_y, max_x, max_y))
}

fn draw_dead_center_text(
    base_image: &DynamicImage,
    text: &str,
    opacity: u8,
    scale: f32,
) -> Result<DynamicImage> {
    let mut rgba = base_image.to_rgba8();
    let (w, h) = rgba.dimensions();

    let font = pick_font()?;
    let max_size = fit_font_size(&font, text, w, h, 0.06);
    let font_size = ((max_size as f32) * scale).max(10.0);
    let px_scale = PxScale::from(font_size);

    // Probe-and-correct placement on a full-size transparent canvas until the rendered bbox center
    // lands on the image center. This mirrors Pillow's anchor-centered behavior.
    let mut x = (w as f32 / 2.0).round() as i32;
    let mut y = (h as f32 / 2.0).round() as i32;
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;

    for _ in 0..4 {
        let mut probe = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
        draw_text_mut(
            &mut probe,
            Rgba([0, 0, 0, 255]),
            x,
            y,
            px_scale,
            &font,
            text,
        );

        let Some((min_x, min_y, max_x, max_y)) = alpha_bbox(&probe) else {
            break;
        };

        let bcx = (min_x as f32 + max_x as f32) / 2.0;
        let bcy = (min_y as f32 + max_y as f32) / 2.0;
        let dx = (cx - bcx).round() as i32;
        let dy = (cy - bcy).round() as i32;

        if dx == 0 && dy == 0 {
            break;
        }
        x += dx;
        y += dy;
    }

    draw_text_mut(
        &mut rgba,
        Rgba([0, 0, 0, opacity]),
        x,
        y,
        px_scale,
        &font,
        text,
    );

    let rgb = DynamicImage::ImageRgba8(rgba).to_rgb8();
    Ok(DynamicImage::ImageRgb8(rgb))
}

pub fn ensure_cover_old(batch_dir: &Path, series_cover: &Path) -> Result<PathBuf> {
    let primary = batch_dir.join("cover_old.jpg");
    if primary.exists() {
        return Ok(primary);
    }

    let target = unique_cover_old_path(batch_dir);
    fs::copy(series_cover, &target).with_context(|| {
        format!(
            "failed to copy series cover from {} to {}",
            series_cover.display(),
            target.display()
        )
    })?;
    Ok(target)
}

pub fn archive_existing_cover_jpg(batch_dir: &Path) -> Result<Option<PathBuf>> {
    let cover = batch_dir.join("cover.jpg");
    if !cover.exists() {
        return Ok(None);
    }

    let destination = unique_cover_old_path(batch_dir);
    fs::rename(&cover, &destination).with_context(|| {
        format!(
            "failed to archive cover from {} to {}",
            cover.display(),
            destination.display()
        )
    })?;

    Ok(Some(destination))
}

pub fn write_numbered_cover(batch_dir: &Path, number: usize, series_cover: &Path) -> Result<()> {
    ensure_dir(batch_dir)?;
    archive_existing_cover_jpg(batch_dir)?;
    let base_cover = ensure_cover_old(batch_dir, series_cover)?;

    let image = ImageReader::open(&base_cover)
        .with_context(|| format!("failed to open base cover image: {}", base_cover.display()))?
        .decode()
        .context("failed to decode base cover image")?;

    let rendered = draw_dead_center_text(&image, &number.to_string(), 255, 0.90)?;
    save_jpeg(&rendered, &batch_dir.join("cover.jpg"))?;
    Ok(())
}

pub fn execute(
    plan: &[BatchPlan],
    series_cover: Option<&Path>,
    log: &mut dyn FnMut(String),
) -> Result<()> {
    for batch in plan {
        ensure_dir(&batch.batch_dir)?;

        log(String::new());
        log("-".repeat(98));
        log(format!(
            "[DO] Batch {}: {}",
            batch.batch_index,
            file_name_text(&batch.batch_dir)
        ));
        log("-".repeat(98));

        for (i, mv) in batch.moves.iter().enumerate() {
            log(format!(
                "[MOVE] ({}/{}) {} -> {}",
                i + 1,
                batch.moves.len(),
                file_name_text(&mv.src),
                mv.dst_name
            ));
            move_file(&mv.src, &mv.dst)?;
        }

        if let Some(cover) = series_cover {
            log(format!(
                "[COVER] Rendering cover.jpg (batch number {})",
                batch.batch_index
            ));
            write_numbered_cover(&batch.batch_dir, batch.batch_index, cover)?;
        }
    }

    log("[COMPLETE] Done.".to_string());
    Ok(())
}

pub fn run_action(
    action: UiAction,
    series_dir: &Path,
    log: &mut dyn FnMut(String),
) -> Result<ActionOutput> {
    if !series_dir.is_dir() {
        bail!("Not a directory: {}", series_dir.display());
    }

    match action {
        UiAction::ShowCover => {
            let series_cover = ensure_series_cover(series_dir, &file_name_text(series_dir), log)?;
            let Some(series_cover) = series_cover else {
                bail!("[COVER-CHECK] No cover found from local files or remote providers.");
            };

            let cover_jpg = ensure_cover_jpg(series_dir, &series_cover)?;
            log(format!("{}", cover_jpg.display()));
            Ok(ActionOutput {
                action,
                cover_path: Some(cover_jpg),
            })
        }
        UiAction::Preview => {
            let series_cover = ensure_series_cover(series_dir, &file_name_text(series_dir), log)?;
            let plan = build_plan(series_dir, series_cover.as_deref())?;
            let plan_text = format_plan(series_dir, &plan, series_cover.as_deref());
            for line in plan_text.lines() {
                log(line.to_string());
            }
            log("[DRY-RUN] Plan printed only. No changes were made.".to_string());
            Ok(ActionOutput {
                action,
                cover_path: None,
            })
        }
        UiAction::Process => {
            let series_cover = ensure_series_cover(series_dir, &file_name_text(series_dir), log)?;
            let plan = build_plan(series_dir, series_cover.as_deref())?;
            let plan_text = format_plan(series_dir, &plan, series_cover.as_deref());
            for line in plan_text.lines() {
                log(line.to_string());
            }
            execute(&plan, series_cover.as_deref(), log)?;
            Ok(ActionOutput {
                action,
                cover_path: None,
            })
        }
    }
}

pub fn prompt_confirm(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    io::stdout().flush().context("failed to flush stdout")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read user input")?;

    let answer = input.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};

    fn bbox_for_mask(mask: impl Iterator<Item = (u32, u32)>) -> Option<(u32, u32, u32, u32)> {
        let mut found = false;
        let mut min_x = u32::MAX;
        let mut min_y = u32::MAX;
        let mut max_x = 0;
        let mut max_y = 0;

        for (x, y) in mask {
            found = true;
            if x < min_x {
                min_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if x > max_x {
                max_x = x;
            }
            if y > max_y {
                max_y = y;
            }
        }

        if !found {
            return None;
        }
        Some((min_x, min_y, max_x, max_y))
    }

    fn center_of_bbox(b: (u32, u32, u32, u32)) -> (f32, f32) {
        let (x0, y0, x1, y1) = b;
        ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0)
    }

    #[test]
    fn centered_text_on_white_canvas() {
        let w = 1000;
        let h = 1500;
        let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([255, 255, 255])));
        let rendered = draw_dead_center_text(&base, "12", 255, 0.90).expect("rendered text");
        let rgb = rendered.to_rgb8();

        let bbox = bbox_for_mask(rgb.enumerate_pixels().filter_map(|(x, y, p)| {
            let [r, g, b] = p.0;
            if r < 250 || g < 250 || b < 250 {
                Some((x, y))
            } else {
                None
            }
        }))
        .expect("text pixels should exist");

        let (cx, cy) = center_of_bbox(bbox);
        assert!(
            (cx - (w as f32 / 2.0)).abs() <= 2.0,
            "x center mismatch: got {cx}, expected {}",
            w as f32 / 2.0
        );
        assert!(
            (cy - (h as f32 / 2.0)).abs() <= 2.0,
            "y center mismatch: got {cy}, expected {}",
            h as f32 / 2.0
        );
    }

    #[test]
    fn centered_text_on_example_cover() {
        let example = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("example_cover")
            .join("cover.jpg");
        let base = ImageReader::open(&example)
            .expect("open example cover")
            .decode()
            .expect("decode example cover");
        let rendered = draw_dead_center_text(&base, "2", 255, 0.90).expect("rendered text");

        let src = base.to_rgb8();
        let out = rendered.to_rgb8();
        let (w, h) = out.dimensions();

        let mut changed_pixels = 0usize;
        let bbox = bbox_for_mask((0..h).flat_map(|y| {
            let src_ref = &src;
            let out_ref = &out;
            (0..w).filter_map(move |x| {
                let a = src_ref.get_pixel(x, y).0;
                let b = out_ref.get_pixel(x, y).0;
                let diff = (a[0] as i32 - b[0] as i32).abs()
                    + (a[1] as i32 - b[1] as i32).abs()
                    + (a[2] as i32 - b[2] as i32).abs();
                if diff > 24 {
                    Some((x, y))
                } else {
                    None
                }
            })
        }));

        for y in 0..h {
            for x in 0..w {
                let a = src.get_pixel(x, y).0;
                let b = out.get_pixel(x, y).0;
                let diff = (a[0] as i32 - b[0] as i32).abs()
                    + (a[1] as i32 - b[1] as i32).abs()
                    + (a[2] as i32 - b[2] as i32).abs();
                if diff > 24 {
                    changed_pixels += 1;
                }
            }
        }

        if let Some((x0, y0, x1, y1)) = bbox {
            let (cx, cy) = center_of_bbox((x0, y0, x1, y1));
            let x_tol = (w as f32 * 0.03).max(6.0);
            let y_tol = (h as f32 * 0.03).max(6.0);
            assert!(
                (cx - (w as f32 / 2.0)).abs() <= x_tol,
                "example x center mismatch: got {cx}, expected {} (tol {x_tol})",
                w as f32 / 2.0
            );
            assert!(
                (cy - (h as f32 / 2.0)).abs() <= y_tol,
                "example y center mismatch: got {cy}, expected {} (tol {y_tol})",
                h as f32 / 2.0
            );
        }

        assert!(
            changed_pixels > 10_000,
            "text appears too small or missing: changed_pixels={changed_pixels}"
        );
    }
}
