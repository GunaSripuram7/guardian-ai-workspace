use async_trait::async_trait;
use tokio::sync::broadcast;
use guardian_core::event::SystemEvent;
use std::error::Error;

/// The pluggable Sensor interface.
/// Any new monitoring module (Files, Network, Registry, Clipboard) 
/// just implements this trait. Guardian Core doesn't need to know how they work.
#[async_trait]
pub trait Sensor: Send + Sync {
    /// Returns the unique identifier for this sensor (e.g., "sensor.fs" or "sensor.network")
    fn name(&self) -> &'static str;
    
    /// Starts the sensor's observation loop. 
    /// It is handed a `Sender` (tx) to the core EventBus.
    /// Whenever the sensor detects something, it packages it into a `SystemEvent` 
    /// and broadcasts it down the `tx` channel.
    async fn start(&mut self, tx: broadcast::Sender<SystemEvent>) -> Result<(), Box<dyn Error + Send + Sync>>;
}
