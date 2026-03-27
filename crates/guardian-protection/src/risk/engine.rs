// crates/guardian-protection/src/risk/engine.rs
use std::sync::Arc;
use tokio::sync::Mutex;
use guardian_core::db::Database;
use guardian_core::types::AgentIntent;
use crate::config::GateConfig;
use crate::risk::policy_trait::RiskPolicy;
use crate::types::{GateDecision, RiskAssessment, SystemContext};

pub struct RiskScoringEngine {
    policies: Vec<Box<dyn RiskPolicy>>,
    config:   GateConfig,
}

impl RiskScoringEngine {
    pub fn new(config: GateConfig) -> Self {
        Self { policies: Vec::new(), config }
    }

    /// Register a policy. Called once at startup.
    /// To add a new risk dimension: one register_policy() call. Nothing else changes.
    pub fn register_policy(&mut self, policy: Box<dyn RiskPolicy>) {
        println!("[RISK ENGINE] Registered policy: '{}'", policy.name());
        self.policies.push(policy);
    }

    /// Evaluate all policies and return a RiskAssessment.
    /// Final score = weighted maximum (not sum) — prevents inflated scores.
    pub async fn evaluate(
        &self,
        intent: &AgentIntent,
        db: Arc<Mutex<Database>>,
    ) -> RiskAssessment {
        let ctx = self.build_context(intent, Arc::clone(&db)).await;
        let mut triggered = Vec::new();
        let mut weighted_max: f32 = 0.0;

        for policy in &self.policies {
            let raw = policy.evaluate(intent, &ctx).await;
            if raw > 0.05 {
                triggered.push(format!("{}={:.2}", policy.name(), raw));
            }
            // ── GAP 3: Apply semantic multiplier to every policy score ────────
            let adjusted = (raw * ctx.semantic_multiplier).min(1.0);
            let weighted  = (adjusted * policy.weight()).min(1.0);
            // ── END GAP 3 ─────────────────────────────────────────────────────
            if weighted > weighted_max {
                weighted_max = weighted;
            }
        }

        let score = weighted_max;
        let recommended_action = self.map_score_to_decision(score, intent);
        RiskAssessment { score, triggered_policies: triggered, recommended_action }
    }

    /// Build context from Phase 1's Knowledge Graph.
    /// All DB reads happen here — policies receive a pre-built struct, no DB access.
    async fn build_context(&self, intent: &AgentIntent, db: Arc<Mutex<Database>>) -> SystemContext {
        let db_lock = db.lock().await;
        let semantic_entity   = db_lock.get_entity_by_uri(&intent.target_uri).ok().flatten();
        let agent_trust_level = db_lock.get_agent_trust_level(&intent.agent_id).ok().flatten();
        let recent_event_count =
            db_lock.count_recent_agent_events(&intent.agent_id, 60).ok().unwrap_or(0);
        drop(db_lock);
        // ── GAP 3: Compute semantic risk multiplier from entity tags ──────────
        // Tags like "role:credential" or "role:os_critical" raise ALL policy scores.
        // Tags like "context:public" lower ALL policy scores.
        // This means sensitivity travels with the DATA, not the path.
        let semantic_multiplier = if let Some(entity) = &semantic_entity {
            let tags = &entity.semantic_tags;
            if tags.iter().any(|t| t.contains("role:credential") || t.contains("role:os_critical")) {
                2.0   // Critical resource — double all risk scores
            } else if tags.iter().any(|t| t.contains("role:system_config")) {
                1.5   // System config — 50% risk increase
            } else if tags.iter().any(|t| t.contains("context:public")) {
                0.5   // Public data — halve the risk
            } else {
                1.0   // No adjustment
            }
        } else {
            1.0  // Unknown resource — no adjustment (ScopeAmbiguity handles this)
        };
        // ── END GAP 3 context build ───────────────────────────────────────────

        SystemContext { semantic_entity, agent_trust_level, recent_event_count, semantic_multiplier }
    }

    /// Map a score to a GateDecision using TOML-loaded thresholds. Not hardcoded.
    fn map_score_to_decision(&self, score: f32, intent: &AgentIntent) -> GateDecision {
        if score >= self.config.block_threshold {
            GateDecision::Block {
                reason: format!(
                    "Risk score {:.2} ≥ block threshold {:.2}. '{}' on '{}' denied.",
                    score, self.config.block_threshold, intent.action, intent.target_uri
                ),
            }
        } else if score >= self.config.confirm_threshold {
            GateDecision::RequireUserConfirmation {
                preview: format!(
                    "Agent '{}' wants to '{}' on '{}'.\nRisk: {:.0}%.\nAllow?",
                    intent.agent_id, intent.action, intent.target_uri, score * 100.0
                ),
            }
        } else if score >= self.config.log_threshold {
            GateDecision::AllowWithLog
        } else {
            GateDecision::Allow
        }
    }
}
