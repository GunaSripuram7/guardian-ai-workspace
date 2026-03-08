// crates/guardian-protection/src/gate/permission_gate.rs
//
// CONCURRENCY MODEL EXPLAINED:
// The gate may need to pause and wait for user input (RequireUserConfirmation).
// This works without blocking other requests because:
//   - Each MCP request runs in its own tokio::spawn() task.
//   - `confirmation_rx.await` suspends ONLY that one task.
//   - All other tasks (other agent requests, sensors, DB writer) continue running.
//   - When the user clicks Allow/Block, the UI resolves the oneshot::Sender,
//     waking up exactly that one suspended task.
// This is cooperative multitasking — no threads blocked, no throughput lost.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot, Mutex};
use uuid::Uuid;
use guardian_core::db::Database;
use guardian_core::notification::UiNotification;
use guardian_core::types::AgentIntent;
use crate::config::GateConfig;
use crate::types::{GateDecision, PermissionToken, ProtectionResult, RiskAssessment};

/// Manages pending user-confirmation requests.
/// Each pending confirmation is a suspended tokio task waiting on a oneshot channel.
pub struct ConfirmationBus {
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

impl ConfirmationBus {
    pub fn new() -> Self {
        Self { pending: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Suspend the current task and wait for user decision (or timeout).
    pub async fn await_decision(
        &self,
        confirmation_id: String,
        timeout_secs: u64,
    ) -> bool {
        let (tx, rx) = oneshot::channel::<bool>();
        self.pending.lock().await.insert(confirmation_id, tx);
        match tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await {
            Ok(Ok(decision)) => decision,
            _ => {
                println!("[GATE] Confirmation timed out — defaulting to DENY (safe).");
                false
            }
        }
    }

    /// Called by the UI (or a test) to resolve a pending confirmation.
    pub async fn resolve(&self, confirmation_id: &str, approved: bool) {
        if let Some(tx) = self.pending.lock().await.remove(confirmation_id) {
            let _ = tx.send(approved);
        }
    }
}

pub struct PermissionGate {
    config:           GateConfig,
    confirmation_bus: Arc<ConfirmationBus>,
    ui_tx:            mpsc::Sender<UiNotification>,
}

impl PermissionGate {
    pub fn new(
        config: GateConfig,
        confirmation_bus: Arc<ConfirmationBus>,
        ui_tx: mpsc::Sender<UiNotification>,
    ) -> Self {
        Self { config, confirmation_bus, ui_tx }
    }

    /// Apply a RiskAssessment to produce the final ProtectionResult.
    /// Issues a PermissionToken on Allow/AllowWithLog, waits on user for Confirm, hard-blocks on Block.
    pub async fn apply(
        &self,
        intent: &AgentIntent,
        assessment: RiskAssessment,
        db: Arc<Mutex<Database>>,
    ) -> ProtectionResult {
        match &assessment.recommended_action.clone() {
            GateDecision::Allow | GateDecision::AllowWithLog => {
                let token = self.issue_token(intent, &assessment, db).await;
                ProtectionResult::Permitted { token, assessment }
            }

            GateDecision::RequireUserConfirmation { preview } => {
                let confirmation_id = Uuid::new_v4().to_string();
                // Send UI notification — the UI renders the buttons, not the gate.
                let mut notif = UiNotification::warning(
                    &format!("Action Requires Approval — {}", intent.agent_id),
                    preview,
                    vec!["Allow", "Block"],
                );
                notif.confirmation_id = Some(confirmation_id.clone());
                let _ = self.ui_tx.send(notif).await;

                // Suspend this task. Other agent requests continue in parallel.
                let approved = self.confirmation_bus
                    .await_decision(confirmation_id.clone(), self.config.confirm_timeout_secs)
                    .await;

                if approved {
                    let token = self.issue_token(intent, &assessment, db).await;
                    ProtectionResult::Permitted { token, assessment }
                } else {
                    ProtectionResult::Denied {
                        reason: "User declined or confirmation timed out.".to_string(),
                        assessment,
                    }
                }
            }

            GateDecision::Block { reason } => {
                // Emit critical UI notification immediately
                let _ = self.ui_tx.send(UiNotification::critical(
                    &format!("BLOCKED: {} by {}", intent.action, intent.agent_id),
                    reason,
                    vec!["Dismiss"],
                )).await;
                ProtectionResult::Denied { reason: reason.clone(), assessment }
            }
        }
    }

    /// Issue a short-lived permission token and persist it to DB.
    async fn issue_token(
        &self,
        intent: &AgentIntent,
        assessment: &RiskAssessment,
        db: Arc<Mutex<Database>>,
    ) -> PermissionToken {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let token = PermissionToken {
            token_id:      Uuid::new_v4().to_string(),
            agent_id:      intent.agent_id.clone(),
            intent_hash:   format!("{:x}", md5_hash(&intent.intent_id)),
            issued_at:     now,
            expires_at:    now + 60, // 60-second expiry
            gate_decision: format!("{:?}", assessment.recommended_action),
        };
        let decision_str = match &assessment.recommended_action {
            GateDecision::Allow         => "Allow",
            GateDecision::AllowWithLog  => "AllowWithLog",
            _                           => "Allow",
        };
        let db_lock = db.lock().await;
        let _ = db_lock.insert_permission_token(
            &token.token_id, &token.agent_id, &token.intent_hash,
            token.issued_at, token.expires_at, decision_str,
        );
        token
    }
}

fn md5_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
