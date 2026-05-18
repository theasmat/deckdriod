use std::collections::VecDeque;
use std::time::Duration;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum AppMode {
    Normal,
    Input,
    Search,
    DeepLink,
    Help,
    Welcome,
    Settings,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum LogLevel {
    Verbose,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn from_str(s: &str) -> Option<Self> {
        if s.contains(" V/") { Some(LogLevel::Verbose) }
        else if s.contains(" D/") { Some(LogLevel::Debug) }
        else if s.contains(" I/") { Some(LogLevel::Info) }
        else if s.contains(" W/") { Some(LogLevel::Warn) }
        else if s.contains(" E/") { Some(LogLevel::Error) }
        else { None }
    }
}

#[derive(Default, Clone)]
pub struct SystemStats {
    pub cpu_usage: VecDeque<f64>,
    pub mem_usage: VecDeque<f64>,
    pub last_cpu: f64,
    pub last_mem: f64,
    pub battery_level: Option<u8>,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Tab {
    Dashboard,
    App,
    Build,
    Errors,
}

#[derive(Clone)]
pub struct AppState {
    pub auto_rebuild: bool,
    pub auto_open: bool,
    pub show_logs: bool,
    pub device_serial: Option<String>,
    pub mode: AppMode,
    pub current_tab: Tab,
    pub input_buffer: String,
    pub search_query: String,
    pub autoscroll: bool,
    pub min_log_level: LogLevel,
    pub stats: SystemStats,
    pub last_crash: Option<String>,
    pub build_task: Option<String>,
    pub build_history: VecDeque<Duration>,
    pub is_recording: bool,
    pub show_layout_bounds: bool,
    pub last_rebuild_at: Option<std::time::Instant>,
    pub settings_index: usize,
    pub log_scroll: u16,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            auto_rebuild: true,
            auto_open: true,
            show_logs: true,
            device_serial: None,
            mode: AppMode::Normal,
            current_tab: Tab::Dashboard,
            input_buffer: String::new(),
            search_query: String::new(),
            autoscroll: true,
            min_log_level: LogLevel::Verbose,
            stats: SystemStats::default(),
            last_crash: None,
            build_task: None,
            build_history: VecDeque::with_capacity(5),
            is_recording: false,
            show_layout_bounds: false,
            last_rebuild_at: None,
            settings_index: 0,
            log_scroll: 0,
        }
    }
}
