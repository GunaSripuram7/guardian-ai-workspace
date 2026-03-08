// crates/guardian-core/src/types.rs
use serde::{Deserialize, Serialize};

/// An AI agent's declared intent. The primary input to the entire
/// Phase 2 protection pipeline. Every field is generic — no hardcoded
/// action verbs or resource categories at this level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIntent {
    pub intent_id:  String,
    pub agent_id:   String,
    /// Abstract action verb declared by the agent: "delete", "read", "write",
    /// "execute", "network_call", "upload", "install" — not defined by us.
    pub action:     String,
    /// The target resource as a URI: "file://...", "process://...", "network://..."
    pub target_uri: String,
    pub metadata:   serde_json::Value,
}

/// A single row from the semantic_entities table.
/// Tells the protection engine what Guardian thinks a resource IS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEntity {
    pub uri:              String,
    /// Abstract tags like ["role:user_data", "context:work", "role:credential"]
    pub semantic_tags:    Vec<String>,
    /// 0.0 = Guardian has no idea what this is. 1.0 = fully classified.
    pub confidence_score: f32,
    pub last_observed:    i64,
}

/// A loaded safety rule from the safety_rules table or safety_rules.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyRule {
    pub rule_id:           String,
    pub rule_type:         RuleType,
    /// Tags that must ALL match for this rule to fire (AND logic).
    /// Tags starting with "action:" match the intent action verb.
    /// Tags starting with "role:" / "context:" match semantic_entity tags.
    pub scope_tags:        Vec<String>,
    /// None = applies to ALL agents. Some("openclaw") = only that agent.
    pub applies_to_agent:  Option<String>,
    pub created_by:        String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RuleType {
    /// Hard block — no score needed, no user prompt. Always denied.
    AlwaysBlock,
    /// Skip risk scoring, go straight to user confirmation.
    AlwaysRequireConfirmation,
    /// The scope is completely off-limits for this agent.
    NeverAllowScope,
}
