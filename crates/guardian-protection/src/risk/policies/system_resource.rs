// crates/guardian-protection/src/risk/policies/system_resource.rs
use async_trait::async_trait;
use guardian_core::types::AgentIntent;
use crate::types::SystemContext;
use crate::risk::policy_trait::RiskPolicy;

/// Returns 1.0 (maximum score) if the target has any critical semantic tag.
///
/// BROAD: The critical_tags list is configurable — not hardcoded to C:\Windows\.
/// Any URI that the learning engine tags as "role:os_critical" or "role:credential"
/// is protected regardless of its physical path.
pub struct SystemResourcePolicy {
    pub critical_tags: Vec<String>,
}

impl Default for SystemResourcePolicy {
    fn default() -> Self {
        Self {
            critical_tags: vec![
                "role:os_critical".to_string(),
                "role:credential".to_string(),
                "role:system_config".to_string(),
            ],
        }
    }
}

#[async_trait]
impl RiskPolicy for SystemResourcePolicy {
    fn name(&self) -> &'static str { "SystemResourcePolicy" }
    fn weight(&self) -> f32 { 2.0 }

    async fn evaluate(&self, _intent: &AgentIntent, ctx: &SystemContext) -> f32 {
        if let Some(entity) = &ctx.semantic_entity {
            if entity.semantic_tags.iter()
                .any(|tag| self.critical_tags.iter().any(|ct| tag.contains(ct.as_str())))
            {
                return 1.0;
            }
        }
        0.0
    }
}
