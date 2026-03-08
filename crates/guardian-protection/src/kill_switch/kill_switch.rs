// crates/guardian-protection/src/kill_switch/kill_switch.rs
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use guardian_core::notification::UiNotification;
use crate::kill_switch::os_suspend::{resume_process, suspend_process};
use crate::kill_switch::process_tree::ProcessTree;

pub struct KillSwitch {
    pub process_tree: Arc<ProcessTree>,
    ui_tx:            mpsc::Sender<UiNotification>,
    suspended_pids:   Arc<Mutex<Vec<u32>>>,
}

impl KillSwitch {
    pub fn new(process_tree: Arc<ProcessTree>, ui_tx: mpsc::Sender<UiNotification>) -> Self {
        Self {
            process_tree,
            ui_tx,
            suspended_pids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Suspend ALL processes associated with an agent.
    /// Triggered by: UI button, global hotkey, or anomaly detector.
    /// BROAD: Works for any agent_id — not agent-specific logic.
    pub async fn emergency_stop(&self, agent_id: &str, trigger_source: &str) {
        let pids = self.process_tree.get_pids(agent_id).await;
        if pids.is_empty() {
            println!("[KILL SWITCH] No tracked PIDs for agent '{}'. Nothing to suspend.", agent_id);
            return;
        }

        println!("[KILL SWITCH] ⚡ EMERGENCY STOP triggered by '{}' for agent '{}'", trigger_source, agent_id);
        let mut frozen = Vec::new();
        for pid in &pids {
            match suspend_process(*pid) {
                Ok(true) => {
                    frozen.push(*pid);
                    println!("[KILL SWITCH] Suspended PID {}", pid);
                }
                Ok(false) => println!("[KILL SWITCH] PID {} not found (already exited?)", pid),
                Err(e)    => eprintln!("[KILL SWITCH] Failed to suspend PID {}: {}", pid, e),
            }
        }
        self.suspended_pids.lock().await.extend(&frozen);

        let _ = self.ui_tx.send(UiNotification::critical(
            &format!("🛑 Agent '{}' SUSPENDED", agent_id),
            &format!(
                "Trigger: {}. {} process(es) frozen: {:?}.\nChoose action:",
                trigger_source, frozen.len(), frozen
            ),
            vec!["Resume Agent", "Terminate Permanently", "Rollback Last Actions"],
        )).await;
    }

    /// Resume all suspended processes for an agent.
    pub async fn resume_agent(&self, agent_id: &str) {
        let pids = self.process_tree.get_pids(agent_id).await;
        for pid in pids {
            match resume_process(pid) {
                Ok(_) => println!("[KILL SWITCH] Resumed PID {}", pid),
                Err(e) => eprintln!("[KILL SWITCH] Failed to resume PID {}: {}", pid, e),
            }
        }
        let mut suspended = self.suspended_pids.lock().await;
        let agent_pids = self.process_tree.get_pids(agent_id).await;
        suspended.retain(|pid| !agent_pids.contains(pid));
    }

    /// Check if any anomaly auto-trigger thresholds are exceeded.
    /// Called by the subscriber loop after each event.
    pub async fn check_anomaly(
        &self,
        agent_id: &str,
        file_ops_per_min: u32,
        configured_limit: u32,
    ) {
        if file_ops_per_min > configured_limit {
            self.emergency_stop(
                agent_id,
                &format!("AnomalyDetector: {} file ops/min > limit {}", file_ops_per_min, configured_limit),
            ).await;
        }
    }
}
