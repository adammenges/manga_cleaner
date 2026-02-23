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
    widget::{button, column, container, horizontal_space, image, row, scrollable as make_scrollable, text, text_input},
    widget::scrollable::{self, Id as ScrollableId, RelativeOffset},
    Alignment, Application, Color, Command, Element, Font, Length, Settings, Subscription,
};
use manga_cleaner::{resolve_series_dir, run_action, ActionOutput, UiAction};
use rfd::FileDialog;

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

#[derive(Debug, Clone)]
enum Message {
    SeriesDirChanged(String),
    SetFolder,
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

            let result = run_action(action, &series_dir, &mut send_log).map_err(|err| err.to_string());
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

    fn status_color(&self) -> Color {
        if self.running {
            Color::from_rgb(0.60, 0.43, 0.12)
        } else if self.status_text.starts_with("Error") || self.status_text == "Failed" {
            Color::from_rgb(0.63, 0.16, 0.22)
        } else {
            Color::from_rgb(0.13, 0.35, 0.60)
        }
    }

    fn current_cover_path_text(&self) -> String {
        self.cover_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "No cover loaded.".to_string())
    }

    fn validate_and_set_folder(&mut self) {
        match resolve_series_dir(&self.series_dir_input) {
            Ok(path) => {
                self.series_dir_input = path.display().to_string();
                self.status_text = "Folder set".to_string();
                self.append_log_line(format!("[UI] Series folder set: {}", path.display()));
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
            app.append_log_line(format!("[UI] Initial folder: {}", app.series_dir_input));
        }

        (app, Command::none())
    }

    fn title(&self) -> String {
        "Manga Cleaner Native".to_string()
    }

    fn theme(&self) -> Self::Theme {
        Theme::Light
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        time::every(Duration::from_millis(150)).map(|_| Message::Tick)
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        let mut command = Command::none();

        match message {
            Message::SeriesDirChanged(value) => {
                self.series_dir_input = value;
            }
            Message::SetFolder => {
                if !self.running {
                    self.validate_and_set_folder();
                }
            }
            Message::BrowseFolder => {
                if self.running {
                    return Command::none();
                }

                if let Some(folder) = FileDialog::new().pick_folder() {
                    self.series_dir_input = folder.display().to_string();
                    self.validate_and_set_folder();
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
        let title = text("Manga Cleaner Native")
            .size(34)
            .style(theme::Text::Color(Color::from_rgb(0.10, 0.20, 0.31)));

        let status_pill = container(
            text(&self.status_text)
                .size(16)
                .style(theme::Text::Color(self.status_color())),
        )
        .padding([8, 14])
        .style(theme::Container::Box);

        let topbar = row![title, horizontal_space(), status_pill]
            .spacing(16)
            .align_items(Alignment::Center);

        let mut set_folder_btn = button(text("Set Folder")).style(theme::Button::Secondary);
        if !self.running {
            set_folder_btn = set_folder_btn.on_press(Message::SetFolder);
        }

        let mut browse_btn = button(text("Browse macOS")).style(theme::Button::Primary);
        if !self.running {
            browse_btn = browse_btn.on_press(Message::BrowseFolder);
        }

        let folder_row = row![
            text_input("/path/to/series-folder", &self.series_dir_input)
                .on_input(Message::SeriesDirChanged)
                .on_submit(Message::SetFolder)
                .padding(10)
                .size(16)
                .width(Length::Fill),
            set_folder_btn,
            browse_btn,
        ]
        .spacing(8)
        .align_items(Alignment::Center);

        let mut show_cover_btn = button(text("Show Cover")).style(theme::Button::Primary);
        if !self.running {
            show_cover_btn = show_cover_btn.on_press(Message::ShowCover);
        }

        let mut show_plan_btn = button(text("Show Plan")).style(theme::Button::Secondary);
        if !self.running {
            show_plan_btn = show_plan_btn.on_press(Message::ShowPlan);
        }

        let mut run_btn = button(text("Commit + Process")).style(theme::Button::Destructive);
        if !self.running {
            run_btn = run_btn.on_press(Message::Process);
        }

        let clear_btn = button(text("Clear Terminal"))
            .style(theme::Button::Secondary)
            .on_press(Message::ClearLog);

        let controls = column![
            text("Series Folder")
                .size(16)
                .style(theme::Text::Color(Color::from_rgb(0.25, 0.32, 0.41))),
            folder_row,
            row![show_cover_btn, show_plan_btn, run_btn, clear_btn].spacing(8),
        ]
        .spacing(10);

        let terminal = make_scrollable(
            container(
                text(&self.log_text)
                    .font(Font::MONOSPACE)
                    .size(14)
                    .style(theme::Text::Color(Color::from_rgb(0.15, 0.25, 0.17))),
            )
            .padding(12)
            .width(Length::Fill)
            .style(theme::Container::Box),
        )
        .id(self.terminal_scroll_id.clone())
        .height(Length::Fill);

        let terminal_card = container(
            column![
                text("Live Output")
                    .size(16)
                    .style(theme::Text::Color(Color::from_rgb(0.20, 0.30, 0.40))),
                terminal,
            ]
            .spacing(8),
        )
        .padding(12)
        .width(Length::FillPortion(2))
        .height(Length::Fill)
        .style(theme::Container::Box);

        let cover_preview: Element<'_, Message> = if let Some(handle) = &self.cover_handle {
            image(handle.clone())
                .content_fit(iced::ContentFit::Contain)
                .width(Length::Fill)
                .height(Length::FillPortion(4))
                .into()
        } else {
            container(text("Run Show Cover to load the resolved cover in-app."))
                .padding(12)
                .width(Length::Fill)
                .height(Length::FillPortion(4))
                .center_x()
                .center_y()
                .style(theme::Container::Box)
                .into()
        };

        let preview_card = container(
            column![
                text("Cover Preview")
                    .size(16)
                    .style(theme::Text::Color(Color::from_rgb(0.20, 0.30, 0.40))),
                cover_preview,
                text(self.current_cover_path_text())
                    .size(13)
                    .style(theme::Text::Color(Color::from_rgb(0.35, 0.42, 0.50))),
                text("Show Plan prints dry-run output. Commit + Process applies moves and cover writes.")
                    .size(13)
                    .style(theme::Text::Color(Color::from_rgb(0.34, 0.44, 0.54))),
            ]
            .spacing(10),
        )
        .padding(12)
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .style(theme::Container::Box);

        let split = row![terminal_card, preview_card]
            .spacing(14)
            .height(Length::Fill);

        let content = column![topbar, container(controls).padding(12).style(theme::Container::Box), split]
            .spacing(14)
            .padding(18)
            .width(Length::Fill)
            .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn main() -> iced::Result {
    let args = UiArgs::parse();

    MangaCleanerApp::run(Settings {
        flags: AppFlags {
            initial_series_dir: args.series_dir.unwrap_or_default(),
        },
        ..Settings::default()
    })
}
