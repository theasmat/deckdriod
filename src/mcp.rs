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
                        "name": "get_recent_logs",
                        "description": "Returns recent app logcat lines. Filter by level: V/D/I/W/E.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "count": { "type": "integer", "description": "Number of lines (default 100)" },
                                "level": { "type": "string", "description": "Min log level: V, D, I, W, E" },
                                "filter": { "type": "string", "description": "Substring filter" }
                            }
                        }
                    },
                    {
                        "name": "get_build_logs",
                        "description": "Returns Gradle build output lines.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "count": { "type": "integer", "description": "Number of lines (default 50)" }
                            }
                        }
                    },
                    {
                        "name": "get_build_status",
                        "description": "Returns current build status: idle/building/success/failed and last build duration.",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "get_errors",
                        "description": "Returns all captured error and crash logs.",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "get_latest_crash",
                        "description": "Returns the full stack trace of the last crash.",
                        "inputSchema": { "type": "object", "properties": {} }
                    }
                ]
            }
        }),
        "tools/call" => {
            let tool_name = request["params"]["name"].as_str().unwrap_or("");
            let result = match tool_name {
                "get_recent_logs" => {
                    let count = request["params"]["arguments"]["count"].as_u64().unwrap_or(100) as usize;
                    let level_filter = request["params"]["arguments"]["level"].as_str().unwrap_or("").to_uppercase();
                    let substr = request["params"]["arguments"]["filter"].as_str().unwrap_or("").to_lowercase();
                    let logs = state.logs.read().unwrap();
                    let level_rank = |l: &str| -> u8 {
                        if l.contains(" E/") || l.contains("[err]") { 4 }
                        else if l.contains(" W/") { 3 }
                        else if l.contains(" I/") { 2 }
                        else if l.contains(" D/") { 1 }
                        else { 0 }
                    };
                    let min_rank: u8 = match level_filter.as_str() {
                        "E" => 4, "W" => 3, "I" => 2, "D" => 1, _ => 0
                    };
                    let filtered: Vec<&String> = logs.app_logs.iter()
                        .filter(|l| level_rank(l) >= min_rank)
                        .filter(|l| substr.is_empty() || l.to_lowercase().contains(&substr))
                        .collect();
                    let start = filtered.len().saturating_sub(count);
                    let text = filtered[start..].iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
                    json!({ "content": [{ "type": "text", "text": text }] })
                },
                "get_build_logs" => {
                    let count = request["params"]["arguments"]["count"].as_u64().unwrap_or(50) as usize;
                    let logs = state.logs.read().unwrap();
                    let start = logs.build_logs.len().saturating_sub(count);
                    let text = logs.build_logs[start..].join("\n");
                    json!({ "content": [{ "type": "text", "text": if text.is_empty() { "No build logs yet.".to_string() } else { text } }] })
                },
                "get_build_status" => {
                    let logs = state.logs.read().unwrap();
                    let status = if logs.build_status.is_empty() { "idle" } else { &logs.build_status };
                    let task = logs.build_task.as_deref().unwrap_or("none");
                    json!({ "content": [{ "type": "text", "text": format!("status: {}\ncurrent_task: {}", status, task) }] })
                },
                "get_errors" => {
                    let logs = state.logs.read().unwrap();
                    let text = if logs.error_logs.is_empty() { "No errors.".to_string() } else { logs.error_logs.join("\n") };
                    json!({ "content": [{ "type": "text", "text": text }] })
                },
                "get_latest_crash" => {
                    let logs = state.logs.read().unwrap();
                    let crash = logs.last_crash.as_deref().unwrap_or("No crash detected.");
                    json!({ "content": [{ "type": "text", "text": crash }] })
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
    format!(r#"# 🚀 DeckDriod Knowledge Base & Usage Guide

## 📱 What is DeckDriod?
DeckDriod is a high-performance, unified terminal dashboard for Android & Kotlin development, built in **Rust**. It eliminates the need for switching between Android Studio, separate Logcat terminals, and resource monitors.

### Key Architecture:
- **Reactive TUI**: 30fps smooth UI using the `ratatui` crate.
- **Non-Blocking Logic**: All ADB, Gradle, and system polling occur in background `tokio` tasks.
- **AI-Powered Debugging**: Built-in **MCP (Model Context Protocol)** server allows LLMs to "see" your device state.
- **Virtualized Rendering**: Handles 5,000+ log lines with near-zero CPU overhead.

## 📑 Views & Navigation
- **[1] Dashboard**: High-level telemetry. Sparklines for CPU/Memory, Active Gradle tasks, and Command Shortcuts.
- **[2] App Logs**: Dedicated Logcat view. Auto-prettifies JSON payloads and dims stack trace boilerplate.
- **[3] Build Logs**: Real-time Gradle output. Perfect for debugging complex build scripts.
- **[4] Errors**: Auto-filtered view showing only crashes (`FATAL EXCEPTION`) and `E/` level logs.

## ⌨️ Essential Hotkeys
- **a / r / Ent**: Build and Launch the application (standard).
- **f**: **Force Rebuild**. Cleans project and ignores Gradle cache for fresh builds.
- **L**: **Launch Only**. Restarts the app on device without waiting for a rebuild.
- **B (Shift+B)**: **Broadcast Mode**. Executes actions on ALL connected devices/emulators at once.
- **E**: **Emulator Selector**. Discover and boot local AVDs without leaving the terminal.
- **M (Shift+M)**: **MCP Toggle**. Starts/Stops the AI bridge server.
- **y**: **Intelligent Yank**. Copies selection (if active) or the top visible log line.
- **s / v**: Instant Screenshot / MP4 Video Recording (saved to your configured `OUTPUT_PATH`).

## 🤖 AI Integration & MCP Prompts
By enabling MCP, you can use AI assistants to solve complex issues. Give your AI the following context or use these prompts:

### Recommended AI System Context:
"You are an expert Android/Kotlin developer. You have access to the DeckDriod MCP server which provides real-time logs and crash reports. Use `get_latest_crash` to analyze errors and `get_recent_logs` to understand app state."

### Powerful AI Prompts:
1. **Debug a Crash**: "I just hit a crash. Use `get_latest_crash` to read the stack trace and explain why it happened in my Kotlin code."
2. **Analyze Performance**: "Read the last 100 lines of logs using `get_recent_logs`. Do you see any repeated network requests or memory warnings?"
3. **Understand App Logic**: "The app logs structured JSON with the prefix `[DeckDriod]`. Use `get_recent_logs` to analyze the most recent app state change."
4. **Fix Build Errors**: "My Gradle build failed. Read the build logs from `get_recent_logs` and suggest what I need to change in my `build.gradle.kts`."

## ⚙️ Configuration (MCP Setup)
Connect any MCP client to: `http://localhost:{}/sse`
You can change the port in `Settings (i)` or `.deckdriodconfig`.
"#, port)
}
