# DeckDriod Improvement Plan

**Current Version:** v1.1.2  
**Codebase Size:** ~2,341 lines of Rust  
**Status:** Functional, needs polish & robustness

---

## 🔴 Critical Fixes

### 1. Error Handling
**Problem:** 20+ `.unwrap()` calls that can panic  
**Impact:** App crashes on edge cases (missing config, bad paths, network issues)  
**Fix:**
- Replace `unwrap()` with `unwrap_or_default()` or `?` operator
- Add graceful fallbacks for all I/O operations
- Show user-friendly error messages instead of panics

### 2. Lock Contention
**Problem:** `shared_logs.write().unwrap()` called in hot path (every log line)  
**Impact:** Potential deadlocks, UI stuttering under heavy logging  
**Fix:**
- Batch log writes (buffer 10-50 lines, flush every 100ms)
- Use `try_write()` with skip on contention
- Consider lock-free ring buffer for MCP reads

### 3. Memory Leaks
**Problem:** `cache_all`, `cache_app`, `cache_build` grow to 5000 lines each  
**Impact:** 15K+ lines in memory = ~5-10MB wasted  
**Fix:**
- Unified cache with single 5000-line circular buffer
- Separate views are just filtered iterators
- Drop old lines more aggressively (keep last 2000)

---

## 🟡 UX Improvements

### 4. Build Feedback
**Problem:** "building..." with no progress indicator  
**Impact:** Users don't know if it's stuck or working  
**Fix:**
- Show elapsed time: `BUILDING 1m 23s`
- Progress bar based on task count (e.g., "12/45 tasks")
- Estimated time remaining from build history

### 5. Log Search
**Problem:** `/` search is substring-only, no regex, no history  
**Impact:** Hard to find specific patterns  
**Fix:**
- Add regex support (toggle with `Ctrl+R`)
- Search history (↑↓ in search mode)
- Highlight all matches, not just filter
- Jump to next/prev match with `n`/`N`

### 6. Device Switching
**Problem:** Must restart to switch devices  
**Impact:** Annoying for multi-device testing  
**Fix:**
- Add `[D]` hotkey to open device selector
- Live-switch without restart
- Remember last device per project

### 7. Tab Persistence
**Problem:** Always starts on Dashboard, loses your place  
**Impact:** Muscle memory broken on restart  
**Fix:**
- Remember last active tab in `.deckdriodconfig`
- Restore scroll position per tab

---

## 🟢 Feature Additions

### 8. Logcat Filters
**Problem:** Only one global filter (log_tag)  
**Impact:** Can't filter by package, PID, or custom tags  
**Fix:**
- Add filter builder UI (press `F`)
- Multiple filters: package, tag, PID, level
- Save filter presets

### 9. Build Profiles
**Problem:** Only `installDebug` hardcoded  
**Impact:** Can't build release, flavors, or custom tasks  
**Fix:**
- Detect build variants from `build.gradle.kts`
- Add variant selector (press `V`)
- Remember last variant per project

### 10. Performance Profiling
**Problem:** CPU/MEM sparklines are app-wide, not per-thread  
**Impact:** Can't identify which thread is leaking  
**Fix:**
- Add thread view (press `T`)
- Show top 5 threads by CPU
- Memory breakdown by heap/native/graphics

### 11. Network Monitor
**Problem:** No visibility into network requests  
**Impact:** Can't debug API failures without Logcat parsing  
**Fix:**
- Parse OkHttp/Retrofit logs automatically
- Show request/response table
- Highlight failed requests (4xx/5xx)

### 12. Crash History
**Problem:** Only shows last crash  
**Impact:** Can't compare multiple crashes  
**Fix:**
- Keep last 10 crashes in memory
- Crash list view (press `H` in Errors tab)
- Export all crashes to JSON

---

## 🔵 Code Quality

### 13. Module Split
**Problem:** `main.rs` is 1176 lines — too large  
**Impact:** Hard to navigate, slow compile times  
**Fix:**
```
src/
  ui/
    mod.rs       - ui() entry point
    dashboard.rs - dashboard rendering
    logs.rs      - log view rendering
    popups.rs    - help, settings, picker
  events/
    mod.rs       - event loop
    keys.rs      - key handlers
    mouse.rs     - mouse handlers
```

### 14. Testing
**Problem:** Zero tests  
**Impact:** Regressions go unnoticed  
**Fix:**
- Unit tests for config parsing
- Integration tests for ADB command building
- Mock ADB for CI/CD

### 15. Documentation
**Problem:** No inline docs, only README  
**Impact:** Hard for contributors  
**Fix:**
- Add rustdoc comments to all public functions
- Architecture diagram in `ARCHITECTURE.md`
- Contributing guide

---

## 🟣 Performance

### 16. Lazy Rendering
**Problem:** Redraws entire UI at 30fps even when idle  
**Impact:** Wastes CPU, drains battery  
**Fix:**
- Only redraw on state change
- Dirty flag per widget
- Drop to 5fps when idle

### 17. Log Parsing
**Problem:** Regex on every line for level detection  
**Impact:** CPU spike on log bursts  
**Fix:**
- Cache parsed log level in struct
- Parse once, render many times

### 18. Sparkline Optimization
**Problem:** Recalculates sparkline data every frame  
**Impact:** Wasted CPU  
**Fix:**
- Only recalculate on new data point
- Pre-scale values to u64 once

---

## 📊 Priority Matrix

| Priority | Item | Effort | Impact |
|----------|------|--------|--------|
| P0 | Error handling (#1) | 2h | High |
| P0 | Lock contention (#2) | 3h | High |
| P1 | Build feedback (#4) | 1h | High |
| P1 | Device switching (#6) | 2h | Medium |
| P1 | Module split (#13) | 4h | Medium |
| P2 | Log search (#5) | 3h | Medium |
| P2 | Memory leaks (#3) | 2h | Low |
| P2 | Lazy rendering (#16) | 2h | Low |
| P3 | Build profiles (#9) | 4h | Medium |
| P3 | Crash history (#12) | 2h | Low |
| P3 | Testing (#14) | 8h | Low |

---

## 🎯 Roadmap

### v1.2.0 — Stability (1 week)
- ✅ Fix all unwrap() panics
- ✅ Fix lock contention
- ✅ Add error recovery
- ✅ Module split

### v1.3.0 — UX Polish (1 week)
- ✅ Build progress indicator
- ✅ Device switcher
- ✅ Tab persistence
- ✅ Better log search

### v1.4.0 — Power Features (2 weeks)
- ✅ Build profiles
- ✅ Logcat filter builder
- ✅ Crash history
- ✅ Network monitor

### v2.0.0 — Performance (1 week)
- ✅ Lazy rendering
- ✅ Memory optimization
- ✅ Log parsing cache
- ✅ Benchmarks

---

## 🐛 Known Bugs

1. **Scroll position resets** when switching tabs → save per-tab scroll
2. **Mouse selection breaks** on wrapped lines → fix coordinate mapping
3. **MCP server doesn't stop** cleanly on quit → add Drop impl
4. **Gradle daemon hangs** on first run → add `--no-daemon` flag option
5. **Battery % shows --** on some devices → fallback to dumpsys battery
6. **Sparklines flicker** on resize → debounce resize events
7. **Config save fails** silently → show error toast
8. **Deep link input** doesn't support paste → add Ctrl+V handler

---

## 💡 Future Ideas

- **Plugin system** — Lua scripts for custom commands
- **Remote debugging** — Connect to device over WiFi
- **APK analyzer** — Show APK size breakdown
- **Git integration** — Show current branch, dirty files
- **Notification system** — Desktop alerts on crash/build fail
- **Multi-project** — Switch between projects without restart
- **Cloud sync** — Sync config across machines
- **Themes** — Custom color schemes
- **Vim mode** — hjkl navigation everywhere
- **Export session** — Save entire session (logs + state) to file

---

## 📝 Notes

- Keep binary size under 5MB (currently ~2.4MB)
- Maintain zero-dependency startup (no internet required)
- Target 60fps UI on M1 Mac, 30fps on older Intel
- Support Android API 21+ devices
- Keep MCP protocol stable for Claude integration
