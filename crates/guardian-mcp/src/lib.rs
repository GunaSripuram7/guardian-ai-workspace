// crates/guardian-mcp/src/lib.rs
pub mod tool_trait;
pub mod tools;
pub mod server;

pub use tool_trait::GuardianTool;
pub use server::{McpServer, McpRequest, McpResponse};
pub use tools::{QuerySemanticContextTool, LogAgentIntentTool, GetSystemStateTool};

