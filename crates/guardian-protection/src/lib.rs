// crates/guardian-protection/src/lib.rs
pub mod config;
pub mod types;
pub mod risk;
pub mod gate;
pub mod safety;
pub mod kill_switch;
pub mod rollback;
pub mod engine;

pub use engine::ProtectionEngine;
pub use types::{GateDecision, PermissionToken, ProtectionResult, RiskAssessment};
pub use gate::ConfirmationBus;
pub use kill_switch::{KillSwitch, ProcessTree};
