use std::collections::VecDeque;
use std::time::Duration;
use std::sync::{Arc, RwLock};
use ratatui::layout::Rect;

#[derive(Clone)]
pub struct LogEntry {
    pub text: String,
    pub level: Option<LogLevel>,
}

impl LogEntry {
    pub fn new(text: String) -> Self {
        let level = LogLevel::from_str(&text);
        Self { text, level }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum AppMode {
    Normal,
    Input,
    Search,
    DeepLink,
    Help,
    Welcome,
    Settings,
    EmulatorSelect,
    NoHardwareHelp,
    PickProject,
    DirPicker,
    DevicePicker,
    ExportFormat,
    ProjectPicker,
    VariantPicker,
    FilterBuilder,
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

#[derive(Default)]
pub struct SharedLogState {
    pub app_logs: Vec<String>,
    pub error_logs: Vec<String>,
    pub last_crash: Option<String>,
    pub build_status: String,
    pub build_logs: Vec<String>,
    pub build_task: Option<String>,
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
    pub search_match_idx: usize,
    pub autoscroll: bool,
    pub min_log_level: LogLevel,
    pub stats: SystemStats,
    pub last_crash: Option<String>,
    pub last_crash_trace: Option<String>,
    pub is_capturing_crash: bool,
    pub crash_history: Vec<String>,
    pub show_crash_history: bool,
    pub build_task: Option<String>,
    pub build_history: VecDeque<Duration>,
    pub is_recording: bool,
    pub show_layout_bounds: bool,
    pub last_rebuild_at: Option<std::time::Instant>,
    pub settings_index: usize,
    pub is_broadcast: bool,
    pub available_avds: Vec<String>,
    pub log_scroll: usize,
    pub mouse_captured: bool,
    
    // Selection Engine
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
    pub log_area_rect: Rect,
    
    pub mcp_server_active: bool,
    pub mcp_port: u16,
    pub shared_logs: Arc<RwLock<SharedLogState>>,

    // Dir picker
    pub dir_picker_cwd: String,
    pub dir_picker_entries: Vec<String>,
    pub dir_picker_idx: usize,
    pub dir_picker_target: u8, // 0=project_path, 1=output_path
    
    // Device switcher
    pub available_devices: Vec<(String, String)>, // (serial, model)
    pub device_picker_idx: usize,
    
    // Project switcher
    pub project_list: Vec<String>,
    pub project_picker_idx: usize,
    
    // Variant selector
    pub variant_list: Vec<String>,
    pub variant_picker_idx: usize,
    
    // Rendering optimization
    pub needs_redraw: bool,
    
    // Filter builder
    pub filter_package: String,
    pub filter_tag: String,
    pub filter_pid: String,
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
            search_match_idx: 0,
            autoscroll: true,
            min_log_level: LogLevel::Verbose,
            stats: SystemStats::default(),
            last_crash: None,
            last_crash_trace: None,
            is_capturing_crash: false,
            crash_history: Vec::new(),
            show_crash_history: false,
            build_task: None,
            build_history: VecDeque::with_capacity(5),
            is_recording: false,
            show_layout_bounds: false,
            last_rebuild_at: None,
            settings_index: 0,
            is_broadcast: false,
            available_avds: Vec::new(),
            log_scroll: 0,
            mouse_captured: true,
            selection_start: None,
            selection_end: None,
            log_area_rect: Rect::default(),
            mcp_server_active: false,
            mcp_port: 3000,
            shared_logs: Arc::new(RwLock::new(SharedLogState::default())),
            dir_picker_cwd: String::new(),
            dir_picker_entries: Vec::new(),
            dir_picker_idx: 0,
            dir_picker_target: 0,
            available_devices: Vec::new(),
            device_picker_idx: 0,
            project_list: Vec::new(),
            project_picker_idx: 0,
            variant_list: Vec::new(),
            variant_picker_idx: 0,
            needs_redraw: true,
            filter_package: String::new(),
            filter_tag: String::new(),
            filter_pid: String::new(),
        }
    }
}
