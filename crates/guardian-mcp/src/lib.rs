// crates/guardian-mcp/src/lib.rs
pub mod tool_trait;
pub mod tools;
pub mod server;

pub use tool_trait::GuardianTool;
pub use server::{McpServer, McpRequest, McpResponse};
// ── FIX #10: Add ValidateTokenTool to exports ─────────────────────────────────
pub use tools::{QuerySemanticContextTool, LogAgentIntentTool, GetSystemStateTool, ValidateTokenTool};
// ── END FIX #10 ───────────────────────────────────────────────────────────────
