// crates/guardian-protection/src/config.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GuardianConfig {
    pub gate:         GateConfig,
    pub kill_switch:  KillSwitchConfig,
    pub rollback:     RollbackConfig,
    pub sensors:      SensorConfig,
}

/// Gate decision thresholds. All configurable — no hardcoded 0.8 in engine code.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GateConfig {
    /// Below this score → Allow silently.
    pub allow_threshold:   f32,
    /// Below this score → AllowWithLog (snapshot taken).
    pub log_threshold:     f32,
    /// Below this score → RequireUserConfirmation.
    pub confirm_threshold: f32,
    /// At or above this score → Block unconditionally.
    pub block_threshold:   f32,
    /// Seconds before a pending confirmation auto-denies.
    pub confirm_timeout_secs: u64,
}

/// Anomaly auto-trigger thresholds for the kill switch.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KillSwitchConfig {
    pub file_ops_per_minute:    u32,
    pub network_reqs_per_minute: u32,
}

/// Rollback vault settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RollbackConfig {
    /// Files larger than this use MetadataOnlyStrategy.
    pub max_file_size_mb:  u64,
    /// Hours before a snapshot is auto-purged.
    pub retention_hours:   u64,
    pub vault_path:        String,
}

// ── ADD THIS ENTIRE STRUCT BELOW ────────────────────────────────────────────
/// Which folders the file-system sensor watches. Fully configurable — no
/// recompile needed to add or remove watch paths.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SensorConfig {
    /// List of absolute folder paths to watch recursively.
    pub watch_paths:         Vec<String>,
    /// How often ProcessSensor polls sysinfo (seconds).
    pub poll_interval_secs:  u64,
}
// ── END ADD ──────────────────────────────────────────────────────────────────

impl Default for GuardianConfig {
    fn default() -> Self {
        Self {
            gate: GateConfig {
                allow_threshold:      0.30,
                log_threshold:        0.60,
                confirm_threshold:    0.80,
                block_threshold:      0.95,
                confirm_timeout_secs: 30,
            },
            kill_switch: KillSwitchConfig {
                file_ops_per_minute:     200,
                network_reqs_per_minute: 50,
            },
            rollback: RollbackConfig {
                max_file_size_mb: 100,
                retention_hours:  48,
                vault_path:       ".guardian_vault".to_string(),
            },
            sensors: SensorConfig {
                watch_paths: vec![
                    "C:/Users/tester/Documents".to_string(),
                    "C:/Users/tester/Desktop".to_string(),
                    "C:/Users/tester/Downloads".to_string(),
                    "C:/Users/tester/test_for_gAI".to_string(),
                ],
                poll_interval_secs: 3,
            },
        }
    }
}

impl GuardianConfig {
    /// Load from guardian_config.toml. Falls back to defaults if file missing.
    pub fn load(path: &str) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }
}
