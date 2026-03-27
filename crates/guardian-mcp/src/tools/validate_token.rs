// guardian-mcp/src/tools/validate_token.rs
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;
use guardian_core::db::Database;
use crate::tool_trait::GuardianTool;

pub struct ValidateTokenTool;

#[async_trait]
impl GuardianTool for ValidateTokenTool {
    // ── FIX 1: &'static str not &str (matches trait signature) ───────────────
    fn name(&self) -> &'static str { "validate_permission_token" }

    fn description(&self) -> &'static str {
        "Validate a PermissionToken before performing an approved action. \
         Token is consumed on first use — cannot be reused. \
         Required fields: token_id (string), agent_id (string)."
    }

    // ── FIX 2: Add missing input_schema() required by GuardianTool trait ──────
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "token_id": {
                    "type": "string",
                    "description": "The permission_token value returned by log_agent_intent"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Your agent_id — must match the one used in log_agent_intent"
                }
            },
            "required": ["token_id", "agent_id"]
        })
    }
    // ── END FIXES ─────────────────────────────────────────────────────────────

    async fn execute(&self, args: Value, db: Arc<Mutex<Database>>) -> Value {
        let token_id = match args["token_id"].as_str() {
            Some(t) => t.to_string(),
            None    => return json!({
                "valid":   false,
                "error":   "Missing required field: token_id",
                "message": "Include token_id from the permission_token you received."
            }),
        };

        let agent_id = match args["agent_id"].as_str() {
            Some(a) => a.to_string(),
            None    => return json!({
                "valid":   false,
                "error":   "Missing required field: agent_id",
                "message": "Include the agent_id you used in log_agent_intent."
            }),
        };

        let db_lock = db.lock().await;
        match db_lock.validate_and_consume_token(&token_id, &agent_id) {
            Ok(true) => {
                // Replace &token_id[..8] with this safe version in BOTH println lines:
                let preview = token_id.get(..8).unwrap_or(&token_id);
                println!("[TOKEN] ✅ Valid token consumed for agent '{}': {}", agent_id, preview);

                json!({
                    "valid":    true,
                    "token_id": token_id,
                    "message":  "Token accepted. You may proceed with the approved action. This token is now consumed and cannot be reused."
                })
            }
            Ok(false) => {
                let preview = token_id.get(..8).unwrap_or(&token_id);
                println!("[TOKEN] ❌ Invalid/expired/used token for agent '{}': {}", agent_id, preview);
                json!({
                    "valid":    false,
                    "token_id": token_id,
                    "message":  "Token invalid, expired, or already used. Call log_agent_intent again to request a new token."
                })
            }
            Err(e) => {
                eprintln!("[TOKEN ERROR] DB error validating token: {}", e);
                json!({
                    "valid": false,
                    "error": format!("Token validation DB error: {}", e),
                })
            }
        }
    }
}
