mod config;
mod state;
mod commands;
mod logcat;
mod watcher;
mod stats;

use anyhow::Result;
use config::Config;
use state::AppState;
use logcat::LogcatManager;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, MouseEventKind, EnableMouseCapture, DisableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap, Sparkline, Clear, Tabs},
    Terminal,
};
use std::io::stdout;
use tokio::sync::mpsc;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

use state::{AppMode, LogLevel, Tab};
use commands::BuildEvent;

struct App {
    config: Config,
    state: AppState,
    logs: VecDeque<String>,
    cache_all: Vec<String>,
    cache_app: Vec<String>,
    cache_build: Vec<String>,
    cache_err: Vec<String>,
}

impl App {
    fn new(config: Config, state: AppState) -> Self {
        Self {
            config,
            state,
            logs: VecDeque::with_capacity(5000),
            cache_all: Vec::new(),
            cache_app: Vec::new(),
            cache_build: Vec::new(),
            cache_err: Vec::new(),
        }
    }

    fn add_log(&mut self, log: String) {
        if self.logs.len() >= 5000 {
            self.logs.pop_front();
        }
        
        let l = log.trim().to_string();

        // 1. Crash Capture
        if l.contains("FATAL EXCEPTION") || l.contains("AndroidRuntime:E") {
            self.state.last_crash = Some(l.clone());
            self.state.last_crash_trace = Some(l.clone());
            self.state.is_capturing_crash = true;
        } else if self.state.is_capturing_crash {
            if l.starts_with("at ") || l.starts_with("\tat ") || l.contains("Caused by:") {
                if let Some(ref mut trace) = self.state.last_crash_trace {
                    trace.push('\n');
                    trace.push_str(&l);
                }
            } else {
                // End of crash trace
                self.state.is_capturing_crash = false;
                if let Some(ref trace) = self.state.last_crash_trace {
                    let _ = std::fs::write("crash_report.txt", trace);
                }
            }
        }

        // 2. Structured App Logging (Expo-style)
        let mut logs_to_add = Vec::new();
        if l.contains("[DeckDriod]") {
            if let Some(json_start) = l.find('{') {
                let json_part = &l[json_start..];
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_part) {
                    if let Ok(pretty) = serde_json::to_string_pretty(&value) {
                        logs_to_add.push("--- App State ---".to_string());
                        for line in pretty.lines() {
                            logs_to_add.push(format!("  {}", line));
                        }
                        logs_to_add.push("-----------------".to_string());
                    }
                }
            }
        }

        if logs_to_add.is_empty() {
            logs_to_add.push(l);
        }

        for entry in logs_to_add {
            self.logs.push_back(entry.clone());
            if self.matches_filter(&entry) {
                self.cache_all.push(entry.clone());
                if self.cache_all.len() > 5000 { self.cache_all.remove(0); }
                
                if entry.contains("[build]") || entry.contains("[build-err]") {
                    self.cache_build.push(entry.clone());
                    if self.cache_build.len() > 5000 { self.cache_build.remove(0); }
                } else {
                    self.cache_app.push(entry.clone());
                    if self.cache_app.len() > 5000 { self.cache_app.remove(0); }
                }

                if entry.contains(" E/") || entry.contains("[err]") || entry.contains("[build-err]") || entry.contains("FATAL") {
                    self.cache_err.push(entry.clone());
                    if self.cache_err.len() > 5000 { self.cache_err.remove(0); }
                }
            }
        }

        if self.state.autoscroll {
            self.state.log_scroll = self.current_log_len().saturating_sub(1);
        }
    }

    fn current_log_len(&self) -> usize {
        match self.state.current_tab {
            Tab::Dashboard => self.cache_all.len(),
            Tab::App => self.cache_app.len(),
            Tab::Build => self.cache_build.len(),
            Tab::Errors => self.cache_err.len(),
        }
    }

    fn matches_filter(&self, log: &str) -> bool {
        // Tag filter
        if !self.config.log_tag.is_empty() {
            if !log.contains(&self.config.log_tag) {
                return false;
            }
        }

        if let Some(level) = LogLevel::from_str(log) {
            if (level as u8) < (self.state.min_log_level as u8) {
                return false;
            }
        }
        if !self.state.search_query.is_empty() {
            if !log.to_lowercase().contains(&self.state.search_query.to_lowercase()) {
                return false;
            }
        }
        true
    }

    fn refresh_filter_cache(&mut self) {
        self.cache_all.clear();
        self.cache_app.clear();
        self.cache_build.clear();
        self.cache_err.clear();

        for l in &self.logs {
            if self.matches_filter(l) {
                self.cache_all.push(l.clone());
                if l.contains("[build]") || l.contains("[build-err]") {
                    self.cache_build.push(l.clone());
                } else {
                    self.cache_app.push(l.clone());
                }
                if l.contains(" E/") || l.contains("[err]") || l.contains("[build-err]") || l.contains("FATAL") {
                    self.cache_err.push(l.clone());
                }
            }
        }
        
        if self.state.autoscroll {
            let current_cache_len = self.current_log_len();
            self.state.log_scroll = current_cache_len.saturating_sub(1);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "-v" | "--version" => { println!("deckdriod v{}", env!("CARGO_PKG_VERSION")); return Ok(()); }
            "update" => {
                println!("Updating deckdriod...");
                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg("set -o pipefail; curl -sSf https://raw.githubusercontent.com/theasmat/deckdriod/master/install.sh | sh")
                    .status()?;
                if status.success() { println!("\n✅ Update successful!"); } 
                else { eprintln!("\n❌ Update failed. Check your internet connection."); std::process::exit(1); }
                return Ok(());
            }
            _ => {}
        }
    }

    let config = Config::load();
    let mut state = AppState::default();

    if !std::path::Path::new(".deckdriodconfig").exists() {
        state.mode = AppMode::Welcome;
    }

    let devices = commands::get_devices().await?;
    if devices.is_empty() {
        anyhow::bail!("no devices connected");
    } else if devices.len() == 1 {
        state.device_serial = Some(devices[0].clone());
    } else {
        println!("Multiple devices detected. Please select one:");
        for (i, dev) in devices.iter().enumerate() { println!("{}: {}", i + 1, dev); }
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let idx: usize = input.trim().parse().unwrap_or(1);
        state.device_serial = Some(devices.get(idx - 1).unwrap_or(&devices[0]).clone());
    }

    let serial = state.device_serial.as_ref().unwrap().clone();

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx_log, mut rx_log) = mpsc::unbounded_channel();
    let (tx_watch, mut rx_watch) = mpsc::channel(100);
    let (tx_stats, mut rx_stats) = mpsc::unbounded_channel();
    let (tx_build, mut rx_build) = mpsc::unbounded_channel();
    
    let mut app = App::new(config, state);
    app.refresh_filter_cache();

    let _watcher = watcher::start_watcher(app.config.watch_latency, tx_watch)?;

    let stats_serial = serial.clone();
    let stats_app_id = app.config.app_id.clone();
    tokio::spawn(stats::start_stats_polling(stats_serial, stats_app_id, tx_stats));

    let mut log_manager = LogcatManager::new();
    if app.state.show_logs {
        log_manager.start(&serial, &app.config.app_id, tx_log.clone()).await?;
    }

    let build_tx = tx_log.clone();
    let build_evt_tx = tx_build.clone();
    let cfg_bg = app.config.clone();
    let st_bg = app.state.clone();
    tokio::spawn(async move {
        let _ = commands::build_and_launch(&cfg_bg, &st_bg, build_tx, build_evt_tx).await;
    });

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let recorder = Arc::new(Mutex::new(commands::Recorder::new()));
    let mut last_draw = std::time::Instant::now();

    loop {
        if last_draw.elapsed() >= std::time::Duration::from_millis(33) {
            terminal.draw(|f| ui(f, &mut app))?;
            last_draw = std::time::Instant::now();
        }

        tokio::select! {
            _ = interval.tick() => {
                if app.state.show_logs && !log_manager.check_status().await {
                    let _ = log_manager.start(&serial, &app.config.app_id, tx_log.clone()).await;
                }
            }
            Some(log) = rx_log.recv() => { app.add_log(log); }
            Some(update) = rx_stats.recv() => {
                app.state.stats.last_cpu = update.cpu;
                app.state.stats.last_mem = update.mem;
                app.state.stats.battery_level = update.battery;
                if update.cpu > 0.0 {
                    app.state.stats.cpu_usage.push_back(update.cpu);
                    if app.state.stats.cpu_usage.len() > 100 { app.state.stats.cpu_usage.pop_front(); }
                }
                if update.mem > 0.0 {
                    app.state.stats.mem_usage.push_back(update.mem);
                    if app.state.stats.mem_usage.len() > 100 { app.state.stats.mem_usage.pop_front(); }
                }
            }
            Some(evt) = rx_build.recv() => {
                match evt {
                    BuildEvent::Task(t) => { app.state.build_task = Some(t); }
                    BuildEvent::Complete(d) => {
                        app.state.build_task = None;
                        app.state.build_history.push_back(d);
                        if app.state.build_history.len() > 5 { app.state.build_history.pop_front(); }
                        
                        // Smooth transition: switch to App tab on successful build
                        app.state.current_tab = Tab::App;
                        app.state.autoscroll = true;
                        app.refresh_filter_cache();
                    }
                    BuildEvent::Failed => { app.state.build_task = None; }
                }
            }
            Some(_) = rx_watch.recv() => {
                if app.state.auto_rebuild {
                    let now = std::time::Instant::now();
                    let should_build = match app.state.last_rebuild_at {
                        Some(last) => now.duration_since(last).as_secs_f64() >= app.config.rebuild_gap,
                        None => true,
                    };
                    if should_build {
                        app.state.last_rebuild_at = Some(now);
                        
                        // Switch to build tab for auto-rebuild too
                        app.state.current_tab = Tab::Build;
                        app.state.autoscroll = true;
                        app.refresh_filter_cache();

                        let build_tx = tx_log.clone();
                        let build_evt_tx = tx_build.clone();
                        let cfg = app.config.clone();
                        let st = app.state.clone();
                        tokio::spawn(async move {
                            let _ = commands::build_and_launch(&cfg, &st, build_tx, build_evt_tx).await;
                        });
                    } else {
                        let remaining = app.config.rebuild_gap - now.duration_since(app.state.last_rebuild_at.unwrap()).as_secs_f64();
                        let _ = tx_log.send(format!("[info] change detected, waiting for build gap ({:.1}s remaining)", remaining));
                    }
                }
            }
            res = tokio::task::spawn_blocking(|| event::poll(std::time::Duration::from_millis(10))) => {
                if let Ok(Ok(true)) = res {
                    let ev = event::read()?;
                    if let Event::Mouse(mouse) = ev {
                        match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                app.state.autoscroll = false;
                                app.state.log_scroll = app.state.log_scroll.saturating_sub(1);
                                app.state.selection_start = None;
                            }
                            MouseEventKind::ScrollDown => {
                                app.state.autoscroll = false;
                                app.state.log_scroll = app.state.log_scroll.saturating_add(1);
                                app.state.selection_start = None;
                            }
                            MouseEventKind::Down(_) => {
                                let log_area_top = if app.state.mode == AppMode::Normal { 5 } else { 8 };
                                if mouse.row >= log_area_top {
                                    app.state.selection_start = Some(app.state.log_scroll + (mouse.row.saturating_sub(log_area_top)) as usize);
                                    app.state.selection_end = app.state.selection_start;
                                } else {
                                    app.state.selection_start = None;
                                }
                            }
                            MouseEventKind::Drag(_) => {
                                let log_area_top = if app.state.mode == AppMode::Normal { 5 } else { 8 };
                                let term_height = terminal.size().unwrap_or(ratatui::layout::Size::new(0, 80)).height;
                                let log_area_bottom = term_height.saturating_sub(2);

                                if let Some(_start) = app.state.selection_start {
                                    let current_row_idx = app.state.log_scroll + (mouse.row.saturating_sub(log_area_top)) as usize;
                                    app.state.selection_end = Some(current_row_idx);

                                    // Boundary Auto-scroll
                                    if mouse.row <= log_area_top + 1 {
                                        app.state.log_scroll = app.state.log_scroll.saturating_sub(1);
                                        app.state.autoscroll = false;
                                    } else if mouse.row >= log_area_bottom {
                                        let max_scroll = app.current_log_len().saturating_sub(1);
                                        if app.state.log_scroll < max_scroll {
                                            app.state.log_scroll += 1;
                                            app.state.autoscroll = false;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    if let Event::Key(key) = ev {
                        match app.state.mode {
                            AppMode::Normal => {
                                match (key.code, key.modifiers) {
                                    (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                                    (KeyCode::Char('r'), _) | (KeyCode::Char('a'), _) | (KeyCode::Enter, _) => {
                                        app.state.last_rebuild_at = Some(std::time::Instant::now());
                                        
                                        // Switch to build tab and enable autoscroll
                                        app.state.current_tab = Tab::Build;
                                        app.state.autoscroll = true;
                                        app.refresh_filter_cache();

                                        let build_tx = tx_log.clone();
                                        let build_evt_tx = tx_build.clone();
                                        let cfg = app.config.clone();
                                        let st = app.state.clone();
                                        tokio::spawn(async move {
                                            let _ = commands::build_and_launch(&cfg, &st, build_tx, build_evt_tx).await;
                                        });
                                    }
                                    (KeyCode::Char('c'), _) => {
                                        app.logs.clear(); app.cache_all.clear(); app.cache_app.clear(); app.cache_build.clear(); app.cache_err.clear();
                                        app.state.last_crash = None; app.state.search_query.clear();
                                        app.state.log_scroll = 0;
                                        log_manager.start(&serial, &app.config.app_id, tx_log.clone()).await?;
                                    }
                                    (KeyCode::Char('h'), _) => { app.state.mode = AppMode::Help; }
                                    (KeyCode::Char('i'), _) => { app.state.mode = AppMode::Settings; app.state.settings_index = 0; }
                                    (KeyCode::Char('w'), _) => { app.state.auto_rebuild = !app.state.auto_rebuild; }
                                    (KeyCode::Char('o'), _) => { app.state.auto_open = !app.state.auto_open; }
                                    (KeyCode::Char('l'), _) => {
                                        app.state.show_logs = !app.state.show_logs;
                                        if app.state.show_logs { log_manager.start(&serial, &app.config.app_id, tx_log.clone()).await?; } else { log_manager.stop(); }
                                    }
                                    (KeyCode::Char('L'), _) => {
                                        app.state.current_tab = Tab::App;
                                        app.state.autoscroll = true;
                                        app.refresh_filter_cache();
                                        let log_tx = tx_log.clone();
                                        let cfg = app.config.clone();
                                        let st = app.state.clone();
                                        tokio::spawn(async move { let _ = commands::launch_app(&cfg, &st, log_tx).await; });
                                    }
                                    (KeyCode::Char('s'), _) => {
                                        let log_tx = tx_log.clone();
                                        let cfg = app.config.clone();
                                        let st = app.state.clone();
                                        tokio::spawn(async move { let _ = commands::take_screenshot(&cfg, &st, log_tx).await; });
                                    }
                                    (KeyCode::Char('v'), _) => {
                                        let rec = Arc::clone(&recorder);
                                        let s = serial.clone();
                                        let log_tx = tx_log.clone();
                                        let cfg = app.config.clone();
                                        if app.state.is_recording {
                                            app.state.is_recording = false;
                                            tokio::spawn(async move {
                                                let mut r = rec.lock().await;
                                                let _ = r.stop(&cfg, &s, log_tx).await;
                                            });
                                        } else {
                                            app.state.is_recording = true;
                                            tokio::spawn(async move {
                                                let mut r = rec.lock().await;
                                                if let Ok(_) = r.start(&s).await {
                                                    let _ = log_tx.send("[info] recording started...".to_string());
                                                }
                                            });
                                        }
                                    }
                                    (KeyCode::Char('b'), _) => { let _ = commands::toggle_layout_bounds(&mut app.state).await; }
                                    (KeyCode::Char('u'), _) => { app.state.mode = AppMode::DeepLink; app.state.input_buffer.clear(); }
                                    (KeyCode::Char('x'), _) => {
                                        let log_tx = tx_log.clone();
                                        let cfg = app.config.clone();
                                        let st = app.state.clone();
                                        tokio::spawn(async move { let _ = commands::clear_app_data(&cfg, &st, log_tx).await; });
                                    }
                                    (KeyCode::Char('d'), _) => {
                                        let s = serial.clone();
                                        tokio::spawn(async move {
                                            let _ = tokio::process::Command::new("adb").args(["-s", &s, "shell", "input", "keyevent", "82"]).status().await;
                                        });
                                    }
                                    (KeyCode::Char('/'), _) => { app.state.mode = AppMode::Search; app.state.input_buffer.clear(); }
                                    (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                                        app.state.autoscroll = false;
                                        app.state.log_scroll = app.state.log_scroll.saturating_sub(1);
                                    }
                                    (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                                        app.state.autoscroll = false;
                                        app.state.log_scroll = app.state.log_scroll.saturating_add(1);
                                    }
                                    (KeyCode::PageUp, _) => {
                                        app.state.autoscroll = false;
                                        app.state.log_scroll = app.state.log_scroll.saturating_sub(20);
                                    }
                                    (KeyCode::PageDown, _) => {
                                        app.state.autoscroll = false;
                                        app.state.log_scroll = app.state.log_scroll.saturating_add(20);
                                    }
                                    (KeyCode::Char('g'), _) => { app.state.autoscroll = false; app.state.log_scroll = 0; }
                                    (KeyCode::Char('G'), _) => { app.state.autoscroll = true; }
                                    (KeyCode::Char('y'), _) => {
                                        // Yanking the last visible line (top of the view)
                                        let current_cache = match app.state.current_tab {
                                            Tab::Dashboard => &app.cache_all, Tab::App => &app.cache_app,
                                            Tab::Build => &app.cache_build, Tab::Errors => &app.cache_err,
                                        };
                                        if let Some(line) = current_cache.get(app.state.log_scroll) {
                                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                                let _ = clipboard.set_text(line.clone());
                                                let _ = tx_log.send("[ok] top visible line yanked".to_string());
                                            }
                                        }
                                    }
                                    (KeyCode::Char('C'), _) => {
                                        if let Some(ref trace) = app.state.last_crash_trace {
                                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                                let _ = clipboard.set_text(trace.clone());
                                                let _ = tx_log.send("[ok] last crash trace yanked".to_string());
                                            }
                                        } else {
                                            let _ = tx_log.send("[warn] no crash trace available to yank".to_string());
                                        }
                                    }
                                    (KeyCode::Char('m'), _) => {
                                        app.state.mouse_captured = !app.state.mouse_captured;
                                        if app.state.mouse_captured {
                                            let _ = execute!(std::io::stdout(), EnableMouseCapture);
                                            let _ = tx_log.send("[info] mouse capture ON (smooth scrolling)".to_string());
                                        } else {
                                            let _ = execute!(std::io::stdout(), DisableMouseCapture);
                                            let _ = tx_log.send("[info] mouse capture OFF (native selection enabled)".to_string());
                                        }
                                    }
                                    (KeyCode::Char('A'), _) => {
                                        let current_cache = match app.state.current_tab {
                                            Tab::Dashboard => &app.cache_all, Tab::App => &app.cache_app,
                                            Tab::Build => &app.cache_build, Tab::Errors => &app.cache_err,
                                        };
                                        if !current_cache.is_empty() {
                                            let content = current_cache.join("\n");
                                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                                let _ = clipboard.set_text(content);
                                                let _ = tx_log.send("[ok] all visible logs yanked to clipboard".to_string());
                                            }
                                        }
                                    }

                                    (KeyCode::Tab, _) => {
                                        app.state.current_tab = match app.state.current_tab {
                                            Tab::Dashboard => Tab::App, Tab::App => Tab::Build,
                                            Tab::Build => Tab::Errors, Tab::Errors => Tab::Dashboard,
                                        };
                                        app.refresh_filter_cache();
                                    }
                                    (KeyCode::BackTab, _) => {
                                        app.state.current_tab = match app.state.current_tab {
                                            Tab::Dashboard => Tab::Errors, Tab::Errors => Tab::Build,
                                            Tab::Build => Tab::App, Tab::App => Tab::Dashboard,
                                        };
                                        app.refresh_filter_cache();
                                    }
                                    (KeyCode::Char('1'), m) if !m.contains(KeyModifiers::ALT) => { app.state.current_tab = Tab::Dashboard; app.refresh_filter_cache(); }
                                    (KeyCode::Char('2'), m) if !m.contains(KeyModifiers::ALT) => { app.state.current_tab = Tab::App; app.refresh_filter_cache(); }
                                    (KeyCode::Char('3'), m) if !m.contains(KeyModifiers::ALT) => { app.state.current_tab = Tab::Build; app.refresh_filter_cache(); }
                                    (KeyCode::Char('4'), m) if !m.contains(KeyModifiers::ALT) => { app.state.current_tab = Tab::Errors; app.refresh_filter_cache(); }

                                    (KeyCode::Char('1'), m) if m.contains(KeyModifiers::ALT) => { app.state.min_log_level = LogLevel::Verbose; app.refresh_filter_cache(); }
                                    (KeyCode::Char('2'), m) if m.contains(KeyModifiers::ALT) => { app.state.min_log_level = LogLevel::Debug; app.refresh_filter_cache(); }
                                    (KeyCode::Char('3'), m) if m.contains(KeyModifiers::ALT) => { app.state.min_log_level = LogLevel::Info; app.refresh_filter_cache(); }
                                    (KeyCode::Char('4'), m) if m.contains(KeyModifiers::ALT) => { app.state.min_log_level = LogLevel::Warn; app.refresh_filter_cache(); }
                                    (KeyCode::Char('5'), m) if m.contains(KeyModifiers::ALT) => { app.state.min_log_level = LogLevel::Error; app.refresh_filter_cache(); }

                                    (KeyCode::Char(c), _) => {
                                        if let Some(cmd_str) = app.config.custom_commands.get(&c.to_ascii_lowercase()) {
                                            let cmd = cmd_str.clone();
                                            tokio::spawn(async move { let _ = tokio::process::Command::new("sh").args(["-c", &cmd]).status().await; });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            AppMode::Search => {
                                match key.code {
                                    KeyCode::Enter => { app.state.search_query = app.state.input_buffer.clone(); app.refresh_filter_cache(); app.state.mode = AppMode::Normal; }
                                    KeyCode::Esc => { app.state.mode = AppMode::Normal; }
                                    KeyCode::Char(c) => { app.state.input_buffer.push(c); }
                                    KeyCode::Backspace => { app.state.input_buffer.pop(); }
                                    _ => {}
                                }
                            }
                            AppMode::DeepLink => {
                                match key.code {
                                    KeyCode::Enter => {
                                        let url = app.state.input_buffer.clone();
                                        let s = serial.clone();
                                        tokio::spawn(async move { let _ = tokio::process::Command::new("adb").args(["-s", &s, "shell", "am", "start", "-d", &url]).status().await; });
                                        app.state.mode = AppMode::Normal;
                                    }
                                    KeyCode::Esc => { app.state.mode = AppMode::Normal; }
                                    KeyCode::Char(c) => { app.state.input_buffer.push(c); }
                                    KeyCode::Backspace => { app.state.input_buffer.pop(); }
                                    _ => {}
                                }
                            }
                            AppMode::Input => {
                                 if key.code == KeyCode::Esc { app.state.mode = AppMode::Normal; }
                                 if key.code == KeyCode::Enter {
                                     let val = app.state.input_buffer.clone();
                                     match app.state.settings_index {
                                         0 => app.config.app_id = val,
                                         1 => app.config.activity = val,
                                         2 => if let Ok(v) = val.parse() { app.config.watch_latency = v; },
                                         3 => if let Ok(v) = val.parse() { app.config.rebuild_gap = v; },
                                         4 => { app.config.log_tag = val; app.refresh_filter_cache(); },
                                         5 => app.config.project_path = val,
                                         6 => app.config.output_path = val,
                                         _ => {}
                                     }
                                     let _ = app.config.save();
                                     app.state.mode = AppMode::Settings;
                                 }
                                 if let KeyCode::Char(c) = key.code { app.state.input_buffer.push(c); }
                                 if key.code == KeyCode::Backspace { app.state.input_buffer.pop(); }
                            }
                            AppMode::Help | AppMode::Welcome => {
                                 if key.code == KeyCode::Esc || key.code == KeyCode::Char('h') || key.code == KeyCode::Char('q') || key.code == KeyCode::Enter { 
                                     if app.state.mode == AppMode::Welcome { let _ = app.config.save(); }
                                     app.state.mode = AppMode::Normal; 
                                 }
                            }
                            AppMode::Settings => {
                                match key.code {
                                    KeyCode::Esc | KeyCode::Char('q') => app.state.mode = AppMode::Normal,
                                    KeyCode::Up | KeyCode::Char('k') => app.state.settings_index = app.state.settings_index.saturating_sub(1),
                                    KeyCode::Down | KeyCode::Char('j') => app.state.settings_index = (app.state.settings_index + 1).min(6),
                                    KeyCode::Enter => {
                                        app.state.mode = AppMode::Input; app.state.input_buffer.clear();
                                        match app.state.settings_index {
                                            0 => app.state.input_buffer = app.config.app_id.clone(),
                                            1 => app.state.input_buffer = app.config.activity.clone(),
                                            2 => app.state.input_buffer = format!("{:.1}", app.config.watch_latency),
                                            3 => app.state.input_buffer = format!("{:.1}", app.config.rebuild_gap),
                                            4 => app.state.input_buffer = app.config.log_tag.clone(),
                                            5 => app.state.input_buffer = app.config.project_path.clone(),
                                            6 => app.state.input_buffer = app.config.output_path.clone(),
                                            _ => {}
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    log_manager.stop();
    Ok(())
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn ui(f: &mut ratatui::Frame, app: &mut App) {
    let area = f.area();
    
    // Safety: Handle extremely small terminals
    if area.height < 5 || area.width < 10 {
        f.render_widget(Paragraph::new("Terminal too small").alignment(ratatui::layout::Alignment::Center), area);
        return;
    }

    let mut main_constraints = vec![
        Constraint::Length(2), // Dashboard info (Compact)
        Constraint::Length(3), // Tabs (Boxed)
        Constraint::Min(0),    // Main Content (Borderless Logs)
        Constraint::Length(1), // Footer (Compact)
    ];
    
    // Hide headers if screen is too short to show logs effectively
    if area.height < 15 {
        main_constraints.remove(0); // Hide info
    }
    if area.height < 10 {
        main_constraints.remove(0); // Hide tabs
    }

    if app.state.mode == AppMode::Search || app.state.mode == AppMode::Input || app.state.mode == AppMode::DeepLink {
        // Find the index to insert search bar (it should be before Main Content)
        let insert_idx = if main_constraints.len() == 4 { 2 } else if main_constraints.len() == 3 { 1 } else { 0 };
        main_constraints.insert(insert_idx, Constraint::Length(3));
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(main_constraints)
        .split(area);

    let mut current_idx = 0;

    // 1. COMPACT DASHBOARD INFO
    if area.height >= 15 {
        let w_status = if app.state.auto_rebuild { "ON".green() } else { "OFF".red() };
        let o_status = if app.state.auto_open { "ON".green() } else { "OFF".red() };
        let l_status = if app.state.show_logs { "ON".green() } else { "OFF".red() };
        let record_status = if app.state.is_recording { "REC".red().bold() } else { "OFF".dark_gray() };
        let mouse_status = if app.state.mouse_captured { "APP".cyan() } else { "NATIVE".yellow().bold() };
        let battery_span = match app.state.stats.battery_level {
            Some(l) => Span::styled(format!("BAT:{}%", l), if l < 20 { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) }),
            None => Span::raw("BAT:--%").dark_gray(),
        };

        let header_line = Line::from(vec![
            Span::styled(" DeckDriod ", Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)),
            Span::raw(format!("({}) ", app.state.device_serial.as_deref().unwrap_or("none"))),
            battery_span,
            Span::raw(" | Mouse:"), mouse_status,
            Span::raw(" | REC:"), record_status,
            Span::raw(" | Watch:"), w_status,
            Span::raw(" Open:"), o_status,
            Span::raw(" Logs:"), l_status,
            Span::raw(format!(" | App:{}", app.config.app_id)).dark_gray(),
        ]);
        f.render_widget(Paragraph::new(header_line), chunks[current_idx]);
        current_idx += 1;
    }

    // 2. BOXED TABS
    if area.height >= 10 {
        let tab_titles = vec![" [1] Dashboard ", " [2] App Logs ", " [3] Build ", " [4] Errors "];
        let tabs = Tabs::new(tab_titles)
            .block(Block::default().borders(Borders::ALL))
            .select(match app.state.current_tab {
                Tab::Dashboard => 0, Tab::App => 1,
                Tab::Build => 2, Tab::Errors => 3,
            })
            .style(Style::default().fg(Color::DarkGray))
            .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
        f.render_widget(tabs, chunks[current_idx]);
        current_idx += 1;
    }

    if app.state.mode == AppMode::Search || app.state.mode == AppMode::Input || app.state.mode == AppMode::DeepLink {
        let title = match app.state.mode { AppMode::Search => " Search Logs ", AppMode::DeepLink => " Deep Link URL ", _ => " Input " };
        f.render_widget(Paragraph::new(app.state.input_buffer.as_str()).block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(Color::Yellow))), chunks[current_idx]);
        current_idx += 1;
    }

    let main_area = chunks[current_idx];

    // 3. MAIN CONTENT (Tab specific)
    match app.state.current_tab {
        Tab::Dashboard => {
            let dash_constraints = if main_area.height > 10 {
                vec![Constraint::Length(7), Constraint::Min(0)]
            } else {
                vec![Constraint::Min(0)]
            };
            
            let dash_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(dash_constraints)
                .split(main_area);

            let stats_idx = if dash_chunks.len() > 1 {
                // Resource Stats (Sparklines)
                let cpu_data: Vec<u64> = app.state.stats.cpu_usage.iter().map(|&v| (v * 10.0) as u64).collect();
                let mem_data: Vec<u64> = app.state.stats.mem_usage.iter().map(|&v| (v * 10.0) as u64).collect();
                let stats_layout = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(dash_chunks[0]);
                
                let mut cpu_title = format!(" CPU: {:.1}% ", app.state.stats.last_cpu);
                let mem_title = format!(" MEM: {:.1}% ", app.state.stats.last_mem);
                if let Some(ref task) = app.state.build_task {
                    cpu_title = format!(" BUILD: {} ", task);
                }

                f.render_widget(Sparkline::default().block(Block::default().borders(Borders::ALL).title(cpu_title)).data(&cpu_data).style(Style::default().fg(Color::Green)), stats_layout[0]);
                f.render_widget(Sparkline::default().block(Block::default().borders(Borders::ALL).title(mem_title)).data(&mem_data).style(Style::default().fg(Color::Blue)), stats_layout[1]);
                1
            } else {
                0
            };

            // Commands List
            let commands_text = vec![
                Line::from(vec![Span::styled(" [a/r/Enter] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Build/Launch"), Span::styled("   [L] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Launch Only "), Span::styled("      [s] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Screenshot")]),
                Line::from(vec![Span::styled(" [v]         ", Style::default().fg(Color::Cyan).bold()), Span::raw("Record Video"), Span::styled("   [u] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Deep Link   "), Span::styled("      [b] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Toggle Bounds")]),
                Line::from(vec![Span::styled(" [x]         ", Style::default().fg(Color::Cyan).bold()), Span::raw("Clear Data  "), Span::styled("   [c] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Clear Logs  "), Span::styled("      [i] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Settings")]),
                Line::from(vec![Span::styled(" [/]         ", Style::default().fg(Color::Cyan).bold()), Span::raw("Search      "), Span::styled("   [m] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Mouse Toggle "), Span::styled("      [A] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Yank All Logs")]),
                Line::from(vec![Span::styled(" [y]         ", Style::default().fg(Color::Cyan).bold()), Span::raw("Yank Line   "), Span::styled("   [C] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Yank Crash   "), Span::styled("      [q] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Quit")]),
                Line::from(vec![Span::raw("")]),
                Line::from(vec![Span::styled(" Log Levels: ", Style::default().bold()), Span::raw("Alt + [1]Verbose [2]Debug [3]Info [4]Warn [5]Error")]),
                Line::from(vec![Span::styled(" Scrolling:  ", Style::default().bold()), Span::raw("Mouse Wheel, Up/Down, PageUp/Down, [G] Follow Bottom")]),
            ];
            
            if let Some(ref crash) = app.state.last_crash {
                f.render_widget(Paragraph::new(format!("⚠️ CRASH DETECTED: {}", crash)).style(Style::default().fg(Color::Red).bold()).block(Block::default().borders(Borders::ALL).title(" Alerts ")), dash_chunks[stats_idx]);
            } else {
                f.render_widget(Paragraph::new(commands_text).block(Block::default().borders(Borders::ALL).title(" Quick Commands ")), dash_chunks[stats_idx]);
            }
        }
        _ => {
            let logs_to_render = match app.state.current_tab {
                Tab::Dashboard => unreachable!(),
                Tab::App => &app.cache_app,
                Tab::Build => &app.cache_build,
                Tab::Errors => &app.cache_err,
            };

            // Optimization: Only process visible lines
            let height = main_area.height as usize;
            let total_lines = logs_to_render.len();
            
            // Clamp scroll to valid range
            if app.state.log_scroll >= total_lines && total_lines > 0 {
                app.state.log_scroll = total_lines.saturating_sub(1);
            }

            let start = app.state.log_scroll;
            let end = (start + height).min(total_lines);
            
            let visible_logs = if start < total_lines {
                &logs_to_render[start..end]
            } else {
                &[]
            };

            let log_lines: Vec<Line> = visible_logs.iter().enumerate().map(|(i, l)| {
                let current_idx = start + i;
                let mut style = if l.contains("[err]") || l.contains("[build-err]") || l.contains(" E/") { Style::default().fg(Color::Red) } 
                else if l.contains("[ok]") { Style::default().fg(Color::Green) } 
                else if l.contains("[build]") || l.contains(" W/") { Style::default().fg(Color::Yellow) } 
                else if l.contains(" I/") { Style::default().fg(Color::Cyan) } 
                else { Style::default() };

                // Apply Selection Highlight
                if let (Some(s), Some(e)) = (app.state.selection_start, app.state.selection_end) {
                    let min = s.min(e);
                    let max = s.max(e);
                    if current_idx >= min && current_idx <= max {
                        style = style.bg(Color::Blue).fg(Color::White);
                    }
                }

                if l.starts_with("at ") || l.starts_with("\tat ") || l.contains("...") {
                    style = Style::default().fg(Color::DarkGray);
                }

                Line::from(Span::styled(l, style))
            }).collect();

            f.render_widget(Paragraph::new(log_lines), main_area);
        }
    }

    // 4. FOOTER
    let scroll_status = if app.state.autoscroll { "FOLLOW".cyan() } else { format!("PAUSED (Line {})", app.state.log_scroll).yellow() };
    let mouse_mode = if app.state.mouse_captured { "APP".cyan() } else { "NATIVE".yellow() };
    let footer = Line::from(vec![
        Span::raw(" [Tab] Views | [m] Mouse:"), mouse_mode,
        Span::raw(" | "), scroll_status,
        Span::raw(" | [h] Advanced Help").dark_gray(),
    ]);
    f.render_widget(Paragraph::new(footer), chunks[chunks.len() - 1]);

    // Modal Overlays
    if app.state.mode == AppMode::Help || app.state.mode == AppMode::Welcome {
        let area = centered_rect(70, 75, f.area());
        f.render_widget(Clear, area);
        let title = if app.state.mode == AppMode::Welcome { " Welcome to DeckDriod! " } else { " Advanced Help " };
        let mut help_popup_text = vec![
            Line::from(vec![Span::styled("--- CLI Commands ---", Style::default().bold())]),
            Line::from(vec![Span::styled(" deckdriod -v      ", Style::default().fg(Color::Cyan)), Span::raw(": Show version info")]),
            Line::from(vec![Span::styled(" deckdriod update  ", Style::default().fg(Color::Cyan)), Span::raw(": Update to latest version")]),
            Line::from(vec![Span::raw("")]),
            Line::from(vec![Span::styled("--- Views ---", Style::default().bold())]),
            Line::from(vec![Span::styled(" Tab / 1-4 ", Style::default().fg(Color::Cyan)), Span::raw(": Switch between Dashboard, App, Build, Errors")]),
            Line::from(vec![Span::raw("")]),
            Line::from(vec![Span::styled("--- Controls ---", Style::default().bold())]),
            Line::from(vec![Span::styled(" a / r / Ent ", Style::default().fg(Color::Cyan)), Span::raw(": Build & Launch")]),
            Line::from(vec![Span::styled(" L           ", Style::default().fg(Color::Cyan)), Span::raw(": Launch Only (Skip Build)")]),
            Line::from(vec![Span::styled(" c           ", Style::default().fg(Color::Cyan)), Span::raw(": Clear Logs & Crash Alert")]),
            Line::from(vec![Span::styled(" i         ", Style::default().fg(Color::Cyan)), Span::raw(": Open Settings Menu")]),
            Line::from(vec![Span::styled(" Alt + 1-5 ", Style::default().fg(Color::Cyan)), Span::raw(": Set Min Log Level")]),
            Line::from(vec![Span::styled(" /         ", Style::default().fg(Color::Cyan)), Span::raw(": Search Logs")]),
            Line::from(vec![Span::styled(" k / j     ", Style::default().fg(Color::Cyan)), Span::raw(": Scroll Up/Down")]),
            Line::from(vec![Span::styled(" m         ", Style::default().fg(Color::Cyan)), Span::raw(": Toggle Mouse (Capture vs Native Selection)")]),
            Line::from(vec![Span::styled(" y         ", Style::default().fg(Color::Cyan)), Span::raw(": Yank (Copy) top line")]),
            Line::from(vec![Span::styled(" A         ", Style::default().fg(Color::Cyan)), Span::raw(": Yank (Copy) ALL visible logs")]),
            Line::from(vec![Span::styled(" C         ", Style::default().fg(Color::Cyan)), Span::raw(": Yank (Copy) last Crash Trace")]),

            Line::from(vec![Span::raw("")]),
            Line::from(vec![Span::styled(" Tip: ", Style::default().fg(Color::Yellow).bold()), Span::raw("Hold Option (Mac) or Shift (Linux) for native selection while in APP mouse mode.")]),
            Line::from(vec![Span::raw("")]),
            Line::from(vec![Span::styled(" Esc / h   ", Style::default().fg(Color::Cyan)), Span::raw(": Close Menu")]),
            Line::from(vec![Span::styled(" q         ", Style::default().fg(Color::Cyan)), Span::raw(": Quit")]),
        ];
        if app.state.mode == AppMode::Welcome {
            help_popup_text.insert(0, Line::from(vec![Span::styled("First run detected! Here are your available commands:", Style::default().fg(Color::Yellow))]));
            help_popup_text.insert(1, Line::from(vec![Span::raw("")]));
        }
        f.render_widget(Paragraph::new(help_popup_text).block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(Color::Cyan))).wrap(Wrap { trim: true }), area);
    }

    if app.state.mode == AppMode::Settings || (app.state.mode == AppMode::Input && app.state.settings_index < 10) {
        let area = centered_rect(60, 50, f.area());
        f.render_widget(Clear, area);
        let watch_latency_str = format!("{:.1}", app.config.watch_latency);
        let rebuild_gap_str = format!("{:.1}", app.config.rebuild_gap);
        let settings = vec![
            ("App ID", &app.config.app_id),
            ("Main Activity", &app.config.activity),
            ("Watch Latency (s)", &watch_latency_str),
            ("Build Gap (s)", &rebuild_gap_str),
            ("Log Tag Filter", &app.config.log_tag),
            ("Project Path", &app.config.project_path),
            ("Output Path", &app.config.output_path),
        ];
        let items: Vec<ListItem> = settings.iter().enumerate().map(|(i, (label, val))| {
            let mut style = Style::default();
            if i == app.state.settings_index { style = style.fg(Color::Yellow).bold(); }
            ListItem::new(Line::from(vec![Span::styled(format!("{:<20}: ", label), style), Span::raw(*val)]))
        }).collect();
        f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).title(" Project Settings ").border_style(Style::default().fg(Color::Yellow))), area);
        if app.state.mode == AppMode::Input {
            let input_area = centered_rect(50, 10, area);
            f.render_widget(Clear, input_area);
            f.render_widget(Paragraph::new(app.state.input_buffer.as_str()).block(Block::default().borders(Borders::ALL).title(" Edit Value ").border_style(Style::default().fg(Color::Yellow))), input_area);
        }
    }
}
