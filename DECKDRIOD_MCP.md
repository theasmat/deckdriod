# DeckDriod MCP Guide — v1.0.9

## What is DeckDriod?
A high-performance terminal dashboard for Android/Kotlin development built in Rust.
Combines build, logcat, crash detection, device stats, and AI integration in one TUI.

## MCP Server
Connect to: `http://localhost:3000/sse`
Toggle with **[M]** inside DeckDriod. Change port via `Settings [i]` or `.deckdriodconfig`.

### Claude Desktop config:
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

## Available MCP Tools

### `get_recent_logs`
Returns recent app logcat lines.
- `count` (int, default 100) — number of lines
- `level` (string) — min log level: `V` `D` `I` `W` `E`
- `filter` (string) — substring filter

### `get_build_logs`
Returns Gradle build output.
- `count` (int, default 50) — number of lines

### `get_build_status`
Returns current build state.
- Output: `status: Building | Success (42.1s) | Failed | idle`
- Output: `current_task: :app:compileDebugKotlin`

### `get_errors`
Returns all captured error-level and crash log lines.

### `get_latest_crash`
Returns the full stack trace of the last `FATAL EXCEPTION`.

## AI Prompts

**Debug a crash:**
> Use `get_latest_crash` to get the stack trace. Explain the root cause and which Kotlin file to fix.

**Fix a failed build:**
> Use `get_build_logs` to read the Gradle output. Find the error and suggest the fix.

**Check build progress:**
> Use `get_build_status` to see what Gradle task is running and whether the build succeeded.

**Analyze app errors:**
> Use `get_errors` to list all recent errors. Group them by type and suggest fixes.

**Filter logs:**
> Use `get_recent_logs` with `level: "E"` and `filter: "NetworkError"` to find network failures.

## Views
| Key | View |
|-----|------|
| `1` | Dashboard — CPU/MEM sparklines, build status, quick commands |
| `2` | App Logs — full logcat output |
| `3` | Build Logs — live Gradle output |
| `4` | Errors — crashes and E/ lines only |

## Key Hotkeys
| Key | Action |
|-----|--------|
| `a/r/Enter` | Build & Launch |
| `f` | Force Rebuild (clean) |
| `L` | Launch Only |
| `B` | Broadcast to all devices |
| `M` | Toggle MCP server |
| `E` | Emulator selector |
| `i` | Settings |
| `/` | Search logs |
| `G` | Resume auto-follow |
| `s/v` | Screenshot / Screen record |
| `x` | Clear app data |
| `h` | Help |
| `q` | Quit |

## Configuration (`.deckdriodconfig`)
```ini
APP_ID=com.example.app
ACTIVITY=com.example.app/.MainActivity
PROJECT_PATH=/path/to/android/project
OUTPUT_PATH=/path/to/screenshots
MCP_PORT=3000
WATCH_LATENCY=1.0
REBUILD_GAP=2.0
```
