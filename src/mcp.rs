use axum::{
    extract::{State, Query},
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::{self, Stream};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::{Arc, RwLock}};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;

use crate::state::SharedLogState;

#[derive(Clone)]
struct AppState {
    logs: Arc<RwLock<SharedLogState>>,
    port: u16,
}

pub async fn run_server(port: u16, shared_logs: Arc<RwLock<SharedLogState>>, mut shutdown_rx: mpsc::Receiver<()>) {
    let state = AppState { logs: shared_logs, port };

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/usage", get(usage_handler))
        .route("/sse", get(sse_handler))
        .route("/messages", post(message_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await.unwrap();
    
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
        })
        .await
        .unwrap();
}

async fn root_handler() -> &'static str {
    "DeckDriod MCP Server is running!\n\nRoutes:\n- /usage : Detailed usage guide & AI Prompts\n- /sse   : MCP SSE endpoint for AI assistants"
}

async fn usage_handler(State(state): State<AppState>) -> String {
    get_detailed_guide(state.port)
}

async fn sse_handler(
    State(_state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let session_id = uuid_v4();
    
    let stream = stream::once(async move {
        let endpoint_url = format!("/messages?sessionId={}", session_id);
        Ok(Event::default().event("endpoint").data(endpoint_url))
    });

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new())
}

#[derive(Deserialize)]
struct MessageQuery {
    #[serde(rename = "sessionId")]
    _session_id: String,
}

async fn message_handler(
    State(state): State<AppState>,
    Query(_query): Query<MessageQuery>,
    Json(request): Json<Value>,
) -> Json<Value> {
    let method = request["method"].as_str().unwrap_or("");
    let id = request["id"].clone();

    let response = match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "deckdriod-mcp", "version": "0.1.0" }
            }
        }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [
                    {
                        "name": "get_logs",
                        "description": "Fetch logs by type. type: 'app' (logcat), 'build' (gradle), 'errors' (E/ + crashes), 'all' (everything), 'crash' (last crash trace).",
                        "inputSchema": {
                            "type": "object",
                            "required": ["type"],
                            "properties": {
                                "type":   { "type": "string", "enum": ["app","build","errors","all","crash"] },
                                "count":  { "type": "integer", "description": "Max lines to return (default 100)" },
                                "level":  { "type": "string",  "description": "Min log level for app logs: V D I W E" },
                                "filter": { "type": "string",  "description": "Substring filter" }
                            }
                        }
                    },
                    {
                        "name": "get_build_status",
                        "description": "Returns current build state: idle/Building/Success/Failed, current Gradle task, and history.",
                        "inputSchema": { "type": "object", "properties": {} }
                    }
                ]
            }
        }),
        "tools/call" => {
            let tool_name = request["params"]["name"].as_str().unwrap_or("");
            let result = match tool_name {
                "get_logs" => {
                    let log_type = request["params"]["arguments"]["type"].as_str().unwrap_or("app");
                    let count    = request["params"]["arguments"]["count"].as_u64().unwrap_or(100) as usize;
                    let level_filter = request["params"]["arguments"]["level"].as_str().unwrap_or("").to_uppercase();
                    let substr   = request["params"]["arguments"]["filter"].as_str().unwrap_or("").to_lowercase();
                    let logs = state.logs.read().unwrap();

                    let level_rank = |l: &str| -> u8 {
                        if l.contains(" E/") || l.contains("[err]") || l.contains("FATAL") { 4 }
                        else if l.contains(" W/") { 3 }
                        else if l.contains(" I/") { 2 }
                        else if l.contains(" D/") { 1 }
                        else { 0 }
                    };
                    let min_rank: u8 = match level_filter.as_str() {
                        "E" => 4, "W" => 3, "I" => 2, "D" => 1, _ => 0
                    };

                    let text = match log_type {
                        "crash" => logs.last_crash.clone().unwrap_or_else(|| "No crash detected.".to_string()),
                        "build" => {
                            let src = logs.build_logs.iter()
                                .filter(|l| substr.is_empty() || l.to_lowercase().contains(&substr))
                                .collect::<Vec<_>>();
                            let start = src.len().saturating_sub(count);
                            if src.is_empty() { "No build logs yet.".to_string() }
                            else { src[start..].iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n") }
                        },
                        "errors" => {
                            let src = logs.error_logs.iter()
                                .filter(|l| substr.is_empty() || l.to_lowercase().contains(&substr))
                                .collect::<Vec<_>>();
                            let start = src.len().saturating_sub(count);
                            if src.is_empty() { "No errors.".to_string() }
                            else { src[start..].iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n") }
                        },
                        "all" => {
                            let mut all: Vec<&String> = logs.app_logs.iter().chain(logs.build_logs.iter()).collect();
                            all.sort_unstable(); // rough chronological order by content
                            let filtered: Vec<_> = all.iter()
                                .filter(|l| level_rank(l) >= min_rank)
                                .filter(|l| substr.is_empty() || l.to_lowercase().contains(&substr))
                                .collect();
                            let start = filtered.len().saturating_sub(count);
                            filtered[start..].iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
                        },
                        _ => { // "app" default
                            let filtered: Vec<_> = logs.app_logs.iter()
                                .filter(|l| level_rank(l) >= min_rank)
                                .filter(|l| substr.is_empty() || l.to_lowercase().contains(&substr))
                                .collect();
                            let start = filtered.len().saturating_sub(count);
                            if filtered.is_empty() { "No app logs yet.".to_string() }
                            else { filtered[start..].iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n") }
                        }
                    };
                    json!({ "content": [{ "type": "text", "text": text }] })
                },
                "get_build_status" => {
                    let logs = state.logs.read().unwrap();
                    let status = if logs.build_status.is_empty() { "idle" } else { &logs.build_status };
                    let task = logs.build_task.as_deref().unwrap_or("none");
                    json!({ "content": [{ "type": "text", "text": format!("status: {}\ncurrent_task: {}", status, task) }] })
                },
                // keep old names as aliases for backwards compat
                "get_recent_logs" => {
                    let count = request["params"]["arguments"]["count"].as_u64().unwrap_or(100) as usize;
                    let logs = state.logs.read().unwrap();
                    let start = logs.app_logs.len().saturating_sub(count);
                    json!({ "content": [{ "type": "text", "text": logs.app_logs[start..].join("\n") }] })
                },
                "get_build_logs" => {
                    let count = request["params"]["arguments"]["count"].as_u64().unwrap_or(50) as usize;
                    let logs = state.logs.read().unwrap();
                    let start = logs.build_logs.len().saturating_sub(count);
                    json!({ "content": [{ "type": "text", "text": logs.build_logs[start..].join("\n") }] })
                },
                "get_errors" => {
                    let logs = state.logs.read().unwrap();
                    json!({ "content": [{ "type": "text", "text": logs.error_logs.join("\n") }] })
                },
                "get_latest_crash" => {
                    let logs = state.logs.read().unwrap();
                    json!({ "content": [{ "type": "text", "text": logs.last_crash.clone().unwrap_or_else(|| "No crash.".to_string()) }] })
                },
                _ => json!({ "isError": true, "content": [{ "type": "text", "text": format!("Unknown tool: {}", tool_name) }] })
            };
            json!({ "jsonrpc": "2.0", "id": id, "result": result })
        },
        _ => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "Method not found" } })
    };

    Json(response)
}

fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{:x}", now)
}

pub fn get_detailed_guide(port: u16) -> String {
    format!(r#"# DeckDriod MCP Guide — v{}

## What is DeckDriod?
A high-performance terminal dashboard for Android/Kotlin development built in Rust.
Combines build, logcat, crash detection, device stats, and AI integration in one TUI.

## MCP Server
Connect to: `http://localhost:{}/sse`
Toggle with **[M]** inside DeckDriod. Change port via `Settings [i]` or `.deckdriodconfig`.

### Claude Desktop config:
```json
{{
  "mcpServers": {{
    "deckdriod": {{
      "command": "curl",
      "args": ["-s", "http://localhost:{}/sse"]
    }}
  }}
}}
```

## Available MCP Tools

### `get_logs` — unified log access
Fetch any log stream with optional filtering.

| param | type | description |
|-------|------|-------------|
| `type` | string **(required)** | `app` · `build` · `errors` · `all` · `crash` |
| `count` | int | max lines (default 100) |
| `level` | string | min level for app logs: `V` `D` `I` `W` `E` |
| `filter` | string | substring filter |

**Examples:**
```
get_logs(type="app")                        → last 100 logcat lines
get_logs(type="app", level="E")             → only error lines
get_logs(type="app", filter="NetworkError") → lines containing NetworkError
get_logs(type="build")                      → last 100 gradle output lines
get_logs(type="errors")                     → all E/ + crash lines
get_logs(type="crash")                      → full last crash stack trace
get_logs(type="all", count=200)             → app + build logs combined
```

### `get_build_status`
Returns current build state and active Gradle task.
```
status: Building | Success (42.1s) | Failed | idle
current_task: :app:compileDebugKotlin
```

## AI Prompts

**Debug a crash:**
> Use `get_logs(type="crash")` to get the stack trace. Explain the root cause and which Kotlin file to fix.

**Fix a failed build:**
> Use `get_logs(type="build")` to read the Gradle output. Find the error and suggest the fix.

**Check build progress:**
> Use `get_build_status` to see what Gradle task is running and whether the build succeeded.

**Analyze app errors only:**
> Use `get_logs(type="errors")` to list all recent errors. Group them by type and suggest fixes.

**Filter specific logs:**
> Use `get_logs(type="app", level="E", filter="Network")` to find network errors.

**Full picture:**
> Use `get_logs(type="all", count=200)` to see everything — app logs and build output together.

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
"#, env!("CARGO_PKG_VERSION"), port, port)
}

pub fn install_mcp_config(port: u16) -> anyhow::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

    let paths = vec![
        // Claude Desktop
        home.join("Library/Application Support/Claude/claude_desktop_config.json"),
        // VS Code (Cline / Roo Code) Mac
        home.join("Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"),
        home.join("Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/cline_mcp_settings.json"),
        // Cursor Mac
        home.join("Library/Application Support/Cursor/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"),
        // VS Code (Cline / Roo Code) Linux
        home.join(".config/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"),
        home.join(".config/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/cline_mcp_settings.json"),
        // Cursor Linux
        home.join(".config/Cursor/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json"),
        // Antigravity IDE
        home.join(".gemini/antigravity-ide/mcp_config.json"),
        // Kiro
        home.join(".kiro/mcp.json"),
    ];

    let deckdriod_config = json!({
        "command": "curl",
        "args": ["-s", format!("http://localhost:{}/sse", port)]
    });

    let mut installed_count = 0;

    for path in paths {
        if let Some(parent) = path.parent() {
            // Only create config if the parent extension dir exists (to avoid creating folders for IDEs the user doesn't have)
            if parent.exists() {
                let mut data: serde_json::Value = if path.exists() {
                    let content = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
                    serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
                } else {
                    json!({})
                };

                if !data.is_object() {
                    data = json!({});
                }

                if data.get("mcpServers").is_none() {
                    data["mcpServers"] = json!({});
                }

                if let Some(servers) = data.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                    servers.insert("deckdriod".to_string(), deckdriod_config.clone());
                }

                if let Ok(new_content) = serde_json::to_string_pretty(&data) {
                    if std::fs::write(&path, new_content).is_ok() {
                        println!("✅ Installed MCP to {}", path.display());
                        installed_count += 1;
                    }
                }
            }
        }
    }

    if installed_count == 0 {
        println!("⚠️ Could not find any supported IDE configurations to install MCP.");
    }

    Ok(())
}
