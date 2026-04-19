use std::{
    ffi::OsString,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Frame, Margin, RichText,
    Stroke, TextEdit, Vec2, ViewportBuilder,
};
use shadow_terminal::{
    shadow_terminal::Config as ShadowConfig,
    steppable_terminal::{Input as TerminalInput, SteppableTerminal},
};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([1380.0, 860.0])
            .with_min_inner_size([1040.0, 720.0])
            .with_title("Velocity"),
        ..Default::default()
    };

    eframe::run_native(
        "Velocity",
        options,
        Box::new(|cc| {
            configure_theme(&cc.egui_ctx);
            Ok(Box::new(VelocityApp::new(cc.egui_ctx.clone())))
        }),
    )
}

fn configure_theme(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "roboto".to_owned(),
        FontData::from_static(include_bytes!("../assets/Roboto-Regular.ttf")).into(),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "roboto".to_owned());
    ctx.set_fonts(fonts);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = Vec2::new(10.0, 10.0);
    style.spacing.button_padding = Vec2::new(14.0, 10.0);
    style.visuals.window_fill = color(15, 16, 19);
    style.visuals.panel_fill = color(15, 16, 19);
    style.visuals.extreme_bg_color = color(12, 13, 16);
    style.visuals.faint_bg_color = color(34, 36, 42);
    style.visuals.widgets.noninteractive.bg_fill = color(34, 36, 42);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, color(56, 58, 64));
    style.visuals.widgets.inactive.bg_fill = color(34, 36, 42);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, color(56, 58, 64));
    style.visuals.widgets.hovered.bg_fill = color(41, 44, 51);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, color(82, 87, 98));
    style.visuals.widgets.active.bg_fill = color(46, 50, 58);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, color(110, 116, 128));
    style.visuals.widgets.open.bg_fill = color(34, 36, 42);
    style.visuals.selection.bg_fill = color(66, 84, 125);
    style.visuals.selection.stroke = Stroke::new(1.0, color(207, 214, 233));
    ctx.set_global_style(style);
}

fn color(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

#[derive(Clone, Default)]
struct TerminalSnapshot {
    screen: String,
}

enum TerminalRequest {
    RunCommand(String),
    Resize { cols: u16, rows: u16 },
    Shutdown,
}

struct TerminalBackend {
    request_tx: Sender<TerminalRequest>,
    update_rx: Receiver<TerminalSnapshot>,
}

impl TerminalBackend {
    fn new(ctx: egui::Context) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<TerminalRequest>();
        let (update_tx, update_rx) = mpsc::channel::<TerminalSnapshot>();

        thread::spawn(move || terminal_worker(request_rx, update_tx, ctx));

        Self {
            request_tx,
            update_rx,
        }
    }

    fn send(&self, request: TerminalRequest) {
        let _ = self.request_tx.send(request);
    }

    fn drain_updates(&self, latest: &mut TerminalSnapshot) {
        while let Ok(snapshot) = self.update_rx.try_recv() {
            *latest = snapshot;
        }
    }
}

impl Drop for TerminalBackend {
    fn drop(&mut self) {
        let _ = self.request_tx.send(TerminalRequest::Shutdown);
    }
}

struct VelocityApp {
    query: String,
    command_input: String,
    terminal: TerminalBackend,
    terminal_snapshot: TerminalSnapshot,
    requested_terminal_size: (u16, u16),
    input_context: InputContext,
    last_context_refresh: Instant,
}

struct InputContext {
    directory: String,
    branch: String,
    added_lines: usize,
    removed_lines: usize,
}

impl VelocityApp {
    fn new(ctx: egui::Context) -> Self {
        Self {
            query: String::new(),
            command_input: String::new(),
            terminal: TerminalBackend::new(ctx),
            terminal_snapshot: TerminalSnapshot::default(),
            requested_terminal_size: (120, 32),
            input_context: read_input_context(),
            last_context_refresh: Instant::now(),
        }
    }

    fn maybe_send_command(&mut self) {
        let command = self.command_input.trim();
        if command.is_empty() {
            return;
        }

        self.terminal
            .send(TerminalRequest::RunCommand(command.to_owned()));
        self.command_input.clear();
    }
}

impl eframe::App for VelocityApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.terminal.drain_updates(&mut self.terminal_snapshot);
        if self.last_context_refresh.elapsed() >= Duration::from_secs(2) {
            self.input_context = read_input_context();
            self.last_context_refresh = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let sidebar_width = (ui.available_width() / 8.0).max(220.0);

        egui::Panel::top("top_bar")
            .exact_size(14.0)
            .frame(Frame::new().fill(color(8, 11, 19)))
            .show_inside(ui, |_| {});

        egui::Panel::left("search_sidebar")
            .exact_size(sidebar_width)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(color(24, 24, 24))
                    .inner_margin(Margin::same(8))
                    .stroke(Stroke::new(1.0, color(43, 43, 43))),
            )
            .show_inside(ui, |ui| {
                paint_sidebar_texture(ui);

                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 8.0);

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
                        draw_plain_search_icon(ui, Vec2::new(14.0, 14.0), color(148, 148, 148));
                        ui.add(
                            TextEdit::singleline(&mut self.query)
                                .hint_text("Search tabs...")
                                .desired_width(ui.available_width() - 52.0)
                                .margin(Vec2::new(0.0, 6.0))
                                .background_color(Color32::TRANSPARENT)
                                .text_color(color(212, 212, 212))
                                .frame(Frame::NONE),
                        );
                        let _ = tiny_icon_button(ui, SideIcon::Tune);
                        let _ = tiny_icon_button(ui, SideIcon::Add);
                    });

                    ui.add_space(2.0);
                    ui.allocate_space(ui.available_size());
                });
            });

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(color(16, 16, 18))
                    .inner_margin(Margin::ZERO),
            )
            .show_inside(ui, |ui| {
                Frame::new()
                    .fill(color(22, 23, 27))
                    .corner_radius(CornerRadius::ZERO)
                    .inner_margin(Margin::same(12))
                    .stroke(Stroke::NONE)
                    .show(ui, |ui| {
                        Frame::new()
                            .fill(color(12, 13, 16))
                            .corner_radius(CornerRadius::ZERO)
                            .inner_margin(Margin::same(12))
                            .stroke(Stroke::NONE)
                            .show(ui, |ui| {
                                let available = ui.available_size_before_wrap();
                                let cols = ((available.x / 8.6).floor() as u16).max(40);
                                let rows = (((available.y - 80.0) / 18.0).floor() as u16).max(12);
                                if (cols, rows) != self.requested_terminal_size {
                                    self.requested_terminal_size = (cols, rows);
                                    self.terminal.send(TerminalRequest::Resize { cols, rows });
                                }

                                let terminal_height = (available.y - 80.0).max(260.0);
                                let mut screen = self.terminal_snapshot.screen.clone();
                                ui.add_sized(
                                    [available.x, terminal_height],
                                    TextEdit::multiline(&mut screen)
                                        .font(egui::TextStyle::Monospace)
                                        .interactive(false)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(rows as usize)
                                        .margin(Vec2::new(10.0, 12.0))
                                        .background_color(color(12, 13, 16))
                                        .text_color(color(215, 217, 222))
                                        .frame(Frame::NONE),
                                );

                                ui.add_space(18.0);

                                ui.horizontal(|ui| {
                                    Frame::new()
                                        .fill(color(29, 31, 37))
                                        .corner_radius(CornerRadius::same(255))
                                        .inner_margin(Margin::symmetric(16, 10))
                                        .stroke(Stroke::new(1.0, color(48, 50, 56)))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing =
                                                        Vec2::new(8.0, 0.0);
                                                    meta_chip(
                                                        ui,
                                                        self.input_context.directory.as_str(),
                                                    );
                                                    meta_chip(
                                                        ui,
                                                        self.input_context.branch.as_str(),
                                                    );
                                                    meta_chip(
                                                        ui,
                                                        &format!(
                                                            "+{}  -{}",
                                                            self.input_context.added_lines,
                                                            self.input_context.removed_lines
                                                        ),
                                                    );
                                                });

                                                ui.horizontal(|ui| {
                                                    let input_response = ui.add_sized(
                                                        [ui.available_width(), 28.0],
                                                        TextEdit::singleline(
                                                            &mut self.command_input,
                                                        )
                                                        .background_color(Color32::TRANSPARENT)
                                                        .text_color(color(245, 246, 248))
                                                        .margin(Vec2::new(0.0, 2.0))
                                                        .frame(Frame::NONE),
                                                    );
                                                    let pressed_enter = input_response.lost_focus()
                                                        && ui.input(|input| {
                                                            input.key_pressed(egui::Key::Enter)
                                                        });
                                                    if pressed_enter {
                                                        self.maybe_send_command();
                                                    }
                                                });
                                            });
                                        });
                                });
                            });
                    });
            });
    }
}

fn terminal_worker(
    request_rx: Receiver<TerminalRequest>,
    update_tx: Sender<TerminalSnapshot>,
    ctx: egui::Context,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = error;
            let _ = update_tx.send(TerminalSnapshot {
                ..TerminalSnapshot::default()
            });
            ctx.request_repaint();
            return;
        }
    };

    runtime.block_on(async move {
        let shell = default_shell_command();

        let config = ShadowConfig {
            width: 120,
            height: 32,
            command: shell,
            scrollback_size: 3000,
            scrollback_step: 5,
        };

        let mut terminal = match SteppableTerminal::start(config).await {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = update_tx.send(TerminalSnapshot {
                    screen: format!("Unable to start the WezTerm-backed terminal.\n\n{error}"),
                });
                ctx.request_repaint();
                return;
            }
        };

        let mut latest_screen = String::new();
        loop {
            let mut should_exit = false;
            while let Ok(message) = request_rx.try_recv() {
                match message {
                    TerminalRequest::RunCommand(command) => {
                        let _ = send_terminal_command(&terminal, &command);
                    }
                    TerminalRequest::Resize { cols, rows } => {
                        let _ = terminal.shadow_terminal.resize(cols, rows);
                    }
                    TerminalRequest::Shutdown => {
                        should_exit = true;
                    }
                }
            }

            if should_exit {
                let _ = terminal.kill();
                break;
            }

            let _ = terminal.render_all_output().await;

            if let Ok(screen) = terminal.screen_as_string() {
                let normalized_screen = normalize_terminal_text(&screen);
                if normalized_screen != latest_screen {
                    latest_screen = normalized_screen;
                    let _ = update_tx.send(TerminalSnapshot {
                        screen: latest_screen.clone(),
                    });
                    ctx.request_repaint();
                }
            }

            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    });
}

fn default_shell_command() -> Vec<OsString> {
    if cfg!(target_os = "windows") {
        vec![
            OsString::from("powershell.exe"),
            OsString::from("-NoLogo"),
            OsString::from("-NoProfile"),
        ]
    } else if let Ok(shell) = std::env::var("SHELL") {
        vec![OsString::from(shell)]
    } else {
        vec![OsString::from("/bin/bash")]
    }
}

fn send_terminal_command(
    terminal: &SteppableTerminal,
    command: &str,
) -> Result<(), shadow_terminal::errors::PTYError> {
    terminal.paste_string(command)?;
    let enter = if cfg!(target_os = "windows") {
        "\r"
    } else {
        "\n"
    };
    terminal.send_input(TerminalInput::Characters(enter.to_owned()))
}

fn normalize_terminal_text(text: &str) -> String {
    let mut normalized = String::new();
    for line in text.lines() {
        normalized.push_str(line.trim_end());
        normalized.push('\n');
    }
    normalized
}

fn meta_chip(ui: &mut egui::Ui, text: &str) {
    Frame::new()
        .fill(color(35, 37, 43))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(10, 5))
        .stroke(Stroke::new(1.0, color(52, 55, 62)))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(color(164, 168, 177)));
        });
}

fn read_input_context() -> InputContext {
    let directory = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string().replace('\\', "/"))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "workspace".to_owned());

    let branch = command_stdout(&["git", "branch", "--show-current"])
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "no-branch".to_owned());

    let diff_text = command_stdout(&["git", "diff", "--shortstat"]).unwrap_or_default();
    let (added_lines, removed_lines) = parse_diff_shortstat(&diff_text);

    InputContext {
        directory,
        branch,
        added_lines,
        removed_lines,
    }
}

fn command_stdout(command: &[&str]) -> Option<String> {
    let (program, args) = command.split_first()?;
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_owned())
}

fn parse_diff_shortstat(diff_text: &str) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;

    for chunk in diff_text.split(',') {
        let trimmed = chunk.trim();
        let number = trimmed
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);

        if trimmed.contains("insertion") {
            added = number;
        } else if trimmed.contains("deletion") {
            removed = number;
        }
    }

    (added, removed)
}

fn draw_plain_search_icon(ui: &mut egui::Ui, size: Vec2, fg: Color32) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();

    let center = rect.center() + Vec2::new(-1.0, -1.0);
    painter.circle_stroke(center, 4.0, Stroke::new(1.35, fg));
    painter.line_segment(
        [center + Vec2::new(3.0, 3.0), center + Vec2::new(6.0, 6.0)],
        Stroke::new(1.35, fg),
    );
}

enum SideIcon {
    Tune,
    Add,
}

fn tiny_icon_button(ui: &mut egui::Ui, icon: SideIcon) -> egui::Response {
    let size = Vec2::new(16.0, 16.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter();
    let fg = color(162, 162, 162);

    match icon {
        SideIcon::Add => {
            painter.line_segment(
                [
                    rect.center_top() + Vec2::new(0.0, 2.0),
                    rect.center_bottom() - Vec2::new(0.0, 2.0),
                ],
                Stroke::new(1.4, fg),
            );
            painter.line_segment(
                [
                    rect.left_center() + Vec2::new(2.0, 0.0),
                    rect.right_center() - Vec2::new(2.0, 0.0),
                ],
                Stroke::new(1.4, fg),
            );
        }
        SideIcon::Tune => {
            let y1 = rect.top() + 4.0;
            let y2 = rect.bottom() - 4.0;
            painter.line_segment(
                [
                    egui::pos2(rect.left() + 1.0, y1),
                    egui::pos2(rect.right() - 1.0, y1),
                ],
                Stroke::new(1.2, fg),
            );
            painter.line_segment(
                [
                    egui::pos2(rect.left() + 1.0, y2),
                    egui::pos2(rect.right() - 1.0, y2),
                ],
                Stroke::new(1.2, fg),
            );
            painter.circle_filled(egui::pos2(rect.left() + 5.0, y1), 1.8, fg);
            painter.circle_filled(egui::pos2(rect.right() - 5.0, y2), 1.8, fg);
        }
    }

    response
}

fn paint_sidebar_texture(ui: &egui::Ui) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    for idx in 0..36 {
        let x_seed = (idx * 37 % 100) as f32 / 100.0;
        let y_seed = (idx * 19 % 100) as f32 / 100.0;
        let x = rect.left() + rect.width() * x_seed;
        let y = rect.top() + rect.height() * y_seed;
        let radius = if idx % 3 == 0 { 3.0 } else { 2.0 };
        let alpha = if idx % 2 == 0 { 18 } else { 10 };
        painter.circle_filled(
            egui::pos2(x, y),
            radius,
            Color32::from_rgba_unmultiplied(255, 255, 255, alpha),
        );
    }
}
