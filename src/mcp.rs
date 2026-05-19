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
}

pub async fn run_server(port: u16, shared_logs: Arc<RwLock<SharedLogState>>, mut shutdown_rx: mpsc::Receiver<()>) {
    let state = AppState { logs: shared_logs };

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

async fn usage_handler() -> String {
    get_detailed_guide()
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
                        "name": "get_usage_guide",
                        "description": "Returns the complete DeckDriod Knowledge Base, including architecture, hotkeys, and AI debugging prompts.",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "get_recent_logs",
                        "description": "Returns the most recent application logs.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "count": { "type": "integer", "description": "Number of lines", "default": 100 }
                            }
                        }
                    },
                    {
                        "name": "get_errors",
                        "description": "Returns captured errors.",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "get_latest_crash",
                        "description": "Returns the last crash trace.",
                        "inputSchema": { "type": "object", "properties": {} }
                    }
                ]
            }
        }),
        "tools/call" => {
            let tool_name = request["params"]["name"].as_str().unwrap_or("");
            let result = match tool_name {
                "get_usage_guide" => {
                    json!({ "content": [{ "type": "text", "text": get_detailed_guide() }] })
                },
                "get_recent_logs" => {
                    let count = request["params"]["arguments"]["count"].as_u64().unwrap_or(100) as usize;
                    let logs = state.logs.read().unwrap();
                    let start = logs.app_logs.len().saturating_sub(count);
                    json!({ "content": [{ "type": "text", "text": logs.app_logs[start..].join("\n") }] })
                },
                "get_errors" => {
                    let logs = state.logs.read().unwrap();
                    json!({ "content": [{ "type": "text", "text": logs.error_logs.join("\n") }] })
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

pub fn get_detailed_guide() -> String {
    r#"# 🚀 DeckDriod Knowledge Base & Usage Guide

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
Connect any MCP client to: `http://localhost:3000/sse`
You can change the port in `Settings (i)` or `.deckdriodconfig`.
"#.to_string()
}
