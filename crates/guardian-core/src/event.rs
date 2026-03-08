use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::SystemTime;

/// Defines the broad category of the event without hardcoding specific triggers.
/// Notice how this doesn't say "FileDeleted" or "ProcessStarted". 
/// It stays abstract so it can apply to anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    Created,
    Modified,
    Deleted,
    Accessed,
    StateChanged, // Used for processes starting/stopping, etc.
    AgentIntent,  // Emitted by the MCP server when an AI agent announces an action
}

/// The Universal Event Payload. 
/// Every sensor (File watcher, Process monitor, etc.) MUST return this struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub id: String,                // A unique UUID for the event
    pub timestamp: SystemTime,     // When it happened
    pub source: String,            // Who reported it (e.g., "sensor.fs", "sensor.process")
    pub event_type: EventType,     // What broad action occurred
    pub target_uri: String,        // The target (e.g., "file://C:/doc.txt" or "process://1204")
    pub metadata: Value,           // Arbitrary JSON (e.g., {"active_app": "word.exe", "size": 1024})
}
