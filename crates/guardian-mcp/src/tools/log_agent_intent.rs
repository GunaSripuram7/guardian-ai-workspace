// crates/guardian-mcp/src/tools/log_agent_intent.rs
// Phase 2 upgrade: now routes through the full protection pipeline.
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;
use uuid::Uuid;
use guardian_core::db::Database;
use guardian_core::event::{EventType, SystemEvent};
use guardian_core::types::AgentIntent;
use guardian_protection::{ProtectionEngine, ProtectionResult};
use crate::tool_trait::GuardianTool;

pub struct LogAgentIntentTool {
    /// Phase 2: protection engine injected at startup. Optional for backward compat.
    pub protection: Option<Arc<ProtectionEngine>>,
}

#[async_trait]
impl GuardianTool for LogAgentIntentTool {
    fn name(&self) -> &'static str { "log_agent_intent" }

    fn description(&self) -> &'static str {
        "Announces the AI agent's intended action to Guardian BEFORE executing it. \
        Guardian runs the action through its Safety Rules Engine, Risk Scoring Engine, \
        and Permission Gate. Returns a PermissionToken on approval that MUST be included \
        in the actual action request. Returns rejection details on denial."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "agent_id":   { "type": "string" },
                "action":     { "type": "string", "description": "Abstract verb: delete/read/write/execute/upload/network_call" },
                "target_uri": { "type": "string", "description": "file://, process://, network://" },
                "metadata":   { "type": "object" }
            },
            "required": ["agent_id", "action", "target_uri"]
        })
    }

    async fn execute(&self, args: Value, db: Arc<Mutex<Database>>) -> Value {
        let agent_id   = args.get("agent_id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let action     = args.get("action").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let target_uri = args.get("target_uri").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let metadata   = args.get("metadata").cloned().unwrap_or(serde_json::json!({}));

        if target_uri.is_empty() {
            return serde_json::json!({ "error": "target_uri is required." });
        }

        let intent = AgentIntent {
            intent_id:  Uuid::new_v4().to_string(),
            agent_id:   agent_id.clone(),
            action:     action.clone(),
            target_uri: target_uri.clone(),
            metadata:   metadata.clone(),
        };

        // Always log the raw intent to the audit trail (Phase 1 behaviour preserved)
        let audit_event = SystemEvent {
            id:         intent.intent_id.clone(),
            timestamp:  SystemTime::now(),
            source:     format!("agent_intent.{}", agent_id),
            event_type: EventType::AgentIntent,
            target_uri: target_uri.clone(),
            metadata:   serde_json::json!({
                "agent_id": agent_id, "declared_action": action,
                "agent_metadata": metadata,
            }),
        };
        let _ = db.lock().await.insert_event(&audit_event);

        // Route through Phase 2 protection pipeline if engine is available
        if let Some(engine) = &self.protection {
            match engine.process_intent(intent).await {
                ProtectionResult::Permitted { token, assessment } =>
                    serde_json::json!({
                        "status":          "permitted",
                        "permission_token": token,
                        "risk_score":      assessment.score,
                        "triggered_policies": assessment.triggered_policies,
                        "message": format!(
                            "Action '{}' on '{}' approved. Include permission_token in your action call. Expires in 60s.",
                            action, target_uri
                        )
                    }),

                ProtectionResult::Denied { reason, assessment } =>
                    serde_json::json!({
                        "status":     "denied",
                        "reason":     reason,
                        "risk_score": assessment.score,
                        "triggered_policies": assessment.triggered_policies,
                    }),

                ProtectionResult::RuleBlocked { rule_id, rule_type } =>
                    serde_json::json!({
                        "status":    "rule_blocked",
                        "rule_id":   rule_id,
                        "rule_type": rule_type,
                        "message":   "Action blocked by an immutable safety rule. No score computed.",
                    }),

                ProtectionResult::PendingConfirmation { confirmation_id, preview, .. } =>
                    serde_json::json!({
                        "status":          "pending_confirmation",
                        "confirmation_id": confirmation_id,
                        "preview":         preview,
                    }),
            }
        } else {
            // Phase 1 fallback: no protection engine yet
            serde_json::json!({
                "status":    "logged",
                "intent_id": audit_event.id,
                "message":   "Intent logged (protection engine not active).",
            })
        }
    }
}
