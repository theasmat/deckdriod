# DeckDriod Architecture

## Overview

DeckDriod is a terminal-based Android development dashboard built with Rust and Ratatui. It provides real-time monitoring, build automation, and device management in a single TUI.

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                         main.rs                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ Event Loop   │  │  UI Renderer │  │  App State   │      │
│  │ (tokio)      │  │  (ratatui)   │  │              │      │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘      │
│         │                  │                  │              │
└─────────┼──────────────────┼──────────────────┼──────────────┘
          │                  │                  │
          ▼                  ▼                  ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│   commands.rs   │  │    state.rs     │  │   config.rs     │
│                 │  │                 │  │                 │
│ • build_and_    │  │ • AppState      │  │ • Config        │
│   launch        │  │ • AppMode       │  │ • load/save     │
│ • take_         │  │ • Tab           │  │                 │
│   screenshot    │  │ • LogLevel      │  │                 │
│ • get_devices   │  │                 │  │                 │
└────────┬────────┘  └─────────────────┘  └─────────────────┘
         │
         ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│   logcat.rs     │  │    stats.rs     │  │     mcp.rs      │
│                 │  │                 │  │                 │
│ • LogcatManager │  │ • CPU/MEM       │  │ • MCP Server    │
│ • start/stop    │  │   polling       │  │ • get_logs      │
│                 │  │ • Battery       │  │ • get_status    │
└─────────────────┘  └─────────────────┘  └─────────────────┘
```

## Core Components

### 1. Main Event Loop (`main.rs`)
- **Tokio async runtime** for concurrent operations
- **Event channels** for log streaming, stats updates, build events
- **Lazy rendering** with dirty flag (only redraws on state change)
- **Key/mouse input handling** via crossterm

### 2. State Management (`state.rs`)
- **AppState**: Central state container
  - Current tab, mode, scroll position
  - Device serial, build history
  - Crash history, search query
  - Filter settings
- **AppMode**: UI mode enum (Normal, Search, Help, Settings, etc.)
- **Tab**: Dashboard, App Logs, Build Logs, Errors
- **LogLevel**: Verbose, Debug, Info, Warn, Error

### 3. Configuration (`config.rs`)
- **Config struct**: App settings
  - Project path, output path
  - App ID, activity name
  - Build variant, MCP port
  - Custom commands
- **Load/Save**: Reads from `.deckdriodconfig`
- **Persistence**: Remembers last device, last tab

### 4. Commands (`commands.rs`)
- **build_and_launch**: Gradle build + ADB install
- **take_screenshot**: ADB screencap + pull
- **get_devices**: List connected devices with model names
- **clear_app_data**: ADB clear + restart
- **Recorder**: Screen recording manager

### 5. Logcat Manager (`logcat.rs`)
- **Async log streaming** from ADB
- **Process management**: Start/stop/restart
- **Filtering**: By app ID, log level
- **Crash detection**: FATAL EXCEPTION, ANR

### 6. Stats Polling (`stats.rs`)
- **CPU/MEM usage**: Per-app via `dumpsys meminfo`
- **Battery level**: Via `dumpsys battery`
- **Sparklines**: Rolling window of last 100 data points

### 7. MCP Server (`mcp.rs`)
- **Model Context Protocol** for AI integration
- **Tools**: get_logs, get_build_status
- **SSE endpoint**: Real-time log streaming
- **Shared state**: RwLock for concurrent access

## Data Flow

### Log Processing
```
ADB logcat → LogcatManager → Channel → App.add_log() → Cache → UI
                                                          ↓
                                                    Crash Detection
                                                          ↓
                                                    Crash History
```

### Build Flow
```
User [r] → build_and_launch() → Gradle → Parse output → BuildEvent
                                              ↓
                                        Update build_task
                                              ↓
                                        Show in Dashboard
```

### Device Management
```
Interval tick → get_devices() → Compare with current → Connect/Disconnect
                                                              ↓
                                                        Start logcat
                                                              ↓
                                                        Start stats
```

## Performance Optimizations

1. **Lazy Rendering**: Only redraws on state change or every 1s
2. **Batched Writes**: Shared logs updated every 10 lines
3. **Cached Filters**: Pre-filtered log caches per tab
4. **Circular Buffers**: 2000 line limit prevents memory growth
5. **Lock-free Reads**: MCP server uses RwLock for concurrent access

## Key Design Decisions

### Why Tokio?
- Async I/O for ADB commands
- Concurrent log streaming, stats polling, build tasks
- Non-blocking UI updates

### Why Ratatui?
- Immediate mode rendering
- Cross-platform terminal UI
- Rich widget library (sparklines, lists, tables)

### Why Channels?
- Decouples producers (logcat, stats) from consumer (UI)
- Backpressure handling
- Clean shutdown

### Why RwLock for MCP?
- Multiple readers (AI queries) don't block each other
- Single writer (log updates) has exclusive access
- Better than Mutex for read-heavy workloads

## Testing Strategy

### Unit Tests
- Config parsing (`config.rs`)
- Log level detection (`state.rs`)
- Command building (`commands.rs`)

### Integration Tests
- CLI flags (`--version`, `--help`)
- Config file loading
- ADB command execution (mocked)

### Manual Testing
- Device connect/disconnect
- Build success/failure
- Crash detection
- MCP server integration

## Future Improvements

1. **Module Split**: Break main.rs into ui/ and events/ modules
2. **Thread Profiling**: Per-thread CPU/MEM view
3. **Advanced Filters**: Regex, PID, package filters
4. **Plugin System**: Custom commands, themes
5. **CI/CD**: Automated testing, release builds

## Contributing

See `IMPROVEMENT_PLAN.md` for roadmap and `README.md` for setup instructions.
