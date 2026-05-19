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
    "DeckDriod MCP Server is running!\n\nRoutes:\n- /usage : Detailed usage guide\n- /sse   : MCP SSE endpoint for AI assistants"
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
                        "description": "Returns a detailed guide on how to use DeckDriod, including all hotkeys and views.",
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
    r#"# 🚀 DeckDriod Usage Guide

DeckDriod is a high-performance Android TUI dashboard.

## 📑 Views
- [1] Dashboard: Resource usage & commands
- [2] App Logs: Main application logcat
- [3] Build: Real-time Gradle output
- [4] Errors: Crashes and filtered errors

## ⌨️ Essential Hotkeys
- a / r : Build & Launch (cached)
- f     : Force Rebuild (clean, no-cache)
- L     : Launch Only (skip Gradle)
- E     : Emulator Selector
- B     : Broadcast Mode (Run on ALL devices)
- M     : MCP Toggle (Enable AI debugging)
- s / v : Screenshot / Video recording
- c     : Clear all logs and alerts
- y / A : Copy selection / ALL logs
- /     : Search current log view

## 🤖 Why use MCP?
By enabling MCP (Shift+M), you allow AI assistants to:
1. Analyze crashes instantly using stack traces.
2. Debug app state by reading recent logs.
3. Monitor build progress and failures.

Connecting AI: Configure your assistant to connect to http://localhost:3000/sse
"#.to_string()
}
