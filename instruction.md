# Plan: Rewrite kdev as a Standalone Rust Binary

## Objective
Provide a highly detailed, step-by-step instruction set for an AI assistant to implement the `kdev` tool as a standalone Rust binary in a new, separate directory (`tools/kdev-rs`). This binary will replace the existing `scripts/kdev/kdev.sh` bash script, eliminating the `fswatch` system dependency by using native Rust crates for file watching and process management.

## AI Implementation Instructions

You are tasked with building a standalone Rust CLI tool that replicates the functionality of the `kdev` bash script. 
Target directory: `tools/kdev-rs`

### Phase 1: Project Setup and Dependencies
1.  **Initialize Project:** Create a new Rust project by running `cargo new tools/kdev-rs`.
2.  **Add Dependencies:** Add the following crates to `Cargo.toml`:
    *   `tokio` (features: `full`) for async runtime and process management.
    *   `notify` and `notify-debouncer-mini` for cross-platform file watching.
    *   `crossterm` for terminal raw mode, keyboard input, and styling.
    *   `dotenvy` for loading `.kdevconfig`.
    *   `anyhow` for error handling.

### Phase 2: Configuration & State Management
1.  **Config Loading:** Create a module to parse `.kdevconfig` from the project root.
    *   Load variables like `WATCH_LATENCY` (default: 1.0) and other constants (`APP_ID`, `ACTIVITY`).
2.  **App State:** Define a central state struct (e.g., using `Arc<RwLock<State>>` or channel communication) to track:
    *   `auto_rebuild: bool`
    *   `auto_open: bool`
    *   `show_logs: bool`
    *   `device_serial: Option<String>`

### Phase 3: Device Discovery & Process Execution
1.  **Device Selection:** Implement a function that runs `adb devices`. If no devices, error out. If one device, use it. If multiple, prompt the user via terminal to select one.
2.  **Command Runners:** Create async functions to handle external commands:
    *   `build_and_launch()`: Spawns `./gradlew :androidApp:installDebug --quiet --parallel --configuration-cache --daemon`. If successful and `auto_open` is true, runs `adb shell am force-stop ...` and `adb shell am start -n ...`.
    *   `take_screenshot()`: Runs `adb shell screencap`, pulls the file, and removes it from the device.
    *   `clear_app_data()`: Runs `adb shell pm clear ...` and re-opens the app.

### Phase 4: Async Logcat Management
1.  **Logcat Task:** Implement a tokio task that spawns `adb logcat -v color -s Timber:V AndroidRuntime:E <APP_ID>:V`.
2.  **Lifecycle:** Ensure this process can be gracefully killed and restarted when the user toggles logs (the `l` key) or clears logs (the `c` key).

### Phase 5: File Watcher
1.  **Watcher Task:** Setup a `notify_debouncer_mini::new_debouncer` with the duration mapped from `WATCH_LATENCY`.
2.  **Paths:** Watch the `androidApp/src` and `shared/src` directories.
3.  **Signal:** When a debounced event occurs, send a `Rebuild` message through an `mpsc` channel to the main event loop.

### Phase 6: Terminal UI & Event Loop
1.  **Raw Mode:** Use `crossterm` to put the terminal into raw mode (`enable_raw_mode()`).
2.  **Header Rendering:** Create a function to clear the screen (`crossterm::terminal::Clear`) and print the formatted header displaying current device, app ID, and the toggle states (ON/OFF) with colors.
3.  **Main Loop:** Create a `tokio::select!` loop that listens to:
    *   **Keyboard Events:** via `crossterm::event::EventStream`. Handle keys:
        *   `r` / `Enter`: Trigger manual rebuild.
        *   `c`: Clear logs/screen.
        *   `w`: Toggle `auto_rebuild`.
        *   `o`: Toggle `auto_open`.
        *   `l`: Toggle `show_logs`.
        *   `s`: Take screenshot.
        *   `x`: Clear data.
        *   `d`: Dev menu (`adb shell input keyevent 82`).
        *   `q` / `Ctrl+C`: Initiate graceful shutdown.
    *   **Watcher Events:** Receive `Rebuild` signals. If `auto_rebuild` is true, trigger `build_and_launch`.
4.  **Graceful Shutdown:** Ensure `disable_raw_mode()` is called, and all child processes (`gradlew`, `adb logcat`) are explicitly killed on exit.

### Phase 7: Integration
1.  **Wrapper Script:** Once tested, modify `.direnv/bin/kdev` to execute the built Rust binary (e.g., `cd tools/kdev-rs && cargo run --release -- "$@"` or compile once and run the executable).

## Verification & Testing
*   The AI must verify that `fswatch` is no longer required.
*   The AI must ensure that terminal raw mode is properly cleaned up on panics or exits.
*   The AI must verify that `gradlew` runs without stealing terminal input focus.

## Migration & Rollback
*   Retain the bash scripts in `scripts/kdev/` during development.
*   The `.direnv/bin/kdev` script serves as the toggle switch between the old bash implementation and the new Rust implementation.