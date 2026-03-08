// crates/guardian-mcp/src/tools/query_semantic_context.rs
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
use guardian_core::db::Database;
use crate::tool_trait::GuardianTool;

/// Resolves abstract semantic tags into concrete physical URIs that
/// Guardian has observed and classified.
///
/// BROAD: Accepts ANY tag — ["role:work_documents"], ["context:personal"],
///        ["type:config"], ["role:source_code", "context:active_project"].
/// NOT NARROW: No hard-coded logic for "photos", "documents", "downloads".
///             The tags are defined by the user and the learning engine — not by us.
pub struct QuerySemanticContextTool;

#[async_trait]
impl GuardianTool for QuerySemanticContextTool {
    fn name(&self) -> &'static str {
        "query_semantic_context"
    }

    fn description(&self) -> &'static str {
        "Resolves abstract semantic tags (e.g. [\"role:work_documents\"]) into \
        the physical resource URIs that Guardian has observed and classified. \
        Use this instead of hard-coded file paths — Guardian knows where things \
        actually live based on your real behaviour."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "One or more abstract semantic tags to resolve. \
                                    Example: [\"role:source_code\", \"context:work\"]"
                }
            },
            "required": ["tags"]
        })
    }

    async fn execute(&self, args: Value, db: Arc<Mutex<Database>>) -> Value {
        let tags: Vec<String> = args
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        if tags.is_empty() {
            return serde_json::json!({
                "error": "No tags provided. Pass at least one semantic tag like \"role:work_documents\"."
            });
        }

        let db_lock = db.lock().await;
        match db_lock.query_entities_by_tags(&tags) {
            Ok(entities) => serde_json::json!({
                "query_tags":    tags,
                "results_count": entities.len(),
                "entities":      entities,
		// Phase 2: agents can now see how confident Guardian is about each result
                // so they understand which paths are well-classified vs ambiguous.
                "interpretation": "Each entity includes confidence_score (0.0–1.0).\
                    Low confidence means Guardian is still learning this resource."
            }),
            Err(e) => serde_json::json!({ "error": format!("DB query failed: {}", e) }),
        }
    }
}
