// crates/guardian-protection/src/kill_switch/process_tree.rs
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Tracks which PIDs belong to which agent.
/// Fed by the ProcessSensor already running from Phase 1 via the EventBus.
/// BROAD: Works for any agent — not hardcoded to specific agent names.
pub struct ProcessTree {
    /// agent_id → Set of PIDs spawned by that agent
    tree: Arc<Mutex<HashMap<String, HashSet<u32>>>>,
}

impl ProcessTree {
    pub fn new() -> Self {
        Self { tree: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Register a PID as belonging to an agent.
    pub async fn register(&self, agent_id: &str, pid: u32) {
        self.tree.lock().await
            .entry(agent_id.to_string())
            .or_insert_with(HashSet::new)
            .insert(pid);
    }

    /// Remove a PID (process terminated).
    pub async fn deregister(&self, pid: u32) {
        let mut tree = self.tree.lock().await;
        for pids in tree.values_mut() {
            pids.remove(&pid);
        }
    }

    /// Get all PIDs associated with an agent.
    pub async fn get_pids(&self, agent_id: &str) -> Vec<u32> {
        self.tree.lock().await
            .get(agent_id)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Get all registered agents.
    pub async fn all_agents(&self) -> Vec<String> {
        self.tree.lock().await.keys().cloned().collect()
    }
}
