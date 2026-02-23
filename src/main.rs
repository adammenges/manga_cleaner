use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use clap::Parser;
use iced::{
    executor,
    theme::{self, Theme},
    time,
    widget::scrollable::{self, Id as ScrollableId, RelativeOffset},
    widget::{
        button, column, container, horizontal_space, image, row, scrollable as make_scrollable,
        text,
    },
    Alignment, Application, Background, Border, Color, Command, Element, Font, Length, Settings,
    Shadow, Size, Subscription, Vector,
};
use manga_cleaner::{resolve_series_dir, run_action, ActionOutput, UiAction};
use rfd::FileDialog;

const FONT_TEXT: Font = Font::with_name("SF Pro Text");
const FONT_DISPLAY: Font = Font::with_name("SF Pro Display");
const FONT_SYMBOLS: Font = Font::with_name("SF Pro");

const ICON_FOLDER: &str = "􀈕";
const ICON_STATUS: &str = "􀐫";
const ICON_COVER: &str = "􀏅";
const ICON_PLAN: &str = "􀙔";
const ICON_PROCESS: &str = "􀒓";
const ICON_LOG: &str = "􀐌";
const ICON_CLEAR: &str = "􀅈";

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

#[derive(Debug)]
enum WorkerEvent {
    Log(String),
    Done(Result<ActionOutput, String>),
}

#[derive(Debug, Clone, Copy)]
enum ButtonTone {
    Primary,
    Secondary,
    Critical,
    Ghost,
}

#[derive(Debug, Clone, Copy)]
struct SwissButton {
    tone: ButtonTone,
}

impl SwissButton {
    const fn new(tone: ButtonTone) -> Self {
        Self { tone }
    }

    fn palette(self) -> (Option<Color>, Color, Color) {
        match self.tone {
            ButtonTone::Primary => (
                Some(Color::from_rgb8(19, 96, 218)),
                Color::from_rgb8(16, 80, 181),
                Color::WHITE,
            ),
            ButtonTone::Secondary => (
                Some(Color::from_rgb8(243, 246, 251)),
                Color::from_rgb8(209, 216, 228),
                Color::from_rgb8(31, 38, 48),
            ),
            ButtonTone::Critical => (
                Some(Color::from_rgb8(181, 57, 63)),
                Color::from_rgb8(151, 42, 47),
                Color::WHITE,
            ),
            ButtonTone::Ghost => (
                None,
                Color::from_rgb8(196, 205, 218),
                Color::from_rgb8(69, 82, 98),
            ),
        }
    }
}

impl iced::widget::button::StyleSheet for SwissButton {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> iced::widget::button::Appearance {
        let (bg, border, text_color) = self.palette();
        let background = match self.tone {
            ButtonTone::Ghost => Some(Background::Color(Color::from_rgba8(237, 242, 250, 0.58))),
            _ => bg.map(Background::Color),
        };

        iced::widget::button::Appearance {
            shadow_offset: Vector::default(),
            background,
            text_color,
            border: Border {
                color: border,
                width: 1.0,
                radius: 14.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba8(10, 18, 32, 0.12),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 3.0,
            },
        }
    }

    fn hovered(&self, _style: &Self::Style) -> iced::widget::button::Appearance {
        let (bg, border, text_color) = self.palette();
        let background = match self.tone {
            ButtonTone::Ghost => Some(Background::Color(Color::from_rgba8(230, 236, 248, 0.86))),
            _ => bg.map(|c| Background::Color(lighten(c, 0.05))),
        };

        iced::widget::button::Appearance {
            shadow_offset: Vector::default(),
            background,
            text_color,
            border: Border {
                color: darken(border, 0.05),
                width: 1.0,
                radius: 14.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba8(10, 18, 32, 0.16),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 6.0,
            },
        }
    }

    fn pressed(&self, _style: &Self::Style) -> iced::widget::button::Appearance {
        let (bg, border, text_color) = self.palette();
        let background = match self.tone {
            ButtonTone::Ghost => Some(Background::Color(Color::from_rgba8(224, 232, 246, 0.9))),
            _ => bg.map(|c| Background::Color(darken(c, 0.07))),
        };

        iced::widget::button::Appearance {
            shadow_offset: Vector::default(),
            background,
            text_color,
            border: Border {
                color: darken(border, 0.08),
                width: 1.0,
                radius: 14.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba8(10, 18, 32, 0.10),
                offset: Vector::new(0.0, 0.0),
                blur_radius: 2.0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum StatusTone {
    Idle,
    Running,
    Success,
    Error,
}

#[derive(Debug, Clone)]
enum Message {
    BrowseFolder,
    ShowCover,
    ShowPlan,
    Process,
    ClearLog,
    Tick,
}

struct MangaCleanerApp {
    series_dir_input: String,
    status_text: String,
    running: bool,
    log_text: String,
    cover_path: Option<PathBuf>,
    cover_handle: Option<iced::widget::image::Handle>,
    worker_rx: Option<Receiver<WorkerEvent>>,
    terminal_scroll_id: ScrollableId,
}

impl MangaCleanerApp {
    fn append_log_line(&mut self, line: impl AsRef<str>) {
        let line = line.as_ref();
        if !self.log_text.is_empty() {
            self.log_text.push('\n');
        }
        self.log_text.push_str(line);

        let max_chars = 180_000;
        if self.log_text.len() > max_chars {
            let mut drop_to = self.log_text.len() - max_chars;
            while drop_to < self.log_text.len() && !self.log_text.is_char_boundary(drop_to) {
                drop_to += 1;
            }
            self.log_text.drain(..drop_to);
        }
    }

    fn set_status_error(&mut self, message: impl AsRef<str>) {
        self.status_text = format!("Error: {}", message.as_ref());
    }

    fn set_cover_path(&mut self, path: Option<PathBuf>) {
        self.cover_path = path.clone();
        self.cover_handle = path.map(iced::widget::image::Handle::from_path);
    }

    fn status_tone(&self) -> StatusTone {
        if self.running {
            StatusTone::Running
        } else if self.status_text.starts_with("Error") || self.status_text == "Failed" {
            StatusTone::Error
        } else if self.status_text.starts_with("Done") || self.status_text == "Folder selected" {
            StatusTone::Success
        } else {
            StatusTone::Idle
        }
    }

    fn can_run_actions(&self) -> bool {
        !self.running && !self.series_dir_input.trim().is_empty()
    }

    fn start_action(&mut self, action: UiAction) {
        if self.running {
            return;
        }

        let series_dir = match resolve_series_dir(&self.series_dir_input) {
            Ok(path) => path,
            Err(err) => {
                self.set_status_error(err.to_string());
                self.append_log_line(format!("[UI-ERROR] {err}"));
                return;
            }
        };

        let (tx, rx) = mpsc::channel();
        self.worker_rx = Some(rx);
        self.running = true;
        self.status_text = format!("Running: {}", action.label());

        self.append_log_line("");
        self.append_log_line("=".repeat(92));
        self.append_log_line(format!("[{}]", action.action_title()));

        thread::spawn(move || {
            let tx_log = tx.clone();
            let mut send_log = move |line: String| {
                let _ = tx_log.send(WorkerEvent::Log(line));
            };

            let result =
                run_action(action, &series_dir, &mut send_log).map_err(|err| err.to_string());
            let _ = tx.send(WorkerEvent::Done(result));
        });
    }

    fn drain_worker_events(&mut self) -> bool {
        let mut appended = false;
        let mut finished = false;
        let Some(rx) = self.worker_rx.take() else {
            return false;
        };

        while let Ok(event) = rx.try_recv() {
            match event {
                WorkerEvent::Log(line) => {
                    self.append_log_line(line);
                    appended = true;
                }
                WorkerEvent::Done(result) => {
                    finished = true;
                    self.running = false;

                    match result {
                        Ok(output) => {
                            self.status_text = format!("Done: {}", output.action.label());
                            self.append_log_line(format!(
                                "[UI] Action completed successfully ({}).",
                                output.action.label()
                            ));
                            appended = true;

                            if let Some(path) = output.cover_path {
                                self.append_log_line(format!(
                                    "[UI] Cover loaded in-app: {}",
                                    path.display()
                                ));
                                appended = true;
                                self.set_cover_path(Some(path));
                            }
                        }
                        Err(err) => {
                            self.status_text = "Failed".to_string();
                            self.append_log_line(format!("[UI] Action failed: {err}"));
                            appended = true;
                        }
                    }
                }
            }
        }

        if !finished {
            self.worker_rx = Some(rx);
        }
        appended
    }

    fn current_cover_path_text(&self) -> String {
        self.cover_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "No cover loaded.".to_string())
    }

    fn current_series_dir_text(&self) -> String {
        let value = self.series_dir_input.trim();
        if value.is_empty() {
            "No folder selected. Use Browse Folder to choose one.".to_string()
        } else {
            self.series_dir_input.clone()
        }
    }

    fn set_series_folder(&mut self, raw_path: impl AsRef<str>) {
        self.series_dir_input = raw_path.as_ref().to_string();

        match resolve_series_dir(&self.series_dir_input) {
            Ok(path) => {
                self.series_dir_input = path.display().to_string();
                self.status_text = "Folder selected".to_string();
                self.append_log_line(format!("[UI] Series folder selected: {}", path.display()));
            }
            Err(err) => {
                self.set_status_error(err.to_string());
                self.append_log_line(format!("[UI-ERROR] {err}"));
            }
        }
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
            status_text: "Idle".to_string(),
            running: false,
            log_text: String::new(),
            cover_path: None,
            cover_handle: None,
            worker_rx: None,
            terminal_scroll_id: ScrollableId::new("live_output_terminal"),
        };

        app.append_log_line("[UI] Ready. Select a series folder, then choose an action.");
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
            "Swiss Modern".to_string(),
            theme::Palette {
                background: Color::from_rgb8(244, 246, 250),
                text: Color::from_rgb8(20, 25, 34),
                primary: Color::from_rgb8(19, 96, 218),
                success: Color::from_rgb8(26, 129, 94),
                danger: Color::from_rgb8(195, 66, 71),
            },
        )
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        time::every(Duration::from_millis(150)).map(|_| Message::Tick)
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        let mut command = Command::none();

        match message {
            Message::BrowseFolder => {
                if self.running {
                    return Command::none();
                }

                if let Some(folder) = FileDialog::new().pick_folder() {
                    self.set_series_folder(folder.display().to_string());
                }
            }
            Message::ShowCover => {
                self.start_action(UiAction::ShowCover);
            }
            Message::ShowPlan => {
                self.start_action(UiAction::Preview);
            }
            Message::Process => {
                self.start_action(UiAction::Process);
            }
            Message::ClearLog => {
                self.log_text.clear();
                self.append_log_line("[UI] Terminal cleared.");
            }
            Message::Tick => {
                if self.drain_worker_events() {
                    command = scrollable::snap_to(
                        self.terminal_scroll_id.clone(),
                        RelativeOffset { x: 0.0, y: 1.0 },
                    );
                }
            }
        }

        command
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let status_tone = self.status_tone();
        let (status_bg, status_border, status_text) = match status_tone {
            StatusTone::Idle => (
                Color::from_rgba8(124, 138, 158, 0.16),
                Color::from_rgba8(124, 138, 158, 0.45),
                Color::from_rgb8(67, 79, 96),
            ),
            StatusTone::Running => (
                Color::from_rgba8(239, 189, 70, 0.2),
                Color::from_rgba8(217, 160, 36, 0.55),
                Color::from_rgb8(125, 82, 8),
            ),
            StatusTone::Success => (
                Color::from_rgba8(52, 167, 114, 0.17),
                Color::from_rgba8(45, 143, 99, 0.45),
                Color::from_rgb8(19, 112, 74),
            ),
            StatusTone::Error => (
                Color::from_rgba8(204, 74, 79, 0.17),
                Color::from_rgba8(182, 54, 60, 0.45),
                Color::from_rgb8(136, 34, 40),
            ),
        };

        let title_block = column![
            text("Manga Cleaner")
                .font(FONT_DISPLAY)
                .size(42)
                .style(theme::Text::Color(Color::from_rgb8(16, 22, 30))),
            text("Native batch cleanup for manga volumes")
                .font(FONT_TEXT)
                .size(14)
                .style(theme::Text::Color(Color::from_rgb8(90, 101, 118))),
        ]
        .spacing(2);

        let status_chip = container(
            row![
                text(ICON_STATUS)
                    .font(FONT_SYMBOLS)
                    .size(16)
                    .style(theme::Text::Color(status_text)),
                text(&self.status_text)
                    .font(FONT_TEXT)
                    .size(15)
                    .style(theme::Text::Color(status_text)),
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
            .spacing(16)
            .align_items(Alignment::Center);

        let mut browse_btn = button(
            row![
                text(ICON_FOLDER)
                    .font(FONT_SYMBOLS)
                    .size(18)
                    .style(theme::Text::Color(Color::WHITE)),
                text("Browse Folder")
                    .font(FONT_DISPLAY)
                    .size(16)
                    .style(theme::Text::Color(Color::WHITE)),
            ]
            .spacing(8)
            .align_items(Alignment::Center),
        )
        .padding([12, 16])
        .style(theme::Button::custom(SwissButton::new(ButtonTone::Primary)));

        if !self.running {
            browse_btn = browse_btn.on_press(Message::BrowseFolder);
        }

        let folder_well = container(
            column![
                text("Selected Series Path")
                    .font(FONT_TEXT)
                    .size(12)
                    .style(theme::Text::Color(Color::from_rgb8(109, 118, 133))),
                text(self.current_series_dir_text())
                    .font(Font::MONOSPACE)
                    .size(14)
                    .style(theme::Text::Color(Color::from_rgb8(37, 45, 58))),
            ]
            .spacing(5),
        )
        .padding([12, 14])
        .width(Length::Fill)
        .style(|_theme: &Theme| iced::widget::container::Appearance {
            text_color: None,
            background: Some(Background::Color(Color::from_rgba8(252, 253, 255, 0.95))),
            border: Border {
                color: Color::from_rgb8(217, 223, 234),
                width: 1.0,
                radius: 12.0.into(),
            },
            shadow: Shadow::default(),
        });

        let controls_card = container(
            column![
                row![
                    text("Source")
                        .font(FONT_DISPLAY)
                        .size(14)
                        .style(theme::Text::Color(Color::from_rgb8(42, 54, 68))),
                    horizontal_space(),
                    browse_btn,
                ]
                .align_items(Alignment::Center),
                folder_well,
            ]
            .spacing(12),
        )
        .padding([16, 18])
        .style(card_surface);

        let can_run = self.can_run_actions();

        let show_cover_btn = action_button(
            ICON_COVER,
            "Show Cover",
            "Resolve and preview the chosen series cover.",
            ButtonTone::Secondary,
            can_run,
            Message::ShowCover,
        );

        let show_plan_btn = action_button(
            ICON_PLAN,
            "Show Plan",
            "Preview folders, file moves, and generated covers.",
            ButtonTone::Secondary,
            can_run,
            Message::ShowPlan,
        );

        let run_btn = action_button(
            ICON_PROCESS,
            "Commit + Process",
            "Apply the full file and cover workflow immediately.",
            ButtonTone::Critical,
            can_run,
            Message::Process,
        );

        let clear_btn = button(
            row![
                text(ICON_CLEAR)
                    .font(FONT_SYMBOLS)
                    .size(16)
                    .style(theme::Text::Color(Color::from_rgb8(69, 82, 98))),
                text("Clear Log")
                    .font(FONT_TEXT)
                    .size(14)
                    .style(theme::Text::Color(Color::from_rgb8(69, 82, 98))),
            ]
            .spacing(6)
            .align_items(Alignment::Center),
        )
        .padding([11, 14])
        .style(theme::Button::custom(SwissButton::new(ButtonTone::Ghost)))
        .on_press(Message::ClearLog);

        let actions_card = container(
            column![
                row![show_cover_btn, show_plan_btn, run_btn]
                    .spacing(10)
                    .width(Length::Fill),
                row![horizontal_space(), clear_btn].align_items(Alignment::Center),
            ]
            .spacing(10),
        )
        .padding([16, 18])
        .style(card_surface);

        let terminal = make_scrollable(
            container(
                text(&self.log_text)
                    .font(Font::MONOSPACE)
                    .size(13)
                    .style(theme::Text::Color(Color::from_rgb8(203, 230, 218))),
            )
            .padding([14, 16])
            .width(Length::Fill)
            .style(terminal_output_surface),
        )
        .id(self.terminal_scroll_id.clone())
        .height(Length::Fill);

        let terminal_card = container(
            column![
                row![
                    text(ICON_LOG)
                        .font(FONT_SYMBOLS)
                        .size(16)
                        .style(theme::Text::Color(Color::from_rgb8(54, 74, 95))),
                    text("Live Output")
                        .font(FONT_DISPLAY)
                        .size(16)
                        .style(theme::Text::Color(Color::from_rgb8(31, 44, 58))),
                ]
                .spacing(8)
                .align_items(Alignment::Center),
                terminal,
            ]
            .spacing(10),
        )
        .padding([16, 18])
        .width(Length::FillPortion(2))
        .height(Length::Fill)
        .style(card_surface);

        let cover_preview: Element<'_, Message> = if let Some(handle) = &self.cover_handle {
            container(
                image(handle.clone())
                    .content_fit(iced::ContentFit::Contain)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .padding(8)
            .height(Length::FillPortion(4))
            .style(cover_frame_surface)
            .into()
        } else {
            container(
                text("Run Show Cover to load the resolved cover in-app.")
                    .font(FONT_TEXT)
                    .size(13)
                    .style(theme::Text::Color(Color::from_rgb8(85, 100, 118))),
            )
            .padding(12)
            .width(Length::Fill)
            .height(Length::FillPortion(4))
            .center_x()
            .center_y()
            .style(cover_placeholder_surface)
            .into()
        };

        let preview_card = container(
            column![
                text("Cover Preview")
                    .font(FONT_DISPLAY)
                    .size(16)
                    .style(theme::Text::Color(Color::from_rgb8(31, 44, 58))),
                cover_preview,
                text(self.current_cover_path_text())
                    .font(Font::MONOSPACE)
                    .size(12)
                    .style(theme::Text::Color(Color::from_rgb8(100, 111, 125))),
                text("Show Plan is dry-run only. Commit + Process writes all planned changes.")
                    .font(FONT_TEXT)
                    .size(12)
                    .style(theme::Text::Color(Color::from_rgb8(92, 106, 122))),
            ]
            .spacing(10),
        )
        .padding([16, 18])
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .style(card_surface);

        let split = row![terminal_card, preview_card]
            .spacing(14)
            .height(Length::Fill);

        let content = column![topbar, controls_card, actions_card, split]
            .spacing(14)
            .padding([18, 20])
            .width(Length::Fill)
            .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| iced::widget::container::Appearance {
                text_color: None,
                background: Some(Background::Color(Color::from_rgb8(241, 244, 249))),
                border: Border::default(),
                shadow: Shadow::default(),
            })
            .into()
    }
}

fn action_button(
    icon: &'static str,
    title: &'static str,
    detail: &'static str,
    tone: ButtonTone,
    enabled: bool,
    message: Message,
) -> iced::widget::Button<'static, Message> {
    let (icon_color, title_color, detail_color) = match tone {
        ButtonTone::Primary | ButtonTone::Critical => (
            Color::WHITE,
            Color::WHITE,
            Color::from_rgba8(255, 255, 255, 0.84),
        ),
        ButtonTone::Secondary | ButtonTone::Ghost => (
            Color::from_rgb8(33, 47, 63),
            Color::from_rgb8(18, 27, 36),
            Color::from_rgb8(89, 102, 118),
        ),
    };

    let alpha = if enabled { 1.0 } else { 0.58 };
    let icon_color = with_alpha(icon_color, alpha);
    let title_color = with_alpha(title_color, alpha);
    let detail_color = with_alpha(detail_color, alpha);

    let content = column![
        text(icon)
            .font(FONT_SYMBOLS)
            .size(18)
            .style(theme::Text::Color(icon_color)),
        text(title)
            .font(FONT_DISPLAY)
            .size(16)
            .style(theme::Text::Color(title_color)),
        text(detail)
            .font(FONT_TEXT)
            .size(12)
            .style(theme::Text::Color(detail_color)),
    ]
    .spacing(3)
    .align_items(Alignment::Start)
    .width(Length::Fill);

    let mut btn = button(content)
        .padding([13, 14])
        .width(Length::FillPortion(1))
        .style(theme::Button::custom(SwissButton::new(tone)));

    if enabled {
        btn = btn.on_press(message);
    }

    btn
}

fn card_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgb8(252, 253, 255))),
        border: Border {
            color: Color::from_rgb8(218, 224, 235),
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(13, 20, 32, 0.06),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 10.0,
        },
    }
}

fn terminal_output_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgb8(21, 31, 40))),
        border: Border {
            color: Color::from_rgb8(29, 46, 59),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.10),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
    }
}

fn cover_placeholder_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgba8(237, 243, 251, 0.88))),
        border: Border {
            color: Color::from_rgb8(206, 217, 232),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow::default(),
    }
}

fn cover_frame_surface(_theme: &Theme) -> iced::widget::container::Appearance {
    iced::widget::container::Appearance {
        text_color: None,
        background: Some(Background::Color(Color::from_rgb8(247, 250, 255))),
        border: Border {
            color: Color::from_rgb8(209, 220, 236),
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

fn with_alpha(color: Color, alpha: f32) -> Color {
    Color {
        a: color.a * alpha.clamp(0.0, 1.0),
        ..color
    }
}

fn main() -> iced::Result {
    let args = UiArgs::parse();

    MangaCleanerApp::run(Settings {
        flags: AppFlags {
            initial_series_dir: args.series_dir.unwrap_or_default(),
        },
        window: iced::window::Settings {
            size: Size::new(1260.0, 840.0),
            min_size: Some(Size::new(980.0, 700.0)),
            ..iced::window::Settings::default()
        },
        default_font: FONT_TEXT,
        antialiasing: true,
        ..Settings::default()
    })
}
