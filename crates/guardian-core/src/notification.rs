// crates/guardian-core/src/notification.rs
use serde::{Deserialize, Serialize};

/// The severity level. Broad: Info/Warning/Critical covers every possible
/// UI scenario from a system tray badge colour to a full-screen alert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationLevel {
    Info,
    Warning,
    Critical,
}

/// A completely UI-agnostic notification payload.
/// The Core Engine emits these; it never knows if the receiver is a
/// system tray, a web dashboard, a terminal, or a phone notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiNotification {
    pub level: NotificationLevel,
    /// Short headline (e.g. "Event: sensor.fs" or "EventBus Lag")
    pub title: String,
    /// Human-readable detail about what happened.
    pub message: String,
    /// Optional buttons the UI can render. Empty = no actions needed.
    /// E.g. vec!["Allow", "Block"] or vec!["Restart", "Quit"]
    pub action_buttons: Vec<String>,
    /// Phase 2: Set when the gate needs user input.
    /// The UI sends this ID back via the ConfirmationBus to resolve the decision.
    pub confirmation_id:  Option<String>,    // ← ADD THIS FIELD
}

impl UiNotification {
    pub fn info(title: &str, message: &str) -> Self {
        Self {
            level: NotificationLevel::Info,
            title: title.to_string(),
            message: message.to_string(),
            action_buttons: vec![],
	    confirmation_id: None
        }
    }

    pub fn warning(title: &str, message: &str, actions: Vec<&str>) -> Self {
        Self {
            level: NotificationLevel::Warning,
            title: title.to_string(),
            message: message.to_string(),
            action_buttons: actions.into_iter().map(|s| s.to_string()).collect(),
            confirmation_id: None
	}
    }

    pub fn critical(title: &str, message: &str, actions: Vec<&str>) -> Self {
        Self {
            level: NotificationLevel::Critical,
            title: title.to_string(),
            message: message.to_string(),
            action_buttons: actions.into_iter().map(|s| s.to_string()).collect(),
	    confirmation_id: None
        }
    }
}
