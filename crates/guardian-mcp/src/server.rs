// crates/guardian-mcp/src/server.rs
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use guardian_core::db::Database;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
    routing::{get, post},
    Json, Router,
};
use crate::tool_trait::GuardianTool;


#[derive(Deserialize, Debug)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id:      Option<Value>,
    pub method:  String,
    pub params:  Option<Value>,
}

#[derive(Serialize, Debug)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id:      Option<Value>,
    pub result:  Option<Value>,
    pub error:   Option<Value>,
}

 // ── SSE session store — maps sessionId → response channel ────────────────────
// Each connected agent gets a UUID session. Responses travel back through SSE.
#[derive(Clone)]
struct SseAppState {
    server:   Arc<McpServer>,
    sessions: Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>,
    base_url: String,
}




/// The MCP Gateway. Holds a Vec<Box<dyn GuardianTool>>.
///
/// Adding a new tool:  mcp_server.register_tool(Box::new(MyNewTool));
/// That is all. This file never changes.
pub struct McpServer {
    db:Arc<Mutex<Database>>,
    tools: Vec<Box<dyn GuardianTool>>,
    // Phase 2: injected protection engine (optional — None = Phase 1 mode)
    pub protection: Option<Arc<guardian_protection::ProtectionEngine>>,
}
async fn sse_handler(
    State(state): State<SseAppState>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let session_id   = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = mpsc::channel::<String>(32);
    state.sessions.lock().await.insert(session_id.clone(), tx);
    let endpoint_url = format!("{}/mcp/{}", state.base_url, session_id);
    println!("[MCP SSE] New session: {}", &session_id[..8]);

    let stream = async_stream::stream! {
        yield Ok(Event::default().event("endpoint").data(endpoint_url));
        while let Some(msg) = rx.recv().await {
            yield Ok(Event::default().event("message").data(msg));
        }
    };
    Sse::new(stream)
}

async fn mcp_messages_handler(
    State(state): State<SseAppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(req): Json<McpRequest>,
) -> StatusCode {
    let session_id = params
        .get("sessionId").cloned()
        .or_else(|| params.get("session_id").cloned())
        .or_else(|| {
            headers.get("Mcp-Session-Id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    if session_id.is_empty() {
        eprintln!("[MCP] Missing session ID on POST /messages");
        return StatusCode::BAD_REQUEST;
    }

    let response     = state.server.process_request(req).await;
    let response_str = serde_json::to_string(&response).unwrap_or_default();

    let tx_opt = { state.sessions.lock().await.get(&session_id).cloned() };
    match tx_opt {
        Some(tx) => if tx.send(response_str).await.is_ok() { StatusCode::ACCEPTED } else { StatusCode::GONE },
        None     => { eprintln!("[MCP] No session: {}", &session_id); StatusCode::NOT_FOUND }
    }
}

async fn mcp_session_path_handler(
    State(state): State<SseAppState>,
    Path(session_id): Path<String>,
    Json(req): Json<McpRequest>,
) -> StatusCode {
    let response     = state.server.process_request(req).await;
    let response_str = serde_json::to_string(&response).unwrap_or_default();

    let tx_opt = { state.sessions.lock().await.get(&session_id).cloned() };
    match tx_opt {
        Some(tx) => if tx.send(response_str).await.is_ok() { StatusCode::ACCEPTED } else { StatusCode::GONE },
        None     => { eprintln!("[MCP] No session: {}", &session_id); StatusCode::NOT_FOUND }
    }
}

async fn handle_get_root() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({
        "server": "guardian-ai", "version": "0.1.0",
        "transport": "http", "note": "POST JSON-RPC to this endpoint"
    })))
}

async fn no_auth_handler() -> StatusCode {
    StatusCode::NOT_FOUND
}


impl McpServer {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db, tools: Vec::new(), protection: None }
    }

    pub fn with_protection(mut self, engine: Arc<guardian_protection::ProtectionEngine>) -> Self {
        self.protection = Some(engine);
        self
    }

    /// Plug a new capability into the Gateway.
    /// No match statements, no hardcoded names — just push to the Vec.
    pub fn register_tool(&mut self, tool: Box<dyn GuardianTool>) {
        println!("Guardian MCP: Registered tool '{}'", tool.name());
        self.tools.push(tool);
    }

    // ── NEW HTTP SERVE IMPLEMENTATION ──────────────────────────────
    /// Start an HTTP JSON-RPC server that implements the MCP HTTP
    /// transport expected by mcporter and other MCP clients.
    /// It listens on 127.0.0.1:port and accepts POST / with a JSON body.
    pub async fn serve(self: Arc<Self>, port: u16) {
    let addr     = SocketAddr::from(([127, 0, 0, 1], port));
    let base_url = format!("http://{}", addr);

    let state = SseAppState {
        server:   Arc::clone(&self),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        base_url,
    };

    let app = Router::new()
        .route("/",        post(handle_mcp_http).get(handle_get_root))
        .route("/sse",     get(sse_handler))
        .route("/messages", post(mcp_messages_handler))
        .route("/mcp/:session_id", post(mcp_session_path_handler))
        .route("/.well-known/oauth-protected-resource",   get(no_auth_handler))
        .route("/.well-known/oauth-authorization-server", get(no_auth_handler))
        .with_state(state);

    println!("[MCP SERVER] Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("[MCP] FATAL: Could not bind to {} — {}", addr, e));

    axum::serve(listener, app)
        .await
        .expect("[MCP] HTTP server crashed");
}


    // ── END NEW SERVE ───────────────────────────────────────────────

    
/*
    // ── SSE SERVER: MCP SSE transport (mcporter/OpenClaw compatible) ──────────
    /// GET /sse  → opens SSE stream, sends endpoint event, streams all responses
    /// POST /messages?sessionId=xxx → receives JSON-RPC, routes back through SSE
    pub async fn serve(self: Arc<Self>, port: u16) {
        let addr     = format!("127.0.0.1:{}", port);
        let base_url = format!("http://{}", addr);

        let state = SseAppState {
            server:   Arc::clone(&self),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            // ── COMPAT: used by sse_handler to emit absolute endpoint URL ─────
            base_url: base_url.clone(),
            // ── END COMPAT ────────────────────────────────────────────────────
        };

        let app = Router::new()
            .route(
                "/mcp",
                axum::routing::post(streamable_post_handler)
                    .get(streamable_get_handler),
            )
            // ── Explicit 404 on OAuth discovery routes — signals no auth required ──
            .route("/.well-known/oauth-protected-resource",  axum::routing::get(no_auth_handler))
            .route("/.well-known/oauth-authorization-server", axum::routing::get(no_auth_handler))
            // ── END OAuth discovery ───────────────────────────────────────────────
            .with_state(state);






        let listener = TcpListener::bind(&addr)
            .await
            .unwrap_or_else(|e| panic!("[MCP] FATAL: Could not bind to {} — {}", addr, e));
        println!("[MCP SERVER] Listening on http://{}", addr);

        axum::serve(listener, app)
            .await
            .unwrap_or_else(|e| eprintln!("[MCP] Server error: {}", e));
    }
    // ── END SSE SERVER ────────────────────────────────────────────────────────
    */

    pub async fn process_request(&self, req: McpRequest) -> McpResponse {
        match req.method.as_str() {
            // ── OpenClaw handshake — must reply before tools/list is called ───
            "initialize" => self.success_response(req.id, serde_json::json!({
                "protocolVersion": "2025-03-26",
                "serverInfo": { "name": "guardian-ai", "version": "0.1.0" },
                "capabilities": { "tools": {} }
            })),
            // ── END ───────────────────────────────────────────────────────────
            "tools/list" => self.handle_list(req.id),
            "tools/call" => self.handle_call(req).await,
            other => self.error_response(
                req.id,
                -32601,
                &format!("Unknown method '{}'. Supported: tools/list, tools/call", other),
            ),
        }
    }

    /// Dynamically builds the tool list from the registered Vec.
    /// No hardcoding — adding a tool via register_tool() automatically
    /// makes it appear here.
    fn handle_list(&self, id: Option<Value>) -> McpResponse {
        let tool_list: Vec<Value> = self.tools.iter().map(|t| {
            serde_json::json!({
                "name":        t.name(),
                "description": t.description(),
                "inputSchema": t.input_schema(),
            })
        }).collect();
        self.success_response(id, serde_json::json!({ "tools": tool_list }))
    }

    /// Finds the tool by name in the Vec and calls execute().
    /// No match on tool names — just a Vec::iter().find().
    async fn handle_call(&self, req: McpRequest) -> McpResponse {
        let params = match req.params {
            Some(p) => p,
            None    => return self.error_response(req.id, -32602, "Missing params"),
        };

        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let args      = params.get("arguments").cloned().unwrap_or(Value::Null);

        match self.tools.iter().find(|t| t.name() == tool_name) {
            Some(tool) => {
                let result = tool.execute(args, Arc::clone(&self.db)).await;
                self.success_response(
                    req.id,
                    serde_json::json!({ "content": [{ "type": "json", "data": result }] }),
                )
            }
            None => self.error_response(
                req.id,
                -32601,
                &format!(
                    "Tool '{}' not registered. Call tools/list to see available tools.",
                    tool_name
                ),
            ),
        }
    }

    fn success_response(&self, id: Option<Value>, result: Value) -> McpResponse {
        McpResponse { jsonrpc: "2.0".to_string(), id, result: Some(result), error: None }
    }

    fn error_response(&self, id: Option<Value>, code: i32, message: &str) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error:  Some(serde_json::json!({ "code": code, "message": message })),
        }
    }
}

// ── HTTP handler for MCP JSON-RPC over HTTP ───────────────────────
async fn handle_mcp_http(
    State(state): State<SseAppState>,
    Json(req): Json<McpRequest>,
) -> impl IntoResponse {
    let res = state.server.process_request(req).await;
    (StatusCode::OK, Json(res))
}


// ── END HTTP HANDLER ──────────────────────────────────────────────


