// crates/guardian-protection/src/engine.rs
// The ProtectionEngine: single entry point for the entire Phase 2 pipeline.
// SafetyRules → Risk Scoring → Permission Gate → PermissionToken or Denial.
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use guardian_core::db::Database;
use guardian_core::notification::UiNotification;
use guardian_core::types::AgentIntent;
use crate::config::GuardianConfig;
use crate::gate::{ConfirmationBus, PermissionGate};
use crate::kill_switch::{KillSwitch, ProcessTree};
use crate::risk::RiskScoringEngine;
use crate::risk::policies::*;
use crate::rollback::{FileCopyStrategy, MetadataOnlyStrategy, VaultManager};
use crate::safety::SafetyRulesEngine;
use crate::types::{GateDecision, ProtectionResult};

pub struct ProtectionEngine {
    pub risk_engine:    RiskScoringEngine,
    pub safety_engine:  SafetyRulesEngine,
    pub gate:           PermissionGate,
    pub kill_switch:    Arc<KillSwitch>,
    pub confirmation_bus: Arc<ConfirmationBus>,
    pub vault:          Arc<VaultManager>,
    pub config:         GuardianConfig,
    db:                 Arc<Mutex<Database>>,
}

impl ProtectionEngine {
    /// Build the complete protection engine. Called once at startup in main.rs.
    pub async fn build(
        db:    Arc<Mutex<Database>>,
        ui_tx: mpsc::Sender<UiNotification>,
        config_path: &str,
        rules_path: &str,
    ) -> Self {
        let config = GuardianConfig::load(config_path);
        println!("[ENGINE] Loaded config from '{}' (or defaults).", config_path);

        // Risk policies — broad, composable, configurable
        let mut risk_engine = RiskScoringEngine::new(config.gate.clone());
        risk_engine.register_policy(Box::new(DestructiveActionPolicy::default()));
        risk_engine.register_policy(Box::new(ScopeAmbiguityPolicy));
        risk_engine.register_policy(Box::new(UntrustedAgentPolicy));
        risk_engine.register_policy(Box::new(SystemResourcePolicy::default()));

        // Safety rules from DB + TOML seed
        let mut safety_engine = SafetyRulesEngine::load(Arc::clone(&db)).await;
        safety_engine.seed_from_toml(rules_path, Arc::clone(&db)).await;

        // Vault (AES-256-GCM encrypted file storage)
        let vault = Arc::new(
            VaultManager::new(&config.rollback.vault_path)
                .expect("[ENGINE] Failed to initialize rollback vault"),
        );
        println!("[ENGINE] Rollback vault ready at '{}'.", config.rollback.vault_path);

        // Kill switch + process tree
        let process_tree = Arc::new(ProcessTree::new());
        let kill_switch  = Arc::new(KillSwitch::new(
            Arc::clone(&process_tree),
            ui_tx.clone(),
        ));

        // Confirmation bus + permission gate
        let confirmation_bus = Arc::new(ConfirmationBus::new());
        let gate = PermissionGate::new(
            config.gate.clone(),
            Arc::clone(&confirmation_bus),
            ui_tx.clone(),
        );

        // Background task: purge expired rollback snapshots every hour
        let purge_db = Arc::clone(&db);
        let vault_for_purge = Arc::clone(&vault);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                std::time::Duration::from_secs(3600)
            );
            loop {
                interval.tick().await;
                let expired_paths = purge_db.lock().await
                    .purge_expired_snapshots()
                    .unwrap_or_default();
                for p in &expired_paths {
                    vault_for_purge.delete(p);
                    println!("[VAULT] Purged expired snapshot: {}", p);
                }
                if !expired_paths.is_empty() {
                    println!("[VAULT] Purged {} expired snapshots.", expired_paths.len());
                }
            }
        });

        Self {
            risk_engine, safety_engine, gate, kill_switch,
            confirmation_bus, vault, config, db,
        }
    }

    /// The complete protection pipeline. Called by log_agent_intent MCP tool.
    /// Flow: Safety Rules → Risk Engine → Permission Gate → Token or Denial
    pub async fn process_intent(&self, intent: AgentIntent) -> ProtectionResult {
        println!("[ENGINE] Processing intent: agent='{}' action='{}' target='{}'",
            intent.agent_id, intent.action, intent.target_uri);

        // Step 1: Safety Rules (short-circuit — no risk scoring needed if blocked)
        if let Some(rule_match) = self.safety_engine
            .evaluate(&intent, Arc::clone(&self.db)).await
        {
            let decision = self.safety_engine.decision_for_match(&rule_match);
            println!("[ENGINE] Safety rule '{}' fired: {:?}", rule_match.rule_id, decision);
            match decision {
                GateDecision::Block { reason: _ } => {
                    return ProtectionResult::RuleBlocked {
                        rule_id:   rule_match.rule_id,
                        rule_type: format!("{:?}", rule_match.rule_type),
                    };
                }
                GateDecision::RequireUserConfirmation { .. } => {
                    // Fall through to gate with forced RequireConfirmation
                    let assessment = crate::types::RiskAssessment {
                        score:              0.75,
                        triggered_policies: vec![format!("SafetyRule:{}", rule_match.rule_id)],
                        recommended_action: decision,
                    };
                    return self.gate.apply(&intent, assessment, Arc::clone(&self.db)).await;
                }
                _ => {}
            }
        }

        // Step 2: Risk Scoring Engine
        let assessment = self.risk_engine
            .evaluate(&intent, Arc::clone(&self.db))
            .await;
        println!("[ENGINE] Risk score: {:.2} | Policies: {:?}", assessment.score, assessment.triggered_policies);

        // Step 3: Rollback snapshot if action will be allowed (before it executes)
        if matches!(assessment.recommended_action,
            GateDecision::Allow | GateDecision::AllowWithLog)
        {
            self.take_snapshot_if_needed(&intent).await;
        }

        // Step 4: Permission Gate — issues token or denies
        self.gate.apply(&intent, assessment, Arc::clone(&self.db)).await
    }

    /// Take a rollback snapshot based on file size. Dispatches to correct strategy.
    async fn take_snapshot_if_needed(&self, intent: &AgentIntent) {
        if !intent.target_uri.starts_with("file://") { return; }
        let path = intent.target_uri.strip_prefix("file://").unwrap_or(&intent.target_uri);
        let size_mb = std::fs::metadata(path)
            .map(|m| m.len() / (1024 * 1024))
            .unwrap_or(0);

        let token_placeholder = "pre-token";
        let strategy: Box<dyn crate::rollback::RollbackStrategy> =
            if size_mb < self.config.rollback.max_file_size_mb {
                Box::new(FileCopyStrategy {
                    vault:           Arc::clone(&self.vault),
                    db:              Arc::clone(&self.db),
                    retention_hours: self.config.rollback.retention_hours,
                })
            } else {
                Box::new(MetadataOnlyStrategy {
                    db:              Arc::clone(&self.db),
                    retention_hours: self.config.rollback.retention_hours,
                })
            };

        let _ = strategy.snapshot(&intent.target_uri, &intent.agent_id, token_placeholder).await;
    }

    /// Expose confirmation bus so the UI can resolve pending confirmations.
    pub fn confirmation_bus(&self) -> Arc<ConfirmationBus> {
        Arc::clone(&self.confirmation_bus)
    }
}
