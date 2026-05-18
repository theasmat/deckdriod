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
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap, Sparkline, Clear},
    Terminal,
};
use std::io::stdout;
use tokio::sync::mpsc;
use std::collections::VecDeque;

use state::{AppMode, LogLevel};

struct App {
    config: Config,
    state: AppState,
    logs: VecDeque<String>,
    log_state: ListState,
}

impl App {
    fn new(config: Config, state: AppState) -> Self {
        Self {
            config,
            state,
            logs: VecDeque::with_capacity(5000),
            log_state: ListState::default(),
        }
    }

    fn add_log(&mut self, log: String) {
        if self.logs.len() >= 5000 {
            self.logs.pop_front();
        }
        
        // Crash detection
        if log.contains("FATAL EXCEPTION") || log.contains("AndroidRuntime:E") {
            self.state.last_crash = Some(log.clone());
        }

        self.logs.push_back(log);
        if self.state.autoscroll {
            let filtered_count = self.filtered_logs().len();
            if filtered_count > 0 {
                self.log_state.select(Some(filtered_count.saturating_sub(1)));
            }
        }
    }

    fn filtered_logs(&self) -> Vec<&String> {
        self.logs.iter().filter(|log| {
            // Level filter
            if let Some(level) = LogLevel::from_str(log) {
                if (level as u8) < (self.state.min_log_level as u8) {
                    return false;
                }
            }
            
            // Search filter
            if !self.state.search_query.is_empty() {
                if !log.to_lowercase().contains(&self.state.search_query.to_lowercase()) {
                    return false;
                }
            }
            
            true
        }).collect()
    }
}

use commands::BuildEvent;

#[tokio::main]
async fn main() -> Result<()> {
    // Argument parsing
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "-v" | "--version" => {
                println!("deckdriod v{}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "update" => {
                println!("Updating deckdriod...");
                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg("curl -sSf https://raw.githubusercontent.com/theasmat/deckdriod/main/install.sh | sh")
                    .status()?;
                if status.success() {
                    println!("Update successful!");
                } else {
                    println!("Update failed.");
                }
                return Ok(());
            }
            _ => {}
        }
    }

    let config = Config::load();
    let mut state = AppState::default();

    // Check for first run in this directory
    if !std::path::Path::new(".deckdriodconfig").exists() {
        state.mode = AppMode::Welcome;
    }

    // Device selection (before entering TUI mode for simplicity)
    let devices = commands::get_devices().await?;
    if devices.is_empty() {
        anyhow::bail!("no devices connected");
    } else if devices.len() == 1 {
        state.device_serial = Some(devices[0].clone());
    } else {
        println!("Multiple devices detected. Please select one:");
        for (i, dev) in devices.iter().enumerate() {
            println!("{}: {}", i + 1, dev);
        }
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let idx: usize = input.trim().parse().unwrap_or(1);
        state.device_serial = Some(devices.get(idx - 1).unwrap_or(&devices[0]).clone());
    }

    let serial = state.device_serial.as_ref().unwrap().clone();

    // UI setup
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
    let _watcher = watcher::start_watcher(app.config.watch_latency, tx_watch)?;

    // Start stats polling
    let stats_serial = serial.clone();
    let stats_app_id = app.config.app_id.clone();
    tokio::spawn(stats::start_stats_polling(stats_serial, stats_app_id, tx_stats));

    let mut log_manager = LogcatManager::new();
    if app.state.show_logs {
        log_manager.start(&serial, &app.config.app_id, tx_log.clone())?;
    }

    // Initial build
    let build_tx = tx_log.clone();
    let build_evt_tx = tx_build.clone();
    let _ = commands::build_and_launch(&app.config, &app.state, build_tx, build_evt_tx).await;

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));

    let mut recorder = commands::Recorder::new();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        tokio::select! {
            _ = interval.tick() => {
                if app.state.show_logs && !log_manager.check_status().await {
                    let _ = log_manager.start(&serial, &app.config.app_id, tx_log.clone());
                }
            }
            Some(log) = rx_log.recv() => {
                app.add_log(log);
            }
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
                        let build_tx = tx_log.clone();
                        let build_evt_tx = tx_build.clone();
                        let _ = commands::build_and_launch(&app.config, &app.state, build_tx, build_evt_tx).await;
                    } else {
                        let remaining = app.config.rebuild_gap - now.duration_since(app.state.last_rebuild_at.unwrap()).as_secs_f64();
                        let _ = tx_log.send(format!("[info] change detected, waiting for build gap ({:.1}s remaining)", remaining));
                    }
                } else {
                    let _ = tx_log.send("[warn] change detected but auto-rebuild is OFF".to_string());
                }
            }
            res = tokio::task::spawn_blocking(|| event::poll(std::time::Duration::from_millis(10))) => {
                if let Ok(Ok(true)) = res {
                    let ev = event::read()?;
                    
                    if let Event::Mouse(mouse) = ev {
                        match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                app.state.autoscroll = false;
                                let current = app.log_state.selected().unwrap_or(0);
                                app.log_state.select(Some(current.saturating_sub(1)));
                            }
                            MouseEventKind::ScrollDown => {
                                app.state.autoscroll = false;
                                let current = app.log_state.selected().unwrap_or(0);
                                let max = app.filtered_logs().len().saturating_sub(1);
                                if current < max {
                                    app.log_state.select(Some(current + 1));
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
                                (KeyCode::Char('r'), _) | (KeyCode::Enter, _) => {
                                    app.state.last_rebuild_at = Some(std::time::Instant::now());
                                    let build_tx = tx_log.clone();
                                    let build_evt_tx = tx_build.clone();
                                    let _ = commands::build_and_launch(&app.config, &app.state, build_tx, build_evt_tx).await;
                                }
                                (KeyCode::Char('c'), _) => {
                                    app.logs.clear();
                                    app.state.last_crash = None;
                                    app.state.search_query.clear();
                                    log_manager.start(&serial, &app.config.app_id, tx_log.clone())?;
                                }
                                (KeyCode::Char('h'), _) => {
                                    app.state.mode = AppMode::Help;
                                }
                                (KeyCode::Char('i'), _) => {
                                    app.state.mode = AppMode::Settings;
                                    app.state.settings_index = 0;
                                }
                                (KeyCode::Char('w'), _) => { app.state.auto_rebuild = !app.state.auto_rebuild; }
                                (KeyCode::Char('o'), _) => { app.state.auto_open = !app.state.auto_open; }
                                (KeyCode::Char('l'), _) => {
                                    app.state.show_logs = !app.state.show_logs;
                                    if app.state.show_logs {
                                        log_manager.start(&serial, &app.config.app_id, tx_log.clone())?;
                                    } else {
                                        log_manager.stop();
                                    }
                                }
                                (KeyCode::Char('s'), _) => {
                                    let build_tx = tx_log.clone();
                                    let _ = commands::take_screenshot(&app.state, build_tx).await;
                                }
                                (KeyCode::Char('v'), _) => {
                                    if app.state.is_recording {
                                        app.state.is_recording = false;
                                        let build_tx = tx_log.clone();
                                        let _ = recorder.stop(&serial, build_tx).await;
                                    } else {
                                        if let Ok(_) = recorder.start(&serial).await {
                                            app.state.is_recording = true;
                                            let _ = tx_log.send("[info] recording started...".to_string());
                                        }
                                    }
                                }
                                (KeyCode::Char('b'), _) => {
                                    let _ = commands::toggle_layout_bounds(&mut app.state).await;
                                }
                                (KeyCode::Char('u'), _) => {
                                    app.state.mode = AppMode::DeepLink;
                                    app.state.input_buffer.clear();
                                }
                                (KeyCode::Char('x'), _) => {
                                    let build_tx = tx_log.clone();
                                    let _ = commands::clear_app_data(&app.config, &app.state, build_tx).await;
                                }
                                (KeyCode::Char('d'), _) => {
                                    let _ = tx_log.send("[info] opening dev menu...".to_string());
                                    let _ = tokio::process::Command::new("adb")
                                        .args(["-s", &serial, "shell", "input", "keyevent", "82"])
                                        .status()
                                        .await;
                                }
                                (KeyCode::Char('/'), _) => {
                                    app.state.mode = AppMode::Search;
                                    app.state.input_buffer.clear();
                                }
                                (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                                    app.state.autoscroll = false;
                                    let current = app.log_state.selected().unwrap_or(0);
                                    app.log_state.select(Some(current.saturating_sub(1)));
                                }
                                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                                    app.state.autoscroll = false;
                                    let current = app.log_state.selected().unwrap_or(0);
                                    let max = app.filtered_logs().len().saturating_sub(1);
                                    if current < max {
                                        app.log_state.select(Some(current + 1));
                                    }
                                }
                                (KeyCode::PageUp, _) => {
                                    app.state.autoscroll = false;
                                    let current = app.log_state.selected().unwrap_or(0);
                                    app.log_state.select(Some(current.saturating_sub(20)));
                                }
                                (KeyCode::PageDown, _) => {
                                    app.state.autoscroll = false;
                                    let current = app.log_state.selected().unwrap_or(0);
                                    let max = app.filtered_logs().len().saturating_sub(1);
                                    app.log_state.select(Some((current + 20).min(max)));
                                }
                                (KeyCode::Char('g'), _) => {
                                    app.state.autoscroll = false;
                                    app.log_state.select(Some(0));
                                }
                                (KeyCode::Char('G'), _) => {
                                    app.state.autoscroll = true;
                                }
                                (KeyCode::Char('y'), _) => {
                                    if let Some(idx) = app.log_state.selected() {
                                        let filtered = app.filtered_logs();
                                        if let Some(line) = filtered.get(idx) {
                                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                                let _ = clipboard.set_text((*line).clone());
                                                let _ = tx_log.send("[ok] line yanked to clipboard".to_string());
                                            }
                                        }
                                    }
                                }
                                (KeyCode::Char('1'), _) => app.state.min_log_level = LogLevel::Verbose,
                                (KeyCode::Char('2'), _) => app.state.min_log_level = LogLevel::Debug,
                                (KeyCode::Char('3'), _) => app.state.min_log_level = LogLevel::Info,
                                (KeyCode::Char('4'), _) => app.state.min_log_level = LogLevel::Warn,
                                (KeyCode::Char('5'), _) => app.state.min_log_level = LogLevel::Error,
                                (KeyCode::Char('e'), _) => {
                                    let content: String = app.logs.iter().cloned().collect::<Vec<String>>().join("\n");
                                    if let Ok(_) = std::fs::write("deckdriod_logs.txt", content) {
                                        let _ = tx_log.send("[ok] logs exported to deckdriod_logs.txt".to_string());
                                    }
                                }
                                (KeyCode::Char(c), _) => {
                                    if let Some(cmd_str) = app.config.custom_commands.get(&c.to_ascii_lowercase()) {
                                        let _ = tx_log.send(format!("[info] custom cmd: {}", cmd_str));
                                        let _ = tokio::process::Command::new("sh")
                                            .args(["-c", cmd_str])
                                            .status()
                                            .await;
                                    }
                                }
                                _ => {}
                            }
                        }
                        AppMode::Search => {
                            match key.code {
                                KeyCode::Enter => {
                                    app.state.search_query = app.state.input_buffer.clone();
                                    app.state.mode = AppMode::Normal;
                                }
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
                                    let _ = tx_log.send(format!("[info] opening deep link: {}", url));
                                    let _ = tokio::process::Command::new("adb")
                                        .args(["-s", &serial, "shell", "am", "start", "-d", &url])
                                        .status()
                                        .await;
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
                                 let mut success = true;
                                 match app.state.settings_index {
                                     0 => app.config.app_id = val,
                                     1 => app.config.activity = val,
                                     2 => if let Ok(v) = val.parse() { app.config.watch_latency = v; } else { success = false; },
                                     3 => if let Ok(v) = val.parse() { app.config.rebuild_gap = v; } else { success = false; },
                                     _ => {}
                                 }
                                 if success {
                                     let _ = app.config.save();
                                     let _ = tx_log.send("[ok] settings updated and saved".to_string());
                                 }
                                 app.state.mode = AppMode::Settings;
                             }
                             if let KeyCode::Char(c) = key.code { app.state.input_buffer.push(c); }
                             if key.code == KeyCode::Backspace { app.state.input_buffer.pop(); }
                        }
                        AppMode::Help | AppMode::Welcome => {
                             if key.code == KeyCode::Esc || key.code == KeyCode::Char('h') || key.code == KeyCode::Char('q') || key.code == KeyCode::Enter { 
                                 if app.state.mode == AppMode::Welcome {
                                     let _ = app.config.save(); // Create initial config file
                                 }
                                 app.state.mode = AppMode::Normal; 
                             }
                        }
                        AppMode::Settings => {
                            match key.code {
                                KeyCode::Esc | KeyCode::Char('q') => app.state.mode = AppMode::Normal,
                                KeyCode::Up | KeyCode::Char('k') => app.state.settings_index = app.state.settings_index.saturating_sub(1),
                                KeyCode::Down | KeyCode::Char('j') => app.state.settings_index = (app.state.settings_index + 1).min(3),
                                KeyCode::Enter => {
                                    app.state.mode = AppMode::Input;
                                    app.state.input_buffer.clear();
                                    match app.state.settings_index {
                                        0 => app.state.input_buffer = app.config.app_id.clone(),
                                        1 => app.state.input_buffer = app.config.activity.clone(),
                                        2 => app.state.input_buffer = format!("{:.1}", app.config.watch_latency),
                                        3 => app.state.input_buffer = format!("{:.1}", app.config.rebuild_gap),
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
    let mut main_constraints = vec![
        Constraint::Length(9), // Header area
        Constraint::Min(0),    // Logs
        Constraint::Length(3), // Help
    ];
    if app.state.mode == AppMode::Search || app.state.mode == AppMode::Input || app.state.mode == AppMode::DeepLink {
        main_constraints.insert(1, Constraint::Length(3)); // Input bar
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(main_constraints)
        .split(f.area());

    // Header Area split into Status and Stats
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(chunks[0]);

    // Status Column
    let w_status = if app.state.auto_rebuild { "ON".green() } else { "OFF".red() };
    let o_status = if app.state.auto_open { "ON".green() } else { "OFF".red() };
    let l_status = if app.state.show_logs { "ON".green() } else { "OFF".red() };
    let scroll_status = if app.state.autoscroll { "FOLLOW".cyan() } else { "PAUSED".yellow() };
    let record_status = if app.state.is_recording { "REC".red().bold() } else { "OFF".dark_gray() };
    let layout_status = if app.state.show_layout_bounds { "ON".green() } else { "OFF".dark_gray() };

    let mut header_text = vec![
        Line::from(vec![
            Span::styled(" DeckDriod Server ", Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)),
            Span::raw(format!("({})", app.state.device_serial.as_deref().unwrap_or("none"))),
            Span::raw("  REC: "), record_status,
            Span::raw("  BAT: "), 
            match app.state.stats.battery_level {
                Some(l) => Span::styled(format!("{}%", l), if l < 20 { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) }),
                None => Span::raw("--%"),
            },
        ]),
        Line::from(vec![
            Span::raw(" App ID  "),
            Span::styled(&app.config.app_id, Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::raw(" Status  "),
            Span::raw("watcher: "), w_status,
            Span::raw("  open: "), o_status,
            Span::raw("  logs: "), l_status,
            Span::raw("  bounds: "), layout_status,
        ]),
        Line::from(vec![
            Span::raw(" Scroll  "), scroll_status,
            Span::raw("  level: "), Span::styled(format!("{:?}", app.state.min_log_level), Style::default().fg(Color::Yellow)),
        ]),
    ];

    if let Some(ref task) = app.state.build_task {
        header_text.push(Line::from(vec![
            Span::styled(" BUILD   ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(task, Style::default().fg(Color::Yellow)),
        ]));
    } else if !app.state.build_history.is_empty() {
        let history: Vec<String> = app.state.build_history.iter().map(|d| format!("{:.1}s", d.as_secs_f32())).collect();
        header_text.push(Line::from(vec![
            Span::raw(" History "),
            Span::styled(history.join(" -> "), Style::default().fg(Color::DarkGray)),
        ]));
    }

    header_text.push(Line::from(vec![
        Span::raw(" Search  "), Span::styled(&app.state.search_query, Style::default().fg(Color::Magenta)),
    ]));

    if let Some(ref crash) = app.state.last_crash {
        header_text.push(Line::from(vec![
            Span::styled(" CRASH   ", Style::default().bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(crash, Style::default().fg(Color::Red)),
        ]));
    }

    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).title(" Dashboard "))
        .wrap(Wrap { trim: true });
    f.render_widget(header, header_chunks[0]);

    // Stats Column
    let cpu_data: Vec<u64> = app.state.stats.cpu_usage.iter().map(|&v| (v * 10.0) as u64).collect();
    let mem_data: Vec<u64> = app.state.stats.mem_usage.iter().map(|&v| (v * 10.0) as u64).collect();

    let stats_block = Block::default().borders(Borders::ALL).title(" Resource Usage ");
    let inner_stats = stats_block.inner(header_chunks[1]);
    f.render_widget(stats_block, header_chunks[1]);

    let stats_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(inner_stats);

    let cpu_sparkline = Sparkline::default()
        .block(Block::default().title(format!(" CPU: {:.1}% ", app.state.stats.last_cpu)))
        .data(&cpu_data)
        .style(Style::default().fg(Color::Green));
    f.render_widget(cpu_sparkline, stats_layout[0]);

    let mem_sparkline = Sparkline::default()
        .block(Block::default().title(format!(" MEM: {:.1}% ", app.state.stats.last_mem)))
        .data(&mem_data)
        .style(Style::default().fg(Color::Blue));
    f.render_widget(mem_sparkline, stats_layout[1]);

    let mut log_chunk_idx = 1;

    // Input Bar (Search/DeepLink)
    if app.state.mode == AppMode::Search || app.state.mode == AppMode::Input || app.state.mode == AppMode::DeepLink {
        let title = match app.state.mode {
            AppMode::Search => " Search Logs ",
            AppMode::DeepLink => " Deep Link URL ",
            _ => " Input ",
        };
        let input = Paragraph::new(app.state.input_buffer.as_str())
            .block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(Color::Yellow)));
        f.render_widget(input, chunks[1]);
        log_chunk_idx = 2;
    }

    // Logs
    let filtered = app.filtered_logs();
    let log_items: Vec<ListItem> = filtered.iter().map(|l| {
        let style = if l.contains("[err]") || l.contains("[build-err]") {
            Style::default().fg(Color::Red)
        } else if l.contains("[ok]") {
            Style::default().fg(Color::Green)
        } else if l.contains("[build]") {
            Style::default().fg(Color::Yellow)
        } else if l.contains(" E/") {
             Style::default().fg(Color::Red)
        } else if l.contains(" W/") {
             Style::default().fg(Color::Yellow)
        } else if l.contains(" I/") {
             Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        ListItem::new(Line::from(Span::styled((*l).to_string(), style)))
    }).collect();

    let logs = List::new(log_items)
        .block(Block::default().borders(Borders::ALL).title(" Logs "))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray));
    
    f.render_stateful_widget(logs, chunks[log_chunk_idx], &mut app.log_state);

    // Help
    let help_chunk_idx = chunks.len() - 1;
    let help_text = vec![
        Line::from(vec![
            "[r] build [c] clear [/] search [e] export [h] advanced help [↑/↓] scroll [q] quit"
                .cyan()
                .italic(),
        ]),
    ];
    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL).title(" Help "));
    f.render_widget(help, chunks[help_chunk_idx]);

    // Help Popup
    if app.state.mode == AppMode::Help || app.state.mode == AppMode::Welcome {
        let area = centered_rect(70, 70, f.area());
        f.render_widget(Clear, area);
        let title = if app.state.mode == AppMode::Welcome { " Welcome to DeckDriod! " } else { " Advanced Help " };
        let mut help_popup_text = vec![
            Line::from(vec![Span::styled("--- CLI Commands ---", Style::default().add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled(" deckdriod -v      ", Style::default().fg(Color::Cyan)), Span::raw(": Show version info")]),
            Line::from(vec![Span::styled(" deckdriod update  ", Style::default().fg(Color::Cyan)), Span::raw(": Update to latest version")]),
            Line::from(vec![Span::raw("")]),
            Line::from(vec![Span::styled("--- Controls ---", Style::default().add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::styled(" r / Enter ", Style::default().fg(Color::Cyan)), Span::raw(": Build & Launch")]),
            Line::from(vec![Span::styled(" c         ", Style::default().fg(Color::Cyan)), Span::raw(": Clear Logs & Crash Alert")]),
            Line::from(vec![Span::styled(" i         ", Style::default().fg(Color::Cyan)), Span::raw(": Open Settings Menu")]),
            Line::from(vec![Span::styled(" v         ", Style::default().fg(Color::Cyan)), Span::raw(": Start/Stop Screen Recording")]),
            Line::from(vec![Span::styled(" s         ", Style::default().fg(Color::Cyan)), Span::raw(": Take Screenshot")]),
            Line::from(vec![Span::styled(" u         ", Style::default().fg(Color::Cyan)), Span::raw(": Open Deep Link")]),
            Line::from(vec![Span::styled(" b         ", Style::default().fg(Color::Cyan)), Span::raw(": Toggle Layout Bounds")]),
            Line::from(vec![Span::styled(" /         ", Style::default().fg(Color::Cyan)), Span::raw(": Search Logs")]),
            Line::from(vec![Span::styled(" 1-5       ", Style::default().fg(Color::Cyan)), Span::raw(": Set Min Log Level")]),
            Line::from(vec![Span::styled(" k / j     ", Style::default().fg(Color::Cyan)), Span::raw(": Scroll Up/Down (Wheel works too)")]),
            Line::from(vec![Span::styled(" PgUp/PgDn ", Style::default().fg(Color::Cyan)), Span::raw(": Scroll 20 lines")]),
            Line::from(vec![Span::styled(" G         ", Style::default().fg(Color::Cyan)), Span::raw(": Follow Bottom")]),
            Line::from(vec![Span::styled(" y         ", Style::default().fg(Color::Cyan)), Span::raw(": Yank (Copy) line to Clipboard")]),
            Line::from(vec![Span::styled(" e         ", Style::default().fg(Color::Cyan)), Span::raw(": Export Logs to deckdriod_logs.txt")]),
            Line::from(vec![Span::styled(" d         ", Style::default().fg(Color::Cyan)), Span::raw(": Android Dev Menu")]),
            Line::from(vec![Span::styled(" x         ", Style::default().fg(Color::Cyan)), Span::raw(": Clear App Data")]),
            Line::from(vec![Span::raw("")]),
            Line::from(vec![Span::styled(" Esc / h   ", Style::default().fg(Color::Cyan)), Span::raw(": Close Menu")]),
            Line::from(vec![Span::styled(" q         ", Style::default().fg(Color::Cyan)), Span::raw(": Quit")]),
        ];

        if app.state.mode == AppMode::Welcome {
            help_popup_text.insert(0, Line::from(vec![Span::styled("First run detected! Here are your available commands:", Style::default().fg(Color::Yellow))]));
            help_popup_text.insert(1, Line::from(vec![Span::raw("")]));
        }

        let popup = Paragraph::new(help_popup_text)
            .block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(Color::Cyan)))
            .wrap(Wrap { trim: true });
        f.render_widget(popup, area);
    }

    // Settings Popup
    if app.state.mode == AppMode::Settings || (app.state.mode == AppMode::Input && app.state.settings_index < 10) {
        let area = centered_rect(60, 40, f.area());
        f.render_widget(Clear, area);
        
        let watch_latency_str = format!("{:.1}", app.config.watch_latency);
        let rebuild_gap_str = format!("{:.1}", app.config.rebuild_gap);

        let settings = vec![
            ("App ID", &app.config.app_id),
            ("Main Activity", &app.config.activity),
            ("Watch Latency (s)", &watch_latency_str),
            ("Build Gap (s)", &rebuild_gap_str),
        ];

        let items: Vec<ListItem> = settings.iter().enumerate().map(|(i, (label, val))| {
            let mut style = Style::default();
            if i == app.state.settings_index {
                style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
            }
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<20}: ", label), style),
                Span::raw(*val),
            ]))
        }).collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Project Settings ").border_style(Style::default().fg(Color::Yellow)));
        f.render_widget(list, area);
        
        if app.state.mode == AppMode::Input {
            let input_area = centered_rect(50, 10, area);
            f.render_widget(Clear, input_area);
            let input = Paragraph::new(app.state.input_buffer.as_str())
                .block(Block::default().borders(Borders::ALL).title(" Edit Value ").border_style(Style::default().fg(Color::Yellow)));
            f.render_widget(input, input_area);
        }
    }
}
