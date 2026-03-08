// crates/guardian-mcp/src/server.rs
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
use guardian_core::db::Database;
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

    pub async fn process_request(&self, req: McpRequest) -> McpResponse {
        match req.method.as_str() {
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
