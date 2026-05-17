# 🚀 K-Dev: The Professional Android Development Dashboard

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**K-Dev** is a high-performance, interactive TUI (Terminal User Interface) built in Rust, designed to replace sluggish bash scripts and provide a unified dashboard for Kotlin and Android developers. It combines building, launching, log monitoring, and device profiling into a single, beautiful terminal view.

---

## ✨ Features

- **📊 Real-time Resource Monitoring:** Live sparklines for CPU and Memory usage of your specific App ID.
- **⚡️ Smart Build Telemetry:** Tracks current Gradle tasks and keeps a history of build durations.
- **🔍 Power Logcat:** Instant search, level filtering (1-5), and manual scroll pause/resume.
- **🚨 Crash Detection:** Automatically highlights fatal exceptions and keeps them visible until cleared.
- **🎥 Media Tools:** One-key screen recording and screenshots pulled directly to your workspace.
- **🔗 Deep Link Tester:** Quickly test deep links without touching your phone.
- **🛠 Layout Debugger:** Toggle system layout bounds with a single hotkey.
- **🔋 Battery Monitor:** Real-time device battery level tracking.
- **⌨️ Custom Hotkeys:** Map your own shell commands to hotkeys in `.deckdriodconfig`.

---

## 🚀 Installation

### 1. Via Quick Install Script (macOS/Linux)
```bash
curl -sSf https://raw.githubusercontent.com/theasmat/deckdriod/main/install.sh | sh
```

### 2. Via Homebrew
```bash
brew tap theasmat/tap
brew install deckdriod
```

### 3. From Source
```bash
cargo install --path .
```

---

## 📖 How to Use

Simply run `deckdriod` in the root of your Kotlin Multiplatform or Android project.

### Hotkeys

| Key | Action |
| :--- | :--- |
| `r` / `Enter` | Trigger Manual Build & Launch |
| `/` | Search Logs |
| `h` | Open Advanced Help Popup |
| `c` | Clear Logs & Crash Alerts |
| `v` | Start/Stop Screen Recording |
| `s` | Take Screenshot |
| `u` | Open Deep Link URL |
| `b` | Toggle Layout Bounds |
| `e` | Export Log Buffer to `deckdriod_logs.txt` |
| `1-5` | Set Minimum Log Level (Verbose to Error) |
| `↑`/`↓` | Scroll Logs (Pauses Auto-follow) |
| `G` | Resume Auto-follow Logs |
| `d` | Open Android Dev Menu |
| `x` | Clear App Data |
| `q` | Quit |

---

## ⚙️ Configuration

Create a `.deckdriodconfig` file in your project root to override defaults:

```ini
APP_ID=com.example.app.dev
ACTIVITY=com.example.app/com.example.app.MainActivity
WATCH_LATENCY=0.5

# Custom hotkeys (DECKDRIOD_CMD_<key>=command)
DECKDRIOD_CMD_T="adb shell am start -a android.intent.action.VIEW -d 'https://test.com'"
```

---

## 📦 Distribution & CI/CD

The project uses GitHub Actions to automatically build and release binaries for macOS and Linux on every tag push.

---

## 📜 License

This project is licensed under the **MIT License**. See the [LICENSE](LICENSE) file for details.

---

Created with ❤️ by Gemini CLI
