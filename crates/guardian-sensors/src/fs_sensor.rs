use async_trait::async_trait;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::error::Error;
use std::path::Path;
use std::time::SystemTime;
use tokio::sync::broadcast;
use uuid::Uuid;

use guardian_core::event::{EventType, SystemEvent};
use crate::trait_def::Sensor;

pub struct FileSystemSensor {
    watch_path: String,
}

impl FileSystemSensor {
    pub fn new(path: &str) -> Self {
        Self {
            watch_path: path.to_string(),
        }
    }
}

#[async_trait]
impl Sensor for FileSystemSensor {
    fn name(&self) -> &'static str {
        "sensor.fs"
    }

    async fn start(&mut self, tx: broadcast::Sender<SystemEvent>) -> Result<(), Box<dyn Error + Send + Sync>> {
        // 1. Create a channel for `notify` to send raw OS events to our tokio task
        let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<notify::Result<Event>>(100);

        // 2. Setup the notify watcher
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                // This closure runs on a background thread spawned by notify.
                // We just pass the result into our async tokio channel.
                let _ = notify_tx.blocking_send(res);
            },
            Config::default(),
        )?;

        // 3. Tell the OS to start watching the path
        watcher.watch(Path::new(&self.watch_path), RecursiveMode::Recursive)?;
        
        println!("Guardian AI: fs_sensor started on {}", self.watch_path);

        let source_name = self.name().to_string();

        // 4. The main async loop: Process events as they come in from the OS
        tokio::spawn(async move {
            // We need to keep `watcher` alive by moving it into the task, 
            // otherwise it drops and stops watching immediately!
            let _kept_watcher = watcher; 

            while let Some(res) = notify_rx.recv().await {
                match res {
                    Ok(event) => {
                        let event_type = match event.kind {
                            notify::EventKind::Create(_) => EventType::Created,
                            notify::EventKind::Modify(_) => EventType::Modified,
                            notify::EventKind::Remove(_) => EventType::Deleted,
                            notify::EventKind::Access(_) => EventType::Accessed,
                            _ => continue, // Ignore other noise for now
                        };

                        // Process all paths affected by this event
                        for path in event.paths {
                            // Convert the path to a standard URI format
                            let uri = format!("file://{}", path.to_string_lossy().replace('\\', "/"));
                            
                            // Package it into the Universal SystemEvent
                            let sys_event = SystemEvent {
                                id: Uuid::new_v4().to_string(),
                                timestamp: SystemTime::now(),
                                source: source_name.clone(),
                                event_type: event_type.clone(),
                                target_uri: uri,
                                metadata: serde_json::json!({
                                    "raw_kind": format!("{:?}", event.kind)
                                }),
                            };

                            // Broadcast it to the rest of Guardian Core
                            let _ = tx.send(sys_event);
                        }
                    }
                    Err(e) => println!("fs_sensor error: {:?}", e),
                }
            }
        });

        Ok(())
    }
}
