mod config;
mod state;
mod commands;
mod logcat;
mod watcher;
mod stats;
mod mcp;

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
    style::{Color, Style},
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

        {
            let mut shared = self.state.shared_logs.write().unwrap();
            shared.app_logs.push(l.clone());
            if shared.app_logs.len() > 1000 { shared.app_logs.remove(0); }
        }

        if l.contains("FATAL EXCEPTION") || l.contains("AndroidRuntime:E") {
            self.state.last_crash = Some(l.clone());
            self.state.last_crash_trace = Some(l.clone());
            self.state.is_capturing_crash = true;
            let mut shared = self.state.shared_logs.write().unwrap();
            shared.last_crash = Some(l.clone());
        } else if self.state.is_capturing_crash {
            if l.starts_with("at ") || l.starts_with("\tat ") || l.contains("Caused by:") {
                if let Some(ref mut trace) = self.state.last_crash_trace {
                    trace.push('\n');
                    trace.push_str(&l);
                    let mut shared = self.state.shared_logs.write().unwrap();
                    shared.last_crash = Some(trace.clone());
                }
            } else {
                self.state.is_capturing_crash = false;
                if let Some(ref trace) = self.state.last_crash_trace {
                    let _ = std::fs::write("crash_report.txt", trace);
                }
            }
        }

        let mut logs_to_add = Vec::new();
        if l.contains("[DeckDriod]") {
            if let Some(json_start) = l.find('{') {
                let json_part = &l[json_start..];
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_part) {
                    if let Ok(pretty) = serde_json::to_string_pretty(&value) {
                        logs_to_add.push("--- App State ---".to_string());
                        for line in pretty.lines() { logs_to_add.push(format!("  {}", line)); }
                        logs_to_add.push("-----------------".to_string());
                    }
                }
            }
        }

        if logs_to_add.is_empty() { logs_to_add.push(l); }

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
                if entry.contains(" E/") || entry.contains("[err]") || entry.contains("[build-err]") || entry.contains(" FATAL") {
                    self.cache_err.push(entry.clone());
                    if self.cache_err.len() > 5000 { self.cache_err.remove(0); }
                    let mut shared = self.state.shared_logs.write().unwrap();
                    shared.error_logs.push(entry.clone());
                    if shared.error_logs.len() > 500 { shared.error_logs.remove(0); }
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
        if !self.config.log_tag.is_empty() && !log.contains(&self.config.log_tag) { return false; }
        if let Some(level) = LogLevel::from_str(log) {
            if (level as u8) < (self.state.min_log_level as u8) { return false; }
        }
        if !self.state.search_query.is_empty() && !log.to_lowercase().contains(&self.state.search_query.to_lowercase()) { return false; }
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
        if self.state.is_broadcast { commands::get_devices().await.unwrap_or_default() } 
        else { self.state.device_serial.as_ref().map(|s| vec![s.clone()]).unwrap_or_default() }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "-v" | "--version" => { println!("deckdriod v{}", env!("CARGO_PKG_VERSION")); return Ok(()); }
            "usage" => { 
                let port = config::Config::load().mcp_port;
                println!("{}", mcp::get_detailed_guide(port)); 
                return Ok(()); 
            }
            "update" => {
                println!("Updating deckdriod...");
                let status = std::process::Command::new("sh").arg("-c").arg("set -o pipefail; curl -sSf https://raw.githubusercontent.com/theasmat/deckdriod/master/install.sh | sh").status()?;
                if status.success() { println!("\n✅ Update successful!"); } 
                else { eprintln!("\n❌ Update failed. Check your internet connection."); std::process::exit(1); }
                return Ok(());
            }
            "mcp-doc" => {
                let port = config::Config::load().mcp_port;
                let content = mcp::get_detailed_guide(port);
                std::fs::write("DECKDRIOD_MCP.md", content)?;
                println!("✅ Generated DECKDRIOD_MCP.md");
                return Ok(());
            }
            _ => {}
        }
    }

    let config = Config::load();
    let mut state = AppState::default();
    state.mcp_port = config.mcp_port;

    if !std::path::Path::new(".deckdriodconfig").exists() { state.mode = AppMode::Welcome; }

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
        tokio::spawn(async move { let _ = commands::build_and_launch(&cfg_bg, serials, auto_open, false, build_tx, build_evt_tx).await; });
    }

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let mut last_draw = std::time::Instant::now();
    let mut mcp_shutdown_tx: Option<mpsc::Sender<()>> = None;

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
                        let mut shared = app.state.shared_logs.write().unwrap();
                        shared.build_status = "Success".to_string();
                    }
                    BuildEvent::Failed => { app.state.build_task = None; let mut shared = app.state.shared_logs.write().unwrap(); shared.build_status = "Failed".to_string(); }
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
                        tokio::spawn(async move { let _ = commands::build_and_launch(&cfg, serials, auto_open, false, build_tx, build_evt_tx).await; });
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
                                if mouse.row >= app.state.log_area_rect.y && mouse.row < app.state.log_area_rect.y + app.state.log_area_rect.height {
                                    app.state.selection_start = Some(app.state.log_scroll + (mouse.row - app.state.log_area_rect.y) as usize);
                                    app.state.selection_end = app.state.selection_start;
                                } else { app.state.selection_start = None; }
                            }
                            MouseEventKind::Drag(_) => {
                                if let Some(_start) = app.state.selection_start {
                                    let current_row_idx = app.state.log_scroll + (mouse.row.saturating_sub(app.state.log_area_rect.y)) as usize;
                                    app.state.selection_end = Some(current_row_idx);
                                    if mouse.row <= app.state.log_area_rect.y + 1 { app.state.log_scroll = app.state.log_scroll.saturating_sub(1); app.state.autoscroll = false; } 
                                    else if mouse.row >= app.state.log_area_rect.y + app.state.log_area_rect.height.saturating_sub(1) { let max_scroll = app.current_log_len().saturating_sub(1); if app.state.log_scroll < max_scroll { app.state.log_scroll += 1; app.state.autoscroll = false; } }
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
                                        tokio::spawn(async move { let _ = commands::build_and_launch(&cfg, serials, auto_open, false, build_tx, build_evt_tx).await; });
                                    }
                                    (KeyCode::Char('f'), _) => {
                                        app.state.last_rebuild_at = Some(std::time::Instant::now());
                                        app.state.current_tab = Tab::Build;
                                        app.state.autoscroll = true;
                                        app.refresh_filter_cache();
                                        let build_tx = tx_log.clone();
                                        let build_evt_tx = tx_build.clone();
                                        let cfg = app.config.clone();
                                        let serials = app.get_target_serials().await;
                                        let auto_open = app.state.auto_open;
                                        tokio::spawn(async move { let _ = commands::build_and_launch(&cfg, serials, auto_open, true, build_tx, build_evt_tx).await; });
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
                                        let msg = if app.state.is_broadcast { "[info] BROADCAST mode ON" } else { "[info] BROADCAST mode OFF" };
                                        let _ = tx_log.send(msg.to_string());
                                    }
                                    (KeyCode::Char('M'), _) => {
                                        app.state.mcp_server_active = !app.state.mcp_server_active;
                                        if app.state.mcp_server_active {
                                            let (tx, rx) = mpsc::channel(1);
                                            mcp_shutdown_tx = Some(tx);
                                            let port = app.state.mcp_port;
                                            let shared = Arc::clone(&app.state.shared_logs);
                                            tokio::spawn(async move { mcp::run_server(port, shared, rx).await; });
                                            let _ = tx_log.send(format!("[info] MCP server started on port {}", port));
                                        } else if let Some(tx) = mcp_shutdown_tx.take() {
                                            let _ = tx.send(()).await;
                                            let _ = tx_log.send("[info] MCP server stopped".to_string());
                                        }
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
                                        tokio::spawn(async move { for s in serials { let _ = tokio::process::Command::new("adb").args(["-s", &s, "shell", "input", "keyevent", "82"]).status().await; } });
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
                                        let content = if let (Some(s), Some(e)) = (app.state.selection_start, app.state.selection_end) {
                                            let min = s.min(e);
                                            let max = s.max(e).min(current_cache.len().saturating_sub(1));
                                            current_cache[min..=max].join("\n")
                                        } else if let Some(line) = current_cache.get(app.state.log_scroll) {
                                            line.clone()
                                        } else { String::new() };
                                        if !content.is_empty() {
                                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                                let _ = clipboard.set_text(content);
                                                let msg = if app.state.selection_start.is_some() { "[ok] selection yanked" } else { "[ok] top visible line yanked" };
                                                let _ = tx_log.send(msg.to_string());
                                            }
                                        }
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
                                         7 => if let Ok(v) = val.parse() { app.config.mcp_port = v; app.state.mcp_port = v; },
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
                                    KeyCode::Down | KeyCode::Char('j') => app.state.settings_index = (app.state.settings_index + 1).min(7),
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
                                            7 => app.state.input_buffer = app.state.mcp_port.to_string(),
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
    let has_input = matches!(app.state.mode, AppMode::Search | AppMode::Input | AppMode::DeepLink);
    // header(3 with border) + tabs(3) + [input(3)] + main(min) + footer(2 with border)
    let mut main_constraints = vec![
        Constraint::Length(3),  // header
        Constraint::Length(3),  // tabs
        Constraint::Min(0),     // main content
        Constraint::Length(2),  // footer
    ];
    if has_input { main_constraints.insert(2, Constraint::Length(3)); }
    let chunks = Layout::default().direction(Direction::Vertical).constraints(main_constraints).split(area);
    let mut current_idx = 0;

    {
        let bat = match app.state.stats.battery_level {
            Some(l) => Span::styled(format!(" BAT:{}%", l), if l < 20 { Style::default().fg(Color::Red).bold() } else { Style::default().fg(Color::Green) }),
            None => Span::styled(" BAT:--%", Style::default().fg(Color::DarkGray)),
        };
        let device_str = app.state.device_serial.as_deref().unwrap_or("no device");
        // Truncate app_id to fit: reserve ~60 chars for left side
        let max_id = (area.width as usize).saturating_sub(70);
        let app_id = &app.config.app_id;
        let app_id_display = if app_id.len() > max_id && max_id > 3 {
            format!("{}...", &app_id[..max_id.saturating_sub(3)])
        } else { app_id.clone() };

        let mut spans = vec![
            Span::styled(" DeckDriod ", Style::default().bold().fg(Color::Cyan)),
            Span::styled(format!("v{} ", env!("CARGO_PKG_VERSION")), Style::default().fg(Color::DarkGray)),
            Span::styled(format!(" {} ", device_str), Style::default().fg(Color::White)),
            bat,
            Span::styled(format!("  CPU:{:.0}%", app.state.stats.last_cpu), Style::default().fg(Color::Green)),
            Span::styled(format!("  MEM:{:.0}%", app.state.stats.last_mem), Style::default().fg(Color::Blue)),
            Span::styled("  |", Style::default().fg(Color::DarkGray)),
            Span::styled(format!(" Watch:{}", if app.state.auto_rebuild { "ON" } else { "OFF" }), if app.state.auto_rebuild { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) }),
            Span::styled(format!(" Open:{}", if app.state.auto_open { "ON" } else { "OFF" }), if app.state.auto_open { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) }),
            Span::styled("  |", Style::default().fg(Color::DarkGray)),
        ];
        if app.state.is_broadcast { spans.push(Span::styled(" [BROADCAST]", Style::default().bg(Color::Red).fg(Color::White).bold())); }
        if app.state.mcp_server_active { spans.push(Span::styled(format!(" [MCP:{}]", app.state.mcp_port), Style::default().bg(Color::Blue).fg(Color::White).bold())); }
        if app.state.is_recording { spans.push(Span::styled(" [REC]", Style::default().bg(Color::Red).fg(Color::White).bold())); }
        spans.push(Span::styled(format!("  {}", app_id_display), Style::default().fg(Color::DarkGray)));

        f.render_widget(
            Paragraph::new(Line::from(spans))
                .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(Color::DarkGray))),
            chunks[current_idx],
        );
        current_idx += 1;
    }

    {
        let tab_titles = vec![
            " [1] Dashboard ".to_string(),
            format!(" [2] App ({}) ", app.cache_app.len()),
            format!(" [3] Build ({}) ", app.cache_build.len()),
            format!(" [4] Errors ({}) ", app.cache_err.len()),
        ];
        let selected = match app.state.current_tab { Tab::Dashboard => 0, Tab::App => 1, Tab::Build => 2, Tab::Errors => 3 };
        let tabs = Tabs::new(tab_titles)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)))
            .select(selected)
            .style(Style::default().fg(Color::DarkGray))
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan).bold());
        f.render_widget(tabs, chunks[current_idx]);
        current_idx += 1;
    }

    if has_input {
        let title = match app.state.mode { AppMode::Search => " / Search ", AppMode::DeepLink => " Deep Link URL ", _ => " Input " };
        f.render_widget(
            Paragraph::new(format!("{}_", app.state.input_buffer))
                .block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(Color::Yellow))),
            chunks[current_idx],
        );
        current_idx += 1;
    }

    let main_area = chunks[current_idx];
    app.state.log_area_rect = main_area;

    match app.state.current_tab {
        Tab::Dashboard => {
            // Responsive: stats(30%) | history(15%) | commands(rest)
            let dash_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(15), Constraint::Min(0)])
                .split(main_area);

            // ── Stats row ──────────────────────────────────────────────
            let stats_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(dash_chunks[0]);

            let cpu_title = if let Some(ref t) = app.state.build_task {
                let max = (stats_layout[0].width as usize).saturating_sub(6);
                let t = if t.len() > max { format!("{}...", &t[..max.saturating_sub(3)]) } else { t.clone() };
                format!(" BUILD: {} ", t)
            } else {
                format!(" CPU {:.1}% ", app.state.stats.last_cpu)
            };
            let cpu_data: Vec<u64> = app.state.stats.cpu_usage.iter().map(|&v| (v * 10.0) as u64).collect();
            f.render_widget(
                Sparkline::default()
                    .block(Block::default().borders(Borders::ALL).title(cpu_title).border_style(Style::default().fg(Color::Green)))
                    .data(&cpu_data)
                    .style(Style::default().fg(Color::Green)),
                stats_layout[0],
            );
            let mem_data: Vec<u64> = app.state.stats.mem_usage.iter().map(|&v| (v * 10.0) as u64).collect();
            f.render_widget(
                Sparkline::default()
                    .block(Block::default().borders(Borders::ALL).title(format!(" MEM {:.1}% ", app.state.stats.last_mem)).border_style(Style::default().fg(Color::Blue)))
                    .data(&mem_data)
                    .style(Style::default().fg(Color::Blue)),
                stats_layout[1],
            );

            // ── Build history ──────────────────────────────────────────
            let history_block = Block::default().borders(Borders::ALL).title(" Build History ").border_style(Style::default().fg(Color::DarkGray));
            let inner_hist = history_block.inner(dash_chunks[1]);
            f.render_widget(history_block, dash_chunks[1]);
            if app.state.build_history.is_empty() {
                f.render_widget(Paragraph::new("  No builds yet. Press [a] to build.").style(Style::default().fg(Color::DarkGray)), inner_hist);
            } else {
                let hist_spans: Vec<Span> = app.state.build_history.iter().enumerate().map(|(i, d)| {
                    let secs = d.as_secs_f64();
                    let color = if secs < 30.0 { Color::Green } else if secs < 90.0 { Color::Yellow } else { Color::Red };
                    Span::styled(format!("  #{} {:.1}s", i + 1, secs), Style::default().fg(color))
                }).collect();
                f.render_widget(Paragraph::new(Line::from(hist_spans)), inner_hist);
            }

            // ── Commands grid ──────────────────────────────────────────
            let cmd_block = Block::default().borders(Borders::ALL).title(" Quick Commands ").border_style(Style::default().fg(Color::DarkGray));
            let inner_cmd = cmd_block.inner(dash_chunks[2]);
            f.render_widget(cmd_block, dash_chunks[2]);

            // Responsive: wide = 3 cols, narrow = 2 cols
            let use_3_cols = area.width >= 80;
            let cmd_layout = if use_3_cols {
                Layout::default().direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(33), Constraint::Percentage(33), Constraint::Percentage(34)])
                    .split(inner_cmd)
            } else {
                Layout::default().direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(inner_cmd)
            };

            let k = |key: &str, desc: &str| -> Line<'static> {
                Line::from(vec![
                    Span::styled(format!(" {:<10}", key), Style::default().fg(Color::Cyan).bold()),
                    Span::styled(desc.to_string(), Style::default().fg(Color::White)),
                ])
            };
            let col1 = vec![k("[a/r/Enter]", "Build & Launch"), k("[f]", "Force Rebuild"), k("[L]", "Launch Only"), k("[c]", "Clear Logs"), k("[/]", "Search")];
            let col2 = vec![k("[s]", "Screenshot"), k("[v]", "Record Video"), k("[x]", "Clear Data"), k("[u]", "Deep Link"), k("[b]", "Layout Bounds")];
            let col3 = vec![k("[B]", "Broadcast"), k("[M]", "MCP Toggle"), k("[E]", "Emulator"), k("[i]", "Settings"), k("[h]", "Help")];

            f.render_widget(Paragraph::new(col1), cmd_layout[0]);
            if use_3_cols {
                f.render_widget(Paragraph::new(col2), cmd_layout[1]);
                f.render_widget(Paragraph::new(col3), cmd_layout[2]);
            } else {
                // Merge col2+col3 into second column
                let mut merged = col2;
                merged.extend(col3);
                f.render_widget(Paragraph::new(merged), cmd_layout[1]);
            }
        }
        _ => {
            let logs = match app.state.current_tab {
                Tab::Dashboard => unreachable!(),
                Tab::App => &app.cache_app,
                Tab::Build => &app.cache_build,
                Tab::Errors => &app.cache_err,
            };
            let tab_label = match app.state.current_tab {
                Tab::App => "App Logs",
                Tab::Build => "Build Logs",
                Tab::Errors => "Errors",
                Tab::Dashboard => unreachable!(),
            };
            let total = logs.len();
            let mut scroll = app.state.log_scroll;
            if scroll >= total && total > 0 { scroll = total.saturating_sub(1); }

            let search_indicator = if !app.state.search_query.is_empty() {
                format!(" /\"{}\" ", app.state.search_query)
            } else { String::new() };
            let level_label = match app.state.min_log_level {
                LogLevel::Verbose => "V+", LogLevel::Debug => "D+",
                LogLevel::Info => "I+", LogLevel::Warn => "W+", LogLevel::Error => "E",
            };
            let scroll_info = if app.state.autoscroll {
                format!(" FOLLOW | {}{} | {} lines ", search_indicator, level_label, total)
            } else {
                format!(" PAUSED {}/{} | {}{} | {} lines ", scroll + 1, total, search_indicator, level_label, total)
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", tab_label))
                .title_bottom(scroll_info.as_str())
                .border_style(Style::default().fg(Color::DarkGray));
            let inner = block.inner(main_area);
            f.render_widget(block, main_area);
            app.state.log_area_rect = inner;

            // Build ALL styled lines (no manual truncation - let Paragraph+Wrap handle it)
            let all_lines: Vec<Line> = logs.iter().enumerate().map(|(idx, l)| {
                let mut style = if l.contains("[err]") || l.contains("[build-err]") || l.contains(" E/") || l.contains(" FATAL") {
                    Style::default().fg(Color::Red)
                } else if l.contains("[ok]") {
                    Style::default().fg(Color::Green)
                } else if l.contains("[build]") || l.contains(" W/") {
                    Style::default().fg(Color::Yellow)
                } else if l.contains(" I/") {
                    Style::default().fg(Color::Cyan)
                } else if l.contains("[info]") {
                    Style::default().fg(Color::Blue)
                } else {
                    Style::default().fg(Color::White)
                };
                if let (Some(s), Some(e)) = (app.state.selection_start, app.state.selection_end) {
                    if idx >= s.min(e) && idx <= s.max(e) { style = style.bg(Color::Blue).fg(Color::White); }
                }
                if l.starts_with("at ") || l.starts_with("\tat ") { style = Style::default().fg(Color::DarkGray); }
                Line::from(Span::styled(l.clone(), style))
            }).collect();

            f.render_widget(
                Paragraph::new(all_lines)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll as u16, 0)),
                inner,
            );
        }
    }

    let crash_span = if app.state.last_crash.is_some() {
        Span::styled(" [CRASH] ", Style::default().bg(Color::Red).fg(Color::White).bold())
    } else { Span::raw("") };
    let build_span = if let Some(ref t) = app.state.build_task {
        let short = if t.len() > 30 { format!("{}...", &t[..29]) } else { t.clone() };
        Span::styled(format!(" [BUILD: {}] ", short), Style::default().fg(Color::Yellow).bold())
    } else { Span::raw("") };
    let footer_line = Line::from(vec![
        Span::styled(" [1-4] Tabs", Style::default().fg(Color::DarkGray)),
        Span::styled("  [/] Search", Style::default().fg(Color::DarkGray)),
        Span::styled("  [G] Follow", Style::default().fg(Color::DarkGray)),
        Span::styled("  [h] Help", Style::default().fg(Color::DarkGray)),
        Span::styled("  [q] Quit", Style::default().fg(Color::DarkGray)),
        Span::raw("   "),
        build_span,
        crash_span,
    ]);
    f.render_widget(
        Paragraph::new(footer_line)
            .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray))),
        chunks[chunks.len() - 1],
    );

    if app.state.mode == AppMode::Help || app.state.mode == AppMode::Welcome {
        let area = centered_rect(70, 80, f.area());
        f.render_widget(Clear, area);
        let title = if app.state.mode == AppMode::Welcome { " Welcome to DeckDriod! " } else { " Advanced Help " };
        let help = vec![
            Line::from(vec![Span::styled("--- CLI Commands ---", Style::default().bold())]),
            Line::from(vec![Span::styled(" deckdriod -v      ", Style::default().fg(Color::Cyan)), Span::raw(": Show version info")]),
            Line::from(vec![Span::styled(" deckdriod usage   ", Style::default().fg(Color::Cyan)), Span::raw(": Print detailed manual")]),
            Line::from(vec![Span::styled(" deckdriod update  ", Style::default().fg(Color::Cyan)), Span::raw(": Update to latest version")]),
            Line::from(vec![Span::styled(" deckdriod mcp-doc ", Style::default().fg(Color::Cyan)), Span::raw(": Generate DECKDRIOD_MCP.md for AI")]),
            Line::from(vec![Span::raw("")]),
            Line::from(vec![Span::styled("--- Controls ---", Style::default().bold())]),
            Line::from(vec![Span::styled(" a / r / Ent ", Style::default().fg(Color::Cyan)), Span::raw(": Build & Launch")]),
            Line::from(vec![Span::styled(" f           ", Style::default().fg(Color::Cyan)), Span::raw(": Force Rebuild (Clean + No Cache)")]),
            Line::from(vec![Span::styled(" L           ", Style::default().fg(Color::Cyan)), Span::raw(": Launch Only")]),
            Line::from(vec![Span::styled(" E           ", Style::default().fg(Color::Cyan)), Span::raw(": Launch Emulator")]),
            Line::from(vec![Span::styled(" B           ", Style::default().fg(Color::Cyan)), Span::raw(": Broadcast Toggle (Run actions on ALL devices)")]),
            Line::from(vec![Span::styled(" M           ", Style::default().fg(Color::Cyan)), Span::raw(": MCP Toggle (Enable AI log analysis)")]),
            Line::from(vec![Span::styled(" c           ", Style::default().fg(Color::Cyan)), Span::raw(": Clear Logs")]),
            Line::from(vec![Span::styled(" i           ", Style::default().fg(Color::Cyan)), Span::raw(": Settings")]),
            Line::from(vec![Span::styled(" s / v       ", Style::default().fg(Color::Cyan)), Span::raw(": Screenshot / Video")]),
            Line::from(vec![Span::styled(" y / A / C   ", Style::default().fg(Color::Cyan)), Span::raw(": Copy Line/All/Crash")]),
            Line::from(vec![Span::styled(" e           ", Style::default().fg(Color::Cyan)), Span::raw(": Export logs to deckdriod_export.txt")]),
            Line::from(vec![Span::styled(" q / Esc     ", Style::default().fg(Color::Cyan)), Span::raw(": Close/Quit")]),
        ];
        f.render_widget(Paragraph::new(help).block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(Color::Cyan))).wrap(Wrap { trim: true }), area);
    }

    if app.state.mode == AppMode::Settings || (app.state.mode == AppMode::Input && app.state.settings_index < 10) {
        let area = centered_rect(60, 50, f.area());
        f.render_widget(Clear, area);
        let latency_str = format!("{:.1}", app.config.watch_latency);
        let gap_str = format!("{:.1}", app.config.rebuild_gap);
        let mcp_port_str = app.state.mcp_port.to_string();
        let settings = vec![("App ID", &app.config.app_id), ("Main Activity", &app.config.activity), ("Watch Latency", &latency_str), ("Build Gap", &gap_str), ("Log Tag", &app.config.log_tag), ("Project Path", &app.config.project_path), ("Output Path", &app.config.output_path), ("MCP Port", &mcp_port_str)];
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
