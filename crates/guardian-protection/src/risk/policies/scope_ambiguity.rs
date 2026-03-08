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
        match &ctx.semantic_entity {
            None                                          => 0.75, // Never seen
            Some(e) if e.confidence_score < 0.30          => 0.60, // Barely classified
            Some(e) if e.confidence_score < 0.60          => 0.35, // Partially known
            Some(_)                                        => 0.05, // Well-known
        }
    }
}
