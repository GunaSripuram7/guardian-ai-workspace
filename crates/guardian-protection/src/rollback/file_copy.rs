// crates/guardian-protection/src/rollback/file_copy.rs
use async_trait::async_trait;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use uuid::Uuid;
use guardian_core::db::Database;
use crate::rollback::strategy_trait::{RollbackStrategy, SnapshotId};
use crate::rollback::vault::VaultManager;

/// For files under max_file_size_mb: copy the entire file content encrypted to vault.
/// BROAD: Works for any file URI, any content type. Not for "documents" only.
pub struct FileCopyStrategy {
    pub vault: Arc<VaultManager>,
    pub db:    Arc<Mutex<Database>>,
    pub retention_hours: u64,
}

#[async_trait]
impl RollbackStrategy for FileCopyStrategy {
    fn name(&self) -> &'static str { "FileCopyStrategy" }

    async fn snapshot(&self, uri: &str, agent_id: &str, permission_token: &str) -> Result<SnapshotId, String> {
        let path = uri.strip_prefix("file://").unwrap_or(uri);
        let content = tokio::fs::read(path).await
            .map_err(|e| format!("Cannot read '{}': {}", path, e))?;
        let snapshot_id = Uuid::new_v4().to_string();
        let vault_path = self.vault.store(&snapshot_id, &content)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let expires = now + (self.retention_hours * 3600) as i64;
        let db_lock = self.db.lock().await;
        db_lock.insert_rollback_snapshot(
            &snapshot_id, agent_id, permission_token, uri,
            self.name(), Some(&vault_path), None, now, expires,
        ).map_err(|e| e.to_string())?;
        println!("[ROLLBACK] FileCopyStrategy: Snapshot '{}' for '{}'", snapshot_id, uri);
        Ok(snapshot_id)
    }

    async fn restore(&self, snapshot_id: &SnapshotId) -> Result<(), String> {
        // In Phase 2 stub: load vault path from DB, decrypt, write back to original URI.
        println!("[ROLLBACK] Restoring snapshot '{}'...", snapshot_id);
        Ok(())
    }
}
