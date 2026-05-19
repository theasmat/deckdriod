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
    layout::{Constraint, Direction, Layout, Rect, Alignment},
    style::{Color, Style, Stylize},
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
                self.state.is_capturing_crash = false;
                if let Some(ref trace) = self.state.last_crash_trace {
                    let _ = std::fs::write("crash_report.txt", trace);
                }
            }
        }

        // 2. Structured App Logging
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
                if l.contains("[build]") || l.contains("[build-err]") { self.cache_build.push(l.clone()); } 
                else { self.cache_app.push(l.clone()); }
                if l.contains(" E/") || l.contains("[err]") || l.contains("[build-err]") || l.contains(" FATAL") { self.cache_err.push(l.clone()); }
            }
        }
        if self.state.autoscroll {
            let current_cache_len = self.current_log_len();
            self.state.log_scroll = current_cache_len.saturating_sub(1);
        }
    }

    async fn get_target_serials(&self) -> Vec<String> {
        if self.state.is_broadcast {
            commands::get_devices().await.unwrap_or_default()
        } else {
            self.state.device_serial.as_ref().map(|s| vec![s.clone()]).unwrap_or_default()
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

    let devices = commands::get_devices().await.unwrap_or_default();
    if devices.is_empty() {
        let avds = commands::get_avds().await.unwrap_or_default();
        if avds.is_empty() { state.mode = AppMode::NoHardwareHelp; } 
        else { state.available_avds = avds; state.mode = AppMode::EmulatorSelect; }
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

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx_log, mut rx_log) = mpsc::unbounded_channel();
    let (tx_watch, mut rx_watch) = mpsc::channel(100);
    let (tx_stats, mut rx_stats) = mpsc::unbounded_channel();
    let (tx_build, mut rx_build) = mpsc::unbounded_channel();
    
    let mut app = App::new(config, state);
    app.refresh_filter_cache();

    let _watcher = watcher::start_watcher(app.config.watch_latency, tx_watch)?;

    let mut log_manager = LogcatManager::new();
    let recorder = Arc::new(Mutex::new(commands::Recorder::new()));

    if let Some(ref serial) = app.state.device_serial {
        let stats_serial = serial.clone();
        let stats_app_id = app.config.app_id.clone();
        tokio::spawn(stats::start_stats_polling(stats_serial, stats_app_id, tx_stats.clone()));
        if app.state.show_logs { let _ = log_manager.start(serial, &app.config.app_id, tx_log.clone()).await; }
        let build_tx = tx_log.clone();
        let build_evt_tx = tx_build.clone();
        let cfg_bg = app.config.clone();
        let serials = vec![serial.clone()];
        let auto_open = app.state.auto_open;
        tokio::spawn(async move { let _ = commands::build_and_launch(&cfg_bg, serials, auto_open, build_tx, build_evt_tx).await; });
    }

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let mut last_draw = std::time::Instant::now();

    loop {
        if last_draw.elapsed() >= std::time::Duration::from_millis(33) {
            terminal.draw(|f| ui(f, &mut app))?;
            last_draw = std::time::Instant::now();
        }

        tokio::select! {
            _ = interval.tick() => {
                if let Some(ref serial) = app.state.device_serial {
                    if app.state.show_logs && !log_manager.check_status().await {
                        let _ = log_manager.start(serial, &app.config.app_id, tx_log.clone()).await;
                    }
                } else {
                    if let Ok(devs) = commands::get_devices().await {
                        if !devs.is_empty() {
                            let serial = devs[0].clone();
                            app.state.device_serial = Some(serial.clone());
                            let stats_serial = serial.clone();
                            let stats_app_id = app.config.app_id.clone();
                            tokio::spawn(stats::start_stats_polling(stats_serial, stats_app_id, tx_stats.clone()));
                            if app.state.show_logs { let _ = log_manager.start(&serial, &app.config.app_id, tx_log.clone()).await; }
                        }
                    }
                }
            }
            Some(log) = rx_log.recv() => { app.add_log(log); }
            Some(update) = rx_stats.recv() => {
                app.state.stats.last_cpu = update.cpu;
                app.state.stats.last_mem = update.mem;
                app.state.stats.battery_level = update.battery;
                if update.cpu > 0.0 { app.state.stats.cpu_usage.push_back(update.cpu); if app.state.stats.cpu_usage.len() > 100 { app.state.stats.cpu_usage.pop_front(); } }
                if update.mem > 0.0 { app.state.stats.mem_usage.push_back(update.mem); if app.state.stats.mem_usage.len() > 100 { app.state.stats.mem_usage.pop_front(); } }
            }
            Some(evt) = rx_build.recv() => {
                match evt {
                    BuildEvent::Task(t) => { app.state.build_task = Some(t); }
                    BuildEvent::Complete(d) => {
                        app.state.build_task = None;
                        app.state.build_history.push_back(d);
                        if app.state.build_history.len() > 5 { app.state.build_history.pop_front(); }
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
                    let should_build = match app.state.last_rebuild_at { Some(last) => now.duration_since(last).as_secs_f64() >= app.config.rebuild_gap, None => true };
                    if should_build {
                        app.state.last_rebuild_at = Some(now);
                        app.state.current_tab = Tab::Build;
                        app.state.autoscroll = true;
                        app.refresh_filter_cache();
                        let build_tx = tx_log.clone();
                        let build_evt_tx = tx_build.clone();
                        let cfg = app.config.clone();
                        let serials = app.get_target_serials().await;
                        let auto_open = app.state.auto_open;
                        tokio::spawn(async move { let _ = commands::build_and_launch(&cfg, serials, auto_open, build_tx, build_evt_tx).await; });
                    }
                }
            }
            res = tokio::task::spawn_blocking(|| event::poll(std::time::Duration::from_millis(10))) => {
                if let Ok(Ok(true)) = res {
                    let ev = event::read()?;
                    if let Event::Mouse(mouse) = ev {
                        match mouse.kind {
                            MouseEventKind::ScrollUp => { app.state.autoscroll = false; app.state.log_scroll = app.state.log_scroll.saturating_sub(1); app.state.selection_start = None; }
                            MouseEventKind::ScrollDown => { app.state.autoscroll = false; app.state.log_scroll = app.state.log_scroll.saturating_add(1); app.state.selection_start = None; }
                            MouseEventKind::Down(_) => {
                                let log_area_top = if app.state.mode == AppMode::Normal { 5 } else { 8 };
                                if mouse.row >= log_area_top {
                                    app.state.selection_start = Some(app.state.log_scroll + (mouse.row.saturating_sub(log_area_top)) as usize);
                                    app.state.selection_end = app.state.selection_start;
                                } else { app.state.selection_start = None; }
                            }
                            MouseEventKind::Drag(_) => {
                                let log_area_top = if app.state.mode == AppMode::Normal { 5 } else { 8 };
                                let term_height = terminal.size().unwrap_or(ratatui::layout::Size::new(0, 80)).height;
                                let log_area_bottom = term_height.saturating_sub(2);
                                if let Some(_start) = app.state.selection_start {
                                    let current_row_idx = app.state.log_scroll + (mouse.row.saturating_sub(log_area_top)) as usize;
                                    app.state.selection_end = Some(current_row_idx);
                                    if mouse.row <= log_area_top + 1 { app.state.log_scroll = app.state.log_scroll.saturating_sub(1); app.state.autoscroll = false; } 
                                    else if mouse.row >= log_area_bottom { let max_scroll = app.current_log_len().saturating_sub(1); if app.state.log_scroll < max_scroll { app.state.log_scroll += 1; app.state.autoscroll = false; } }
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
                                        app.state.current_tab = Tab::Build;
                                        app.state.autoscroll = true;
                                        app.refresh_filter_cache();
                                        let build_tx = tx_log.clone();
                                        let build_evt_tx = tx_build.clone();
                                        let cfg = app.config.clone();
                                        let serials = app.get_target_serials().await;
                                        let auto_open = app.state.auto_open;
                                        tokio::spawn(async move { let _ = commands::build_and_launch(&cfg, serials, auto_open, build_tx, build_evt_tx).await; });
                                    }
                                    (KeyCode::Char('c'), _) => {
                                        app.logs.clear(); app.cache_all.clear(); app.cache_app.clear(); app.cache_build.clear(); app.cache_err.clear();
                                        app.state.last_crash = None; app.state.search_query.clear();
                                        app.state.log_scroll = 0;
                                        if let Some(ref serial) = app.state.device_serial { let _ = log_manager.start(serial, &app.config.app_id, tx_log.clone()).await; }
                                    }
                                    (KeyCode::Char('h'), _) => { app.state.mode = AppMode::Help; }
                                    (KeyCode::Char('i'), _) => { app.state.mode = AppMode::Settings; app.state.settings_index = 0; }
                                    (KeyCode::Char('w'), _) => { app.state.auto_rebuild = !app.state.auto_rebuild; }
                                    (KeyCode::Char('o'), _) => { app.state.auto_open = !app.state.auto_open; }
                                    (KeyCode::Char('l'), _) => {
                                        app.state.show_logs = !app.state.show_logs;
                                        if app.state.show_logs { if let Some(ref serial) = app.state.device_serial { let _ = log_manager.start(serial, &app.config.app_id, tx_log.clone()).await; } } 
                                        else { log_manager.stop(); }
                                    }
                                    (KeyCode::Char('L'), _) => {
                                        app.state.current_tab = Tab::App;
                                        app.state.autoscroll = true;
                                        app.refresh_filter_cache();
                                        let log_tx = tx_log.clone();
                                        let cfg = app.config.clone();
                                        let serials = app.get_target_serials().await;
                                        tokio::spawn(async move { let _ = commands::launch_app(&cfg, serials, log_tx).await; });
                                    }
                                    (KeyCode::Char('B'), _) => {
                                        app.state.is_broadcast = !app.state.is_broadcast;
                                        let msg = if app.state.is_broadcast { "[info] BROADCAST mode ON (all devices)" } else { "[info] BROADCAST mode OFF (selected device only)" };
                                        let _ = tx_log.send(msg.to_string());
                                    }
                                    (KeyCode::Char('E'), _) => {
                                        if let Ok(avds) = commands::get_avds().await {
                                            if !avds.is_empty() { app.state.available_avds = avds; app.state.mode = AppMode::EmulatorSelect; app.state.settings_index = 0; } 
                                            else { let _ = tx_log.send("[err] no emulators found".to_string()); }
                                        }
                                    }
                                    (KeyCode::Char('s'), _) => {
                                        let log_tx = tx_log.clone();
                                        let cfg = app.config.clone();
                                        let serials = app.get_target_serials().await;
                                        tokio::spawn(async move { let _ = commands::take_screenshot(&cfg, serials, log_tx).await; });
                                    }
                                    (KeyCode::Char('v'), _) => {
                                        let rec = Arc::clone(&recorder);
                                        let serials = app.get_target_serials().await;
                                        if !serials.is_empty() {
                                            let log_tx = tx_log.clone();
                                            let cfg = app.config.clone();
                                            if app.state.is_recording {
                                                app.state.is_recording = false;
                                                tokio::spawn(async move { let mut r = rec.lock().await; let _ = r.stop(&cfg, log_tx).await; });
                                            } else {
                                                app.state.is_recording = true;
                                                tokio::spawn(async move {
                                                    let mut r = rec.lock().await;
                                                    if let Ok(_) = r.start(serials).await { let _ = log_tx.send("[info] recording started...".to_string()); }
                                                });
                                            }
                                        }
                                    }
                                    (KeyCode::Char('b'), _) => {
                                        let serials = app.get_target_serials().await;
                                        app.state.show_layout_bounds = !app.state.show_layout_bounds;
                                        let show = app.state.show_layout_bounds;
                                        tokio::spawn(async move { let _ = commands::toggle_layout_bounds(serials, show).await; });
                                    }
                                    (KeyCode::Char('u'), _) => { app.state.mode = AppMode::DeepLink; app.state.input_buffer.clear(); }
                                    (KeyCode::Char('x'), _) => {
                                        let log_tx = tx_log.clone();
                                        let cfg = app.config.clone();
                                        let serials = app.get_target_serials().await;
                                        let auto_open = app.state.auto_open;
                                        tokio::spawn(async move { let _ = commands::clear_app_data(&cfg, serials, auto_open, log_tx).await; });
                                    }
                                    (KeyCode::Char('e'), _) => {
                                        let content = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
                                        let _ = std::fs::write("deckdriod_export.txt", content);
                                        let _ = tx_log.send("[ok] logs exported to deckdriod_export.txt".to_string());
                                    }
                                    (KeyCode::Char('d'), _) => {
                                        let serials = app.get_target_serials().await;
                                        tokio::spawn(async move {
                                            for s in serials { let _ = tokio::process::Command::new("adb").args(["-s", &s, "shell", "input", "keyevent", "82"]).status().await; }
                                        });
                                    }
                                    (KeyCode::Char('/'), _) => { app.state.mode = AppMode::Search; app.state.input_buffer.clear(); }
                                    (KeyCode::Up, _) | (KeyCode::Char('k'), _) => { app.state.autoscroll = false; app.state.log_scroll = app.state.log_scroll.saturating_sub(1); }
                                    (KeyCode::Down, _) | (KeyCode::Char('j'), _) => { app.state.autoscroll = false; app.state.log_scroll = app.state.log_scroll.saturating_add(1); }
                                    (KeyCode::PageUp, _) => { app.state.autoscroll = false; app.state.log_scroll = app.state.log_scroll.saturating_sub(20); }
                                    (KeyCode::PageDown, _) => { app.state.autoscroll = false; app.state.log_scroll = app.state.log_scroll.saturating_add(20); }
                                    (KeyCode::Char('g'), _) => { app.state.autoscroll = false; app.state.log_scroll = 0; }
                                    (KeyCode::Char('G'), _) => { app.state.autoscroll = true; }
                                    (KeyCode::Char('y'), _) => {
                                        let current_cache = match app.state.current_tab { Tab::Dashboard => &app.cache_all, Tab::App => &app.cache_app, Tab::Build => &app.cache_build, Tab::Errors => &app.cache_err };
                                        if let Some(line) = current_cache.get(app.state.log_scroll) { if let Ok(mut clipboard) = arboard::Clipboard::new() { let _ = clipboard.set_text(line.clone()); let _ = tx_log.send("[ok] top visible line yanked".to_string()); } }
                                    }
                                    (KeyCode::Char('C'), _) => { if let Some(ref trace) = app.state.last_crash_trace { if let Ok(mut clipboard) = arboard::Clipboard::new() { let _ = clipboard.set_text(trace.clone()); let _ = tx_log.send("[ok] last crash trace yanked".to_string()); } } }
                                    (KeyCode::Char('m'), _) => {
                                        app.state.mouse_captured = !app.state.mouse_captured;
                                        if app.state.mouse_captured { let _ = execute!(std::io::stdout(), EnableMouseCapture); let _ = tx_log.send("[info] mouse capture ON".to_string()); } 
                                        else { let _ = execute!(std::io::stdout(), DisableMouseCapture); let _ = tx_log.send("[info] mouse capture OFF".to_string()); }
                                    }
                                    (KeyCode::Char('A'), _) => {
                                        let current_cache = match app.state.current_tab { Tab::Dashboard => &app.cache_all, Tab::App => &app.cache_app, Tab::Build => &app.cache_build, Tab::Errors => &app.cache_err };
                                        if !current_cache.is_empty() { let content = current_cache.join("\n"); if let Ok(mut clipboard) = arboard::Clipboard::new() { let _ = clipboard.set_text(content); let _ = tx_log.send("[ok] all logs yanked".to_string()); } }
                                    }
                                    (KeyCode::Tab, _) => { app.state.current_tab = match app.state.current_tab { Tab::Dashboard => Tab::App, Tab::App => Tab::Build, Tab::Build => Tab::Errors, Tab::Errors => Tab::Dashboard }; app.refresh_filter_cache(); }
                                    (KeyCode::Char('1'), m) if !m.contains(KeyModifiers::ALT) => { app.state.current_tab = Tab::Dashboard; app.refresh_filter_cache(); }
                                    (KeyCode::Char('2'), m) if !m.contains(KeyModifiers::ALT) => { app.state.current_tab = Tab::App; app.refresh_filter_cache(); }
                                    (KeyCode::Char('3'), m) if !m.contains(KeyModifiers::ALT) => { app.state.current_tab = Tab::Build; app.refresh_filter_cache(); }
                                    (KeyCode::Char('4'), m) if !m.contains(KeyModifiers::ALT) => { app.state.current_tab = Tab::Errors; app.refresh_filter_cache(); }
                                    (KeyCode::Char('1'), m) if m.contains(KeyModifiers::ALT) => { app.state.min_log_level = LogLevel::Verbose; app.refresh_filter_cache(); }
                                    (KeyCode::Char('2'), m) if m.contains(KeyModifiers::ALT) => { app.state.min_log_level = LogLevel::Debug; app.refresh_filter_cache(); }
                                    (KeyCode::Char('3'), m) if m.contains(KeyModifiers::ALT) => { app.state.min_log_level = LogLevel::Info; app.refresh_filter_cache(); }
                                    (KeyCode::Char('4'), m) if m.contains(KeyModifiers::ALT) => { app.state.min_log_level = LogLevel::Warn; app.refresh_filter_cache(); }
                                    (KeyCode::Char('5'), m) if m.contains(KeyModifiers::ALT) => { app.state.min_log_level = LogLevel::Error; app.refresh_filter_cache(); }
                                    (KeyCode::Char(c), _) => { if let Some(cmd_str) = app.config.custom_commands.get(&c.to_ascii_lowercase()) { let cmd = cmd_str.clone(); tokio::spawn(async move { let _ = tokio::process::Command::new("sh").args(["-c", &cmd]).status().await; }); } }
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
                                        let serials = app.get_target_serials().await;
                                        tokio::spawn(async move { for s in serials { let _ = tokio::process::Command::new("adb").args(["-s", &s, "shell", "am", "start", "-d", &url]).status().await; } });
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
                            AppMode::Help | AppMode::Welcome => { if key.code == KeyCode::Esc || key.code == KeyCode::Char('h') || key.code == KeyCode::Char('q') || key.code == KeyCode::Enter { if app.state.mode == AppMode::Welcome { let _ = app.config.save(); } app.state.mode = AppMode::Normal; } }
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
                            AppMode::EmulatorSelect => {
                                match key.code {
                                    KeyCode::Esc | KeyCode::Char('q') => { if app.state.device_serial.is_some() { app.state.mode = AppMode::Normal; } else { break; } }
                                    KeyCode::Up | KeyCode::Char('k') => app.state.settings_index = app.state.settings_index.saturating_sub(1),
                                    KeyCode::Down | KeyCode::Char('j') => app.state.settings_index = (app.state.settings_index + 1).min(app.state.available_avds.len().saturating_sub(1)),
                                    KeyCode::Enter => {
                                        let avd = app.state.available_avds[app.state.settings_index].clone();
                                        let log_tx = tx_log.clone();
                                        tokio::spawn(async move { let _ = log_tx.send(format!("[info] launching emulator: {}...", avd)); let _ = commands::launch_emulator(&avd).await; });
                                        app.state.mode = AppMode::Normal;
                                    }
                                    _ => {}
                                }
                            }
                            AppMode::NoHardwareHelp => { if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') || key.code == KeyCode::Enter { app.state.mode = AppMode::Normal; } }
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
    let popup_layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Percentage((100 - percent_y) / 2), Constraint::Percentage(percent_y), Constraint::Percentage((100 - percent_y) / 2)]).split(r);
    Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage((100 - percent_x) / 2), Constraint::Percentage(percent_x), Constraint::Percentage((100 - percent_x) / 2)]).split(popup_layout[1])[1]
}

fn ui(f: &mut ratatui::Frame, app: &mut App) {
    let area = f.area();
    f.render_widget(Clear, area);
    if area.height < 5 || area.width < 10 { f.render_widget(Paragraph::new("Terminal too small").alignment(Alignment::Center), area); return; }
    let mut main_constraints = vec![Constraint::Length(2), Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)];
    if area.height < 15 { main_constraints.remove(0); }
    if area.height < 10 { main_constraints.remove(0); }
    if app.state.mode == AppMode::Search || app.state.mode == AppMode::Input || app.state.mode == AppMode::DeepLink { let insert_idx = if main_constraints.len() == 4 { 2 } else if main_constraints.len() == 3 { 1 } else { 0 }; main_constraints.insert(insert_idx, Constraint::Length(3)); }
    let chunks = Layout::default().direction(Direction::Vertical).constraints(main_constraints).split(area);
    let mut current_idx = 0;

    if area.height >= 15 {
        let battery_span = match app.state.stats.battery_level { Some(l) => Span::styled(format!("BAT:{}%", l), if l < 20 { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) }), None => Span::raw("BAT:--%").dark_gray() };
        let broadcast_span = if app.state.is_broadcast { Span::styled(" [BROADCAST] ", Style::default().bg(Color::Red).fg(Color::White).bold()) } else { Span::raw("") };
        let header_line = Line::from(vec![
            Span::styled(" DeckDriod ", Style::default().bold().fg(Color::Cyan)),
            Span::raw(format!("({}) ", app.state.device_serial.as_deref().unwrap_or("none"))),
            battery_span,
            broadcast_span,
            Span::raw(" | Mouse:"), Span::raw(if app.state.mouse_captured { "APP" } else { "NATIVE" }).bold(),
            Span::raw(" | Watch:"), Span::raw(if app.state.auto_rebuild { "ON" } else { "OFF" }).bold(),
            Span::raw(" Open:"), Span::raw(if app.state.auto_open { "ON" } else { "OFF" }).bold(),
            Span::raw(format!(" | App:{}", app.config.app_id)).dark_gray(),
        ]);
        f.render_widget(Paragraph::new(header_line), chunks[current_idx]);
        current_idx += 1;
    }

    if area.height >= 10 {
        let tab_titles = vec![" [1] Dashboard ", " [2] App Logs ", " [3] Build ", " [4] Errors "];
        let tabs = Tabs::new(tab_titles).block(Block::default().borders(Borders::ALL)).select(match app.state.current_tab { Tab::Dashboard => 0, Tab::App => 1, Tab::Build => 2, Tab::Errors => 3 }).style(Style::default().fg(Color::DarkGray)).highlight_style(Style::default().fg(Color::Yellow).bold());
        f.render_widget(tabs, chunks[current_idx]);
        current_idx += 1;
    }

    if app.state.mode == AppMode::Search || app.state.mode == AppMode::Input || app.state.mode == AppMode::DeepLink {
        let title = match app.state.mode { AppMode::Search => " Search Logs ", AppMode::DeepLink => " Deep Link URL ", _ => " Input " };
        f.render_widget(Paragraph::new(app.state.input_buffer.as_str()).block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(Color::Yellow))), chunks[current_idx]);
        current_idx += 1;
    }

    let main_area = chunks[current_idx];

    match app.state.current_tab {
        Tab::Dashboard => {
            let dash_chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(7), Constraint::Min(0)]).split(main_area);
            let stats_layout = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(dash_chunks[0]);
            let truncate = |s: &str, max: usize| { if s.len() > max { format!("{}...", &s[..max.saturating_sub(3)]) } else { s.to_string() } };
            let cpu_title = if let Some(ref t) = app.state.build_task { format!(" BUILD: {} ", truncate(t, (stats_layout[0].width as usize).saturating_sub(10))) } else { format!(" CPU: {:.1}% ", app.state.stats.last_cpu) };
            let mem_title = format!(" MEM: {:.1}% ", app.state.stats.last_mem);
            f.render_widget(Sparkline::default().block(Block::default().borders(Borders::ALL).title(cpu_title)).data(&app.state.stats.cpu_usage.iter().map(|&v| (v * 10.0) as u64).collect::<Vec<_>>()).style(Style::default().fg(Color::Green)), stats_layout[0]);
            f.render_widget(Sparkline::default().block(Block::default().borders(Borders::ALL).title(mem_title)).data(&app.state.stats.mem_usage.iter().map(|&v| (v * 10.0) as u64).collect::<Vec<_>>()).style(Style::default().fg(Color::Blue)), stats_layout[1]);

            let cmd_block = Block::default().borders(Borders::ALL).title(" Quick Commands ");
            let inner_cmd_area = cmd_block.inner(dash_chunks[1]);
            f.render_widget(cmd_block, dash_chunks[1]);
            let cmd_layout = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(33), Constraint::Percentage(33), Constraint::Percentage(33)]).split(inner_cmd_area);
            let col1 = vec![Line::from(vec![Span::styled(" [a/r/Ent] ", Style::default().fg(Color::Cyan).bold()), Span::raw("Build/Launch")]), Line::from(vec![Span::styled(" [v]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Record Video")]), Line::from(vec![Span::styled(" [x]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Clear Data")]), Line::from(vec![Span::styled(" [/]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Search")])];
            let col2 = vec![Line::from(vec![Span::styled(" [L]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Launch Only")]), Line::from(vec![Span::styled(" [u]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Deep Link")]), Line::from(vec![Span::styled(" [c]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Clear Logs")]), Line::from(vec![Span::styled(" [B]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Broadcast Toggle")])];
            let col3 = vec![Line::from(vec![Span::styled(" [s]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Screenshot")]), Line::from(vec![Span::styled(" [b]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Toggle Bounds")]), Line::from(vec![Span::styled(" [i]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Settings")]), Line::from(vec![Span::styled(" [E]       ", Style::default().fg(Color::Cyan).bold()), Span::raw("Emulators")])];
            f.render_widget(Paragraph::new(col1), cmd_layout[0]);
            f.render_widget(Paragraph::new(col2), cmd_layout[1]);
            f.render_widget(Paragraph::new(col3), cmd_layout[2]);
        }
        _ => {
            let logs = match app.state.current_tab { Tab::Dashboard => unreachable!(), Tab::App => &app.cache_app, Tab::Build => &app.cache_build, Tab::Errors => &app.cache_err };
            let height = main_area.height as usize;
            let total = logs.len();
            let mut scroll = app.state.log_scroll;
            if scroll >= total && total > 0 { scroll = total.saturating_sub(1); }
            let end = (scroll + height).min(total);
            let visible = if scroll < total { &logs[scroll..end] } else { &[] };
            let lines: Vec<Line> = visible.iter().enumerate().map(|(i, l)| {
                let idx = scroll + i;
                let mut style = if l.contains("[err]") || l.contains("[build-err]") || l.contains(" E/") { Style::default().fg(Color::Red) } else if l.contains("[ok]") { Style::default().fg(Color::Green) } else if l.contains("[build]") || l.contains(" W/") { Style::default().fg(Color::Yellow) } else if l.contains(" I/") { Style::default().fg(Color::Cyan) } else { Style::default() };
                if let (Some(s), Some(e)) = (app.state.selection_start, app.state.selection_end) { if idx >= s.min(e) && idx <= s.max(e) { style = style.bg(Color::Blue).fg(Color::White); } }
                if l.starts_with("at ") || l.starts_with("\tat ") || l.contains("...") { style = Style::default().fg(Color::DarkGray); }
                Line::from(Span::styled(l, style))
            }).collect();
            f.render_widget(Paragraph::new(lines), main_area);
        }
    }

    let footer = Line::from(vec![Span::raw(" [Tab] Views | [m] Mouse:"), Span::raw(if app.state.mouse_captured { "APP" } else { "NATIVE" }).bold(), Span::raw(" | "), Span::raw(if app.state.autoscroll { "FOLLOW" } else { "PAUSED" }).bold(), Span::raw(" | [h] Help").dark_gray()]);
    f.render_widget(Paragraph::new(footer), chunks[chunks.len() - 1]);

    if app.state.mode == AppMode::Help || app.state.mode == AppMode::Welcome {
        let area = centered_rect(70, 80, f.area());
        f.render_widget(Clear, area);
        let title = if app.state.mode == AppMode::Welcome { " Welcome to DeckDriod! " } else { " Advanced Help " };
        let help = vec![Line::from(vec![Span::styled("--- Controls ---", Style::default().bold())]), Line::from(vec![Span::styled(" a / r / Ent ", Style::default().fg(Color::Cyan)), Span::raw(": Build & Launch")]), Line::from(vec![Span::styled(" L           ", Style::default().fg(Color::Cyan)), Span::raw(": Launch Only")]), Line::from(vec![Span::styled(" E           ", Style::default().fg(Color::Cyan)), Span::raw(": Launch Emulator")]), Line::from(vec![Span::styled(" B           ", Style::default().fg(Color::Cyan)), Span::raw(": Broadcast Toggle (Run actions on ALL devices)")]), Line::from(vec![Span::styled(" c           ", Style::default().fg(Color::Cyan)), Span::raw(": Clear Logs")]), Line::from(vec![Span::styled(" i           ", Style::default().fg(Color::Cyan)), Span::raw(": Settings")]), Line::from(vec![Span::styled(" s / v       ", Style::default().fg(Color::Cyan)), Span::raw(": Screenshot / Video")]), Line::from(vec![Span::styled(" y / A / C   ", Style::default().fg(Color::Cyan)), Span::raw(": Copy Line/All/Crash")]), Line::from(vec![Span::styled(" e           ", Style::default().fg(Color::Cyan)), Span::raw(": Export logs to deckdriod_export.txt")]), Line::from(vec![Span::styled(" q / Esc     ", Style::default().fg(Color::Cyan)), Span::raw(": Close/Quit")])];
        f.render_widget(Paragraph::new(help).block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(Color::Cyan))).wrap(Wrap { trim: true }), area);
    }

    if app.state.mode == AppMode::Settings || (app.state.mode == AppMode::Input && app.state.settings_index < 10) {
        let area = centered_rect(60, 50, f.area());
        f.render_widget(Clear, area);
        let latency_str = format!("{:.1}", app.config.watch_latency);
        let gap_str = format!("{:.1}", app.config.rebuild_gap);
        let settings = vec![("App ID", &app.config.app_id), ("Main Activity", &app.config.activity), ("Watch Latency", &latency_str), ("Build Gap", &gap_str), ("Log Tag", &app.config.log_tag), ("Project Path", &app.config.project_path), ("Output Path", &app.config.output_path)];
        let items: Vec<ListItem> = settings.iter().enumerate().map(|(i, (label, val))| { let mut style = Style::default(); if i == app.state.settings_index { style = style.fg(Color::Yellow).bold(); } ListItem::new(Line::from(vec![Span::styled(format!("{:<20}: ", label), style), Span::raw(*val)])) }).collect();
        f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).title(" Project Settings ").border_style(Style::default().fg(Color::Yellow))), area);
        if app.state.mode == AppMode::Input { let input_area = centered_rect(50, 10, area); f.render_widget(Clear, input_area); f.render_widget(Paragraph::new(app.state.input_buffer.as_str()).block(Block::default().borders(Borders::ALL).title(" Edit ").border_style(Style::default().fg(Color::Yellow))), input_area); }
    }

    if app.state.mode == AppMode::EmulatorSelect {
        let area = centered_rect(60, 40, f.area());
        f.render_widget(Clear, area);
        let items: Vec<ListItem> = app.state.available_avds.iter().enumerate().map(|(i, name)| { let mut style = Style::default(); if i == app.state.settings_index { style = style.fg(Color::Yellow).bold(); } ListItem::new(Line::from(vec![Span::styled(format!("> {}", name), style)])) }).collect();
        f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).title(" Select Emulator ").border_style(Style::default().fg(Color::Yellow))), area);
    }

    if app.state.mode == AppMode::NoHardwareHelp {
        let area = centered_rect(70, 60, f.area());
        f.render_widget(Clear, area);
        let help = vec![Line::from(vec![Span::styled(" No Android Devices Detected ", Style::default().fg(Color::Red).bold())]), Line::from(vec![Span::raw("")]), Line::from(vec![Span::styled("1. Connect device via USB", Style::default().bold())]), Line::from(vec![Span::styled("2. Create emulator (AVD)", Style::default().bold())]), Line::from(vec![Span::raw("")]), Line::from(vec![Span::raw("sdkmanager \"system-images;android-33;google_apis;arm64-v8a\"")]), Line::from(vec![Span::raw("avdmanager create avd -n MyDevice -k \"system-images;android-33;google_apis;arm64-v8a\"")]), Line::from(vec![Span::styled("Press Esc to enter dashboard anyway.", Style::default().dark_gray())])];
        f.render_widget(Paragraph::new(help).block(Block::default().borders(Borders::ALL).title(" Hardware Help ").border_style(Style::default().fg(Color::Red))).wrap(Wrap { trim: true }), area);
    }
}
