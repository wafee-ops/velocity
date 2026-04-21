use std::{
    ffi::OsString,
    fs,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame,
    Margin, RichText, ScrollArea, Stroke, StrokeKind, TextEdit, Vec2, ViewportBuilder,
};
use shadow_terminal::{
    shadow_terminal::Config as ShadowConfig,
    steppable_terminal::{Input as TerminalInput, SteppableTerminal},
    termwiz::color::{ColorAttribute, SrgbaTuple},
    wezterm_term,
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
    if let Some(jetbrains) = load_first_system_font(&[
        "C:\\Users\\wafee\\AppData\\Local\\Microsoft\\Windows\\Fonts\\JetBrainsMonoNerdFontMono-Regular.ttf",
        "C:\\Users\\wafee\\AppData\\Local\\Microsoft\\Windows\\Fonts\\JetBrainsMonoNerdFont-Regular.ttf",
        "C:\\Windows\\Fonts\\JetBrainsMono-Regular.ttf",
        "C:\\Windows\\Fonts\\consola.ttf",
    ]) {
        fonts.font_data.insert("jetbrains-mono".to_owned(), jetbrains.into());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "jetbrains-mono".to_owned());
    }
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

fn load_system_font(path: &str) -> Option<FontData> {
    fs::read(path).ok().map(FontData::from_owned)
}

fn load_first_system_font(paths: &[&str]) -> Option<FontData> {
    paths.iter().find_map(|path| load_system_font(path))
}

#[derive(Clone, Default)]
struct TerminalSnapshot {
    lines: Vec<Vec<TerminalCell>>,
    cursor: Option<TerminalCursor>,
    cli_active: bool,
}

#[derive(Clone)]
struct SearchTab {
    title: String,
    directory: String,
    branch: String,
    accent: Color32,
    icon: TabIcon,
}

#[derive(Clone, Copy)]
struct TabIcon {
    kind: TabIconKind,
}

#[derive(Clone, Copy)]
enum TabIconKind {
    DefaultTerminal,
    Badge {
        label: &'static str,
        foreground: Color32,
        background: Color32,
    },
}

#[derive(Clone)]
struct TerminalCell {
    text: String,
    foreground: Color32,
    background: Color32,
    width: usize,
}

#[derive(Clone)]
struct TerminalCursor {
    x: usize,
    y: usize,
    color: Color32,
}

const SEARCH_INPUT_ID: &str = "search_sidebar_input";

enum TerminalRequest {
    RunCommand(String),
    Resize { cols: u16, rows: u16 },
    SendInput(TerminalInbound),
    Shutdown,
}

enum TerminalInbound {
    Characters(String),
    Event(String),
    Paste(String),
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
    command_input_id: egui::Id,
    tabs: Vec<SearchTab>,
    selected_tab: usize,
    tab_menu_open: Option<usize>,
    rename_buffer: String,
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
            command_input_id: egui::Id::new("command_input"),
            tabs: load_tabs_from_workspace(),
            selected_tab: 0,
            tab_menu_open: None,
            rename_buffer: String::new(),
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

    fn forward_terminal_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|input| input.events.clone());
        for event in events {
            match event {
                egui::Event::Text(text) => {
                    if !text.is_empty() {
                        self.terminal.send(TerminalRequest::SendInput(
                            TerminalInbound::Characters(text),
                        ));
                    }
                }
                egui::Event::Paste(text) => {
                    if !text.is_empty() {
                        self.terminal
                            .send(TerminalRequest::SendInput(TerminalInbound::Paste(text)));
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(input) = map_key_event(key, modifiers) {
                        self.terminal
                            .send(TerminalRequest::SendInput(input));
                    }
                }
                _ => {}
            }
        }
    }

    fn add_sidebar_tab(&mut self) {
        let raw_value = self.query.trim();
        let new_tab = if raw_value.is_empty() {
            default_new_tab(&self.tabs)
        } else {
            let directory = raw_value.replace('\\', "/");
            let title = unique_tab_title(&self.tabs, &tab_title_from_value(raw_value, &directory));
            SearchTab {
                title,
                branch: read_branch_for_directory(&directory),
                accent: default_tab_accent(),
                directory,
                icon: tab_icon_for(raw_value),
            }
        };
        self.tabs.insert(0, new_tab);
        self.selected_tab = 0;
        self.tab_menu_open = None;
        self.query.clear();
    }

    fn close_tab(&mut self, index: usize) {
        if self.tabs.len() == 1 {
            self.tabs[0] = default_new_tab(&[]);
            self.selected_tab = 0;
            self.tab_menu_open = None;
            return;
        }

        self.tabs.remove(index);
        if self.selected_tab >= self.tabs.len() {
            self.selected_tab = self.tabs.len().saturating_sub(1);
        } else if index < self.selected_tab {
            self.selected_tab = self.selected_tab.saturating_sub(1);
        } else if index == self.selected_tab {
            self.selected_tab = self.selected_tab.min(self.tabs.len().saturating_sub(1));
        }

        self.tab_menu_open = match self.tab_menu_open {
            Some(open_index) if open_index == index => None,
            Some(open_index) if open_index > index => Some(open_index - 1),
            other => other,
        };
    }

    fn close_other_tabs(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get(index).cloned() {
            self.tabs = vec![tab];
            self.selected_tab = 0;
            self.tab_menu_open = Some(0);
        }
    }
}

struct TabCardOutput {
    response: egui::Response,
    close_clicked: bool,
    menu_clicked: bool,
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
        let terminal_padding = Vec2::new(14.0, 10.0);
        let sidebar_width = (ui.available_width() / 8.0).max(220.0);
        let terminal_font = FontId::new(14.0, FontFamily::Monospace);
        let sample_text = "WWWWWWWWWWWWWWWW";
        let char_size = ui
            .painter()
            .layout_no_wrap(sample_text.to_owned(), terminal_font.clone(), Color32::WHITE)
            .size();
        let cell_width = (char_size.x / sample_text.len() as f32).max(7.0);
        let cell_height = (char_size.y + 3.0).max(18.0);

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
                                .id(egui::Id::new(SEARCH_INPUT_ID))
                                .hint_text("Search tabs...")
                                .desired_width(ui.available_width() - 52.0)
                                .margin(Vec2::new(0.0, 6.0))
                                .background_color(Color32::TRANSPARENT)
                                .text_color(color(212, 212, 212))
                                .frame(Frame::NONE),
                        );
                        let _ = tiny_icon_button(ui, SideIcon::Tune);
                        let add_response = tiny_icon_button(ui, SideIcon::Add);
                        if add_response.clicked() {
                            self.add_sidebar_tab();
                        }
                    });

                    if ui.input(|input| input.key_pressed(egui::Key::Enter))
                        && ui.memory(|memory| memory.has_focus(egui::Id::new(SEARCH_INPUT_ID)))
                    {
                        self.add_sidebar_tab();
                    }

                    ui.add_space(2.0);
                    let query = self.query.trim().to_lowercase();
                    let matching_indices: Vec<usize> = self
                        .tabs
                        .iter()
                        .enumerate()
                        .filter(|(_, tab)| {
                            query.is_empty()
                                || tab.title.to_lowercase().contains(&query)
                                || tab.directory.to_lowercase().contains(&query)
                        })
                        .map(|(index, _)| index)
                        .collect();

                    ui.label(
                        RichText::new(if matching_indices.is_empty() {
                            "No matching tabs"
                        } else {
                            "Tabs"
                        })
                        .size(11.0)
                        .color(color(132, 136, 145)),
                    );

                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(0.0, 6.0);

                            if matching_indices.is_empty() {
                                sidebar_empty_state(ui, self.query.trim());
                                return;
                            }

                            for index in matching_indices {
                                let response = tab_card(
                                    ui,
                                    &self.tabs[index],
                                    index == self.selected_tab,
                                );
                                if response.clicked() {
                                    self.selected_tab = index;
                                }
                            }
                        });
                });
            });

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(color(16, 16, 18))
                    .inner_margin(Margin::ZERO),
            )
            .show_inside(ui, |ui| {
                let available = ui.available_size_before_wrap();
                let command_bar_visible = !self.terminal_snapshot.cli_active;
                let bottom_bar_height = if command_bar_visible { 108.0 } else { 0.0 };
                let usable_terminal_width = (available.x - terminal_padding.x * 2.0).max(cell_width * 40.0);
                let usable_terminal_height =
                    (available.y - bottom_bar_height - terminal_padding.y * 2.0).max(cell_height * 12.0);
                let cols = ((usable_terminal_width / cell_width).floor() as u16).max(40);
                let rows = ((usable_terminal_height / cell_height).floor() as u16).max(12);
                if (cols, rows) != self.requested_terminal_size {
                    self.requested_terminal_size = (cols, rows);
                    self.terminal.send(TerminalRequest::Resize { cols, rows });
                }

                let terminal_height = (available.y - bottom_bar_height).max(260.0);
                let terminal_size = Vec2::new(available.x, terminal_height);
                let (terminal_rect, terminal_response) =
                    ui.allocate_exact_size(terminal_size, egui::Sense::click());
                if terminal_response.clicked() {
                    terminal_response.request_focus();
                }
                if terminal_response.has_focus() {
                    self.forward_terminal_input(ui.ctx());
                }
                paint_terminal(
                    ui,
                    terminal_rect,
                    &self.terminal_snapshot,
                    terminal_response.has_focus(),
                    &terminal_font,
                    terminal_padding,
                    cell_width,
                    cell_height,
                );

                if command_bar_visible {
                    ui.add_space(12.0);

                    let bar_rect = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 96.0), egui::Sense::hover())
                        .0;
                    paint_command_bar(ui, bar_rect);
                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(bar_rect.shrink2(Vec2::new(10.0, 8.0))),
                        |ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(8.0, 5.0);
                            ui.horizontal(|ui| {
                                meta_chip(ui, self.input_context.directory.as_str());
                                meta_chip(ui, self.input_context.branch.as_str());
                                meta_chip(
                                    ui,
                                    &format!(
                                        "+{}  -{}",
                                        self.input_context.added_lines, self.input_context.removed_lines
                                    ),
                                );
                            });

                        let input_response = ui.add_sized(
                            [ui.available_width(), 50.0],
                            TextEdit::singleline(&mut self.command_input)
                                .id(self.command_input_id)
                                .font(eframe::egui::TextStyle::Monospace)
                                .background_color(Color32::TRANSPARENT)
                                .text_color(color(245, 246, 248))
                                .margin(Vec2::new(0.0, 14.0))
                                .frame(Frame::NONE),
                        );
                            if input_response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter))
                            {
                                self.maybe_send_command();
                                input_response.request_focus();
                            }
                        },
                    );
                }
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
                let _ = update_tx.send(snapshot_from_text(format!(
                    "Unable to start the WezTerm-backed terminal.\n\n{error}"
                )));
                ctx.request_repaint();
                return;
            }
        };

        let initial_snapshot = snapshot_terminal(&mut terminal);
        let mut latest_screen = snapshot_fingerprint(&initial_snapshot);
        let _ = update_tx.send(initial_snapshot);
        ctx.request_repaint();

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
                    TerminalRequest::SendInput(input) => match input {
                        TerminalInbound::Characters(text) => {
                            let _ = terminal.send_input(TerminalInput::Characters(text));
                        }
                        TerminalInbound::Event(text) => {
                            let _ = terminal.send_input(TerminalInput::Event(text));
                        }
                        TerminalInbound::Paste(text) => {
                            let _ = terminal.paste_string(&text);
                        }
                    },
                    TerminalRequest::Shutdown => {
                        should_exit = true;
                    }
                }
            }

            if should_exit {
                let _ = terminal.kill();
                break;
            }

            if let Err(error) = terminal.render_all_output().await {
                let _ = update_tx.send(snapshot_from_text(format!(
                    "Terminal render failed.\n\n{error}"
                )));
                ctx.request_repaint();
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }

            let snapshot = snapshot_terminal(&mut terminal);
            let fingerprint = snapshot_fingerprint(&snapshot);
            if fingerprint != latest_screen {
                latest_screen = fingerprint;
                let _ = update_tx.send(snapshot);
                ctx.request_repaint();
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

fn snapshot_terminal(terminal: &mut SteppableTerminal) -> TerminalSnapshot {
    let size = terminal.shadow_terminal.terminal.get_size();
    let mut screen = terminal.shadow_terminal.terminal.screen().clone();
    let cursor = terminal.shadow_terminal.terminal.cursor_pos();
    let mut lines = Vec::with_capacity(size.rows);

    for y in 0..size.rows {
        let mut row = Vec::with_capacity(size.cols);
        for x in 0..size.cols {
            let cell = screen
                .get_cell(x, y as i64)
                .cloned()
                .unwrap_or_else(wezterm_term::Cell::blank);
            let attrs = cell.attrs().clone();
            row.push(TerminalCell {
                text: cell.str().to_owned(),
                foreground: color_attribute_to_egui(attrs.foreground(), color(215, 217, 222)),
                background: color_attribute_to_egui(attrs.background(), color(12, 13, 16)),
                width: cell.width().max(1),
            });
        }
        lines.push(row);
    }

    TerminalSnapshot {
        lines,
        cursor: Some(TerminalCursor {
            x: cursor.x,
            y: cursor.y.max(0) as usize,
            color: color(238, 240, 244),
        }),
        cli_active: terminal.shadow_terminal.terminal.is_alt_screen_active(),
    }
}

fn snapshot_from_text(text: String) -> TerminalSnapshot {
    TerminalSnapshot {
        lines: text
            .lines()
            .map(|line| {
                vec![TerminalCell {
                    text: line.to_owned(),
                    foreground: color(215, 217, 222),
                    background: color(12, 13, 16),
                    width: line.chars().count().max(1),
                }]
            })
            .collect(),
        cursor: None,
        cli_active: false,
    }
}

fn snapshot_fingerprint(snapshot: &TerminalSnapshot) -> String {
    let mut fingerprint = String::new();
    for row in &snapshot.lines {
        for cell in row {
            fingerprint.push_str(&cell.text);
            fingerprint.push('|');
            fingerprint.push_str(&format!(
                "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                cell.foreground.r(),
                cell.foreground.g(),
                cell.foreground.b(),
                cell.background.r(),
                cell.background.g(),
                cell.background.b()
            ));
        }
        fingerprint.push('\n');
    }
    if let Some(cursor) = &snapshot.cursor {
        fingerprint.push_str(&format!("{}:{};", cursor.x, cursor.y));
    }
    fingerprint
}

fn color_attribute_to_egui(attribute: ColorAttribute, default: Color32) -> Color32 {
    match attribute {
        ColorAttribute::Default => default,
        ColorAttribute::PaletteIndex(index) => ansi_palette_color(index),
        ColorAttribute::TrueColorWithPaletteFallback(SrgbaTuple(r, g, b, _), _)
        | ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(r, g, b, _)) => {
            Color32::from_rgb(
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
            )
        }
    }
}

fn ansi_palette_color(index: u8) -> Color32 {
    match index {
        0 => color(12, 13, 16),
        1 => color(205, 49, 49),
        2 => color(13, 188, 121),
        3 => color(229, 229, 16),
        4 => color(36, 114, 200),
        5 => color(188, 63, 188),
        6 => color(17, 168, 205),
        7 => color(229, 229, 229),
        8 => color(102, 102, 102),
        9 => color(241, 76, 76),
        10 => color(35, 209, 139),
        11 => color(245, 245, 67),
        12 => color(59, 142, 234),
        13 => color(214, 112, 214),
        14 => color(41, 184, 219),
        15 => color(255, 255, 255),
        16..=231 => {
            let value = index - 16;
            let r = value / 36;
            let g = (value % 36) / 6;
            let b = value % 6;
            let component = |level: u8| if level == 0 { 0 } else { 55 + level * 40 };
            color(component(r), component(g), component(b))
        }
        232..=255 => {
            let shade = 8 + (index - 232) * 10;
            color(shade, shade, shade)
        }
    }
}

fn map_key_event(key: egui::Key, modifiers: egui::Modifiers) -> Option<TerminalInbound> {
    if modifiers.ctrl {
        let ctrl_input = match key {
            egui::Key::A => "\u{1}",
            egui::Key::B => "\u{2}",
            egui::Key::C => "\u{3}",
            egui::Key::D => "\u{4}",
            egui::Key::E => "\u{5}",
            egui::Key::F => "\u{6}",
            egui::Key::G => "\u{7}",
            egui::Key::H => "\u{8}",
            egui::Key::I => "\t",
            egui::Key::J => "\n",
            egui::Key::K => "\u{b}",
            egui::Key::L => "\u{c}",
            egui::Key::M => "\r",
            egui::Key::N => "\u{e}",
            egui::Key::O => "\u{f}",
            egui::Key::P => "\u{10}",
            egui::Key::Q => "\u{11}",
            egui::Key::R => "\u{12}",
            egui::Key::S => "\u{13}",
            egui::Key::T => "\u{14}",
            egui::Key::U => "\u{15}",
            egui::Key::V => "\u{16}",
            egui::Key::W => "\u{17}",
            egui::Key::X => "\u{18}",
            egui::Key::Y => "\u{19}",
            egui::Key::Z => "\u{1a}",
            _ => "",
        };
        if !ctrl_input.is_empty() {
            return Some(TerminalInbound::Characters(ctrl_input.to_owned()));
        }
    }

    let event = match key {
        egui::Key::Enter => TerminalInbound::Characters(
            if cfg!(target_os = "windows") { "\r" } else { "\n" }.to_owned(),
        ),
        egui::Key::Tab => TerminalInbound::Characters("\t".to_owned()),
        egui::Key::Backspace => TerminalInbound::Characters("\u{8}".to_owned()),
        egui::Key::Delete => TerminalInbound::Event("\u{1b}[3~".to_owned()),
        egui::Key::Escape => TerminalInbound::Characters("\u{1b}".to_owned()),
        egui::Key::ArrowUp => TerminalInbound::Event("\u{1b}[A".to_owned()),
        egui::Key::ArrowDown => TerminalInbound::Event("\u{1b}[B".to_owned()),
        egui::Key::ArrowRight => TerminalInbound::Event("\u{1b}[C".to_owned()),
        egui::Key::ArrowLeft => TerminalInbound::Event("\u{1b}[D".to_owned()),
        egui::Key::Home => TerminalInbound::Event("\u{1b}[H".to_owned()),
        egui::Key::End => TerminalInbound::Event("\u{1b}[F".to_owned()),
        egui::Key::PageUp => TerminalInbound::Event("\u{1b}[5~".to_owned()),
        egui::Key::PageDown => TerminalInbound::Event("\u{1b}[6~".to_owned()),
        _ => return None,
    };

    Some(event)
}

fn paint_terminal(
    ui: &egui::Ui,
    rect: egui::Rect,
    snapshot: &TerminalSnapshot,
    focused: bool,
    font_id: &FontId,
    padding: Vec2,
    cell_width: f32,
    cell_height: f32,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, color(12, 13, 16));
    let content_rect = rect.shrink2(padding);

    for (row_index, row) in snapshot.lines.iter().enumerate() {
        let y = content_rect.top() + row_index as f32 * cell_height;
        if y >= content_rect.bottom() {
            break;
        }

        let mut x = content_rect.left();
        for cell in row {
            let width = cell.width.max(1) as f32 * cell_width;
            let cell_rect =
                egui::Rect::from_min_size(egui::pos2(x, y), Vec2::new(width, cell_height));
            if cell.background != color(12, 13, 16) {
                painter.rect_filled(cell_rect, CornerRadius::ZERO, cell.background);
            }

            if !cell.text.is_empty() && cell.text != " " {
                let galley = painter.layout_no_wrap(
                    cell.text.clone(),
                    font_id.clone(),
                    cell.foreground,
                );
                painter.galley(egui::pos2(x, y + 1.0), galley, cell.foreground);
            }

            x += width;
            if x >= content_rect.right() {
                break;
            }
        }
    }

    if let Some(cursor) = &snapshot.cursor {
        let cursor_rect = egui::Rect::from_min_size(
            egui::pos2(
                content_rect.left() + cursor.x as f32 * cell_width,
                content_rect.top() + cursor.y as f32 * cell_height,
            ),
            Vec2::new(2.0, cell_height),
        );
        painter.rect_filled(cursor_rect, CornerRadius::ZERO, cursor.color);
    }

    let border_color = if focused {
        color(90, 122, 184)
    } else {
        color(33, 36, 43)
    };
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, border_color),
        StrokeKind::Outside,
    );
}

fn paint_command_bar(ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, color(17, 19, 23));
    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(1.0, color(58, 62, 72)),
    );
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, color(58, 62, 72)),
    );
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, color(58, 62, 72)),
    );
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

fn load_tabs_from_workspace() -> Vec<SearchTab> {
    let current = current_directory_label();
    vec![SearchTab {
        title: tab_title_from_value("", &current),
        branch: read_branch_for_directory(&current),
        directory: current,
        icon: default_tab_icon(),
    }]
}

fn default_new_tab(existing_tabs: &[SearchTab]) -> SearchTab {
    let directory = current_directory_label();
    SearchTab {
        title: unique_tab_title(existing_tabs, "New tab"),
        branch: read_branch_for_directory(&directory),
        directory,
        icon: default_tab_icon(),
    }
}

fn current_directory_label() -> String {
    std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string().replace('\\', "/"))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "workspace".to_owned())
}

fn tab_title_from_value(raw_value: &str, directory: &str) -> String {
    if !raw_value.is_empty() {
        let normalized = raw_value.replace('\\', "/");
        if let Some(segment) = normalized.rsplit('/').find(|segment| !segment.is_empty()) {
            return segment.to_owned();
        }
    }

    directory
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or("workspace")
        .to_owned()
}

fn unique_tab_title(existing_tabs: &[SearchTab], base_title: &str) -> String {
    if existing_tabs.iter().all(|tab| tab.title != base_title) {
        return base_title.to_owned();
    }

    for index in 2.. {
        let candidate = format!("{base_title} {index}");
        if existing_tabs.iter().all(|tab| tab.title != candidate) {
            return candidate;
        }
    }

    base_title.to_owned()
}

fn read_branch_for_directory(directory: &str) -> String {
    command_stdout(&["git", "-C", directory, "branch", "--show-current"])
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "no-branch".to_owned())
}

fn default_tab_icon() -> TabIcon {
    TabIcon {
        kind: TabIconKind::DefaultTerminal,
    }
}

fn tab_icon_for(value: &str) -> TabIcon {
    let normalized = value.trim().to_lowercase();

    if normalized.contains("claude") {
        TabIcon {
            kind: TabIconKind::Badge {
                label: "Cl",
                foreground: color(41, 24, 10),
                background: color(235, 161, 96),
            },
        }
    } else if normalized.contains("codex") || normalized.contains("openai") {
        TabIcon {
            kind: TabIconKind::Badge {
                label: "Co",
                foreground: color(231, 248, 244),
                background: color(28, 126, 98),
            },
        }
    } else if normalized.contains("gemini") || normalized.contains("google") {
        TabIcon {
            kind: TabIconKind::Badge {
                label: "Ge",
                foreground: color(240, 245, 255),
                background: color(92, 108, 234),
            },
        }
    } else if normalized.contains("cursor") {
        TabIcon {
            kind: TabIconKind::Badge {
                label: "Cu",
                foreground: color(243, 244, 246),
                background: color(73, 79, 92),
            },
        }
    } else if normalized.contains("github") {
        TabIcon {
            kind: TabIconKind::Badge {
                label: "Gh",
                foreground: color(246, 247, 249),
                background: color(51, 55, 64),
            },
        }
    } else if normalized.contains("terminal")
        || normalized.contains("shell")
        || normalized.contains("powershell")
        || normalized.contains("bash")
    {
        TabIcon {
            kind: TabIconKind::Badge {
                label: "Sh",
                foreground: color(234, 248, 237),
                background: color(66, 138, 84),
            },
        }
    } else {
        default_tab_icon()
    }
}

fn tab_card(ui: &mut egui::Ui, tab: &SearchTab, selected: bool) -> egui::Response {
    let desired = Vec2::new(ui.available_width(), 46.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let fill = if selected {
        color(57, 60, 68)
    } else {
        color(31, 32, 35)
    };
    let stroke = Stroke::new(1.0, if selected { color(76, 80, 90) } else { color(34, 35, 39) });

    ui.painter().rect(
        rect,
        CornerRadius::same(8),
        fill,
        stroke,
        StrokeKind::Outside,
    );
    let icon_rect = egui::Rect::from_min_size(
        rect.min + Vec2::new(10.0, 11.0),
        Vec2::new(18.0, 18.0),
    );
    paint_tab_icon(ui.painter(), icon_rect, tab.icon);
    ui.painter().text(
        rect.min + Vec2::new(38.0, 8.0),
        Align2::LEFT_TOP,
        &tab.title,
        FontId::proportional(12.5),
        color(242, 243, 245),
    );
    ui.painter().text(
        rect.min + Vec2::new(38.0, 25.0),
        Align2::LEFT_TOP,
        format!("git {}", tab.branch),
        FontId::proportional(10.0),
        color(163, 167, 175),
    );

    response
}

fn paint_tab_icon(painter: &egui::Painter, rect: egui::Rect, icon: TabIcon) {
    match icon.kind {
        TabIconKind::DefaultTerminal => {
            painter.circle_filled(rect.center(), rect.width() * 0.5, color(21, 22, 25));
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "*",
                FontId::proportional(10.5),
                color(245, 246, 248),
            );
        }
        TabIconKind::Badge {
            label,
            foreground,
            background,
        } => {
            painter.circle_filled(rect.center(), rect.width() * 0.5, background);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(9.5),
                foreground,
            );
        }
    }
}

fn sidebar_empty_state(ui: &mut egui::Ui, query: &str) {
    Frame::new()
        .fill(color(26, 28, 33))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
        .stroke(Stroke::new(1.0, color(44, 47, 54)))
        .show(ui, |ui| {
            let text = if query.is_empty() {
                "No tabs yet. Use + to add one."
            } else {
                "No tabs match your search. Press Enter or + to add it."
            };
            ui.label(RichText::new(text).size(12.0).color(color(166, 170, 178)));
        });
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
