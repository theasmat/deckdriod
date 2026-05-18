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
- **🚨 Advanced Crash Detection:** Automatically highlights fatal exceptions, captures the full stack trace, and saves it to `crash_report.txt`.
- **📟 Smart Stack Traces:** Boilerplate lines are dimmed to make the root cause of an error stand out.
- **✨ Expo-Style Structured Logging:** Intercepts `[DeckDriod]` log prefixes and pretty-prints JSON payloads for modern app-side debugging.
- **🎥 Media Tools:** One-key screen recording and screenshots pulled directly to your workspace.
- **🔗 Deep Link Tester:** Quickly test deep links without touching your phone.
- **🛠 Layout Debugger:** Toggle system layout bounds with a single hotkey.
- **🔋 Battery Monitor:** Real-time device battery level tracking.
- **⌨️ Custom Hotkeys:** Map your own shell commands to hotkeys in `.deckdriodconfig`.

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

Simply run `deckdriod` in the root of your Kotlin Multiplatform or Android project.

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
| `r` / `Enter` | Trigger Manual Build & Launch |
| `/` | Search Logs (Substring matching) |
| `h` | Open Advanced Help Popup |
| `c` | Clear Logs & Crash Alerts |
| `i` | Open Interactive Settings Menu |
| `v` | Start/Stop Screen Recording (`.mp4`) |
| `s` | Take Screenshot (`.png`) |
| `u` | Open Deep Link URL |
| `b` | Toggle Layout Bounds on Device |
| `e` | Export current log buffer to `deckdriod_logs.txt` |
| `y` | Yank (Copy) the top visible log line to clipboard |
| `Alt + 1-5` | Set Minimum Log Level (Verbose to Error) |
| `↑`/`↓` / `Wheel` | Scroll Logs (Pauses Auto-follow) |
| `G` | Resume Auto-follow Logs |
| `d` | Open Android Dev Menu |
| `x` | Clear App Data |
| `q` | Quit |

---

## ⚙️ Configuration

Create a `.deckdriodconfig` file in your project root to override defaults. You can also edit these interactively by pressing `i` inside the app.

```ini
APP_ID=com.example.app.dev
ACTIVITY=com.example.app/com.example.app.MainActivity
WATCH_LATENCY=1.0
REBUILD_GAP=2.0

# Custom hotkeys (DECKDRIOD_CMD_<key>=command)
DECKDRIOD_CMD_T="adb shell input text 'testuser'"
```

### Available Settings

- **APP_ID**: The package name of your Android application.
- **ACTIVITY**: The full component name of your main activity (used for launching).
- **WATCH_LATENCY**: Delay in seconds for the file watcher to detect changes.
- **REBUILD_GAP**: Minimum time in seconds between automatic rebuilds (throttling).

---

## 🤝 Contributing

Contributions are welcome! If you'd like to improve DeckDriod, please follow these steps:

1.  **Fork** the repository.
2.  **Create a branch** for your feature or fix (`git checkout -b feature/amazing-feature`).
3.  **Commit** your changes (`git commit -m 'Add amazing feature'`).
4.  **Push** to the branch (`git push origin feature/amazing-feature`).
5.  **Open a Pull Request**.

### Development Setup
```bash
git clone git@github.com:theasmat/deckdriod.git
cd deckdriod
cargo build
```

---

## 📜 License

This project is licensed under the **MIT License**. See the [LICENSE](LICENSE) file for details.

---

Created with ❤️ by theasmat
