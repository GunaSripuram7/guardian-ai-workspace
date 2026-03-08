// crates/guardian-protection/src/risk/policies/untrusted_agent.rs
use async_trait::async_trait;
use guardian_core::types::AgentIntent;
use crate::types::SystemContext;
use crate::risk::policy_trait::RiskPolicy;

/// Scores high when the requesting agent has low trust in the agent_registry.
///
/// BROAD: Any agent_id is evaluated — not just "OpenClaw" or "Claude".
/// An unregistered agent (None) is treated as maximally untrusted.
/// Trust levels: 0-20=untrusted, 21-50=low, 51-80=standard, 81-100=verified.
pub struct UntrustedAgentPolicy;

#[async_trait]
impl RiskPolicy for UntrustedAgentPolicy {
    fn name(&self) -> &'static str { "UntrustedAgentPolicy" }
    fn weight(&self) -> f32 { 1.2 }

    async fn evaluate(&self, _intent: &AgentIntent, ctx: &SystemContext) -> f32 {
        match ctx.agent_trust_level {
            None          => 0.90,
            Some(t) if t <= 20 => 0.75,
            Some(t) if t <= 50 => 0.45,
            Some(t) if t <= 80 => 0.15,
            Some(_)            => 0.05,
        }
    }
}
