# 🚀 DeckDriod: The Professional Android Development Dashboard

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**DeckDriod** is a high-performance, interactive TUI (Terminal User Interface) built in Rust, designed to provide a unified dashboard for Kotlin and Android developers. It combines building, launching, log monitoring, and device profiling into a single, beautiful terminal view.

---

## ✨ Features

- **📊 Real-time Resource Monitoring:** Live sparklines for CPU and Memory usage of your specific App ID.
- **⚡️ Smart Build Telemetry:** Tracks current Gradle tasks and keeps a history of build durations.
- **🔍 Power Logcat:** Instant search, level filtering (Alt + 1-5), and manual scroll pause/resume.
- **📑 Multi-Tab Management:** Categorize logs into Dashboard, App, Build, and Errors views.
- **📡 Multi-Device Broadcast:** Run actions (build, launch, clear data, etc.) on **all** connected devices simultaneously.
- **🚀 Automated "Smooth" Transitions:** Auto-switches to the Build tab on start and App tab on success.
- **🚨 Advanced Crash Detection:** Automatically highlights fatal exceptions, captures the full stack trace, and saves it to `crash_report.txt`.
- **📟 Smart Stack Traces:** Boilerplate lines are dimmed to make the root cause of an error stand out.
- **🤖 AI Integration (MCP):** Expose live logs and crashes to AI assistants like Claude or Gemini via the Model Context Protocol.
- **🎥 Media Tools:** One-key screen recording and screenshots pulled directly to your specified output directory.
- **🔗 Deep Link Tester:** Quickly test deep links without touching your phone.
- **🛠 Layout Debugger:** Toggle system layout bounds with a single hotkey.
- **🔋 Battery Monitor:** Real-time device battery level tracking.
- **⌨️ Custom Hotkeys:** Map your own shell commands to hotkeys in `.deckdriodconfig`.
- **🏗 Cross-Directory Support:** Run `deckdriod` from anywhere by specifying your project path.

---

## 🤖 AI Integration (MCP)

DeckDriod supports the **Model Context Protocol (MCP)**, allowing AI assistants to directly analyze your Android logs and crashes.

### How to use with Claude Desktop:
1.  Open DeckDriod and press **`M`** (Shift+M) to start the MCP server. You will see `[MCP:3000]` in the header.
2.  Add DeckDriod to your `claude_desktop_config.json`:
    ```json
    {
      "mcpServers": {
        "deckdriod": {
          "command": "curl",
          "args": ["-s", "http://localhost:3000/sse"]
        }
      }
    }
    ```
3.  Restart Claude. It can now use tools like `get_recent_logs`, `get_errors`, and `get_latest_crash` to help you debug!

---

## 🚀 Installation

### 1. Via Homebrew
```bash
brew tap theasmat/tap
brew install deckdriod
```

### 2. Via Quick Install Script (macOS/Linux)
```bash
curl -sSf https://raw.githubusercontent.com/theasmat/deckdriod/master/install.sh | sh
```

### 3. From Source
```bash
cargo install --path .
```

---

## 📖 How to Use

Simply run `deckdriod` in the root of your project, or configure a `PROJECT_PATH` to run it from anywhere.

### Views

| Key | View |
| :--- | :--- |
| `1` | **Dashboard**: Status summary, resource usage, and command list. |
| `2` | **App Logs**: Full-width scrollable Logcat output. |
| `3` | **Build Logs**: Full-width scrollable Gradle output. |
| `4` | **Errors**: Focused view for crashes and error-level logs. |

### Hotkeys

| Key | Action |
| :--- | :--- |
| `r` / `Enter` | **Build & Launch**: Rebuilds and restarts the app. |
| `f` | **Force Rebuild**: Clean + Build without cache. |
| `l` | **Launch Only**: Restarts the app without rebuilding. |
| `d` | **Device Switcher**: Select from connected devices. |
| `k` | **Kill App**: Force stop the app. |
| `E` | **Emulator Selector**: Select and launch an Android emulator. |
| `M` (Shift+m) | **MCP Toggle**: Enable/Disable AI log analysis server. |
| `e` | **Export Logs**: Save current log buffer to `deckdriod_export.txt`. |
| `/` | Search Logs (Substring matching) |
| `h` | Open Advanced Help Popup |
| `c` | Clear Logs & Crash Alerts |
| `i` | Open Interactive Settings Menu |
| `v` | Start/Stop Screen Recording (`.mp4`) |
| `s` | Take Screenshot (`.png`) |
| `u` | Open Deep Link URL |
| `b` | Toggle Layout Bounds on Device |
| `m` | Toggle Mouse (App vs Native Selection) |
| `y` | Yank (Copy) selection or top visible log line to clipboard |
| `A` | Yank (Copy) ALL visible logs to clipboard |
| `C` | Yank (Copy) the last crash trace to clipboard |
| `Alt + 1-5` | Set Minimum Log Level (Verbose to Error) |
| `↑`/`↓` / `Wheel` | Scroll Logs (Pauses Auto-follow) |
| `G` | Resume Auto-follow Logs |
| `d` | Open Android Dev Menu |
| `x` | Clear App Data |
| `q` | Quit |

---

## ⚙️ Configuration

Create a `.deckdriodconfig` file in your project root OR your home directory (`~/.deckdriodconfig`) to override defaults.

```ini
APP_ID=com.example.app.dev
ACTIVITY=com.example.app/com.example.app.MainActivity
WATCH_LATENCY=1.0
REBUILD_GAP=2.0
PROJECT_PATH=/path/to/your/kotlin/project
OUTPUT_PATH=/path/to/screenshots/folder
MCP_PORT=3000

# Custom hotkeys (DECKDRIOD_CMD_<key>=command)
DECKDRIOD_CMD_T="adb shell input text 'testuser'"
```

---

## 📜 License

This project is licensed under the **MIT License**. See the [LICENSE](LICENSE) file for details.

---

Created with ❤️ by theasmat
