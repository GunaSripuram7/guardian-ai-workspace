// crates/guardian-protection/src/types.rs
use serde::{Deserialize, Serialize};

/// The final permission decision — the single chokepoint for every agent action.
/// BROAD: Four states cover every possible scenario without hardcoding specifics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "decision_type")]
pub enum GateDecision {
    /// Score below allow_threshold. Proceed silently.
    Allow,
    /// Score below log_threshold. Proceed but take a rollback snapshot.
    AllowWithLog,
    /// Score below confirm_threshold. Pause and show user a preview.
    RequireUserConfirmation { preview: String },
    /// Score at or above block_threshold. Hard denied.
    Block { reason: String },
}

/// The output of the Risk Scoring Engine after evaluating an intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub score:              f32,
    pub triggered_policies: Vec<String>,
    pub recommended_action: GateDecision,
}

/// Short-lived token issued to an agent after Allow or AllowWithLog.
/// The agent MUST include this in its actual action request.
/// Prevents replay attacks and unauthorized unvalidated actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionToken {
    pub token_id:      String,
    pub agent_id:      String,
    pub intent_hash:   String,
    pub issued_at:     i64,
    pub expires_at:    i64,
    pub gate_decision: String,
}

/// The final output of the complete protection pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result_type")]
pub enum ProtectionResult {
    /// Agent may proceed. Must use the token in its actual action call.
    Permitted {
        token:      PermissionToken,
        assessment: RiskAssessment,
    },
    /// Action is waiting for user to click Allow or Block in the UI.
    PendingConfirmation {
        confirmation_id: String,
        preview:         String,
        assessment:      RiskAssessment,
    },
    /// Action denied by risk score or gate decision.
    Denied {
        reason:     String,
        assessment: RiskAssessment,
    },
    /// Denied immediately by a safety rule — no scoring performed.
    RuleBlocked {
        rule_id:   String,
        rule_type: String,
    },
}

/// Context pulled from Phase 1's Knowledge Graph to supply risk policies.
/// Built once per intent evaluation. Policies read from this — they don't query DB.
#[derive(Debug, Clone)]
pub struct SystemContext {
    pub semantic_entity:    Option<guardian_core::types::SemanticEntity>,
    pub agent_trust_level:  Option<i32>,
    pub recent_event_count: usize,
     // ── GAP 3: ADD THIS FIELD ─────────────────────────────────────────────────
    pub semantic_multiplier: f32,
    // ── END ADD ───────────────────────────────────────────────────────────────
}
