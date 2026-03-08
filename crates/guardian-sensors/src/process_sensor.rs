use async_trait::async_trait;
use sysinfo::System;
use std::error::Error;
use std::time::{Duration, SystemTime};
use std::collections::{HashMap, HashSet};
use tokio::sync::broadcast;
use uuid::Uuid;

use guardian_core::event::{EventType, SystemEvent};
use crate::trait_def::Sensor;

pub struct ProcessSensor {
    poll_interval_secs: u64,
}

impl ProcessSensor {
    pub fn new(poll_interval_secs: u64) -> Self {
        Self { poll_interval_secs }
    }
}

#[async_trait]
impl Sensor for ProcessSensor {
    fn name(&self) -> &'static str {
        "sensor.process"
    }

    async fn start(&mut self, tx: broadcast::Sender<SystemEvent>) -> Result<(), Box<dyn Error + Send + Sync>> {
        let source_name = self.name().to_string();
        let interval = self.poll_interval_secs;

        tokio::spawn(async move {
            let mut sys = System::new_all();
            let mut known_pids = HashSet::new();
 	    let mut process_cache: HashMap<u32, (String, String)> = HashMap::new(); // ← ADD THIS

            // Initial population
            sys.refresh_processes();
            for (pid, process) in sys.processes() {
    		known_pids.insert(pid.as_u32());
    		// ↓ ADD THESE 2 LINES
    		let exe = process.exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| "unknown".to_string());
	        process_cache.insert(pid.as_u32(), (process.name().to_string(), exe));
	    }

            println!("Guardian AI: process_sensor started (polling every {}s)", interval);

            loop {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                sys.refresh_processes();
	
		// ← ADD THESE TWO LINES
    		// println!("[DEBUG] Poll fired. Total processes sysinfo sees: {}", sys.processes().len());
	        // println!("[DEBUG] Known PIDs count: {}", known_pids.len());

                let mut current_pids = HashSet::new();

                for (pid, process) in sys.processes() {
                    let pid_u32 = pid.as_u32();
                    current_pids.insert(pid_u32);

                    // If we haven't seen this PID before, it's a new process!
                    if !known_pids.contains(&pid_u32) {
                        let exe_path = process.exe()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| "unknown".to_string());

                        // Ignore rapid background system noise for cleaner logs
                        
				process_cache.insert(pid_u32, (process.name().to_string(), exe_path.clone())); // ← ADD THIS
                            let sys_event = SystemEvent {

                                id: Uuid::new_v4().to_string(),
                                timestamp: SystemTime::now(),
                                source: source_name.clone(),
                                event_type: EventType::Accessed, // Treating execution as "Access"
                                target_uri: format!("process://{}", pid_u32),
                                metadata: serde_json::json!({
                                    "action": "Started",
                                    "name": process.name().to_string(),
                                    "executable": exe_path,
                                    "memory_kb": process.memory() / 1024,
                                }),
                            };

                            let _ = tx.send(sys_event);
                        
                    }
                }

                // Check for terminated processes
                for old_pid in known_pids.iter() {
                    if !current_pids.contains(old_pid) {
                        let sys_event = SystemEvent {
                            id: Uuid::new_v4().to_string(),
                            timestamp: SystemTime::now(),
                            source: source_name.clone(),
                            event_type: EventType::Deleted, // Treating termination as "Deleted"
                            target_uri: format!("process://{}", old_pid),
                            metadata: serde_json::json!({
    				"action":     "Terminated",
    				"name":       process_cache.get(old_pid).map(|(n,_)| n.as_str()).unwrap_or("unknown"),
    				"executable": process_cache.get(old_pid).map(|(_,e)| e.as_str()).unwrap_or("unknown"),
			    }),
                        };
                        let _ = tx.send(sys_event);
                    }
                }

                // Update our state for the next loop
                known_pids = current_pids;
		process_cache.retain(|pid, _| known_pids.contains(pid));
            }
        });

        Ok(())
    }
}
