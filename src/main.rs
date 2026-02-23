use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use clap::Parser;
use iced::{
    executor,
    theme::{self, Theme},
    time,
    widget::{
        button, column, container, horizontal_rule, horizontal_space, image, progress_bar, row,
        scrollable, text,
    },
    Alignment, Application, Background, Border, Color, Command, Element, Font, Length, Settings,
    Shadow, Size, Subscription, Vector,
};
use manga_cleaner::{
    build_plan, ensure_cover_jpg, ensure_series_cover, execute, resolve_series_dir, BatchPlan,
    FILES_PER_FOLDER,
};
use rfd::FileDialog;

const FONT_TEXT: Font = Font::with_name("SF Pro Text");
const FONT_DISPLAY: Font = Font::with_name("SF Pro Display");
const FONT_SYMBOLS: Font = Font::with_name("SF Pro");

const ICON_FOLDER: &str = "􀈕";
const ICON_REFRESH: &str = "􀣓";
const ICON_FLOW: &str = "􀐫";
const ICON_COVER: &str = "􀏅";
const ICON_PROCESS: &str = "􀒓";
const ICON_ACTIVITY: &str = "􀐌";
const ICON_WAITING: &str = "􀆈";
const ICON_RUNNING: &str = "􀐰";
const ICON_DONE: &str = "􀆅";
const ICON_ERROR: &str = "􀅚";
const ICON_ARROW: &str = "􀄯";

#[derive(Debug, Parser)]
#[command(name = "manga_cleaner_native")]
#[command(about = "Native Iced UI for manga_cleaner (Rust).")]
struct UiArgs {
    #[arg(help = "Optional starting series folder path")]
    series_dir: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct AppFlags {
    initial_series_dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageState {
    Pending,
    Running,
    Complete,
    Error,
}

#[derive(Debug, Clone, Copy)]
enum ActivityTone {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
struct ActivityItem {
    tone: ActivityTone,
    message: String,
}

#[derive(Debug, Clone)]
struct AnalysisSnapshot {
    resolved_dir: PathBuf,
    cover_path: Option<PathBuf>,
    plan: Vec<BatchPlan>,
    volume_count: usize,
    rename_count: usize,
}

impl AnalysisSnapshot {
    fn batch_count(&self) -> usize {
        self.plan.len()
    }

    fn cover_batch_count(&self) -> usize {
        self.plan
            .iter()
            .filter(|batch| batch.will_make_cover)
            .count()
    }
}

#[derive(Debug)]
enum WorkerEvent {
    Activity(String),
    AnalysisComplete(Result<AnalysisSnapshot, String>),
    ProcessProgress {
        completed_batches: usize,
        total_batches: usize,
        label: String,
    },
    ProcessComplete(Result<(), String>),
}

#[derive(Debug, Clone, Copy)]
enum ButtonTone {
    Accent,
    Secondary,
    Danger,
    Ghost,
}

#[derive(Debug, Clone, Copy)]
struct NativeButton {
    tone: ButtonTone,
}

impl NativeButton {
    const fn new(tone: ButtonTone) -> Self {
        Self { tone }
    }

    fn palette(self) -> (Option<Color>, Color, Color) {
        match self.tone {
            ButtonTone::Accent => (
                Some(Color::from_rgb8(17, 108, 219)),
                Color::from_rgb8(12, 85, 179),
                Color::WHITE,
            ),
            ButtonTone::Secondary => (
                Some(Color::from_rgb8(239, 244, 252)),
                Color::from_rgb8(212, 223, 238),
                Color::from_rgb8(26, 40, 57),
            ),
            ButtonTone::Danger => (
                Some(Color::from_rgb8(188, 61, 67)),
                Color::from_rgb8(153, 47, 52),
                Color::WHITE,
            ),
            ButtonTone::Ghost => (
                Some(Color::from_rgba8(236, 242, 251, 0.74)),
                Color::from_rgb8(210, 220, 236),
                Color::from_rgb8(70, 84, 104),
            ),
        }
    }
}

impl iced::widget::button::StyleSheet for NativeButton {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> iced::widget::button::Appearance {
        let (bg, border, text_color) = self.palette();
        iced::widget::button::Appearance {
            shadow_offset: Vector::default(),
            background: bg.map(Background::Color),
            text_color,
            border: Border {
                color: border,
                width: 1.0,
                radius: 14.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba8(9, 16, 30, 0.10),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 4.0,
            },
        }
    }

    fn hovered(&self, _style: &Self::Style) -> iced::widget::button::Appearance {
        let (bg, border, text_color) = self.palette();
        iced::widget::button::Appearance {
            shadow_offset: Vector::default(),
            background: bg.map(|value| Background::Color(lighten(value, 0.03))),
            text_color,
            border: Border {
                color: darken(border, 0.04),
                width: 1.0,
                radius: 14.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba8(9, 16, 30, 0.14),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            },
        }
    }

    fn pressed(&self, _style: &Self::Style) -> iced::widget::button::Appearance {
        let (bg, border, text_color) = self.palette();
        iced::widget::button::Appearance {
            shadow_offset: Vector::default(),
            background: bg.map(|value| Background::Color(darken(value, 0.07))),
            text_color,
            border: Border {
                color: darken(border, 0.08),
                width: 1.0,
                radius: 14.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba8(9, 16, 30, 0.08),
                offset: Vector::new(0.0, 0.0),
                blur_radius: 2.0,
            },
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    BrowseFolder,
    RefreshAnalysis,
    RequestProcess,
    CancelProcessConfirmation,
    ConfirmProcess,
    Tick,
}

struct MangaCleanerApp {
    series_dir_input: String,
    status_text: String,
    analysis_stage: StageState,
    plan_stage: StageState,
    process_stage: StageState,
    analysis_running: bool,
    processing_running: bool,
    show_confirm_sheet: bool,
    process_progress: f32,
    process_label: String,
    analysis: Option<AnalysisSnapshot>,
    cover_path: Option<PathBuf>,
    cover_handle: Option<iced::widget::image::Handle>,
    activity: Vec<ActivityItem>,
    worker_rx: Option<Receiver<WorkerEvent>>,
}

impl MangaCleanerApp {
    fn is_busy(&self) -> bool {
        self.analysis_running || self.processing_running
    }

    fn can_refresh(&self) -> bool {
        !self.is_busy() && !self.series_dir_input.trim().is_empty()
    }

    fn can_process(&self) -> bool {
        !self.is_busy()
            && self.analysis.is_some()
            && self.analysis_stage == StageState::Complete
            && self.plan_stage == StageState::Complete
            && self.process_stage != StageState::Complete
    }

    fn append_activity(&mut self, tone: ActivityTone, message: impl AsRef<str>) {
        let text = message.as_ref().trim();
        if text.is_empty() {
            return;
        }

        self.activity.push(ActivityItem {
            tone,
            message: text.to_string(),
        });

        let max_items = 180;
        if self.activity.len() > max_items {
            let remove_count = self.activity.len() - max_items;
            self.activity.drain(0..remove_count);
        }
    }

    fn reset_for_new_analysis(&mut self) {
        self.analysis = None;
        self.set_cover_path(None);
        self.process_progress = 0.0;
        self.process_label = "Waiting for analysis".to_string();
        self.show_confirm_sheet = false;
        self.process_stage = StageState::Pending;
        self.analysis_stage = StageState::Pending;
        self.plan_stage = StageState::Pending;
    }

    fn set_cover_path(&mut self, path: Option<PathBuf>) {
        self.cover_path = path.clone();
        self.cover_handle = path.map(iced::widget::image::Handle::from_path);
    }

    fn set_series_folder(&mut self, raw_path: impl AsRef<str>) {
        self.series_dir_input = raw_path.as_ref().to_string();
        self.activity.clear();
        self.reset_for_new_analysis();
        self.start_analysis();
    }

    fn start_analysis(&mut self) {
        if self.is_busy() {
            return;
        }

        let raw_path = self.series_dir_input.trim().to_string();
        if raw_path.is_empty() {
            self.status_text = "Choose a series folder to begin.".to_string();
            return;
        }

        self.analysis_running = true;
        self.processing_running = false;
        self.analysis_stage = StageState::Running;
        self.plan_stage = StageState::Pending;
        self.process_stage = StageState::Pending;
        self.status_text = "Running automatic checks...".to_string();
        self.process_label = "Preparing analysis...".to_string();
        self.append_activity(
            ActivityTone::Info,
            "Running automatic checks and building a processing plan.",
        );

        let (tx, rx) = mpsc::channel();
        self.worker_rx = Some(rx);

        thread::spawn(move || {
            let resolved = match resolve_series_dir(&raw_path) {
                Ok(path) => path,
                Err(err) => {
                    let _ = tx.send(WorkerEvent::AnalysisComplete(Err(err.to_string())));
                    return;
                }
            };

            let _ = tx.send(WorkerEvent::Activity(format!(
                "Analyzing source folder: {}",
                resolved.display()
            )));

            let series_title = leaf_name(&resolved);
            let mut log = |line: String| {
                let _ = tx.send(WorkerEvent::Activity(line));
            };

            let result = (|| -> Result<AnalysisSnapshot, String> {
                let series_cover = ensure_series_cover(&resolved, &series_title, &mut log)
                    .map_err(|err| err.to_string())?;

                let cover_path = if let Some(ref selected_cover) = series_cover {
                    Some(
                        ensure_cover_jpg(&resolved, selected_cover)
                            .map_err(|err| err.to_string())?,
                    )
                } else {
                    None
                };

                let plan = build_plan(&resolved, series_cover.as_deref())
                    .map_err(|err| err.to_string())?;
                let volume_count: usize = plan.iter().map(|batch| batch.moves.len()).sum();
                let rename_count = plan
                    .iter()
                    .flat_map(|batch| batch.moves.iter())
                    .filter(|mv| leaf_name(&mv.src) != mv.dst_name)
                    .count();

                Ok(AnalysisSnapshot {
                    resolved_dir: resolved,
                    cover_path,
                    plan,
                    volume_count,
                    rename_count,
                })
            })();

            let _ = tx.send(WorkerEvent::AnalysisComplete(result));
        });
    }

    fn start_process(&mut self) {
        if !self.can_process() {
            return;
        }

        let Some(snapshot) = self.analysis.clone() else {
            return;
        };

        let plan = snapshot.plan.clone();
        let series_cover = snapshot.cover_path.clone();
        let total_batches = plan.len().max(1);

        self.processing_running = true;
        self.analysis_running = false;
        self.show_confirm_sheet = false;
        self.process_stage = StageState::Running;
        self.status_text = "Applying file changes...".to_string();
        self.process_progress = 0.0;
        self.process_label = format!("Starting {} batches", plan.len());
        self.append_activity(
            ActivityTone::Info,
            "Confirmation received. Applying the approved batch plan.",
        );

        let (tx, rx) = mpsc::channel();
        self.worker_rx = Some(rx);

        thread::spawn(move || {
            let mut log = |line: String| {
                if let Some((batch_index, batch_name)) = parse_batch_start(&line) {
                    let completed = batch_index.saturating_sub(1);
                    let _ = tx.send(WorkerEvent::ProcessProgress {
                        completed_batches: completed,
                        total_batches,
                        label: format!("Processing {batch_name}"),
                    });
                }

                if line.starts_with("[COMPLETE]") {
                    let _ = tx.send(WorkerEvent::ProcessProgress {
                        completed_batches: total_batches,
                        total_batches,
                        label: "Finalizing".to_string(),
                    });
                }

                let _ = tx.send(WorkerEvent::Activity(line));
            };

            let result =
                execute(&plan, series_cover.as_deref(), &mut log).map_err(|err| err.to_string());
            let _ = tx.send(WorkerEvent::ProcessComplete(result));
        });
    }

    fn drain_worker_events(&mut self) {
        let Some(rx) = self.worker_rx.take() else {
            return;
        };

        let mut finished = false;

        while let Ok(event) = rx.try_recv() {
            match event {
                WorkerEvent::Activity(line) => {
                    if let Some(message) = humanize_activity_line(&line) {
                        self.append_activity(activity_tone(&line), message);
                    }
                }
                WorkerEvent::AnalysisComplete(result) => {
                    finished = true;
                    self.analysis_running = false;

                    match result {
                        Ok(snapshot) => {
                            let cover_path = snapshot.cover_path.clone();
                            self.status_text =
                                "Plan ready. Review and confirm processing.".to_string();
                            self.analysis_stage = StageState::Complete;
                            self.plan_stage = StageState::Complete;
                            self.process_stage = StageState::Pending;
                            self.process_progress = 0.0;
                            self.process_label =
                                format!("{} batches ready", snapshot.batch_count());

                            self.append_activity(
                                ActivityTone::Success,
                                format!(
                                    "{} volumes organized into {} batches.",
                                    snapshot.volume_count,
                                    snapshot.batch_count()
                                ),
                            );

                            if let Some(path) = cover_path.clone() {
                                self.append_activity(
                                    ActivityTone::Success,
                                    format!("Cover preview ready from {}", path.display()),
                                );
                            } else {
                                self.append_activity(
                                    ActivityTone::Warning,
                                    "No cover found. Batch folders will skip generated covers.",
                                );
                            }

                            self.analysis = Some(snapshot);
                            self.set_cover_path(cover_path);
                        }
                        Err(err) => {
                            self.status_text = format!("Could not build plan: {err}");
                            self.analysis_stage = StageState::Error;
                            self.plan_stage = StageState::Error;
                            self.process_stage = StageState::Pending;
                            self.analysis = None;
                            self.set_cover_path(None);
                            self.append_activity(
                                ActivityTone::Error,
                                format!("Automatic checks failed: {err}"),
                            );
                        }
                    }
                }
                WorkerEvent::ProcessProgress {
                    completed_batches,
                    total_batches,
                    label,
                } => {
                    let pct = if total_batches == 0 {
                        0.0
                    } else {
                        completed_batches as f32 / total_batches as f32
                    };
                    self.process_progress = pct.clamp(0.0, 1.0);
                    self.process_label = label;
                }
                WorkerEvent::ProcessComplete(result) => {
                    finished = true;
                    self.processing_running = false;

                    match result {
                        Ok(()) => {
                            self.process_stage = StageState::Complete;
                            self.process_progress = 1.0;
                            self.process_label = "All batches complete".to_string();
                            self.status_text = "Processing finished.".to_string();
                            self.append_activity(
                                ActivityTone::Success,
                                "Processing finished. Files and covers were updated.",
                            );
                        }
                        Err(err) => {
                            self.process_stage = StageState::Error;
                            self.status_text = format!("Processing failed: {err}");
                            self.append_activity(
                                ActivityTone::Error,
                                format!("Processing failed: {err}"),
                            );
                        }
                    }
                }
            }
        }

        if !finished {
            self.worker_rx = Some(rx);
        }
    }

    fn status_chip_palette(&self) -> (Color, Color, Color, &'static str) {
        if self.analysis_stage == StageState::Error || self.process_stage == StageState::Error {
            (
                Color::from_rgba8(208, 79, 84, 0.18),
                Color::from_rgba8(181, 53, 58, 0.45),
                Color::from_rgb8(138, 35, 40),
                ICON_ERROR,
            )
        } else if self.is_busy() {
            (
                Color::from_rgba8(237, 184, 63, 0.22),
                Color::from_rgba8(214, 151, 28, 0.50),
                Color::from_rgb8(122, 78, 10),
                ICON_RUNNING,
            )
        } else if self.process_stage == StageState::Complete {
            (
                Color::from_rgba8(55, 165, 116, 0.18),
                Color::from_rgba8(42, 138, 94, 0.46),
                Color::from_rgb8(25, 106, 73),
                ICON_DONE,
            )
        } else {
            (
                Color::from_rgba8(99, 119, 147, 0.16),
                Color::from_rgba8(99, 119, 147, 0.42),
                Color::from_rgb8(67, 82, 103),
                ICON_FLOW,
            )
        }
    }

    fn plan_detail_text(&self) -> String {
        if self.plan_stage == StageState::Running {
            "Building move map".to_string()
        } else if let Some(snapshot) = &self.analysis {
            format!(
                "{} batches, {} renames",
                snapshot.batch_count(),
                snapshot.rename_count
            )
        } else if self.plan_stage == StageState::Error {
            "Plan unavailable".to_string()
        } else {
            "Awaiting folder analysis".to_string()
        }
    }

    fn process_detail_text(&self) -> String {
        if self.processing_running {
            self.process_label.clone()
        } else if self.process_stage == StageState::Complete {
            "Completed".to_string()
        } else if self.process_stage == StageState::Error {
            "Failed".to_string()
        } else {
            "Final confirmation required".to_string()
        }
    }

    fn render_cover_preview(&self) -> Element<'_, Message> {
        if let Some(handle) = &self.cover_handle {
            container(
                image(handle.clone())
                    .content_fit(iced::ContentFit::Contain)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(cover_surface)
            .into()
        } else {
            container(
                column![
                    text(ICON_COVER)
                        .font(FONT_SYMBOLS)
                        .size(30)
                        .style(theme::Text::Color(Color::from_rgb8(117, 131, 152))),
                    text("Cover preview will appear here")
                        .font(FONT_DISPLAY)
                        .size(15)
                        .style(theme::Text::Color(Color::from_rgb8(74, 88, 109))),
                    text("Auto checks resolve the best local or remote cover before planning.")
                        .font(FONT_TEXT)
                        .size(12)
                        .style(theme::Text::Color(Color::from_rgb8(103, 116, 136))),
                ]
                .spacing(8)
                .align_items(Alignment::Center),
            )
            .padding([16, 18])
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .style(cover_placeholder_surface)
            .into()
        }
    }

    fn render_plan_tree(&self) -> Element<'_, Message> {
        let Some(snapshot) = &self.analysis else {
            return container(
                column![
                    text("Plan preview")
                        .font(FONT_DISPLAY)
                        .size(17)
                        .style(theme::Text::Color(Color::from_rgb8(35, 48, 64))),
                    text("Choose a folder to generate batch and move previews automatically.")
                        .font(FONT_TEXT)
                        .size(13)
                        .style(theme::Text::Color(Color::from_rgb8(89, 104, 124))),
                ]
                .spacing(8),
            )
            .padding([18, 20])
            .style(card_surface)
            .into();
        };

        let mut batches = column![
            row![
                text("Planned File Tree")
                    .font(FONT_DISPLAY)
                    .size(18)
                    .style(theme::Text::Color(Color::from_rgb8(33, 46, 62))),
                horizontal_space(),
                chip(
                    format!("{} batches", snapshot.batch_count()),
                    Color::from_rgba8(36, 128, 197, 0.14),
                    Color::from_rgba8(36, 128, 197, 0.35),
                    Color::from_rgb8(23, 87, 132),
                ),
            ]
            .align_items(Alignment::Center),
            text("Preview of destination folders and move/rename operations.")
                .font(FONT_TEXT)
                .size(12)
                .style(theme::Text::Color(Color::from_rgb8(94, 109, 129))),
            horizontal_rule(1),
        ]
        .spacing(10);

        for batch in &snapshot.plan {
            let mut rows = column![].spacing(8);
            for mv in &batch.moves {
                let src_name = leaf_name(&mv.src);
                let renamed = src_name != mv.dst_name;
                let action_chip = if renamed {
                    chip(
                        "Rename".to_string(),
                        Color::from_rgba8(44, 132, 208, 0.14),
                        Color::from_rgba8(44, 132, 208, 0.34),
                        Color::from_rgb8(19, 92, 145),
                    )
                } else {
                    chip(
                        "Move".to_string(),
                        Color::from_rgba8(104, 123, 150, 0.14),
                        Color::from_rgba8(104, 123, 150, 0.30),
                        Color::from_rgb8(66, 81, 100),
                    )
                };

                rows = rows.push(
                    row![
                        action_chip,
                        text(src_name)
                            .font(FONT_TEXT)
                            .size(13)
                            .style(theme::Text::Color(Color::from_rgb8(44, 57, 74))),
                        text(ICON_ARROW)
                            .font(FONT_SYMBOLS)
                            .size(12)
                            .style(theme::Text::Color(Color::from_rgb8(109, 122, 141))),
                        text(&mv.dst_name)
                            .font(FONT_TEXT)
                            .size(13)
                            .style(theme::Text::Color(Color::from_rgb8(25, 37, 52))),
                    ]
                    .spacing(8)
                    .align_items(Alignment::Center),
                );
            }

            if batch.will_make_cover {
                rows = rows.push(
                    row![
                        chip(
                            "Cover".to_string(),
                            Color::from_rgba8(52, 158, 116, 0.14),
                            Color::from_rgba8(52, 158, 116, 0.34),
                            Color::from_rgb8(26, 116, 84),
                        ),
                        text("Create cover.jpg and preserve existing covers as cover_old_*.")
                            .font(FONT_TEXT)
                            .size(12)
                            .style(theme::Text::Color(Color::from_rgb8(54, 73, 93))),
                    ]
                    .spacing(8)
                    .align_items(Alignment::Center),
                );
            }

            let start = (batch.batch_index - 1) * FILES_PER_FOLDER + 1;
            let end = start + batch.moves.len().saturating_sub(1);

            let batch_card = container(
                column![
                    row![
                        text(format!(
                            "Batch {}  (volumes {}-{})",
                            batch.batch_index, start, end
                        ))
                        .font(FONT_DISPLAY)
                        .size(16)
                        .style(theme::Text::Color(Color::from_rgb8(33, 47, 63))),
                        horizontal_space(),
                        chip(
                            format!("{} files", batch.moves.len()),
                            Color::from_rgba8(88, 106, 136, 0.14),
                            Color::from_rgba8(88, 106, 136, 0.30),
                            Color::from_rgb8(62, 77, 98),
                        ),
                    ]
                    .align_items(Alignment::Center),
                    text(batch.batch_dir.display().to_string())
                        .font(FONT_TEXT)
                        .size(12)
                        .style(theme::Text::Color(Color::from_rgb8(100, 114, 133))),
                    horizontal_rule(1),
                    rows,
                ]
                .spacing(9),
            )
            .padding([14, 15])
            .style(plan_batch_surface);

            batches = batches.push(batch_card);
        }

        container(scrollable(batches).height(Length::Fill))
            .padding([14, 15])
            .height(Length::Fill)
            .style(card_surface)
            .into()
    }
}

impl Application for MangaCleanerApp {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = AppFlags;

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let mut app = Self {
            series_dir_input: flags.initial_series_dir,
            status_text: "Choose a folder to start.".to_string(),
            analysis_stage: StageState::Pending,
            plan_stage: StageState::Pending,
            process_stage: StageState::Pending,
            analysis_running: false,
            processing_running: false,
            show_confirm_sheet: false,
            process_progress: 0.0,
            process_label: "Waiting for analysis".to_string(),
            analysis: None,
            cover_path: None,
            cover_handle: None,
            activity: Vec::new(),
            worker_rx: None,
        };

        app.append_activity(
            ActivityTone::Info,
            "Select a series folder. Checks and planning run automatically.",
        );

        if !app.series_dir_input.trim().is_empty() {
            let initial = app.series_dir_input.clone();
            app.set_series_folder(initial);
        }

        (app, Command::none())
    }

    fn title(&self) -> String {
        "Manga Cleaner Native".to_string()
    }

    fn theme(&self) -> Self::Theme {
        Theme::custom(
            "Quartz Native".to_string(),
            theme::Palette {
                background: Color::from_rgb8(243, 247, 252),
                text: Color::from_rgb8(21, 30, 41),
                primary: Color::from_rgb8(17, 108, 219),
                success: Color::from_rgb8(31, 142, 101),
                danger: Color::from_rgb8(194, 66, 72),
            },
        )
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        time::every(Duration::from_millis(120)).map(|_| Message::Tick)
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::BrowseFolder => {
                if self.is_busy() {
                    return Command::none();
                }

                if let Some(folder) = FileDialog::new().pick_folder() {
                    self.set_series_folder(folder.display().to_string());
                }
            }
            Message::RefreshAnalysis => {
                if self.can_refresh() {
                    self.activity.clear();
                    self.reset_for_new_analysis();
                    self.start_analysis();
                }
            }
            Message::RequestProcess => {
                if self.can_process() {
                    self.show_confirm_sheet = true;
                }
            }
            Message::CancelProcessConfirmation => {
                self.show_confirm_sheet = false;
            }
            Message::ConfirmProcess => {
                self.show_confirm_sheet = false;
                self.start_process();
            }
            Message::Tick => {
                self.drain_worker_events();
            }
        }

        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let (status_bg, status_border, status_color, status_icon) = self.status_chip_palette();

        let title_block = column![
            text("Manga Cleaner")
                .font(FONT_DISPLAY)
                .size(44)
                .style(theme::Text::Color(Color::from_rgb8(16, 25, 35))),
            text("Native-first manga batching with automatic checks and visual previews")
                .font(FONT_TEXT)
                .size(14)
                .style(theme::Text::Color(Color::from_rgb8(87, 101, 121))),
        ]
        .spacing(4);

        let status_chip = container(
            row![
                text(status_icon)
                    .font(FONT_SYMBOLS)
                    .size(16)
                    .style(theme::Text::Color(status_color)),
                text(&self.status_text)
                    .font(FONT_TEXT)
                    .size(14)
                    .style(theme::Text::Color(status_color)),
            ]
            .spacing(8)
            .align_items(Alignment::Center),
        )
        .padding([10, 14])
        .style(move |_theme: &Theme| iced::widget::container::Appearance {
            text_color: None,
            background: Some(Background::Color(status_bg)),
            border: Border {
                color: status_border,
                width: 1.0,
                radius: 999.0.into(),
            },
            shadow: Shadow::default(),
        });

        let topbar = row![title_block, horizontal_space(), status_chip]
            .spacing(12)
            .align_items(Alignment::Center);

        let mut browse_button = button(
            row![
                text(ICON_FOLDER)
                    .font(FONT_SYMBOLS)
                    .size(17)
                    .style(theme::Text::Color(Color::WHITE)),
                text("Choose Folder")
                    .font(FONT_DISPLAY)
                    .size(15)
                    .style(theme::Text::Color(Color::WHITE)),
            ]
            .spacing(7)
            .align_items(Alignment::Center),
        )
        .padding([11, 15])
        .style(theme::Button::custom(NativeButton::new(ButtonTone::Accent)));

        if !self.is_busy() {
            browse_button = browse_button.on_press(Message::BrowseFolder);
        }

        let mut refresh_button = button(
            row![
                text(ICON_REFRESH)
                    .font(FONT_SYMBOLS)
                    .size(16)
                    .style(theme::Text::Color(Color::from_rgb8(40, 57, 77))),
                text("Run Checks Again")
                    .font(FONT_TEXT)
                    .size(14)
                    .style(theme::Text::Color(Color::from_rgb8(40, 57, 77))),
            ]
            .spacing(7)
            .align_items(Alignment::Center),
        )
        .padding([11, 14])
        .style(theme::Button::custom(NativeButton::new(
            ButtonTone::Secondary,
        )));

        if self.can_refresh() {
            refresh_button = refresh_button.on_press(Message::RefreshAnalysis);
        }

        let source_card = container(
            column![
                row![
                    column![
                        text("Source Folder")
                            .font(FONT_DISPLAY)
                            .size(15)
                            .style(theme::Text::Color(Color::from_rgb8(37, 52, 70))),
                        text("Auto checks run immediately. Only processing asks for confirmation.")
                            .font(FONT_TEXT)
                            .size(12)
                            .style(theme::Text::Color(Color::from_rgb8(97, 111, 131))),
                    ]
                    .spacing(4),
                    horizontal_space(),
                    row![browse_button, refresh_button].spacing(10),
                ]
                .align_items(Alignment::Center),
                container(
                    text(if self.series_dir_input.trim().is_empty() {
                        "No folder selected yet.".to_string()
                    } else {
                        self.series_dir_input.clone()
                    })
                    .font(FONT_TEXT)
                    .size(13)
                    .style(theme::Text::Color(Color::from_rgb8(52, 66, 84))),
                )
                .padding([12, 14])
                .width(Length::Fill)
                .style(path_well_surface),
            ]
            .spacing(12),
        )
        .padding([16, 18])
        .style(card_surface);

        let analysis_detail = if self.analysis_running {
            "Scanning files and cover sources".to_string()
        } else if let Some(snapshot) = &self.analysis {
            format!("{} files validated", snapshot.volume_count)
        } else if self.analysis_stage == StageState::Error {
            "Could not analyze source".to_string()
        } else {
            "Waiting for folder selection".to_string()
        };

        let flow_card = container(
            row![
                stage_card("Auto Checks", analysis_detail, self.analysis_stage),
                stage_card("Plan Preview", self.plan_detail_text(), self.plan_stage),
                stage_card("Process", self.process_detail_text(), self.process_stage),
            ]
            .spacing(10),
        )
        .padding([14, 16])
        .style(card_surface);

        let cover_card = container(
            column![
                row![
                    text("Cover Preview")
                        .font(FONT_DISPLAY)
                        .size(16)
                        .style(theme::Text::Color(Color::from_rgb8(31, 45, 62))),
                    horizontal_space(),
                    if self.cover_path.is_some() {
                        chip(
                            "Ready".to_string(),
                            Color::from_rgba8(52, 158, 116, 0.14),
                            Color::from_rgba8(52, 158, 116, 0.36),
                            Color::from_rgb8(23, 110, 79),
                        )
                    } else {
                        chip(
                            "Pending".to_string(),
                            Color::from_rgba8(120, 136, 160, 0.16),
                            Color::from_rgba8(120, 136, 160, 0.36),
                            Color::from_rgb8(77, 92, 112),
                        )
                    }
                ]
                .align_items(Alignment::Center),
                container(self.render_cover_preview())
                    .height(Length::FillPortion(4))
                    .width(Length::Fill),
                text(
                    self.cover_path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "No cover selected yet.".to_string()),
                )
                .font(FONT_TEXT)
                .size(12)
                .style(theme::Text::Color(Color::from_rgb8(101, 116, 136))),
            ]
            .spacing(10),
        )
        .padding([15, 16])
        .height(Length::FillPortion(2))
        .style(card_surface);

        let summary_content = if let Some(snapshot) = &self.analysis {
            column![
                stat_line("Source", snapshot.resolved_dir.display().to_string()),
                stat_line("Volumes", snapshot.volume_count.to_string()),
                stat_line("Batches", snapshot.batch_count().to_string()),
                stat_line("Renames", snapshot.rename_count.to_string()),
                stat_line(
                    "Cover output",
                    if snapshot.cover_batch_count() > 0 {
                        format!("{} folders", snapshot.cover_batch_count())
                    } else {
                        "Skipped".to_string()
                    }
                ),
            ]
            .spacing(8)
        } else {
            column![text("Plan summary will appear after automatic checks.")
                .font(FONT_TEXT)
                .size(13)
                .style(theme::Text::Color(Color::from_rgb8(89, 104, 124))),]
        };

        let process_label = if self.processing_running {
            "Processing..."
        } else if self.process_stage == StageState::Complete {
            "Completed"
        } else {
            "Process Files"
        };

        let mut process_button = button(
            row![
                text(ICON_PROCESS)
                    .font(FONT_SYMBOLS)
                    .size(17)
                    .style(theme::Text::Color(Color::WHITE)),
                text(process_label)
                    .font(FONT_DISPLAY)
                    .size(15)
                    .style(theme::Text::Color(Color::WHITE)),
            ]
            .spacing(8)
            .align_items(Alignment::Center),
        )
        .padding([12, 14])
        .style(theme::Button::custom(NativeButton::new(ButtonTone::Danger)))
        .width(Length::Fill);

        if self.can_process() {
            process_button = process_button.on_press(Message::RequestProcess);
        }

        let mut summary_column = column![
            row![
                text("Execution")
                    .font(FONT_DISPLAY)
                    .size(16)
                    .style(theme::Text::Color(Color::from_rgb8(31, 45, 62))),
                horizontal_space(),
                if self.processing_running {
                    chip(
                        "Running".to_string(),
                        Color::from_rgba8(235, 180, 57, 0.18),
                        Color::from_rgba8(210, 147, 22, 0.40),
                        Color::from_rgb8(122, 80, 14),
                    )
                } else if self.process_stage == StageState::Complete {
                    chip(
                        "Finished".to_string(),
                        Color::from_rgba8(52, 158, 116, 0.14),
                        Color::from_rgba8(52, 158, 116, 0.36),
                        Color::from_rgb8(23, 110, 79),
                    )
                } else {
                    chip(
                        "Awaiting approval".to_string(),
                        Color::from_rgba8(118, 134, 158, 0.16),
                        Color::from_rgba8(118, 134, 158, 0.36),
                        Color::from_rgb8(74, 89, 109),
                    )
                }
            ]
            .align_items(Alignment::Center),
            summary_content,
            progress_bar(0.0..=1.0, self.process_progress).height(Length::Fixed(8.0)),
            text(&self.process_label)
                .font(FONT_TEXT)
                .size(12)
                .style(theme::Text::Color(Color::from_rgb8(94, 108, 128))),
            process_button,
        ]
        .spacing(10);

        if self.show_confirm_sheet {
            let destructive_summary = if let Some(snapshot) = &self.analysis {
                format!(
                    "This will move {} volume files into {} destination folders and write batch covers where available.",
                    snapshot.volume_count,
                    snapshot.batch_count()
                )
            } else {
                "This will apply the prepared file and cover changes.".to_string()
            };

            let cancel_btn = button(
                text("Not yet")
                    .font(FONT_TEXT)
                    .size(14)
                    .style(theme::Text::Color(Color::from_rgb8(53, 69, 89))),
            )
            .padding([10, 14])
            .style(theme::Button::custom(NativeButton::new(ButtonTone::Ghost)))
            .on_press(Message::CancelProcessConfirmation);

            let confirm_btn = button(
                text("Confirm and Process")
                    .font(FONT_DISPLAY)
                    .size(14)
                    .style(theme::Text::Color(Color::WHITE)),
            )
            .padding([10, 14])
            .style(theme::Button::custom(NativeButton::new(ButtonTone::Danger)))
            .on_press(Message::ConfirmProcess);

            summary_column = summary_column.push(
                container(
                    column![
                        text("Final Confirmation")
                            .font(FONT_DISPLAY)
                            .size(14)
                            .style(theme::Text::Color(Color::from_rgb8(118, 34, 39))),
                        text(destructive_summary)
                            .font(FONT_TEXT)
                            .size(12)
                            .style(theme::Text::Color(Color::from_rgb8(113, 50, 54))),
                        row![cancel_btn, confirm_btn]
                            .spacing(9)
                            .align_items(Alignment::Center),
                    ]
                    .spacing(10),
                )
                .padding([12, 13])
                .style(confirm_surface),
            );
        }

        let summary_card = container(summary_column)
            .padding([15, 16])
            .height(Length::FillPortion(2))
            .style(card_surface);

        let left_column = column![cover_card, summary_card]
            .spacing(12)
            .width(Length::FillPortion(1))
            .height(Length::Fill);

        let plan_panel = container(self.render_plan_tree())
            .width(Length::FillPortion(2))
            .height(Length::Fill);

        let workspace_row = row![left_column, plan_panel]
            .spacing(12)
            .height(Length::FillPortion(3));

        let mut activity_list = column![
            row![
                text(ICON_ACTIVITY)
                    .font(FONT_SYMBOLS)
                    .size(16)
                    .style(theme::Text::Color(Color::from_rgb8(57, 74, 96))),
                text("Activity")
                    .font(FONT_DISPLAY)
                    .size(16)
                    .style(theme::Text::Color(Color::from_rgb8(33, 47, 63))),
            ]
            .spacing(8)
            .align_items(Alignment::Center),
            horizontal_rule(1),
        ]
        .spacing(9);

        if self.activity.is_empty() {
            activity_list = activity_list.push(
                text("No activity yet.")
                    .font(FONT_TEXT)
                    .size(12)
                    .style(theme::Text::Color(Color::from_rgb8(96, 111, 131))),
            );
        } else {
            for entry in &self.activity {
                let dot_color = match entry.tone {
                    ActivityTone::Info => Color::from_rgb8(86, 106, 134),
                    ActivityTone::Success => Color::from_rgb8(37, 140, 101),
                    ActivityTone::Warning => Color::from_rgb8(165, 112, 22),
                    ActivityTone::Error => Color::from_rgb8(178, 53, 59),
                };

                let symbol = match entry.tone {
                    ActivityTone::Info => ICON_WAITING,
                    ActivityTone::Success => ICON_DONE,
                    ActivityTone::Warning => ICON_RUNNING,
                    ActivityTone::Error => ICON_ERROR,
                };

                activity_list = activity_list.push(
                    row![
                        text(symbol)
                            .font(FONT_SYMBOLS)
                            .size(12)
                            .style(theme::Text::Color(dot_color)),
                        text(&entry.message)
                            .font(FONT_TEXT)
                            .size(12)
                            .style(theme::Text::Color(Color::from_rgb8(58, 73, 92))),
                    ]
                    .spacing(7)
                    .align_items(Alignment::Center),
                );
            }
        }

        let activity_card = container(scrollable(activity_list).height(Length::Fill))
            .padding([14, 15])
            .height(Length::FillPortion(1))
            .style(card_surface);

        let content = column![topbar, source_card, flow_card, workspace_row, activity_card]
            .spacing(12)
            .padding(top_content_padding())
            .width(Length::Fill)
            .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(app_surface)
            .into()
    }
}

fn stage_card<'a>(title: &'a str, detail: String, state: StageState) -> Element<'a, Message> {
    let (badge_text, badge_bg, badge_border, badge_color, icon) = stage_palette(state);

    container(
        column![
            row![
                text(icon)
                    .font(FONT_SYMBOLS)
                    .size(14)
                    .style(theme::Text::Color(badge_color)),
                text(title)
                    .font(FONT_DISPLAY)
                    .size(14)
                    .style(theme::Text::Color(Color::from_rgb8(35, 50, 68))),
                horizontal_space(),
                chip(badge_text.to_string(), badge_bg, badge_border, badge_color),
            ]
            .spacing(6)
            .align_items(Alignment::Center),
            text(detail)
                .font(FONT_TEXT)
                .size(12)
                .style(theme::Text::Color(Color::from_rgb8(95, 111, 131))),
        ]
        .spacing(8),
    )
    .padding([11, 12])
    .width(Length::FillPortion(1))
    .style(flow_step_surface)
    .into()
}

fn stat_line<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    row![
        text(label)
            .font(FONT_TEXT)
            .size(12)
            .style(theme::Text::Color(Color::from_rgb8(96, 111, 131))),
        horizontal_space(),
        text(value)
            .font(FONT_TEXT)
            .size(12)
            .style(theme::Text::Color(Color::from_rgb8(43, 56, 74))),
    ]
    .align_items(Alignment::Center)
    .into()
}

fn chip<'a>(
    value: String,
    bg: Color,
    border: Color,
    text_color: Color,
) -> iced::widget::Container<'a, Message> {
    container(
        text(value)
            .font(FONT_TEXT)
            .size(11)
            .style(theme::Text::Color(text_color)),
    )
    .padding([4, 9])
    .style(move |_theme: &Theme| iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: 999.0.into(),
        },
        shadow: Shadow::default(),
    })
}

fn stage_palette(state: StageState) -> (&'static str, Color, Color, Color, &'static str) {
    match state {
        StageState::Pending => (
            "Waiting",
            Color::from_rgba8(118, 134, 158, 0.14),
            Color::from_rgba8(118, 134, 158, 0.32),
            Color::from_rgb8(78, 92, 112),
            ICON_WAITING,
        ),
        StageState::Running => (
            "In progress",
            Color::from_rgba8(235, 180, 57, 0.18),
            Color::from_rgba8(210, 147, 22, 0.40),
            Color::from_rgb8(122, 80, 14),
            ICON_RUNNING,
        ),
        StageState::Complete => (
            "Ready",
            Color::from_rgba8(52, 158, 116, 0.14),
            Color::from_rgba8(52, 158, 116, 0.34),
            Color::from_rgb8(23, 110, 79),
            ICON_DONE,
        ),
        StageState::Error => (
            "Action needed",
            Color::from_rgba8(208, 79, 84, 0.15),
            Color::from_rgba8(181, 53, 58, 0.34),
            Color::from_rgb8(138, 35, 40),
            ICON_ERROR,
        ),
    }
}

fn humanize_activity_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.chars().all(|ch| ch == '-' || ch == '=') {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix("[COVER] ") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("[WARN] ") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("[ERROR] ") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("[DO] Batch ") {
        return Some(format!("Processing {rest}"));
    }
    if let Some(rest) = trimmed.strip_prefix("[MOVE] ") {
        return Some(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("[COMPLETE] ") {
        return Some(rest.to_string());
    }

    if trimmed.starts_with("[PLAN]") {
        return None;
    }

    Some(trimmed.to_string())
}

fn activity_tone(line: &str) -> ActivityTone {
    let lower = line.to_ascii_lowercase();
    if line.contains("[ERROR]") || lower.contains("failed") {
        ActivityTone::Error
    } else if line.contains("[WARN]") {
        ActivityTone::Warning
    } else if line.contains("[COMPLETE]") || line.contains("[COVER]") {
        ActivityTone::Success
    } else {
        ActivityTone::Info
    }
}

fn parse_batch_start(line: &str) -> Option<(usize, String)> {
    let rest = line.strip_prefix("[DO] Batch ")?;
    let (index_raw, name_raw) = rest.split_once(':')?;
    let batch_index = index_raw.trim().parse::<usize>().ok()?;
    Some((batch_index, name_raw.trim().to_string()))
}

fn leaf_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn app_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgb8(240, 246, 252))),
        border: Border::default(),
        shadow: Shadow::default(),
    }
}

fn top_content_padding() -> [u16; 4] {
    if cfg!(target_os = "macos") {
        // Leave room for macOS traffic-light controls when content extends into the titlebar.
        [42, 20, 18, 20]
    } else {
        [18, 20, 18, 20]
    }
}

fn card_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgb8(252, 254, 255))),
        border: Border {
            color: Color::from_rgb8(215, 225, 238),
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(11, 22, 38, 0.07),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 12.0,
        },
    }
}

fn flow_step_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgb8(246, 250, 255))),
        border: Border {
            color: Color::from_rgb8(219, 228, 241),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow::default(),
    }
}

fn path_well_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgba8(241, 246, 253, 0.88))),
        border: Border {
            color: Color::from_rgb8(212, 223, 238),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow::default(),
    }
}

fn cover_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgb8(248, 252, 255))),
        border: Border {
            color: Color::from_rgb8(210, 222, 238),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow::default(),
    }
}

fn cover_placeholder_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgba8(238, 244, 253, 0.84))),
        border: Border {
            color: Color::from_rgb8(206, 219, 236),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow::default(),
    }
}

fn confirm_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgba8(252, 233, 234, 0.90))),
        border: Border {
            color: Color::from_rgb8(227, 166, 170),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow::default(),
    }
}

fn plan_batch_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgb8(247, 251, 255))),
        border: Border {
            color: Color::from_rgb8(213, 224, 240),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow::default(),
    }
}

fn lighten(color: Color, amount: f32) -> Color {
    mix(color, Color::WHITE, amount)
}

fn darken(color: Color, amount: f32) -> Color {
    mix(color, Color::BLACK, amount)
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn main() -> iced::Result {
    let args = UiArgs::parse();

    let mut window_settings = iced::window::Settings {
        size: Size::new(1280.0, 860.0),
        min_size: Some(Size::new(1080.0, 760.0)),
        ..iced::window::Settings::default()
    };

    #[cfg(target_os = "macos")]
    {
        window_settings.platform_specific.title_hidden = true;
        window_settings.platform_specific.titlebar_transparent = true;
        window_settings.platform_specific.fullsize_content_view = true;
    }

    MangaCleanerApp::run(Settings {
        flags: AppFlags {
            initial_series_dir: args.series_dir.unwrap_or_default(),
        },
        window: window_settings,
        default_font: FONT_TEXT,
        antialiasing: true,
        ..Settings::default()
    })
}
