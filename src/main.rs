use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame,
    Margin, RichText, ScrollArea, Stroke, StrokeKind, TextBuffer, TextEdit, TextFormat, Vec2,
    ViewportBuilder,
};
use eframe::egui::text::LayoutJob;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use shadow_terminal::{
    shadow_terminal::Config as ShadowConfig,
    steppable_terminal::{Input as TerminalInput, SteppableTerminal},
    termwiz::color::{ColorAttribute, SrgbaTuple},
    wezterm_term,
};
use syntect::{
    easy::HighlightLines,
    highlighting::{Style, Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([1380.0, 860.0])
            .with_min_inner_size([1040.0, 720.0])
            .with_title("Velocity"),
        renderer: eframe::Renderer::Wgpu,
        hardware_acceleration: eframe::HardwareAcceleration::Required,
        vsync: true,
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
    if let Some(jetbrains_mono) = load_first_system_font(&[
        "C:\\Users\\wafee\\AppData\\Local\\Microsoft\\Windows\\Fonts\\JetBrainsMonoNerdFontMono-Regular.ttf",
        "C:\\Users\\wafee\\AppData\\Local\\Microsoft\\Windows\\Fonts\\JetBrainsMonoNerdFont-Regular.ttf",
        "C:\\Windows\\Fonts\\JetBrainsMono-Regular.ttf",
        "C:\\Windows\\Fonts\\consola.ttf",
    ]) {
        fonts
            .font_data
            .insert("jetbrains-nerd-mono".to_owned(), jetbrains_mono.into());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "jetbrains-nerd-mono".to_owned());
    }
    if let Some(jetbrains_proportional) = load_first_system_font(&[
        "C:\\Users\\wafee\\AppData\\Local\\Microsoft\\Windows\\Fonts\\JetBrainsMonoNerdFontPropo-Regular.ttf",
        "C:\\Users\\wafee\\AppData\\Local\\Microsoft\\Windows\\Fonts\\JetBrainsMonoNerdFont-Regular.ttf",
        "C:\\Windows\\Fonts\\JetBrainsMono-Regular.ttf",
    ]) {
        fonts.font_data.insert(
            "jetbrains-nerd-proportional".to_owned(),
            jetbrains_proportional.into(),
        );
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "jetbrains-nerd-proportional".to_owned());
    }
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push("roboto".to_owned());
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
const GROQ_API_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const GROQ_MODEL: &str = "openai/gpt-oss-120b";
const GROQ_API_KEY_ENV: &str = "GROQ_API_KEY";
const LOCAL_ENV_FILE: &str = ".env.local";
const AGENT_SYSTEM_PROMPT: &str = "You are an action-oriented coding agent inside a desktop app. \
You can use local tools when needed. \
When you want to use a tool, respond with only valid JSON. \
Do not wrap the JSON in code fences. Do not add commentary before or after it. \
Available tools: \
1. shell: runs a shell command in the current working directory and returns stdout/stderr. \
2. read_file: reads a UTF-8 text file relative to the current working directory. \
3. write_file: replaces a UTF-8 text file with the provided content. \
4. append_file: appends UTF-8 text to a file. \
5. replace_in_file: replaces one exact substring with another in a file. \
6. list_dir: lists files and folders in a directory. \
7. mkdir: creates a directory recursively. \
8. rename_path: renames or moves a file or folder. \
9. delete_path: deletes a file or folder recursively. \
10. file_info: returns metadata for a path. \
11. git_status: returns git status for the current working directory. \
12. git_diff: returns git diff, optionally for a specific file. \
After you receive a tool result, either answer normally in Markdown or emit another JSON tool call if you still need one. \
Keep answers practical, concise, and grounded in the tool results.";
const AGENT_COMMAND_SYSTEM_PROMPT: &str = "You are a shell command generator inside a desktop app. \
Return exactly one shell command for the current working directory. \
Do not include markdown, JSON, explanations, or multiple commands. \
Choose the most direct command that satisfies the user's request.";

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

struct TerminalPane {
    id: egui::Id,
    backend: TerminalBackend,
    snapshot: TerminalSnapshot,
    requested_size: (u16, u16),
    command_input: String,
    command_input_id: egui::Id,
}

struct CommandExecutor {
    request_tx: Sender<CommandExecutionRequest>,
    result_rx: Receiver<CommandExecutionResult>,
}

struct AgentExecutor {
    request_tx: Sender<AgentChatRequest>,
    result_rx: Receiver<AgentChatResult>,
}

#[derive(Clone)]
struct CommandExecutionRequest {
    id: u64,
    command: String,
    working_directory: String,
}

#[derive(Clone)]
struct CommandExecutionResult {
    id: u64,
    output_chunk: String,
    done: bool,
    success: bool,
}

enum CommandStreamMessage {
    Chunk(String),
    Closed,
}

#[derive(Clone)]
struct AgentChatRequest {
    id: u64,
    prompt: String,
    working_directory: String,
    mode: AgentRequestMode,
}

#[derive(Clone)]
struct AgentChatResult {
    id: u64,
    response: String,
    success: bool,
    mode: AgentRequestMode,
    generated_command: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AgentRequestMode {
    Chat,
    Command,
}

#[derive(Clone, Copy)]
enum MainInputSubmitMode {
    Command,
    AiCommand,
}

#[derive(Clone)]
struct CommandBlock {
    id: u64,
    command: String,
    output: String,
    status: CommandBlockStatus,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandBlockStatus {
    Running,
    Success,
    Error,
}

#[derive(Clone)]
struct AgentChatBox {
    id: u64,
    prompt: String,
    response: String,
    status: AgentChatStatus,
    created_at: Instant,
    completed_at: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AgentChatStatus {
    Running,
    Success,
    Error,
}

#[derive(Clone)]
enum TranscriptEntry {
    Command(CommandBlock),
    Agent(AgentChatBox),
}

impl TerminalBackend {
    fn new(ctx: egui::Context) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<TerminalRequest>();
        let (update_tx, update_rx) = mpsc::channel::<TerminalSnapshot>();

        thread::spawn(move || {
            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                terminal_worker(request_rx, update_tx.clone(), ctx.clone());
            }));

            if let Err(payload) = panic_result {
                let panic_message = if let Some(message) = payload.downcast_ref::<&str>() {
                    (*message).to_owned()
                } else if let Some(message) = payload.downcast_ref::<String>() {
                    message.clone()
                } else {
                    "Unknown panic".to_owned()
                };

                let _ = update_tx.send(snapshot_from_text(format!(
                    "Terminal backend crashed.\n\n{panic_message}"
                )));
                ctx.request_repaint();
            }
        });

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

impl TerminalPane {
    fn new(ctx: egui::Context, index: usize) -> Self {
        Self {
            id: egui::Id::new(("terminal_pane", index)),
            backend: TerminalBackend::new(ctx),
            snapshot: TerminalSnapshot::default(),
            requested_size: (120, 32),
            command_input: String::new(),
            command_input_id: egui::Id::new(("terminal_pane_input", index)),
        }
    }
}

impl CommandExecutor {
    fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<CommandExecutionRequest>();
        let (result_tx, result_rx) = mpsc::channel::<CommandExecutionResult>();

        thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                execute_command_request(&request, &result_tx);
            }
        });

        Self {
            request_tx,
            result_rx,
        }
    }

    fn execute(&self, request: CommandExecutionRequest) {
        let _ = self.request_tx.send(request);
    }

    fn drain_results(&self, blocks: &mut Vec<CommandBlock>) {
        while let Ok(result) = self.result_rx.try_recv() {
            if let Some(block) = blocks.iter_mut().find(|block| block.id == result.id) {
                block.output.push_str(&result.output_chunk);
                if result.done {
                    block.status = if result.success {
                        CommandBlockStatus::Success
                    } else {
                        CommandBlockStatus::Error
                    };
                }
            }
        }
    }
}

impl AgentExecutor {
    fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<AgentChatRequest>();
        let (result_tx, result_rx) = mpsc::channel::<AgentChatResult>();

        thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                let result = execute_agent_request(&request);
                let _ = result_tx.send(result);
            }
        });

        Self {
            request_tx,
            result_rx,
        }
    }

    fn execute(&self, request: AgentChatRequest) {
        let _ = self.request_tx.send(request);
    }

    fn drain_results(&self) -> Vec<AgentChatResult> {
        let mut results = Vec::new();
        while let Ok(result) = self.result_rx.try_recv() {
            results.push(result);
        }
        results
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
    refocus_command_input: bool,
    command_history: Vec<String>,
    available_commands: Vec<String>,
    shell_directory: String,
    tabs: Vec<SearchTab>,
    selected_tab: usize,
    terminals: Vec<TerminalPane>,
    active_terminal: usize,
    command_executor: CommandExecutor,
    agent_executor: AgentExecutor,
    transcript_entries: Vec<TranscriptEntry>,
    next_command_id: u64,
    input_context: InputContext,
    last_context_refresh: Instant,
    diff_navigation_open: bool,
    selected_diff_file: Option<String>,
    refocus_terminal_input: Option<usize>,
}

struct InputContext {
    branch: String,
    added_lines: usize,
    removed_lines: usize,
    diff_files: Vec<DiffFileEntry>,
}

#[derive(Clone)]
struct DiffFileEntry {
    path: String,
}

impl VelocityApp {
    fn new(ctx: egui::Context) -> Self {
        let shell_directory = current_directory_label();
        Self {
            query: String::new(),
            command_input: String::new(),
            command_input_id: egui::Id::new("command_input"),
            refocus_command_input: false,
            command_history: default_command_history(),
            available_commands: discover_available_commands(),
            shell_directory: shell_directory.clone(),
            tabs: load_tabs_from_workspace(),
            selected_tab: 0,
            terminals: vec![TerminalPane::new(ctx.clone(), 0)],
            active_terminal: 0,
            command_executor: CommandExecutor::new(),
            agent_executor: AgentExecutor::new(),
            transcript_entries: Vec::new(),
            next_command_id: 1,
            input_context: read_input_context_for_directory(&shell_directory),
            last_context_refresh: Instant::now(),
            diff_navigation_open: false,
            selected_diff_file: None,
            refocus_terminal_input: None,
        }
    }

    fn maybe_send_command(&mut self) {
        let command = self.command_input.trim().to_owned();
        if command.is_empty() {
            return;
        }

        self.record_command_history(&command);
        if self.try_handle_directory_change(&command) {
            self.command_input.clear();
            return;
        }

        if self.try_handle_agent_command(&command) {
            self.command_input.clear();
            return;
        }

        if looks_interactive_command(&command) {
            self.launch_interactive_command(&command);
            self.command_input.clear();
            return;
        }

        let command_id = self.next_command_id;
        self.next_command_id += 1;
        self.transcript_entries.push(TranscriptEntry::Command(CommandBlock {
            id: command_id,
            command: command.clone(),
            output: String::new(),
            status: CommandBlockStatus::Running,
        }));
        self.command_executor.execute(CommandExecutionRequest {
            id: command_id,
            command,
            working_directory: self.shell_directory.clone(),
        });
        self.command_input.clear();
    }

    fn maybe_send_ai_command(&mut self) {
        let prompt = self.command_input.trim().to_owned();
        if prompt.is_empty() {
            return;
        }

        self.record_command_history(&prompt);
        let request_id = self.next_command_id;
        self.next_command_id += 1;
        self.agent_executor.execute(AgentChatRequest {
            id: request_id,
            prompt,
            working_directory: self.shell_directory.clone(),
            mode: AgentRequestMode::Command,
        });
        self.command_input.clear();
    }

    fn submit_main_input(&mut self, mode: MainInputSubmitMode) {
        match mode {
            MainInputSubmitMode::Command => self.maybe_send_command(),
            MainInputSubmitMode::AiCommand => self.maybe_send_ai_command(),
        }
        self.refocus_command_input = true;
    }

    fn forward_terminal_input(&mut self, ctx: &egui::Context) {
        if self
            .active_terminal_pane()
            .and_then(|pane| Some(pane.id))
            .is_none()
        {
            return;
        }
        let events = ctx.input(|input| input.events.clone());
        let mut sent_terminal_input = false;
        for event in events {
            match event {
                egui::Event::Text(text) => {
                    if !text.is_empty() {
                        if let Some(pane) = self.active_terminal_pane() {
                            pane.backend.send(TerminalRequest::SendInput(
                                TerminalInbound::Characters(text),
                            ));
                        }
                        sent_terminal_input = true;
                    }
                }
                egui::Event::Paste(text) => {
                    if !text.is_empty() {
                        if let Some(pane) = self.active_terminal_pane() {
                            pane.backend
                                .send(TerminalRequest::SendInput(TerminalInbound::Paste(text)));
                        }
                        sent_terminal_input = true;
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(input) = map_key_event(key, modifiers) {
                        if let Some(pane) = self.active_terminal_pane() {
                            pane.backend.send(TerminalRequest::SendInput(input));
                        }
                        sent_terminal_input = true;
                    }
                }
                _ => {}
            }
        }
        if sent_terminal_input {
            ctx.request_repaint();
        }
    }

    fn active_terminal_pane(&mut self) -> Option<&mut TerminalPane> {
        self.terminals.get_mut(self.active_terminal)
    }

    fn split_active_terminal(&mut self, ctx: &egui::Context) {
        let new_index = self.terminals.len();
        let pane = TerminalPane::new(ctx.clone(), new_index);
        pane.backend.send(TerminalRequest::RunCommand(compose_directory_change_command(
            &self.shell_directory,
        )));
        self.terminals.push(pane);
        self.active_terminal = new_index;
    }

    fn terminal_workspace_visible(&self) -> bool {
        self.terminals.len() > 1
            || self
                .terminals
                .get(self.active_terminal)
                .map(|pane| pane.snapshot.cli_active)
                .unwrap_or(false)
    }

    fn run_terminal_pane_command(&mut self, index: usize) {
        let Some(command) = self
            .terminals
            .get(index)
            .map(|pane| pane.command_input.trim().to_owned())
        else {
            return;
        };
        if command.is_empty() {
            return;
        }

        self.record_command_history(&command);
        if let Some(pane) = self.terminals.get_mut(index) {
            pane.backend.send(TerminalRequest::RunCommand(compose_terminal_command(
                &self.shell_directory,
                &command,
            )));
            pane.command_input.clear();
        }
        self.refocus_terminal_input = Some(index);
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
                directory,
                icon: tab_icon_for(raw_value),
            }
        };
        self.tabs.insert(0, new_tab);
        self.selected_tab = 0;
        self.query.clear();
    }

    fn close_tab(&mut self, index: usize) {
        if self.tabs.len() == 1 {
            self.tabs[0] = default_new_tab(&[]);
            self.selected_tab = 0;
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

    }

    fn refresh_tab_contexts(&mut self) {
        for tab in &mut self.tabs {
            tab.branch = read_branch_for_directory(&tab.directory);
        }
    }

    fn record_command_history(&mut self, command: &str) {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return;
        }

        self.command_history.retain(|existing| existing != trimmed);
        self.command_history.insert(0, trimmed.to_owned());
        self.command_history.truncate(200);
    }

    fn try_handle_directory_change(&mut self, command: &str) -> bool {
        let Some(target) = parse_cd_target(command) else {
            return false;
        };

        let Some(resolved_directory) = resolve_directory_target(&self.shell_directory, target) else {
            let command_id = self.next_command_id;
            self.next_command_id += 1;
            self.transcript_entries
                .push(TranscriptEntry::Command(CommandBlock {
                id: command_id,
                command: command.to_owned(),
                output: format!("Directory not found: {target}"),
                status: CommandBlockStatus::Error,
            }));
            return true;
        };

        self.shell_directory = resolved_directory.clone();
        self.input_context = read_input_context_for_directory(&resolved_directory);
        self.ensure_selected_diff_file();
        if let Some(tab) = self.tabs.get_mut(self.selected_tab) {
            tab.directory = resolved_directory.clone();
            tab.branch = self.input_context.branch.clone();
            tab.title = tab_title_from_value("", &resolved_directory);
        }

        let command_id = self.next_command_id;
        self.next_command_id += 1;
        self.transcript_entries
            .push(TranscriptEntry::Command(CommandBlock {
            id: command_id,
            command: command.to_owned(),
            output: format!("Changed directory to {resolved_directory}"),
            status: CommandBlockStatus::Success,
        }));
        true
    }

    fn try_handle_agent_command(&mut self, command: &str) -> bool {
        let Some(prompt) = parse_agent_prompt(command) else {
            return false;
        };

        let chat_id = self.next_command_id;
        self.next_command_id += 1;
        let prompt = prompt.to_owned();
        self.transcript_entries
            .push(TranscriptEntry::Agent(AgentChatBox {
                id: chat_id,
                prompt: prompt.clone(),
                response: String::new(),
                status: AgentChatStatus::Running,
                created_at: Instant::now(),
                completed_at: None,
            }));
        self.agent_executor.execute(AgentChatRequest {
            id: chat_id,
            prompt,
            working_directory: self.shell_directory.clone(),
            mode: AgentRequestMode::Chat,
        });
        true
    }

    fn apply_agent_result(&mut self, result: AgentChatResult) {
        match result.mode {
            AgentRequestMode::Chat => {
                for entry in &mut self.transcript_entries {
                    if let TranscriptEntry::Agent(chat) = entry {
                        if chat.id == result.id {
                            chat.response = result.response.clone();
                            chat.status = if result.success {
                                AgentChatStatus::Success
                            } else {
                                AgentChatStatus::Error
                            };
                            chat.completed_at = Some(Instant::now());
                            break;
                        }
                    }
                }
            }
            AgentRequestMode::Command => {
                if let Some(command) = result.generated_command {
                    let command_id = self.next_command_id;
                    self.next_command_id += 1;
                    self.transcript_entries
                        .push(TranscriptEntry::Command(CommandBlock {
                            id: command_id,
                            command: command.clone(),
                            output: String::new(),
                            status: CommandBlockStatus::Running,
                        }));
                    self.command_executor.execute(CommandExecutionRequest {
                        id: command_id,
                        command,
                        working_directory: self.shell_directory.clone(),
                    });
                } else {
                    let command_id = self.next_command_id;
                    self.next_command_id += 1;
                    self.transcript_entries
                        .push(TranscriptEntry::Command(CommandBlock {
                            id: command_id,
                            command: "AI command generation".to_owned(),
                            output: result.response,
                            status: CommandBlockStatus::Error,
                        }));
                }
            }
        }
    }

    fn launch_interactive_command(&mut self, command: &str) {
        if let Some(pane) = self.active_terminal_pane() {
            pane.backend.send(TerminalRequest::RunCommand(command.to_owned()));
        }
    }

    fn toggle_diff_navigation(&mut self) {
        self.diff_navigation_open = !self.diff_navigation_open;
        self.ensure_selected_diff_file();
    }

    fn ensure_selected_diff_file(&mut self) {
        let selected_exists = self.selected_diff_file.as_ref().is_some_and(|selected| {
            self.input_context
                .diff_files
                .iter()
                .any(|file| &file.path == selected)
        });

        if !selected_exists {
            self.selected_diff_file = None;
        }
    }

    fn render_search_sidebar(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(color(24, 24, 24))
            .inner_margin(Margin::same(8))
            .stroke(Stroke::new(1.0, color(43, 43, 43)))
            .show(ui, |ui| {
                ui.set_min_height(ui.available_height());
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
                        .id_salt("search_sidebar_tabs_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = Vec2::ZERO;

                            if matching_indices.is_empty() {
                                sidebar_empty_state(ui, self.query.trim());
                                return;
                            }

                            for index in matching_indices {
                                let card = tab_card(ui, &self.tabs[index], index == self.selected_tab);
                                if card.response.clicked() {
                                    self.selected_tab = index;
                                }
                                if card.close_clicked {
                                    self.close_tab(index);
                                    break;
                                }
                            }
                        });
                });
            });
    }
}

struct TabCardOutput {
    response: egui::Response,
    close_clicked: bool,
}

struct CommandSuggestion {
    completion: String,
}

impl eframe::App for VelocityApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let previous_fingerprints: Vec<String> = self
            .terminals
            .iter()
            .map(|pane| snapshot_fingerprint(&pane.snapshot))
            .collect();
        for pane in &mut self.terminals {
            pane.backend.drain_updates(&mut pane.snapshot);
        }
        drain_command_results(&self.command_executor, &mut self.transcript_entries);
        for result in self.agent_executor.drain_results() {
            self.apply_agent_result(result);
        }
        let terminal_changed = self
            .terminals
            .iter()
            .zip(previous_fingerprints.iter())
            .any(|(pane, previous)| snapshot_fingerprint(&pane.snapshot) != *previous);
        if terminal_changed {
            ctx.request_repaint();
        }
        if self.last_context_refresh.elapsed() >= Duration::from_secs(2) {
            self.input_context = read_input_context_for_directory(&self.shell_directory);
            self.ensure_selected_diff_file();
            self.refresh_tab_contexts();
            self.last_context_refresh = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(8));
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

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(color(16, 16, 18))
                    .inner_margin(Margin::ZERO),
            )
            .show_inside(ui, |ui| {
                let terminal_workspace_visible = self.terminal_workspace_visible();
                if !terminal_workspace_visible {
                    let root_rect = ui.max_rect();
                    let bar_height = 72.0;
                    let divider = 1.0;
                    let drawer_width = if self.diff_navigation_open {
                        (root_rect.width() * 0.44).clamp(420.0, 760.0)
                    } else {
                        0.0
                    };

                    let sidebar_rect = egui::Rect::from_min_max(
                        root_rect.min,
                        egui::pos2(root_rect.left() + sidebar_width, root_rect.bottom()),
                    );
                    let diff_rect = if self.diff_navigation_open {
                        Some(egui::Rect::from_min_max(
                            egui::pos2(root_rect.right() - drawer_width, root_rect.top()),
                            root_rect.right_bottom(),
                        ))
                    } else {
                        None
                    };
                    let center_left = sidebar_rect.right() + divider;
                    let center_right = diff_rect
                        .map(|rect| rect.left() - divider)
                        .unwrap_or(root_rect.right());
                    let center_rect = egui::Rect::from_min_max(
                        egui::pos2(center_left, root_rect.top()),
                        egui::pos2(center_right.max(center_left), root_rect.bottom()),
                    );
                    let bar_rect = egui::Rect::from_min_max(
                        egui::pos2(center_rect.left(), (center_rect.bottom() - bar_height).max(center_rect.top())),
                        center_rect.right_bottom(),
                    );
                    let transcript_rect = egui::Rect::from_min_max(
                        center_rect.min,
                        egui::pos2(center_rect.right(), (bar_rect.top() - divider).max(center_rect.top())),
                    );

                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(sidebar_rect),
                        |ui| self.render_search_sidebar(ui),
                    );

                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(transcript_rect),
                        |ui| {
                            let transcript_height = transcript_rect.height().max(180.0);
                            ScrollArea::vertical()
                                .id_salt("command_transcript_scroll")
                                .auto_shrink([false, false])
                                .stick_to_bottom(true)
                                .max_height(transcript_height)
                                .show(ui, |ui| {
                                    ui.set_min_height(transcript_height);
                                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                                    let estimated_block_height = 86.0;
                                    let bottom_padding = (transcript_height
                                        - self.transcript_entries.len() as f32 * estimated_block_height)
                                        .max(0.0);
                                    if bottom_padding > 0.0 {
                                        ui.add_space(bottom_padding);
                                    }
                                    for entry in &self.transcript_entries {
                                        transcript_entry_card(ui, entry);
                                    }
                                });
                        },
                    );

                    paint_command_bar(ui, bar_rect);
                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(bar_rect.shrink2(Vec2::new(10.0, 8.0))),
                        |ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                            let context_rect = ui
                                .allocate_exact_size(
                                    Vec2::new(ui.available_width(), 28.0),
                                    egui::Sense::hover(),
                                )
                                .0;
                            let context_output = paint_command_context_boxes(
                                ui,
                                context_rect,
                                &self.shell_directory,
                                &self.input_context,
                            );
                            if context_output.diff_clicked
                                && !self.input_context.diff_files.is_empty()
                            {
                                self.toggle_diff_navigation();
                            }

                            let suggestion = command_suggestion(
                                &self.command_input,
                                &self.command_history,
                                &self.available_commands,
                                &self.shell_directory,
                            );
                            let mut layouter = |ui: &egui::Ui, text: &dyn TextBuffer, wrap_width: f32| {
                                let mut job = command_input_layout_job(text.as_str());
                                job.wrap.max_width = wrap_width;
                                ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
                            };
                            let input_response = ui.add_sized(
                                [ui.available_width(), 24.0],
                                TextEdit::singleline(&mut self.command_input)
                                    .id(self.command_input_id)
                                    .font(eframe::egui::TextStyle::Monospace)
                                    .cursor_at_end(true)
                                    .lock_focus(true)
                                    .background_color(Color32::TRANSPARENT)
                                    .text_color(color(245, 246, 248))
                                    .margin(Vec2::new(0.0, 2.0))
                                    .layouter(&mut layouter)
                                    .frame(Frame::NONE),
                            );
                            if self.refocus_command_input {
                                input_response.request_focus();
                                self.refocus_command_input = false;
                            }
                            if input_response.has_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Tab))
                            {
                                if let Some(suggestion) = &suggestion {
                                    self.command_input = suggestion.completion.clone();
                                    ui.ctx().request_repaint();
                                }
                            }
                            if ui.memory(|memory| memory.has_focus(self.command_input_id))
                                && ui.input_mut(|input| {
                                    input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter)
                                })
                            {
                                self.submit_main_input(MainInputSubmitMode::AiCommand);
                                input_response.request_focus();
                                ui.memory_mut(|memory| memory.request_focus(self.command_input_id));
                                ui.ctx().request_repaint();
                            } else if ui.memory(|memory| memory.has_focus(self.command_input_id))
                                && ui.input_mut(|input| {
                                    input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                })
                            {
                                self.submit_main_input(MainInputSubmitMode::Command);
                                input_response.request_focus();
                                ui.memory_mut(|memory| memory.request_focus(self.command_input_id));
                                ui.ctx().request_repaint();
                            }
                            if let Some(suggestion) = suggestion.as_ref() {
                                if input_response.has_focus()
                                    && suggestion.completion != self.command_input
                                {
                                    paint_command_suggestion(
                                        ui,
                                        input_response.rect,
                                        &self.command_input,
                                        &suggestion.completion,
                                    );
                                }
                            }
                        },
                    );

                    if let Some(diff_rect) = diff_rect {
                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(diff_rect),
                            |ui| {
                                render_diff_navigation_panel(
                                    ui,
                                    &self.shell_directory,
                                    &self.input_context.diff_files,
                                    &mut self.selected_diff_file,
                                    &mut self.diff_navigation_open,
                                );
                            },
                        );
                    }
                } else {
                    let available = ui.available_size_before_wrap();
                    let pane_count = self.terminals.len().max(1);
                    let separator_height = 10.0;
                    let pane_input_height = 46.0;
                    let total_separator_height =
                        separator_height * self.terminals.len().saturating_sub(1) as f32;
                    let pane_height = ((available.y - total_separator_height) / pane_count as f32)
                        .max(170.0);

                    for index in 0..self.terminals.len() {
                        let pane_width = ui.available_width();
                        let terminal_height = (pane_height - pane_input_height).max(120.0);
                        let (terminal_rect, terminal_response) = ui.allocate_exact_size(
                            Vec2::new(pane_width, terminal_height),
                            egui::Sense::click(),
                        );
                        if terminal_response.clicked() {
                            self.active_terminal = index;
                            terminal_response.request_focus();
                        }
                        let pane_has_focus =
                            self.active_terminal == index && terminal_response.has_focus();
                        if pane_has_focus
                            && ui.input(|input| {
                                input.modifiers.ctrl && input.key_pressed(egui::Key::D)
                            })
                        {
                            self.split_active_terminal(ui.ctx());
                            ui.ctx().request_repaint();
                            return;
                        }

                        let usable_terminal_width =
                            (pane_width - terminal_padding.x * 2.0).max(cell_width * 40.0);
                        let usable_terminal_height =
                            (terminal_height - terminal_padding.y * 2.0).max(cell_height * 12.0);
                        let cols = ((usable_terminal_width / cell_width).floor() as u16).max(40);
                        let rows = ((usable_terminal_height / cell_height).floor() as u16).max(12);
                        if pane_has_focus {
                            self.forward_terminal_input(ui.ctx());
                        }
                        if let Some(pane) = self.terminals.get_mut(index) {
                            if (cols, rows) != pane.requested_size {
                                pane.requested_size = (cols, rows);
                                pane.backend.send(TerminalRequest::Resize { cols, rows });
                            }
                            paint_terminal(
                                ui,
                                terminal_rect,
                                &pane.snapshot,
                                pane_has_focus,
                                &terminal_font,
                                terminal_padding,
                                cell_width,
                                cell_height,
                            );
                        }

                        ui.add_space(6.0);
                        let input_rect = ui
                            .allocate_exact_size(
                                Vec2::new(pane_width, pane_input_height - 6.0),
                                egui::Sense::hover(),
                            )
                            .0;
                        paint_command_bar(ui, input_rect);
                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(input_rect.shrink2(Vec2::new(10.0, 6.0))),
                            |ui| {
                                let suggestion = self.terminals.get(index).and_then(|pane| {
                                    command_suggestion(
                                        &pane.command_input,
                                        &self.command_history,
                                        &self.available_commands,
                                        &self.shell_directory,
                                    )
                                });
                                let input_response = if let Some(pane) = self.terminals.get_mut(index) {
                                    let mut layouter =
                                        |ui: &egui::Ui, text: &dyn TextBuffer, wrap_width: f32| {
                                            let mut job = command_input_layout_job(text.as_str());
                                            job.wrap.max_width = wrap_width;
                                            ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
                                        };
                                    ui.add_sized(
                                        [ui.available_width(), 28.0],
                                        TextEdit::singleline(&mut pane.command_input)
                                            .id(pane.command_input_id)
                                            .font(eframe::egui::TextStyle::Monospace)
                                            .lock_focus(true)
                                            .background_color(Color32::TRANSPARENT)
                                            .text_color(color(245, 246, 248))
                                            .margin(Vec2::new(0.0, 4.0))
                                            .layouter(&mut layouter)
                                            .frame(Frame::NONE),
                                    )
                                } else {
                                    return;
                                };

                                if input_response.clicked() {
                                    self.active_terminal = index;
                                    input_response.request_focus();
                                }
                                if self.refocus_terminal_input == Some(index) {
                                    input_response.request_focus();
                                    self.refocus_terminal_input = None;
                                }
                                if input_response.has_focus()
                                    && ui.input(|input| input.key_pressed(egui::Key::Tab))
                                {
                                    if let Some(suggestion) = &suggestion {
                                        if let Some(pane) = self.terminals.get_mut(index) {
                                            pane.command_input = suggestion.completion.clone();
                                        }
                                        ui.ctx().request_repaint();
                                    }
                                }
                                if let Some(command_input_id) =
                                    self.terminals.get(index).map(|pane| pane.command_input_id)
                                {
                                    if ui.memory(|memory| memory.has_focus(command_input_id))
                                        && ui.input_mut(|input| {
                                            input.consume_key(
                                                egui::Modifiers::NONE,
                                                egui::Key::Enter,
                                            )
                                        })
                                    {
                                        input_response.request_focus();
                                        ui.memory_mut(|memory| memory.request_focus(command_input_id));
                                        self.run_terminal_pane_command(index);
                                        input_response.request_focus();
                                        ui.memory_mut(|memory| memory.request_focus(command_input_id));
                                        ui.ctx().request_repaint();
                                    }
                                }
                                if let Some(suggestion) = suggestion.as_ref() {
                                    if input_response.has_focus() {
                                        let current_input = self
                                            .terminals
                                            .get(index)
                                            .map(|pane| pane.command_input.clone())
                                            .unwrap_or_default();
                                        if suggestion.completion != current_input {
                                            paint_command_suggestion(
                                                ui,
                                                input_response.rect,
                                                &current_input,
                                                &suggestion.completion,
                                            );
                                        }
                                    }
                                }
                            },
                        );
                        if index + 1 < self.terminals.len() {
                            let separator_rect = ui
                                .allocate_exact_size(
                                    Vec2::new(ui.available_width(), separator_height),
                                    egui::Sense::hover(),
                                )
                                .0;
                            paint_split_separator(ui, separator_rect);
                        }
                    }
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
            let _ = update_tx.send(snapshot_from_text(format!(
                "Unable to create terminal runtime.\n\n{error}"
            )));
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

            tokio::time::sleep(Duration::from_millis(4)).await;
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
    let enter = if cfg!(target_os = "windows") { "\r" } else { "\n" };
    terminal.paste_string(&format!("{command}{enter}"))
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
    painter.rect_filled(rect, CornerRadius::ZERO, color(14, 15, 18));
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, color(58, 62, 72)),
    );
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, color(58, 62, 72)),
    );
}

fn paint_split_separator(ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, color(16, 16, 18));
    painter.line_segment(
        [
            egui::pos2(rect.left(), rect.center().y),
            egui::pos2(rect.right(), rect.center().y),
        ],
        Stroke::new(1.0, color(64, 68, 77)),
    );
}

struct CommandContextOutput {
    diff_clicked: bool,
}

fn paint_command_context_boxes(
    ui: &egui::Ui,
    rect: egui::Rect,
    directory: &str,
    input_context: &InputContext,
) -> CommandContextOutput {
    let painter = ui.painter();
    let font = FontId::new(12.0, FontFamily::Monospace);
    let box_fill = color(28, 29, 33);
    let box_stroke = Stroke::new(1.0, color(44, 47, 54));
    let dir_text_color = color(132, 208, 255);
    let branch_text_color = color(255, 208, 102);
    let added_text_color = color(123, 216, 143);
    let removed_text_color = color(255, 128, 128);
    let inner_pad = 6.0;
    let gap = 6.0;
    let outer_pad = 8.0;
    let box_height = (rect.height() - 12.0).max(18.0);
    let y = rect.top() + 1.0;

    let dir_text = display_tab_path(directory);
    let dir_galley = painter.layout_no_wrap(dir_text, font.clone(), dir_text_color);
    let dir_w = dir_galley.size().x + inner_pad * 2.0;
    let dir_box = egui::Rect::from_min_size(
        egui::pos2(rect.left() + outer_pad, y),
        Vec2::new(dir_w, box_height),
    );
    painter.rect_filled(dir_box, CornerRadius::ZERO, box_fill);
    painter.rect_stroke(dir_box, CornerRadius::ZERO, box_stroke, StrokeKind::Outside);
    painter.galley(
        egui::pos2(
            dir_box.left() + inner_pad,
            dir_box.center().y - dir_galley.size().y / 2.0,
        ),
        dir_galley,
        dir_text_color,
    );

    let branch_text = input_context.branch.clone();
    let branch_galley = painter.layout_no_wrap(branch_text, font.clone(), branch_text_color);
    let branch_icon_size = Vec2::new(12.0, 12.0);
    let branch_content_w = branch_icon_size.x + 6.0 + branch_galley.size().x;
    let branch_w = branch_content_w + inner_pad * 2.0;
    let branch_box = egui::Rect::from_min_size(
        egui::pos2(dir_box.right() + gap, y),
        Vec2::new(branch_w, box_height),
    );
    painter.rect_filled(branch_box, CornerRadius::ZERO, box_fill);
    painter.rect_stroke(branch_box, CornerRadius::ZERO, box_stroke, StrokeKind::Outside);
    let branch_icon_origin = egui::pos2(
        branch_box.left() + inner_pad,
        branch_box.center().y - branch_icon_size.y / 2.0,
    );
    paint_branch_badge_icon(
        painter,
        egui::Rect::from_min_size(branch_icon_origin, branch_icon_size),
        branch_text_color,
    );
    painter.galley(
        egui::pos2(
            branch_box.left() + inner_pad + branch_icon_size.x + 6.0,
            branch_box.center().y - branch_galley.size().y / 2.0,
        ),
        branch_galley,
        branch_text_color,
    );

    let added_text = format!("+{}", input_context.added_lines);
    let removed_text = format!("-{}", input_context.removed_lines);
    let added_galley = painter.layout_no_wrap(added_text, font.clone(), added_text_color);
    let removed_galley = painter.layout_no_wrap(removed_text, font, removed_text_color);
    let changes_gap = 8.0;
    let added_w = added_galley.size().x;
    let changes_w = added_w + removed_galley.size().x + changes_gap + inner_pad * 2.0;
    let changes_box = egui::Rect::from_min_size(
        egui::pos2(branch_box.right() + gap, y),
        Vec2::new(changes_w, box_height),
    );
    let changes_response = ui.interact(
        changes_box,
        ui.id().with("command_context_diff_box"),
        egui::Sense::click(),
    );
    let changes_fill = if changes_response.hovered() && !input_context.diff_files.is_empty() {
        color(35, 38, 44)
    } else {
        box_fill
    };
    painter.rect_filled(changes_box, CornerRadius::ZERO, changes_fill);
    painter.rect_stroke(changes_box, CornerRadius::ZERO, box_stroke, StrokeKind::Outside);
    let changes_text_y = changes_box.center().y - added_galley.size().y / 2.0;
    painter.galley(
        egui::pos2(changes_box.left() + inner_pad, changes_text_y),
        added_galley,
        added_text_color,
    );
    painter.galley(
        egui::pos2(
            changes_box.left() + inner_pad + added_w + changes_gap,
            changes_text_y,
        ),
        removed_galley,
        removed_text_color,
    );

    CommandContextOutput {
        diff_clicked: changes_response.clicked(),
    }
}

fn paint_branch_badge_icon(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let top = egui::pos2(rect.center().x, rect.top() + 2.0);
    let mid = egui::pos2(rect.center().x, rect.center().y);
    let left = egui::pos2(rect.left() + 3.0, rect.bottom() - 2.5);
    let right = egui::pos2(rect.right() - 2.5, rect.bottom() - 2.5);

    painter.line_segment([top, mid], stroke);
    painter.line_segment([mid, left], stroke);
    painter.line_segment([mid, right], stroke);
    painter.circle_filled(top, 2.0, color);
    painter.circle_filled(left, 2.0, color);
    painter.circle_filled(right, 2.0, color);
}

fn read_input_context_for_directory(directory: &str) -> InputContext {
    let branch = command_stdout(&["git", "-C", directory, "branch", "--show-current"])
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "no-branch".to_owned());

    let diff_text = command_stdout(&["git", "-C", directory, "diff", "--shortstat"])
        .unwrap_or_default();
    let (added_lines, removed_lines) = parse_diff_shortstat(&diff_text);
    let diff_files = read_diff_files_for_directory(directory);

    InputContext {
        branch,
        added_lines,
        removed_lines,
        diff_files,
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

fn read_diff_files_for_directory(directory: &str) -> Vec<DiffFileEntry> {
    let output = command_stdout(&["git", "-C", directory, "diff", "--name-status"]).unwrap_or_default();
    output
        .lines()
        .filter_map(parse_diff_file_line)
        .collect()
}

fn parse_diff_file_line(line: &str) -> Option<DiffFileEntry> {
    let mut parts = line.split_whitespace();
    let _status = parts.next()?;
    let path = parts.last()?.replace('\\', "/");
    Some(DiffFileEntry { path })
}

fn default_command_history() -> Vec<String> {
    [
        "git status",
        "git pull",
        "git push",
        "cargo check",
        "cargo run",
        "npm run dev",
        "docker ps",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn discover_available_commands() -> Vec<String> {
    let mut commands = BTreeSet::new();

    for builtin in [
        "cd",
        "cls",
        "clear",
        "dir",
        "echo",
        "ls",
        "pwd",
        "set",
        "type",
        "where",
    ] {
        commands.insert(builtin.to_owned());
    }

    let path_separator = if cfg!(target_os = "windows") { ';' } else { ':' };
    let executable_suffixes: &[&str] = if cfg!(target_os = "windows") {
        &["exe", "cmd", "bat", "com", "ps1"]
    } else {
        &[""]
    };

    if let Ok(path_value) = std::env::var("PATH") {
        for directory in path_value.split(path_separator) {
            let trimmed = directory.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(entries) = fs::read_dir(trimmed) else {
                continue;
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                if cfg!(target_os = "windows") {
                    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
                        continue;
                    };
                    if !executable_suffixes
                        .iter()
                        .any(|suffix| extension.eq_ignore_ascii_case(suffix))
                    {
                        continue;
                    }
                }

                let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                if stem.is_empty() {
                    continue;
                }

                commands.insert(stem.to_lowercase());
            }
        }
    }

    commands.into_iter().collect()
}

fn command_suggestion(
    input: &str,
    history: &[String],
    available_commands: &[String],
    shell_directory: &str,
) -> Option<CommandSuggestion> {
    let trimmed_start = input.trim_start();
    if trimmed_start.is_empty() {
        return None;
    }

    if "/agent".starts_with(trimmed_start) && trimmed_start != "/agent" {
        return Some(CommandSuggestion {
            completion: "/agent ".to_owned(),
        });
    }

    if let Some(partial) = parse_cd_target(trimmed_start) {
        let suggestion = suggest_directory_completion(shell_directory, partial)?;
        return Some(CommandSuggestion {
            completion: compose_cd_completion(trimmed_start, &suggestion),
        });
    }

    let mut candidates: Vec<(String, i32)> = Vec::new();
    let command_name = trimmed_start
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let has_arguments = trimmed_start.split_whitespace().nth(1).is_some() || trimmed_start.ends_with(' ');

    for (index, command) in history.iter().enumerate() {
        if command.starts_with(trimmed_start) && command != trimmed_start {
            candidates.push((command.clone(), 10_000 - index as i32));
        }
    }

    if !has_arguments {
        for (index, command) in available_commands.iter().enumerate() {
            if command.starts_with(&command_name) && command != trimmed_start {
                candidates.push((command.clone(), 8_000 - index as i32));
            }
        }
    } else if available_commands
        .iter()
        .any(|command| command.eq_ignore_ascii_case(&command_name))
    {
        for phrase in known_command_phrases(trimmed_start) {
            if phrase.starts_with(trimmed_start) && phrase != trimmed_start {
                let score = 5_000 - phrase.len() as i32;
                candidates.push((phrase.to_owned(), score));
            }
        }
    } else {
        for phrase in known_command_phrases(trimmed_start) {
            if phrase.starts_with(trimmed_start) && phrase != trimmed_start {
                let score = 5_000 - phrase.len() as i32;
                candidates.push((phrase.to_owned(), score));
            }
        }
    }

    candidates.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    candidates.dedup_by(|left, right| left.0 == right.0);

    let completion = candidates.first()?.0.clone();
    Some(CommandSuggestion { completion })
}

fn known_command_phrases(input: &str) -> Vec<&'static str> {
    let normalized = input.trim_start();
    let command = normalized.split_whitespace().next().unwrap_or_default();

    let mut phrases = vec![
        "git status",
        "git status --short",
        "git branch",
        "git branch -a",
        "git switch",
        "git switch -c",
        "git checkout",
        "git pull",
        "git push",
        "git push --force-with-lease",
        "git fetch",
        "git add .",
        "git add -A",
        "git add -p",
        "git commit -m \"\"",
        "git commit --amend",
        "git restore .",
        "git restore --staged .",
        "git diff",
        "git diff --staged",
        "git log --oneline",
        "git merge",
        "git cherry-pick",
        "git revert HEAD",
        "git remote -v",
        "cargo check",
        "cargo run",
        "cargo build",
        "cargo build --release",
        "cargo test",
        "cargo fmt",
        "cargo clippy",
        "cargo update",
        "cargo doc --open",
        "cargo watch -x check",
        "npm install",
        "npm ci",
        "npm run dev",
        "npm run build",
        "npm run lint",
        "npm run test",
        "npm start",
        "npm test",
        "pnpm install",
        "pnpm add",
        "pnpm dev",
        "pnpm build",
        "pnpm lint",
        "pnpm test",
        "yarn install",
        "yarn dev",
        "yarn build",
        "yarn lint",
        "yarn start",
        "docker ps",
        "docker ps -a",
        "docker compose up",
        "docker compose up -d",
        "docker compose down",
        "docker build .",
        "docker build -t app .",
        "docker run --rm -it",
        "docker stop",
        "docker rm",
        "kubectl get pods",
        "kubectl get deployments",
        "kubectl get svc",
        "kubectl describe pod",
        "kubectl logs -f",
        "kubectl apply -f",
        "kubectl delete -f",
        "gh pr status",
        "gh pr create",
        "gh pr checkout",
        "gh repo view",
        "gh issue status",
        "python -m venv .venv",
        "python -m pytest",
        "python -m pip install -r requirements.txt",
        "python manage.py runserver",
        "uv run",
        "uv sync",
        "uv add",
        "bun run dev",
        "bun install",
        "ls",
        "dir",
        "cd ..",
        "pwd",
        "clear",
        "cls",
        "rg",
        "rg --files",
        "fd",
        "node --watch",
        "npx create-next-app",
        "npx vite",
        "make test",
        "make build",
        "go test ./...",
        "go run .",
        "go build ./...",
        "pytest -q",
        "pytest -k",
        "uvicorn main:app --reload",
        "/agent ",
    ];

    match command {
        "git" => phrases.extend([
            "git stash",
            "git stash pop",
            "git stash push -u",
            "git reset --soft HEAD~1",
            "git reset --hard HEAD~1",
            "git rebase -i HEAD~3",
            "git clean -fd",
            "git show HEAD",
            "git blame",
        ]),
        "cargo" => phrases.extend([
            "cargo clean",
            "cargo doc",
            "cargo bench",
            "cargo nextest run",
            "cargo expand",
        ]),
        "docker" => phrases.extend([
            "docker logs",
            "docker logs -f",
            "docker images",
            "docker exec -it",
            "docker inspect",
        ]),
        "npm" | "pnpm" | "yarn" => phrases.extend([
            "npm run build",
            "npm run lint",
            "pnpm lint",
            "yarn test",
            "pnpm typecheck",
            "npm run typecheck",
        ]),
        "kubectl" => phrases.extend([
            "kubectl config get-contexts",
            "kubectl config use-context",
            "kubectl rollout restart deployment",
        ]),
        "gh" => phrases.extend([
            "gh auth status",
            "gh pr diff",
            "gh pr view --web",
        ]),
        "python" => phrases.extend([
            "python -m pip install",
            "python -m http.server",
        ]),
        _ => {}
    }

    phrases
}

fn paint_command_suggestion(
    ui: &egui::Ui,
    rect: egui::Rect,
    current_input: &str,
    completion: &str,
) {
    if !completion.starts_with(current_input) {
        return;
    }

    let suffix = &completion[current_input.len()..];
    if suffix.is_empty() {
        return;
    }

    let font_id = FontId::new(14.0, FontFamily::Monospace);
    let prefix_width = ui
        .painter()
        .layout_no_wrap(current_input.to_owned(), font_id.clone(), Color32::WHITE)
        .size()
        .x;
    let pos = egui::pos2(
        rect.left() + 4.0 + prefix_width,
        rect.top() + 2.0,
    );
    ui.painter().text(
        pos,
        Align2::LEFT_TOP,
        suffix,
        font_id,
        color(104, 108, 116),
    );
}

fn command_input_layout_job(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let base = TextFormat {
        font_id: FontId::new(14.0, FontFamily::Monospace),
        color: color(245, 246, 248),
        ..Default::default()
    };
    let agent_chip = TextFormat {
        font_id: FontId::new(14.0, FontFamily::Monospace),
        color: color(255, 214, 102),
        background: color(39, 31, 12),
        ..Default::default()
    };

    if let Some(rest) = text.strip_prefix("/agent") {
        job.append("/agent", 0.0, agent_chip);
        if !rest.is_empty() {
            job.append(rest, 0.0, base);
        }
    } else {
        job.append(text, 0.0, base);
    }

    job
}

fn transcript_entry_card(ui: &mut egui::Ui, entry: &TranscriptEntry) {
    match entry {
        TranscriptEntry::Command(block) => command_block_card(ui, block),
        TranscriptEntry::Agent(chat) => agent_chat_box_card(ui, chat),
    }
}

fn command_block_card(ui: &mut egui::Ui, block: &CommandBlock) {
    let available_width = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(available_width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            Frame::new()
                .fill(color(20, 21, 24))
                .corner_radius(CornerRadius::ZERO)
                .inner_margin(Margin::same(12))
                .stroke(Stroke::new(1.0, color(44, 47, 54)))
                .show(ui, |ui| {
                    ui.set_min_width(available_width - 2.0);
                    ui.label(
                        RichText::new(&block.command)
                            .family(FontFamily::Monospace)
                            .size(13.0)
                            .color(color(242, 244, 247)),
                    );
                    if !block.output.trim().is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(&block.output)
                                .family(FontFamily::Monospace)
                                .size(12.0)
                                .color(color(215, 217, 222)),
                        );
                    }
                });
        },
    );
}

fn agent_chat_box_card(ui: &mut egui::Ui, chat: &AgentChatBox) {
    let available_width = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(available_width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            Frame::new()
                .fill(color(20, 21, 24))
                .corner_radius(CornerRadius::ZERO)
                .inner_margin(Margin::same(12))
                .stroke(Stroke::new(1.0, color(44, 47, 54)))
                .show(ui, |ui| {
                    ui.set_min_width(available_width - 2.0);
                    ui.horizontal_wrapped(|ui| {
                        Frame::new()
                            .fill(color(39, 31, 12))
                            .corner_radius(CornerRadius::ZERO)
                            .inner_margin(Margin::symmetric(6, 2))
                            .stroke(Stroke::new(1.0, color(93, 76, 28)))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("/agent")
                                        .family(FontFamily::Monospace)
                                        .size(12.0)
                                        .color(color(255, 208, 102)),
                                );
                            });
                        if !chat.prompt.trim().is_empty() {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(&chat.prompt)
                                    .family(FontFamily::Monospace)
                                    .size(13.0)
                                    .color(color(242, 244, 247)),
                            );
                        }
                    });
                    ui.add_space(8.0);
                    render_agent_chat_output(ui, chat);
                });
        },
    );
}

fn render_agent_chat_output(ui: &mut egui::Ui, chat: &AgentChatBox) {
    match chat.status {
        AgentChatStatus::Running => {
            let dots = ((chat.created_at.elapsed().as_millis() / 400) % 4) as usize;
            let thinking = format!("Thinking{}", ".".repeat(dots));
            ui.ctx().request_repaint_after(Duration::from_millis(120));
            ui.label(
                RichText::new(thinking)
                    .family(FontFamily::Monospace)
                    .size(12.0)
                    .color(color(255, 208, 102)),
            );
        }
        AgentChatStatus::Success | AgentChatStatus::Error => {
            render_ai_markdown(
                ui,
                &chat.response,
                chat.status == AgentChatStatus::Error,
            );
        }
    }
}

fn render_ai_markdown(ui: &mut egui::Ui, text: &str, is_error: bool) {
    let text = normalize_markdown_text(text);
    let base_color = if is_error {
        color(255, 128, 128)
    } else {
        color(215, 217, 222)
    };
    let heading_color = if is_error {
        color(255, 160, 160)
    } else {
        color(242, 244, 247)
    };
    let bullet_color = if is_error {
        color(255, 145, 145)
    } else {
        color(132, 208, 255)
    };
    let code_bg = if is_error {
        color(38, 18, 18)
    } else {
        color(12, 13, 16)
    };

    let mut in_code_block = false;
    let mut code_lines: Vec<String> = Vec::new();
    let mut paragraph_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("```") {
            if !paragraph_lines.is_empty() {
                render_markdown_paragraph(ui, &paragraph_lines, base_color, code_bg, is_error);
                paragraph_lines.clear();
            }
            if in_code_block {
                render_code_block(ui, &code_lines.join("\n"), base_color, code_bg);
                code_lines.clear();
                in_code_block = false;
            } else {
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_lines.push(trimmed.to_owned());
            continue;
        }

        if trimmed.is_empty() {
            if !paragraph_lines.is_empty() {
                render_markdown_paragraph(ui, &paragraph_lines, base_color, code_bg, is_error);
                paragraph_lines.clear();
            }
            ui.add_space(4.0);
            continue;
        }

        if let Some((level, heading)) = parse_markdown_heading(trimmed) {
            if !paragraph_lines.is_empty() {
                render_markdown_paragraph(ui, &paragraph_lines, base_color, code_bg, is_error);
                paragraph_lines.clear();
            }
            let size = match level {
                1 => 18.0,
                2 => 15.0,
                _ => 13.0,
            };
            ui.add(
                egui::Label::new(inline_markdown_job(
                    heading,
                    heading_color,
                    code_bg,
                    true,
                    size,
                    is_error,
                ))
                .wrap(),
            );
            continue;
        }

        if is_markdown_rule(trimmed) {
            if !paragraph_lines.is_empty() {
                render_markdown_paragraph(ui, &paragraph_lines, base_color, code_bg, is_error);
                paragraph_lines.clear();
            }
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 8.0), egui::Sense::hover());
            let y = rect.center().y;
            ui.painter().line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                Stroke::new(1.0, color(44, 47, 54)),
            );
            continue;
        }

        if let Some((marker, item)) = parse_markdown_list_item_ascii(trimmed) {
            if !paragraph_lines.is_empty() {
                render_markdown_paragraph(ui, &paragraph_lines, base_color, code_bg, is_error);
                paragraph_lines.clear();
            }
            ui.horizontal_wrapped(|ui| {
                ui.add_sized(
                    [18.0, 0.0],
                    egui::Label::new(
                        RichText::new(marker)
                            .family(FontFamily::Monospace)
                            .size(12.0)
                            .color(bullet_color),
                    ),
                );
                ui.add(
                    egui::Label::new(inline_markdown_job(
                        item,
                        base_color,
                        code_bg,
                        false,
                        12.0,
                        is_error,
                    ))
                    .wrap(),
                );
            });
            continue;
        }

        if let Some(quote) = trimmed.strip_prefix("> ") {
            if !paragraph_lines.is_empty() {
                render_markdown_paragraph(ui, &paragraph_lines, base_color, code_bg, is_error);
                paragraph_lines.clear();
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(">")
                        .family(FontFamily::Monospace)
                        .size(12.0)
                        .color(bullet_color),
                );
                ui.add(
                    egui::Label::new(inline_markdown_job(
                        quote,
                        base_color,
                        code_bg,
                        false,
                        12.0,
                        is_error,
                    ))
                    .wrap(),
                );
            });
            continue;
        }

        if looks_like_markdown_table_row(trimmed) {
            if !paragraph_lines.is_empty() {
                render_markdown_paragraph(ui, &paragraph_lines, base_color, code_bg, is_error);
                paragraph_lines.clear();
            }
            ui.label(
                RichText::new(trimmed)
                    .family(FontFamily::Monospace)
                    .size(12.0)
                    .color(base_color),
            );
            continue;
        }

        paragraph_lines.push(trimmed.to_owned());
    }

    if !paragraph_lines.is_empty() {
        render_markdown_paragraph(ui, &paragraph_lines, base_color, code_bg, is_error);
    }

    if in_code_block && !code_lines.is_empty() {
        render_code_block(ui, &code_lines.join("\n"), base_color, code_bg);
    }
}

fn render_markdown_paragraph(
    ui: &mut egui::Ui,
    lines: &[String],
    base_color: Color32,
    code_bg: Color32,
    is_error: bool,
) {
    let text = lines.join(" ");
    ui.add(
        egui::Label::new(inline_markdown_job(
            &text,
            base_color,
            code_bg,
            false,
            12.0,
            is_error,
        ))
        .wrap(),
    );
}

fn normalize_markdown_text(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\u{00A0}' | '\u{2000}'..='\u{200B}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
            '\r' => ' ',
            _ => ch,
        })
        .collect()
}

fn parse_markdown_heading(line: &str) -> Option<(usize, &str)> {
    for level in 1..=3 {
        let marker = format!("{} ", "#".repeat(level));
        if let Some(rest) = line.strip_prefix(&marker) {
            return Some((level, rest));
        }
    }
    None
}

fn is_markdown_rule(line: &str) -> bool {
    matches!(line, "---" | "***" | "___")
}

fn parse_markdown_list_item_ascii(line: &str) -> Option<(String, &str)> {
    if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some(("-".to_owned(), item));
    }

    let digits_end = line.bytes().take_while(|byte| byte.is_ascii_digit()).count();
    if digits_end > 0 {
        let rest = &line[digits_end..];
        if let Some(item) = rest.strip_prefix(". ") {
            return Some((format!("{}.", &line[..digits_end]), item));
        }
    }

    None
}

fn looks_like_markdown_table_row(line: &str) -> bool {
    line.matches('|').count() >= 2
}

#[allow(dead_code)]
fn parse_markdown_list_item(line: &str) -> Option<(String, &str)> {
    if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some(("•".to_owned(), item));
    }

    let digits_end = line.bytes().take_while(|byte| byte.is_ascii_digit()).count();
    if digits_end > 0 {
        let rest = &line[digits_end..];
        if let Some(item) = rest.strip_prefix(". ") {
            return Some((format!("{}.", &line[..digits_end]), item));
        }
    }

    None
}

fn inline_markdown_job(
    text: &str,
    base_color: Color32,
    code_bg: Color32,
    strong: bool,
    font_size: f32,
    is_error: bool,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    append_inline_markdown(
        &mut job,
        text,
        base_color,
        code_bg,
        strong,
        false,
        font_size,
        is_error,
    );
    job
}

fn append_inline_markdown(
    job: &mut LayoutJob,
    text: &str,
    base_color: Color32,
    code_bg: Color32,
    strong: bool,
    italics: bool,
    font_size: f32,
    is_error: bool,
) {
    let default_format = TextFormat {
        font_id: FontId::new(font_size, FontFamily::Proportional),
        color: base_color,
        italics,
        ..Default::default()
    };
    let strong_format = TextFormat {
        font_id: FontId::new(font_size + 0.2, FontFamily::Proportional),
        color: base_color,
        italics,
        ..Default::default()
    };
    let code_format = TextFormat {
        font_id: FontId::new((font_size - 0.5).max(11.0), FontFamily::Monospace),
        color: if is_error {
            color(255, 214, 214)
        } else {
            color(244, 246, 248)
        },
        ..Default::default()
    };
    let link_color = if is_error {
        color(255, 178, 178)
    } else {
        color(132, 208, 255)
    };
    let link_format = TextFormat {
        font_id: FontId::new(font_size, FontFamily::Proportional),
        color: link_color,
        underline: Stroke::new(1.0, link_color),
        italics,
        ..Default::default()
    };

    let mut i = 0usize;
    while i < text.len() {
        let rest = &text[i..];

        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                job.append(&after[..end], 0.0, code_format.clone());
                i += end + 2;
                continue;
            }
        }

        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                append_inline_markdown(
                    job,
                    &after[..end],
                    base_color,
                    code_bg,
                    true,
                    italics,
                    font_size,
                    is_error,
                );
                i += end + 4;
                continue;
            }
        }

        if let Some(after) = rest.strip_prefix("__") {
            if let Some(end) = after.find("__") {
                append_inline_markdown(
                    job,
                    &after[..end],
                    base_color,
                    code_bg,
                    true,
                    italics,
                    font_size,
                    is_error,
                );
                i += end + 4;
                continue;
            }
        }

        if let Some(after) = rest.strip_prefix('*') {
            if let Some(end) = after.find('*') {
                append_inline_markdown(
                    job,
                    &after[..end],
                    base_color,
                    code_bg,
                    strong,
                    true,
                    font_size,
                    is_error,
                );
                i += end + 2;
                continue;
            }
        }

        if let Some(after) = rest.strip_prefix('_') {
            if let Some(end) = after.find('_') {
                append_inline_markdown(
                    job,
                    &after[..end],
                    base_color,
                    code_bg,
                    strong,
                    true,
                    font_size,
                    is_error,
                );
                i += end + 2;
                continue;
            }
        }

        if let Some(after_open) = rest.strip_prefix('[') {
            if let Some(label_end) = after_open.find("](") {
                let label = &after_open[..label_end];
                let after_label = &after_open[label_end + 2..];
                if let Some(url_end) = after_label.find(')') {
                    let url = &after_label[..url_end];
                    job.append(label, 0.0, link_format.clone());
                    if !url.is_empty() {
                        job.append(
                            &format!(" ({url})"),
                            0.0,
                            TextFormat {
                                font_id: FontId::new(font_size - 1.0, FontFamily::Monospace),
                                color: color(150, 154, 162),
                                ..Default::default()
                            },
                        );
                    }
                    i += label_end + url_end + 4;
                    continue;
                }
            }
        }

        let next_special = rest.find(['`', '*', '_', '[']).unwrap_or(rest.len());
        let plain_len = next_special.max(1);
        let plain = &rest[..plain_len];
        let format = if strong {
            strong_format.clone()
        } else {
            default_format.clone()
        };
        job.append(plain, 0.0, format);
        i += plain.len();
    }
}

#[allow(dead_code)]
fn render_markdown_preview(ui: &mut egui::Ui, text: &str, is_error: bool) {
    let base_color = if is_error {
        color(255, 128, 128)
    } else {
        color(215, 217, 222)
    };
    let heading_color = if is_error {
        color(255, 160, 160)
    } else {
        color(242, 244, 247)
    };
    let bullet_color = if is_error {
        color(255, 145, 145)
    } else {
        color(132, 208, 255)
    };
    let code_bg = if is_error {
        color(38, 18, 18)
    } else {
        color(12, 13, 16)
    };

    let mut in_code_block = false;
    let mut code_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("```") {
            if in_code_block {
                render_code_block(ui, &code_lines.join("\n"), base_color, code_bg);
                code_lines.clear();
                in_code_block = false;
            } else {
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_lines.push(trimmed.to_owned());
            continue;
        }

        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }

        if let Some(heading) = trimmed.strip_prefix("# ") {
            ui.label(
                RichText::new(heading)
                    .size(18.0)
                    .color(heading_color),
            );
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("## ") {
            ui.label(
                RichText::new(heading)
                    .size(15.0)
                    .color(heading_color),
            );
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("•")
                        .family(FontFamily::Monospace)
                        .size(12.0)
                        .color(bullet_color),
                );
                ui.label(
                    RichText::new(item)
                        .family(FontFamily::Monospace)
                        .size(12.0)
                        .color(base_color),
                );
            });
            continue;
        }

        ui.label(
            RichText::new(trimmed)
                .family(FontFamily::Monospace)
                .size(12.0)
                .color(base_color),
        );
    }

    if in_code_block && !code_lines.is_empty() {
        render_code_block(ui, &code_lines.join("\n"), base_color, code_bg);
    }
}

fn render_code_block(ui: &mut egui::Ui, text: &str, text_color: Color32, bg_color: Color32) {
    Frame::new()
        .fill(bg_color)
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::same(10))
        .stroke(Stroke::new(1.0, color(44, 47, 54)))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .family(FontFamily::Monospace)
                    .size(12.0)
                    .color(text_color),
            );
        });
}

fn render_diff_navigation_panel(
    ui: &mut egui::Ui,
    directory: &str,
    diff_files: &[DiffFileEntry],
    selected_diff_file: &mut Option<String>,
    diff_navigation_open: &mut bool,
) {
    Frame::new()
        .fill(color(18, 19, 23))
        .inner_margin(Margin::same(12))
        .stroke(Stroke::new(1.0, color(44, 47, 54)))
        .show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Diff navigator")
                        .size(14.0)
                        .color(color(236, 238, 241)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Close").size(11.0))
                                .fill(color(28, 29, 33))
                                .stroke(Stroke::new(1.0, color(56, 58, 64))),
                        )
                        .clicked()
                    {
                        *diff_navigation_open = false;
                    }
                });
            });
            ui.add_space(8.0);

            if diff_files.is_empty() {
                ui.label(
                    RichText::new("No edited files in the current working tree.")
                        .size(12.0)
                        .color(color(166, 170, 178)),
                );
                return;
            }

            ui.label(
                RichText::new("Edited files")
                    .size(11.0)
                    .color(color(132, 136, 145)),
            );
            ui.add_space(6.0);

            let list_height = (ui.available_height() * 0.26).clamp(120.0, 220.0);
            ScrollArea::vertical()
                .id_salt("diff_file_list_scroll")
                .auto_shrink([false, false])
                .max_height(list_height)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 6.0);
                    for file in diff_files {
                        let is_selected = selected_diff_file.as_ref() == Some(&file.path);
                        if diff_file_row(ui, file, is_selected).clicked() {
                            if is_selected {
                                *selected_diff_file = None;
                            } else {
                                *selected_diff_file = Some(file.path.clone());
                            }
                        }
                    }
                });

            ui.add_space(10.0);
            if let Some(path) = selected_diff_file.as_deref() {
                render_selected_diff_code(ui, directory, path);
            } else {
                ui.label(
                    RichText::new("Pick a file to inspect its edited code.")
                        .size(12.0)
                        .color(color(166, 170, 178)),
                );
            }
        });
}

fn diff_file_row(ui: &mut egui::Ui, file: &DiffFileEntry, selected: bool) -> egui::Response {
    let desired = Vec2::new(ui.available_width(), 30.0);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let font = FontId::new(11.0, FontFamily::Monospace);
    let text_width = text_width(ui, &file.path, &font);
    let button_rect = egui::Rect::from_min_size(
        rect.min + Vec2::new(6.0, 4.0),
        Vec2::new((text_width + 12.0).min(rect.width() - 12.0).max(36.0), 22.0),
    );
    let response = ui.interact(
        button_rect,
        ui.id().with(("diff_file_row", &file.path)),
        egui::Sense::click(),
    );
    let fill = if selected {
        color(37, 40, 48)
    } else if response.hovered() {
        color(31, 33, 39)
    } else {
        color(22, 23, 28)
    };
    ui.painter().rect(
        rect,
        CornerRadius::ZERO,
        fill,
        Stroke::new(1.0, color(44, 47, 54)),
        StrokeKind::Outside,
    );
    if response.hovered() || selected {
        ui.painter().rect_filled(
            button_rect,
            CornerRadius::ZERO,
            if selected { color(48, 52, 62) } else { color(37, 40, 48) },
        );
    }
    ui.painter().text(
        rect.min + Vec2::new(10.0, 7.0),
        Align2::LEFT_TOP,
        &file.path,
        font,
        color(223, 226, 230),
    );
    response
}

fn render_selected_diff_code(ui: &mut egui::Ui, directory: &str, relative_path: &str) {
    let title = format!("{}  {}", file_language_label(relative_path), relative_path);
    ui.label(
        RichText::new(title)
            .family(FontFamily::Monospace)
            .size(12.0)
            .color(color(188, 193, 202)),
    );
    ui.add_space(6.0);

    let mut diff_contents = load_diff_patch_for_file(directory, relative_path);
    if diff_contents.trim().is_empty() {
        diff_contents = "No line-level diff available for this file yet.".to_owned();
    }

    Frame::new()
        .fill(color(12, 13, 16))
        .stroke(Stroke::new(1.0, color(44, 47, 54)))
        .show(ui, |ui| {
            ScrollArea::both()
                .id_salt(("diff_code_scroll", relative_path))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                let mut layouter = |ui: &egui::Ui, text: &dyn TextBuffer, wrap_width: f32| {
                    let mut job = highlight_diff_patch(text.as_str());
                    job.wrap.max_width = wrap_width;
                    ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
                };
                ui.add(
                    TextEdit::multiline(&mut diff_contents)
                        .font(eframe::egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .interactive(false)
                        .layouter(&mut layouter)
                        .margin(Vec2::new(10.0, 10.0))
                        .frame(Frame::NONE),
                );
            });
        });
}

fn load_diff_patch_for_file(directory: &str, relative_path: &str) -> String {
    command_stdout(&["git", "-C", directory, "diff", "--no-color", "--", relative_path])
        .unwrap_or_default()
}

struct SyntaxHighlightResources {
    syntax_set: SyntaxSet,
    theme: Theme,
}

fn syntax_highlight_resources() -> &'static SyntaxHighlightResources {
    static RESOURCES: OnceLock<SyntaxHighlightResources> = OnceLock::new();
    RESOURCES.get_or_init(|| {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let themes = ThemeSet::load_defaults();
        let theme = themes
            .themes
            .get("base16-ocean.dark")
            .or_else(|| themes.themes.values().next())
            .cloned()
            .unwrap_or_default();
        SyntaxHighlightResources { syntax_set, theme }
    })
}

fn highlight_diff_patch(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let default = TextFormat {
        font_id: FontId::new(13.0, FontFamily::Monospace),
        color: color(222, 225, 230),
        ..Default::default()
    };

    let resources = syntax_highlight_resources();
    let syntax = resources
        .syntax_set
        .find_syntax_by_extension("diff")
        .unwrap_or_else(|| resources.syntax_set.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, &resources.theme);

    for line in LinesWithEndings::from(text) {
        match highlighter.highlight_line(line, &resources.syntax_set) {
            Ok(ranges) => {
                for (style, part) in ranges {
                    job.append(part, 0.0, syntax_text_format(style, &default));
                }
            }
            Err(_) => job.append(line, 0.0, default.clone()),
        }
    }

    job
}

fn syntax_text_format(style: Style, default: &TextFormat) -> TextFormat {
    TextFormat {
        color: Color32::from_rgb(style.foreground.r, style.foreground.g, style.foreground.b),
        ..default.clone()
    }
}

fn file_language_label(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("rs") => "Rust",
        Some("js") | Some("jsx") => "JavaScript",
        Some("ts") | Some("tsx") => "TypeScript",
        Some("py") => "Python",
        Some("json") => "JSON",
        Some("toml") => "TOML",
        Some("md") => "Markdown",
        Some("html") => "HTML",
        Some("css") => "CSS",
        Some("yml") | Some("yaml") => "YAML",
        _ => "Code",
    }
}

fn parse_cd_target(command: &str) -> Option<&str> {
    let trimmed = command.trim_start();
    let rest = trimmed.strip_prefix("cd ")?;
    Some(rest.trim())
}

fn resolve_directory_target(base_directory: &str, target: &str) -> Option<String> {
    let trimmed = target.trim();
    let candidate = if trimmed.is_empty() || trimmed == "~" {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok()
            .map(PathBuf::from)?
    } else {
        let normalized = trimmed.replace('/', "\\");
        let path = Path::new(&normalized);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            Path::new(base_directory).join(path)
        }
    };

    let canonical = candidate.canonicalize().ok()?;
    if canonical.is_dir() {
        Some(canonical.display().to_string().replace('\\', "/"))
    } else {
        None
    }
}

fn suggest_directory_completion(base_directory: &str, partial: &str) -> Option<String> {
    let trimmed = partial.trim();
    let base = Path::new(base_directory);
    let (search_root, prefix, parent_display) = if trimmed.is_empty() {
        (base.to_path_buf(), String::new(), String::new())
    } else {
        let normalized = trimmed.replace('/', "\\");
        let partial_path = Path::new(&normalized);
        let parent = partial_path.parent().filter(|parent| !parent.as_os_str().is_empty());
        let search_root = match parent {
            Some(parent) if partial_path.is_absolute() => parent.to_path_buf(),
            Some(parent) => base.join(parent),
            None => base.to_path_buf(),
        };
        let prefix = partial_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_owned();
        let parent_display = parent
            .and_then(|parent| parent.to_str())
            .unwrap_or("")
            .replace('\\', "/");
        (search_root, prefix, parent_display)
    };

    let mut matches = Vec::new();
    let entries = fs::read_dir(&search_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.to_lowercase().starts_with(&prefix.to_lowercase()) {
            matches.push(name.to_owned());
        }
    }

    matches.sort();
    let only_match = matches.into_iter().next()?;
    if trimmed.contains('/') || trimmed.contains('\\') {
        if parent_display.is_empty() {
            Some(only_match)
        } else {
            Some(format!("{parent_display}/{only_match}"))
        }
    } else {
        Some(only_match)
    }
}

fn compose_cd_completion(current_input: &str, directory: &str) -> String {
    if current_input.trim_end() == "cd" {
        format!("cd {directory}")
    } else {
        let trimmed_end = current_input.trim_end();
        let prefix = trimmed_end
            .strip_prefix("cd")
            .map(str::trim_start)
            .unwrap_or(trimmed_end);
        if prefix.is_empty() {
            format!("cd {directory}")
        } else {
            format!("cd {directory}")
        }
    }
}

fn looks_interactive_command(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or_default().to_lowercase();
    matches!(
        first.as_str(),
        "vim"
            | "nvim"
            | "nano"
            | "htop"
            | "btop"
            | "lazygit"
            | "less"
            | "more"
            | "ssh"
            | "top"
            | "python"
            | "node"
            | "irb"
            | "sqlite3"
    )
}

fn parse_agent_prompt(command: &str) -> Option<&str> {
    let trimmed = command.trim();
    if trimmed == "/agent" {
        return Some("");
    }
    trimmed
        .strip_prefix("/agent ")
        .map(str::trim)
}

fn compose_terminal_command(directory: &str, command: &str) -> String {
    if cfg!(target_os = "windows") {
        let escaped = directory.replace('\'', "''");
        format!("Set-Location -LiteralPath '{escaped}'; {command}")
    } else {
        format!("cd '{}' && {command}", directory.replace('\'', "'\\''"))
    }
}

fn compose_directory_change_command(directory: &str) -> String {
    if cfg!(target_os = "windows") {
        let escaped = directory.replace('\'', "''");
        format!("Set-Location -LiteralPath '{escaped}'")
    } else {
        format!("cd '{}'", directory.replace('\'', "'\\''"))
    }
}

fn execute_command_request(request: &CommandExecutionRequest, result_tx: &Sender<CommandExecutionResult>) {
    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoLogo", "-NoProfile", "-Command", &request.command]);
        command
    } else {
        let mut command = Command::new("/bin/bash");
        command.args(["-lc", &request.command]);
        command
    };

    let child = command
        .current_dir(&request.working_directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            let _ = result_tx.send(CommandExecutionResult {
                id: request.id,
                output_chunk: error.to_string(),
                done: true,
                success: false,
            });
            return;
        }
    };

    let (stream_tx, stream_rx) = mpsc::channel::<CommandStreamMessage>();
    let mut open_streams = 0usize;

    if let Some(stdout) = child.stdout.take() {
        open_streams += 1;
        let stream_tx = stream_tx.clone();
        thread::spawn(move || {
            stream_reader(stdout, stream_tx);
        });
    }

    if let Some(stderr) = child.stderr.take() {
        open_streams += 1;
        let stream_tx = stream_tx.clone();
        thread::spawn(move || {
            stream_reader(stderr, stream_tx);
        });
    }

    drop(stream_tx);

    let mut saw_output = false;
    while open_streams > 0 {
        match stream_rx.recv() {
            Ok(CommandStreamMessage::Chunk(chunk)) => {
                saw_output = true;
                let _ = result_tx.send(CommandExecutionResult {
                    id: request.id,
                    output_chunk: chunk,
                    done: false,
                    success: true,
                });
            }
            Ok(CommandStreamMessage::Closed) => {
                open_streams = open_streams.saturating_sub(1);
            }
            Err(_) => break,
        }
    }

    let status = child.wait();
    match status {
        Ok(status) => {
            let final_chunk = if saw_output {
                String::new()
            } else if status.success() {
                "Done.".to_owned()
            } else {
                format!("Command exited with status {}", status)
            };
            let _ = result_tx.send(CommandExecutionResult {
                id: request.id,
                output_chunk: final_chunk,
                done: true,
                success: status.success(),
            });
        }
        Err(error) => {
            let _ = result_tx.send(CommandExecutionResult {
                id: request.id,
                output_chunk: error.to_string(),
                done: true,
                success: false,
            });
        }
    }
}

fn stream_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    stream_tx: Sender<CommandStreamMessage>,
) {
    let mut reader = BufReader::new(reader);
    let mut buffer = String::new();
    loop {
        buffer.clear();
        match reader.read_line(&mut buffer) {
            Ok(0) => break,
            Ok(_) => {
                let _ = stream_tx.send(CommandStreamMessage::Chunk(buffer.clone()));
            }
            Err(error) => {
                let _ = stream_tx.send(CommandStreamMessage::Chunk(format!("{error}\n")));
                break;
            }
        }
    }
    let _ = stream_tx.send(CommandStreamMessage::Closed);
}

fn drain_command_results(command_executor: &CommandExecutor, entries: &mut [TranscriptEntry]) {
    let mut blocks = Vec::new();
    for entry in entries.iter() {
        if let TranscriptEntry::Command(block) = entry {
            blocks.push(block.clone());
        }
    }

    command_executor.drain_results(&mut blocks);

    for updated_block in blocks {
        for entry in entries.iter_mut() {
            if let TranscriptEntry::Command(block) = entry {
                if block.id == updated_block.id {
                    *block = updated_block.clone();
                    break;
                }
            }
        }
    }
}

fn execute_agent_request(request: &AgentChatRequest) -> AgentChatResult {
    if request.prompt.trim().is_empty() {
        return AgentChatResult {
            id: request.id,
            response: "Usage: /agent <prompt>".to_owned(),
            success: false,
            mode: request.mode,
            generated_command: None,
        };
    }

    let api_key = match load_groq_api_key() {
        Some(value) => value,
        _ => {
            return AgentChatResult {
                id: request.id,
                response: format!(
                    "Missing Groq API key. Add {} to {} or set {}.",
                    GROQ_API_KEY_ENV, LOCAL_ENV_FILE, GROQ_API_KEY_ENV
                ),
                success: false,
                mode: request.mode,
                generated_command: None,
            };
        }
    };

    let client = match Client::builder().timeout(Duration::from_secs(90)).build() {
        Ok(client) => client,
        Err(error) => {
            return AgentChatResult {
                id: request.id,
                response: format!("Unable to create the Groq HTTP client: {error}"),
                success: false,
                mode: request.mode,
                generated_command: None,
            };
        }
    };

    let system_prompt = match request.mode {
        AgentRequestMode::Chat => AGENT_SYSTEM_PROMPT,
        AgentRequestMode::Command => AGENT_COMMAND_SYSTEM_PROMPT,
    };

    let mut messages = vec![
        GroqMessage {
            role: "system".to_owned(),
            content: system_prompt.to_owned(),
        },
        GroqMessage {
            role: "user".to_owned(),
            content: format!(
                "Current working directory: {}\n\nUser request:\n{}",
                request.working_directory, request.prompt
            ),
        },
    ];

    for _ in 0..4 {
        let payload = GroqChatRequest {
            model: GROQ_MODEL.to_owned(),
            messages: messages.clone(),
        };

        let response = client
            .post(GROQ_API_URL)
            .bearer_auth(&api_key)
            .json(&payload)
            .send();

        let content = match response {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    let error_text =
                        response.text().unwrap_or_else(|_| "Unknown error".to_owned());
                    return AgentChatResult {
                        id: request.id,
                        response: format!("Groq request failed with {}: {}", status, error_text),
                        success: false,
                        mode: request.mode,
                        generated_command: None,
                    };
                }

                match response.json::<GroqChatResponse>() {
                    Ok(body) => body
                        .choices
                        .into_iter()
                        .next()
                        .map(|choice| choice.message.content.trim().to_owned())
                        .filter(|content| !content.is_empty())
                        .unwrap_or_else(|| "Groq returned an empty response.".to_owned()),
                    Err(error) => {
                        return AgentChatResult {
                            id: request.id,
                            response: format!("Unable to decode the Groq response: {error}"),
                            success: false,
                            mode: request.mode,
                            generated_command: None,
                        };
                    }
                }
            }
            Err(error) => {
                return AgentChatResult {
                    id: request.id,
                    response: format!("Unable to reach Groq: {error}"),
                    success: false,
                    mode: request.mode,
                    generated_command: None,
                };
            }
        };

        if request.mode == AgentRequestMode::Command {
            let generated_command = extract_generated_command(&content);
            return AgentChatResult {
                id: request.id,
                response: generated_command
                    .clone()
                    .unwrap_or_else(|| format!("The AI did not return a runnable command: {content}")),
                success: generated_command.is_some(),
                mode: request.mode,
                generated_command,
            };
        }

        if let Some(tool_call) = parse_agent_tool_call(&content) {
            let tool_result = run_agent_tool(&request.working_directory, tool_call);
            messages.push(GroqMessage {
                role: "assistant".to_owned(),
                content,
            });
            messages.push(GroqMessage {
                role: "user".to_owned(),
                content: format!(
                    "Tool result:\n```text\n{}\n```\nIf you have enough information, answer normally in Markdown. Otherwise emit another JSON tool call.",
                    tool_result.trim_end()
                ),
            });
            continue;
        }

        return AgentChatResult {
            id: request.id,
            response: content,
            success: true,
            mode: request.mode,
            generated_command: None,
        };
    }

    AgentChatResult {
        id: request.id,
        response: "The agent used too many tool steps without reaching a final answer.".to_owned(),
        success: false,
        mode: request.mode,
        generated_command: None,
    }
}

#[derive(Deserialize)]
struct AgentToolCall {
    tool: String,
    command: Option<String>,
    path: Option<String>,
    new_path: Option<String>,
    content: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

fn parse_agent_tool_call(content: &str) -> Option<AgentToolCall> {
    serde_json::from_str::<AgentToolCall>(content).ok()
}

fn extract_generated_command(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed.starts_with('{') {
        return None;
    }

    let unwrapped = trimmed
        .strip_prefix("```")
        .and_then(|rest| rest.split_once('\n').map(|(_, body)| body))
        .and_then(|body| body.rsplit_once("```").map(|(body, _)| body.trim()))
        .unwrap_or(trimmed);

    let command = unwrapped.lines().next()?.trim();
    if command.is_empty() {
        None
    } else {
        Some(command.to_owned())
    }
}

fn run_agent_tool(working_directory: &str, call: AgentToolCall) -> String {
    match call.tool.as_str() {
        "shell" => {
            let Some(command) = call.command else {
                return "Tool error: missing `command` for shell tool.".to_owned();
            };
            execute_shell_tool(working_directory, &command)
        }
        "read_file" => {
            let Some(path) = call.path else {
                return "Tool error: missing `path` for read_file tool.".to_owned();
            };
            execute_read_file_tool(working_directory, &path)
        }
        "write_file" => {
            let Some(path) = call.path else {
                return "Tool error: missing `path` for write_file tool.".to_owned();
            };
            let Some(content) = call.content else {
                return "Tool error: missing `content` for write_file tool.".to_owned();
            };
            execute_write_file_tool(working_directory, &path, &content)
        }
        "append_file" => {
            let Some(path) = call.path else {
                return "Tool error: missing `path` for append_file tool.".to_owned();
            };
            let Some(content) = call.content else {
                return "Tool error: missing `content` for append_file tool.".to_owned();
            };
            execute_append_file_tool(working_directory, &path, &content)
        }
        "replace_in_file" => {
            let Some(path) = call.path else {
                return "Tool error: missing `path` for replace_in_file tool.".to_owned();
            };
            let Some(from) = call.from else {
                return "Tool error: missing `from` for replace_in_file tool.".to_owned();
            };
            let Some(to) = call.to else {
                return "Tool error: missing `to` for replace_in_file tool.".to_owned();
            };
            execute_replace_in_file_tool(working_directory, &path, &from, &to)
        }
        "list_dir" => execute_list_dir_tool(working_directory, call.path.as_deref()),
        "mkdir" => {
            let Some(path) = call.path else {
                return "Tool error: missing `path` for mkdir tool.".to_owned();
            };
            execute_mkdir_tool(working_directory, &path)
        }
        "rename_path" => {
            let Some(path) = call.path else {
                return "Tool error: missing `path` for rename_path tool.".to_owned();
            };
            let Some(new_path) = call.new_path else {
                return "Tool error: missing `new_path` for rename_path tool.".to_owned();
            };
            execute_rename_path_tool(working_directory, &path, &new_path)
        }
        "delete_path" => {
            let Some(path) = call.path else {
                return "Tool error: missing `path` for delete_path tool.".to_owned();
            };
            execute_delete_path_tool(working_directory, &path)
        }
        "file_info" => {
            let Some(path) = call.path else {
                return "Tool error: missing `path` for file_info tool.".to_owned();
            };
            execute_file_info_tool(working_directory, &path)
        }
        "git_status" => execute_git_status_tool(working_directory),
        "git_diff" => execute_git_diff_tool(working_directory, call.path.as_deref()),
        other => format!("Tool error: unsupported tool `{other}`."),
    }
}

fn execute_shell_tool(working_directory: &str, command: &str) -> String {
    let output = if cfg!(target_os = "windows") {
        Command::new("powershell.exe")
            .args(["-NoLogo", "-NoProfile", "-Command", command])
            .current_dir(working_directory)
            .output()
    } else {
        Command::new("/bin/bash")
            .args(["-lc", command])
            .current_dir(working_directory)
            .output()
    };

    match output {
        Ok(output) => {
            let mut rendered = String::new();
            if !output.stdout.is_empty() {
                rendered.push_str(&String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                if !rendered.is_empty() && !rendered.ends_with('\n') {
                    rendered.push('\n');
                }
                rendered.push_str(&String::from_utf8_lossy(&output.stderr));
            }
            if rendered.trim().is_empty() {
                format!("Command finished with status {}", output.status)
            } else {
                rendered
            }
        }
        Err(error) => format!("Tool error: {error}"),
    }
}

fn execute_read_file_tool(working_directory: &str, path: &str) -> String {
    let full_path = match resolve_agent_relative_path(working_directory, path) {
        Ok(path) => path,
        Err(error) => return error,
    };
    match fs::read_to_string(&full_path) {
        Ok(contents) => {
            truncate_agent_tool_output(contents)
        }
        Err(error) => format!("Tool error: {error}"),
    }
}

fn execute_write_file_tool(working_directory: &str, path: &str, content: &str) -> String {
    let full_path = match resolve_agent_relative_path(working_directory, path) {
        Ok(path) => path,
        Err(error) => return error,
    };
    if let Some(parent) = full_path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return format!("Tool error: {error}");
        }
    }
    match fs::write(&full_path, content) {
        Ok(_) => format!("Wrote {} bytes to {}", content.len(), full_path.display()),
        Err(error) => format!("Tool error: {error}"),
    }
}

fn execute_append_file_tool(working_directory: &str, path: &str, content: &str) -> String {
    let full_path = match resolve_agent_relative_path(working_directory, path) {
        Ok(path) => path,
        Err(error) => return error,
    };
    if let Some(parent) = full_path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return format!("Tool error: {error}");
        }
    }
    match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&full_path)
    {
        Ok(mut file) => match std::io::Write::write_all(&mut file, content.as_bytes()) {
            Ok(_) => format!("Appended {} bytes to {}", content.len(), full_path.display()),
            Err(error) => format!("Tool error: {error}"),
        },
        Err(error) => format!("Tool error: {error}"),
    }
}

fn execute_replace_in_file_tool(
    working_directory: &str,
    path: &str,
    from: &str,
    to: &str,
) -> String {
    let full_path = match resolve_agent_relative_path(working_directory, path) {
        Ok(path) => path,
        Err(error) => return error,
    };
    let contents = match fs::read_to_string(&full_path) {
        Ok(contents) => contents,
        Err(error) => return format!("Tool error: {error}"),
    };
    if !contents.contains(from) {
        return "Tool error: target text not found in file.".to_owned();
    }
    let replaced = contents.replacen(from, to, 1);
    match fs::write(&full_path, replaced) {
        Ok(_) => format!("Replaced text in {}", full_path.display()),
        Err(error) => format!("Tool error: {error}"),
    }
}

fn execute_list_dir_tool(working_directory: &str, path: Option<&str>) -> String {
    let full_path = match path {
        Some(path) => match resolve_agent_relative_path(working_directory, path) {
            Ok(path) => path,
            Err(error) => return error,
        },
        None => PathBuf::from(working_directory),
    };
    let entries = match fs::read_dir(&full_path) {
        Ok(entries) => entries,
        Err(error) => return format!("Tool error: {error}"),
    };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = if path.is_dir() { "dir" } else { "file" };
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("<invalid>");
        rows.push(format!("{file_type}\t{name}"));
    }
    rows.sort();
    if rows.is_empty() {
        "Directory is empty.".to_owned()
    } else {
        truncate_agent_tool_output(rows.join("\n"))
    }
}

fn execute_mkdir_tool(working_directory: &str, path: &str) -> String {
    let full_path = match resolve_agent_relative_path(working_directory, path) {
        Ok(path) => path,
        Err(error) => return error,
    };
    match fs::create_dir_all(&full_path) {
        Ok(_) => format!("Created directory {}", full_path.display()),
        Err(error) => format!("Tool error: {error}"),
    }
}

fn execute_rename_path_tool(working_directory: &str, path: &str, new_path: &str) -> String {
    let from_path = match resolve_agent_relative_path(working_directory, path) {
        Ok(path) => path,
        Err(error) => return error,
    };
    let to_path = match resolve_agent_relative_path(working_directory, new_path) {
        Ok(path) => path,
        Err(error) => return error,
    };
    if let Some(parent) = to_path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return format!("Tool error: {error}");
        }
    }
    match fs::rename(&from_path, &to_path) {
        Ok(_) => format!("Moved {} to {}", from_path.display(), to_path.display()),
        Err(error) => format!("Tool error: {error}"),
    }
}

fn execute_delete_path_tool(working_directory: &str, path: &str) -> String {
    let full_path = match resolve_agent_relative_path(working_directory, path) {
        Ok(path) => path,
        Err(error) => return error,
    };
    let result = if full_path.is_dir() {
        fs::remove_dir_all(&full_path)
    } else {
        fs::remove_file(&full_path)
    };
    match result {
        Ok(_) => format!("Deleted {}", full_path.display()),
        Err(error) => format!("Tool error: {error}"),
    }
}

fn execute_file_info_tool(working_directory: &str, path: &str) -> String {
    let full_path = match resolve_agent_relative_path(working_directory, path) {
        Ok(path) => path,
        Err(error) => return error,
    };
    match fs::metadata(&full_path) {
        Ok(metadata) => format!(
            "path: {}\ntype: {}\nsize: {}\nreadonly: {}",
            full_path.display(),
            if metadata.is_dir() { "dir" } else { "file" },
            metadata.len(),
            metadata.permissions().readonly()
        ),
        Err(error) => format!("Tool error: {error}"),
    }
}

fn execute_git_status_tool(working_directory: &str) -> String {
    truncate_agent_tool_output(
        command_stdout(&["git", "-C", working_directory, "status", "--short", "--branch"])
            .unwrap_or_else(|| "Tool error: unable to run git status.".to_owned()),
    )
}

fn execute_git_diff_tool(working_directory: &str, path: Option<&str>) -> String {
    let mut args = vec!["git", "-C", working_directory, "diff", "--no-color"];
    let owned_path;
    if let Some(path) = path {
        owned_path = path.to_owned();
        args.push("--");
        args.push(&owned_path);
    }
    truncate_agent_tool_output(
        command_stdout(&args).unwrap_or_else(|| "Tool error: unable to run git diff.".to_owned()),
    )
}

fn resolve_agent_relative_path(working_directory: &str, path: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err("Tool error: absolute paths are not allowed.".to_owned());
    }
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir | std::path::Component::Prefix(_) | std::path::Component::RootDir))
    {
        return Err("Tool error: paths must stay inside the current working directory.".to_owned());
    }
    Ok(Path::new(working_directory).join(candidate))
}

fn truncate_agent_tool_output(mut text: String) -> String {
    if text.len() > 24_000 {
        text.truncate(24_000);
        text.push_str("\n...<truncated>...");
    }
    text
}

fn load_groq_api_key() -> Option<String> {
    if let Some(value) = load_key_from_local_env_file(LOCAL_ENV_FILE, GROQ_API_KEY_ENV) {
        return Some(value);
    }

    std::env::var(GROQ_API_KEY_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn load_key_from_local_env_file(path: &str, key: &str) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (candidate_key, candidate_value) = line.split_once('=')?;
        if candidate_key.trim() != key {
            continue;
        }

        let value = candidate_value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_owned();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

#[derive(Serialize)]
struct GroqChatRequest {
    model: String,
    messages: Vec<GroqMessage>,
}

#[derive(Clone, Serialize, Deserialize)]
struct GroqMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct GroqChatResponse {
    choices: Vec<GroqChoice>,
}

#[derive(Deserialize)]
struct GroqChoice {
    message: GroqMessage,
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

fn display_tab_path(directory: &str) -> String {
    directory.replace('/', "\\")
}

fn trailing_tab_path_for_width(ui: &egui::Ui, directory: &str, font: &FontId, max_width: f32) -> String {
    let normalized = display_tab_path(directory);
    if text_width(ui, &normalized, font) <= max_width {
        return normalized;
    }

    let parts: Vec<&str> = normalized
        .split('\\')
        .filter(|part| !part.is_empty())
        .collect();
    let mut suffix = String::new();

    for part in parts.iter().rev() {
        let candidate = if suffix.is_empty() {
            (*part).to_owned()
        } else {
            format!("{part}\\{suffix}")
        };
        let faded_candidate = format!("...\\{candidate}");
        if text_width(ui, &faded_candidate, font) > max_width {
            break;
        }
        suffix = candidate;
    }

    if suffix.is_empty() {
        fit_text_from_end(ui, &normalized, font, max_width)
    } else if suffix == normalized {
        suffix
    } else {
        format!("...\\{suffix}")
    }
}

fn fit_text_from_end(ui: &egui::Ui, text: &str, font: &FontId, max_width: f32) -> String {
    let mut suffix = String::new();
    for ch in text.chars().rev() {
        let candidate = format!("{ch}{suffix}");
        if text_width(ui, &format!("...{candidate}"), font) > max_width {
            break;
        }
        suffix = candidate;
    }
    format!("...{suffix}")
}

fn text_width(ui: &egui::Ui, text: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
}

fn paint_tab_path(ui: &egui::Ui, rect: egui::Rect, directory: &str) {
    let font = FontId::proportional(9.5);
    let shown_path = trailing_tab_path_for_width(ui, directory, &font, rect.width().max(24.0));
    let path_color = color(138, 142, 149);
    let fade_color = Color32::from_rgba_unmultiplied(path_color.r(), path_color.g(), path_color.b(), 72);

    if let Some(rest) = shown_path.strip_prefix("...\\") {
        let fade = "...\\";
        ui.painter().text(
            rect.min,
            Align2::LEFT_TOP,
            fade,
            font.clone(),
            fade_color,
        );
        let fade_width = text_width(ui, fade, &font);
        ui.painter().text(
            rect.min + Vec2::new(fade_width, 0.0),
            Align2::LEFT_TOP,
            rest,
            font,
            path_color,
        );
    } else if let Some(rest) = shown_path.strip_prefix("...") {
        let fade = "...";
        ui.painter().text(
            rect.min,
            Align2::LEFT_TOP,
            fade,
            font.clone(),
            fade_color,
        );
        let fade_width = text_width(ui, fade, &font);
        ui.painter().text(
            rect.min + Vec2::new(fade_width, 0.0),
            Align2::LEFT_TOP,
            rest,
            font,
            path_color,
        );
    } else {
        ui.painter().text(
            rect.min,
            Align2::LEFT_TOP,
            shown_path,
            font,
            path_color,
        );
    }
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

fn tab_card(ui: &mut egui::Ui, tab: &SearchTab, selected: bool) -> TabCardOutput {
    let desired = Vec2::new(ui.available_width(), 54.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let card_rect = rect;
    let fill = if selected {
        color(58, 58, 61)
    } else {
        color(34, 34, 36)
    };
    let stroke = Stroke::new(1.0, if selected { color(68, 68, 72) } else { color(34, 34, 36) });
    let hovered = response.hovered();

    ui.painter().rect(
        card_rect,
        CornerRadius::ZERO,
        fill,
        stroke,
        StrokeKind::Outside,
    );
    let icon_rect = egui::Rect::from_min_size(
        card_rect.min + Vec2::new(10.0, 9.0),
        Vec2::new(16.0, 16.0),
    );
    paint_tab_icon(ui.painter(), icon_rect, tab.icon);
    ui.painter().text(
        card_rect.min + Vec2::new(34.0, 6.0),
        Align2::LEFT_TOP,
        &tab.title,
        FontId::proportional(12.0),
        color(243, 243, 244),
    );
    let title_width = ui
        .painter()
        .layout_no_wrap(
            tab.title.clone(),
            FontId::proportional(12.0),
            color(243, 243, 244),
        )
        .size()
        .x;
    let git_badge_rect = egui::Rect::from_min_size(
        card_rect.min + Vec2::new(34.0 + title_width + 8.0, 7.0),
        Vec2::new(12.0, 12.0),
    );
    paint_branch_badge_icon(ui.painter(), git_badge_rect, color(232, 234, 237));
    ui.painter().text(
        card_rect.min + Vec2::new(34.0 + title_width + 22.0, 6.0),
        Align2::LEFT_TOP,
        &tab.branch,
        FontId::proportional(9.5),
        color(154, 158, 165),
    );
    let path_rect = egui::Rect::from_min_size(
        card_rect.min + Vec2::new(10.0, 24.0),
        Vec2::new((card_rect.width() - 20.0).max(24.0), 14.0),
    );
    paint_tab_path(ui, path_rect, &tab.directory);

    let close_rect = egui::Rect::from_min_size(
        egui::pos2(card_rect.right() - 28.0, card_rect.top() + 5.0),
        Vec2::new(22.0, 22.0),
    );
    let close_response = ui.interact(close_rect, response.id.with("close"), egui::Sense::click());
    if hovered {
        paint_tab_hover_button(ui.painter(), close_rect, "x", close_response.hovered());
    }

    TabCardOutput {
        response: response.on_hover_text(display_tab_path(&tab.directory)),
        close_clicked: close_response.clicked(),
    }
}

fn paint_tab_icon(painter: &egui::Painter, rect: egui::Rect, icon: TabIcon) {
    match icon.kind {
        TabIconKind::DefaultTerminal => {
            painter.circle_filled(rect.center(), rect.width() * 0.5, color(24, 24, 26));
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "*",
                FontId::proportional(8.5),
                color(239, 240, 242),
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
                FontId::proportional(7.5),
                foreground,
            );
        }
    }
}

fn paint_tab_hover_button(
    painter: &egui::Painter,
    rect: egui::Rect,
    label: &str,
    hovered: bool,
) {
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.0),
        if hovered { color(236, 238, 241) } else { color(125, 129, 136) },
    );
}

fn sidebar_empty_state(ui: &mut egui::Ui, query: &str) {
    Frame::new()
        .fill(color(26, 28, 33))
        .corner_radius(CornerRadius::ZERO)
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
