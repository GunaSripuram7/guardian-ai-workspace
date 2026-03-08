// crates/guardian-protection/src/safety/rules_engine.rs
use std::sync::Arc;
use tokio::sync::Mutex;
use guardian_core::db::Database;
use guardian_core::types::{AgentIntent, RuleType, SafetyRule};
use crate::types::GateDecision;

pub struct RuleMatch {
    pub rule_id:   String,
    pub rule_type: RuleType,
}

/// Evaluates ALL active safety rules against an intent.
/// Runs BEFORE the Risk Scoring Engine — a matching AlwaysBlock rule
/// short-circuits everything with zero DB overhead from the risk engine.
///
/// BROAD: Rules match on abstract action tags ("action:delete") OR
/// semantic entity tags ("role:credential"). Not hardcoded paths or agent names.
pub struct SafetyRulesEngine {
    /// In-memory cache of active rules. Reloaded on config change.
    rules: Vec<SafetyRule>,
}

impl SafetyRulesEngine {
    /// Load rules from DB at startup. Falls back to empty if DB has none.
    pub async fn load(db: Arc<Mutex<Database>>) -> Self {
        let rules = db.lock().await
            .get_active_safety_rules()
            .unwrap_or_default();
        println!("[SAFETY ENGINE] Loaded {} safety rules.", rules.len());
        Self { rules }
    }

    /// Load additional rules from safety_rules.toml and seed into DB.
    pub async fn seed_from_toml(&mut self, path: &str, db: Arc<Mutex<Database>>) {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(parsed) = toml::from_str::<TomlRulesFile>(&content) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let db_lock = db.lock().await;
                for r in &parsed.rules {
                    let rule_type_str = match r.rule_type.as_str() {
                        "AlwaysBlock"                => "always_block",
                        "AlwaysRequireConfirmation"  => "always_require_confirmation",
                        "NeverAllowScope"            => "never_allow_scope",
                        _                            => "always_block",
                    };
                    let tags_json = serde_json::to_string(&r.scope_tags).unwrap_or_default();
                    
		    let _ = db_lock.insert_safety_rule(
                        &uuid::Uuid::new_v4().to_string(),
                        rule_type_str,
                        &tags_json,
                        r.applies_to_agent.as_deref(),
                        now,
                        "safety_rules.toml",
                    );
                }
                drop(db_lock);
                self.rules = db.lock().await
                    .get_active_safety_rules()
                    .unwrap_or_default();

                println!("[SAFETY ENGINE] Seeded {} rules from {}", self.rules.len(), path);
            }
        }
    }

    /// Check if any safety rule blocks or overrides this intent.
    /// Returns Some(RuleMatch) if a rule fires, None if risk engine should proceed.
    pub async fn evaluate(
        &self,
        intent: &AgentIntent,
        db: Arc<Mutex<Database>>,
    ) -> Option<RuleMatch> {
        // Fetch semantic tags for the target resource (lightweight — single indexed query)
        let entity_tags: Vec<String> = db.lock().await
            .get_entity_by_uri(&intent.target_uri)
            .ok()
            .flatten()
            .map(|e| e.semantic_tags)
            .unwrap_or_default();

        for rule in &self.rules {
            if self.rule_matches(rule, intent, &entity_tags) {
                return Some(RuleMatch {
                    rule_id:   rule.rule_id.clone(),
                    rule_type: rule.rule_type.clone(),
                });
            }
        }
        None
    }

    /// Check if a single rule fires for this intent.
    /// BROAD: Matching logic is tag-based, not path-based.
    fn rule_matches(&self, rule: &SafetyRule, intent: &AgentIntent, entity_tags: &[String]) -> bool {
        // Agent filter
        if let Some(agent) = &rule.applies_to_agent {
            if agent != "all" && agent != &intent.agent_id {
                return false;
            }
        }
        // ALL scope_tags must match (AND logic)
        for tag in &rule.scope_tags {
            let matched = if tag.starts_with("action:") {
                let verb = &tag[7..];
                intent.action.to_lowercase().contains(verb)
            } else {
                entity_tags.iter().any(|t| t == tag)
            };
            if !matched { return false; }
        }
        true
    }

    /// Convert a RuleMatch into the appropriate GateDecision.
    pub fn decision_for_match(&self, rm: &RuleMatch) -> GateDecision {
        match rm.rule_type {
            RuleType::AlwaysBlock | RuleType::NeverAllowScope =>
                GateDecision::Block {
                    reason: format!("Blocked by safety rule '{}' (type: {:?})", rm.rule_id, rm.rule_type),
                },
            RuleType::AlwaysRequireConfirmation =>
                GateDecision::RequireUserConfirmation {
                    preview: format!("Safety rule '{}' requires user confirmation.", rm.rule_id),
                },
        }
    }
}

// ── TOML deserialization helpers ─────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct TomlRulesFile {
    rules: Vec<TomlRule>,
}

#[derive(serde::Deserialize)]
struct TomlRule {
    rule_type:          String,
    scope_tags:         Vec<String>,
    #[serde(default)]
    applies_to_agent:   Option<String>,
}
