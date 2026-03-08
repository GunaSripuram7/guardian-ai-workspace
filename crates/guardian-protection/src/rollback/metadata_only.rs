// crates/guardian-protection/src/rollback/metadata_only.rs
use async_trait::async_trait;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use uuid::Uuid;
use sha2::{Sha256, Digest};
use guardian_core::db::Database;
use crate::rollback::strategy_trait::{RollbackStrategy, SnapshotId};

/// For files too large to copy: record SHA-256 hash, size, modification time.
/// Cannot restore content but can DETECT if the file was tampered with afterward.
/// BROAD: Works for any resource type with a file:// URI.
pub struct MetadataOnlyStrategy {
    pub db:              Arc<Mutex<Database>>,
    pub retention_hours: u64,
}

#[async_trait]
impl RollbackStrategy for MetadataOnlyStrategy {
    fn name(&self) -> &'static str { "MetadataOnlyStrategy" }

    async fn snapshot(&self, uri: &str, agent_id: &str, permission_token: &str) -> Result<SnapshotId, String> {
        let path = uri.strip_prefix("file://").unwrap_or(uri);
        let metadata = tokio::fs::metadata(path).await
            .map_err(|e| format!("Cannot stat '{}': {}", path, e))?;
        let content = tokio::fs::read(path).await
            .map_err(|e| format!("Cannot hash '{}': {}", path, e))?;
        let hash = format!("{:x}", Sha256::digest(&content));
        let meta_json = serde_json::json!({
            "sha256":         hash,
            "size_bytes":     metadata.len(),
            "modified":       metadata.modified().ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs()).unwrap_or(0),
        }).to_string();
        let snapshot_id = Uuid::new_v4().to_string();
        let now     = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let expires = now + (self.retention_hours * 3600) as i64;
        let db_lock = self.db.lock().await;
        db_lock.insert_rollback_snapshot(
            &snapshot_id, agent_id, permission_token, uri,
            self.name(), None, Some(&meta_json), now, expires,
        ).map_err(|e| e.to_string())?;
        println!("[ROLLBACK] MetadataOnly: Snapshot '{}' hash={}", snapshot_id, &hash[..16]);
        Ok(snapshot_id)
    }

    async fn restore(&self, snapshot_id: &SnapshotId) -> Result<(), String> {
        println!("[ROLLBACK] MetadataOnly: Cannot restore content for '{}'. Tamper detection only.", snapshot_id);
        Err("MetadataOnlyStrategy cannot restore file content.".to_string())
    }
}
