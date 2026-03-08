// crates/guardian-mcp/src/tool_trait.rs
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
use guardian_core::db::Database;

/// The pluggable Tool interface for the MCP Gateway.
///
/// BAD (narrow): A giant `match tool_name { "resolve_path" => ..., "get_photos" => ... }`
/// GOOD (broad): Every capability is a struct. The server holds Vec<Box<dyn GuardianTool>>
///               and dispatches dynamically — no match statement ever needs to change.
///
/// To add a new tool tomorrow (e.g. NetworkScanTool, RegistryWatchTool):
///   1. Create a new struct that implements this trait.
///   2. Call `server.register_tool(Box::new(YourNewTool))` in main.rs.
///   That is ALL. Zero other changes.
#[async_trait]
pub trait GuardianTool: Send + Sync {
    /// Unique name used in MCP "tools/call" → params.name
    fn name(&self) -> &'static str;

    /// Human-readable description returned in "tools/list"
    fn description(&self) -> &'static str;

    /// JSON Schema for the tool's input arguments.
    /// Kept as Value so any schema shape is supported without modifying this trait.
    fn input_schema(&self) -> Value;

    /// Execute the tool. Receives raw JSON args and shared DB access.
    /// Returns a generic JSON Value — the server wraps it in the MCP envelope.
    async fn execute(&self, args: Value, db: Arc<Mutex<Database>>) -> Value;
}
