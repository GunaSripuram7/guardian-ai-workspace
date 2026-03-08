// crates/guardian-protection/src/risk/policy_trait.rs
use async_trait::async_trait;
use guardian_core::types::AgentIntent;
use crate::types::SystemContext;

/// The pluggable Risk Policy interface.
///
/// BROAD: Each policy is one composable rule. The engine holds Vec<Box<dyn RiskPolicy>>.
/// To add a new risk dimension tomorrow (TimeOfDayPolicy, GeoFencePolicy, NetworkPolicy):
///   1. Create a new struct implementing this trait.
///   2. Register it with engine.register_policy(). Done.
///
/// BAD: giant `match` or `if action == "delete" && path.contains("Photos")`
/// GOOD: DestructiveActionPolicy::evaluate() → 0.85
#[async_trait]
pub trait RiskPolicy: Send + Sync {
    /// Unique name used in RiskAssessment.triggered_policies.
    fn name(&self) -> &'static str;
    /// Weight multiplier (default 1.0). A weight of 2.0 gives double influence.
    fn weight(&self) -> f32 { 1.0 }
    /// Return a raw risk score: 0.0 = no risk, 1.0 = maximum risk.
    async fn evaluate(&self, intent: &AgentIntent, context: &SystemContext) -> f32;
}
