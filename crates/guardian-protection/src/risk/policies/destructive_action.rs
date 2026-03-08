// crates/guardian-protection/src/risk/policies/destructive_action.rs
use async_trait::async_trait;
use guardian_core::types::AgentIntent;
use crate::types::SystemContext;
use crate::risk::policy_trait::RiskPolicy;

/// Scores high for destructive action verbs declared by the agent.
///
/// BROAD: Checks the abstract action verb from AgentIntent, NOT the file path.
/// "delete a photo" and "delete a system config" get identical raw scores here.
/// The SystemResourcePolicy separately escalates score for critical targets.
/// This separation means each dimension is composable and independently configurable.
pub struct DestructiveActionPolicy {
    /// Configurable list of destructive verbs. Default covers common cases.
    pub destructive_actions: Vec<String>,
}

impl Default for DestructiveActionPolicy {
    fn default() -> Self {
        Self {
            destructive_actions: vec![
                "delete", "overwrite", "format", "truncate",
                "wipe", "remove", "drop", "purge", "unlink", "rmdir",
            ].into_iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[async_trait]
impl RiskPolicy for DestructiveActionPolicy {
    fn name(&self) -> &'static str { "DestructiveActionPolicy" }
    fn weight(&self) -> f32 { 1.5 }

    async fn evaluate(&self, intent: &AgentIntent, _ctx: &SystemContext) -> f32 {
        let action = intent.action.to_lowercase();
        if self.destructive_actions.iter().any(|d| action.contains(d.as_str())) {
            0.85
        } else {
            0.0
        }
    }
}
