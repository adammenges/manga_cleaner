use std::process;

use anyhow::{bail, Result};
use clap::Parser;
use manga_cleaner::{
    build_plan, ensure_cover_jpg, ensure_series_cover, execute, format_plan, open_image, prompt_confirm,
    resolve_series_dir,
};

#[derive(Debug, Parser)]
#[command(name = "process_manga_rs")]
#[command(about = "Clean and batch manga files with numbered covers (Rust port).")]
struct Args {
    #[arg(help = "Path to the series folder")]
    series_dir: String,

    #[arg(long, help = "Resolve selected cover, ensure cover.jpg exists, open it, then exit.")]
    show_cover: bool,

    #[arg(long, help = "Resolve selected cover, ensure cover.jpg exists, print path, then exit.")]
    print_cover_path: bool,

    #[arg(short = 'y', long, help = "Execute all planned actions without confirmation.")]
    yes: bool,

    #[arg(long, help = "Print full plan and exit without changing files.")]
    dry_run: bool,
}

fn run() -> Result<i32> {
    let args = Args::parse();

    if args.show_cover && (args.print_cover_path || args.yes || args.dry_run) {
        bail!("--show-cover cannot be combined with --print-cover-path, --yes, or --dry-run");
    }
    if args.print_cover_path && (args.show_cover || args.yes || args.dry_run) {
        bail!("--print-cover-path cannot be combined with --show-cover, --yes, or --dry-run");
    }

    let series_dir = resolve_series_dir(&args.series_dir)?;
    let series_title = series_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| series_dir.display().to_string());

    let mut log = |line: String| println!("{line}");

    if args.show_cover {
        let Some(series_cover) = ensure_series_cover(&series_dir, &series_title, &mut log)? else {
            eprintln!("[COVER-CHECK] No cover found from local files or remote providers.");
            return Ok(1);
        };

        let cover_jpg = ensure_cover_jpg(&series_dir, &series_cover)?;
        println!("[COVER-CHECK] Opening: {}", cover_jpg.display());
        open_image(&cover_jpg)?;
        return Ok(0);
    }

    if args.print_cover_path {
        let Some(series_cover) = ensure_series_cover(&series_dir, &series_title, &mut log)? else {
            eprintln!("[COVER-CHECK] No cover found from local files or remote providers.");
            return Ok(1);
        };

        let cover_jpg = ensure_cover_jpg(&series_dir, &series_cover)?;
        println!("{}", cover_jpg.display());
        return Ok(0);
    }

    let series_cover = ensure_series_cover(&series_dir, &series_title, &mut log)?;

    let plan = build_plan(&series_dir, series_cover.as_deref())?;
    print!("{}", format_plan(&series_dir, &plan, series_cover.as_deref()));

    if args.dry_run {
        println!("[DRY-RUN] Plan printed only. No changes were made.");
        return Ok(0);
    }

    if args.yes {
        execute(&plan, series_cover.as_deref(), &mut log)?;
        return Ok(0);
    }

    if !prompt_confirm("\nProceed and execute everything now? [y/N]: ")? {
        println!("[SKIP] Aborted by user.");
        return Ok(0);
    }

    execute(&plan, series_cover.as_deref(), &mut log)?;
    Ok(0)
}

fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(err) => {
            eprintln!("[ERROR] {err}");
            process::exit(2);
        }
    }
}
