// crates/guardian-mcp/src/tools/get_system_state.rs
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
use guardian_core::db::Database;
use crate::tool_trait::GuardianTool;

/// Returns a generic snapshot of Guardian's observed system state from the DB.
///
/// BROAD: Returns events from ANY sensor source, newest-first.
///        The caller chooses the source_filter and limit — this tool
///        never hard-codes "get running apps" or "get recent file changes".
pub struct GetSystemStateTool;

#[async_trait]
impl GuardianTool for GetSystemStateTool {
    fn name(&self) -> &'static str {
        "get_system_state"
    }

    fn description(&self) -> &'static str {
        "Returns a generic snapshot of Guardian's observed system state \
        from the knowledge graph. Includes recent events from all active sensors \
        (file system, processes, agent intents, and any future sensors). \
        Use source_filter to focus on a specific sensor stream."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Max number of recent events to return. Default: 20.",
                    "default": 20
                },
                "source_filter": {
                    "type": "string",
                    "description": "Optional. Filter by sensor source string, e.g. \
                                    \"sensor.process\", \"sensor.fs\", \"agent_intent.claude-code\". \
                                    Omit to get events from all sources."
                }
            }
        })
    }

    async fn execute(&self, args: Value, db: Arc<Mutex<Database>>) -> Value {
        let limit         = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
        let source_filter = args.get("source_filter").and_then(|v| v.as_str());

        let db_lock = db.lock().await;
        let result = match source_filter {
            Some(src) => db_lock.get_recent_events_by_source(src, limit),
            None      => db_lock.get_recent_events(limit),
        };

        match result {
            Ok(events) => {
                // Phase 2: also return active safety rules for the requesting agent
                let safety_rules: Vec<serde_json::Value> = {
                    let db_lock_again = db.lock().await;
                    db_lock_again.get_active_safety_rules()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|r| serde_json::json!({
                            "rule_id":          r.rule_id,
                            "rule_type":        format!("{:?}", r.rule_type),
                            "scope_tags":       r.scope_tags,
                            "applies_to_agent": r.applies_to_agent,
                        }))
                        .collect()
                };
                serde_json::json!({
                    "source_filter":  source_filter.unwrap_or("all"),
                    "event_count":    events.len(),
                    "recent_events":  events,
                    "active_safety_rules": safety_rules,
                    "phase": 2
                })
            }
            Err(e) => serde_json::json!({ "error": format!("DB query failed: {}", e) }),
        }
    }
}
