// crates/guardian-protection/src/risk/policies/scope_ambiguity.rs
use async_trait::async_trait;
use guardian_core::types::AgentIntent;
use crate::types::SystemContext;
use crate::risk::policy_trait::RiskPolicy;

/// Scores high when Guardian doesn't know what the target resource IS.
///
/// BROAD: Applies to ANY resource type — file, process, network endpoint.
/// If Guardian has never classified this URI, it's ambiguous = risky.
/// A low confidence_score means Guardian saw it but couldn't classify it yet.
pub struct ScopeAmbiguityPolicy;

#[async_trait]
impl RiskPolicy for ScopeAmbiguityPolicy {
    fn name(&self) -> &'static str { "ScopeAmbiguityPolicy" }

        async fn evaluate(&self, _intent: &AgentIntent, ctx: &SystemContext) -> f32 {
        // ── GAP 1 FIX: Calibrated scoring ────────────────────────────────────
        // OLD: None → 0.75 (triggered on every new file = too aggressive)
        // NEW: Base ambiguity score is lower; action type adjusts it upward.
        // A plain "read" on an unknown file = 0.20 (low risk, just log it).
        // A "write/delete" on an unknown file = 0.55 (medium, needs attention).
        // A well-known file (high confidence) = 0.05 regardless of action.
         let base: f32 = match &ctx.semantic_entity {
            None                                 => 0.20, // Never seen — mildly risky
            Some(e) if e.confidence_score < 0.30 => 0.15, // Barely classified
            Some(e) if e.confidence_score < 0.60 => 0.08, // Partially known
            Some(_)                              => 0.02, // Well-known — almost no ambiguity
        };

        // Destructive actions on unknown resources add extra ambiguity score
        let action = _intent.action.to_lowercase();
        let action_multiplier: f32 = if action.contains("delete")
            || action.contains("write")
            || action.contains("overwrite")
            || action.contains("execute")
        {
            2.5  // Unknown target + destructive action = more concerning
        } else {
            1.0  // Read/list/query — low additional concern
        };

        (base * action_multiplier).min(0.60_f32) // Cap at 0.60 — DestructiveActionPolicy handles the rest
        // ── END GAP 1 FIX ────────────────────────────────────────────────────
    }

}
