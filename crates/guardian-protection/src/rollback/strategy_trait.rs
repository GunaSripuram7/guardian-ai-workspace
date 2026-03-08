// crates/guardian-protection/src/rollback/strategy_trait.rs
use async_trait::async_trait;

pub type SnapshotId = String;

/// The pluggable Rollback Strategy interface.
///
/// BROAD: Different resource types need different snapshot strategies.
/// A small file → copy it. A huge file → record its hash/metadata.
/// A database → export a dump. Future strategies plug in here with zero core changes.
#[async_trait]
pub trait RollbackStrategy: Send + Sync {
    fn name(&self) -> &'static str;
    /// Take a snapshot of the resource at the given URI.
    async fn snapshot(
        &self,
        uri: &str,
        agent_id: &str,
        permission_token: &str,
    ) -> Result<SnapshotId, String>;
    /// Restore a resource from a snapshot.
    async fn restore(&self, snapshot_id: &SnapshotId) -> Result<(), String>;
}
