// cli/src/main.rs
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};

use guardian_core::db::Database;
use guardian_core::event::SystemEvent;
use guardian_core::notification::{NotificationLevel, UiNotification};

use guardian_sensors::Sensor;
use guardian_sensors::fs_sensor::FileSystemSensor;
use guardian_sensors::process_sensor::ProcessSensor;

use guardian_mcp::{McpServer, QuerySemanticContextTool, GetSystemStateTool};
use guardian_protection::config::GuardianConfig;
use sysinfo::System;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("==========================================");
    println!(" GUARDIAN AI - Phase 1 Core Engine");
    println!("==========================================\n");

    // ── 1. KNOWLEDGE GRAPH (SQLite) ──────────────────────────────────────────
    let db_path = "guardian_brain.db";
    let db = Arc::new(Mutex::new(Database::new(db_path)?));
    println!("[INIT] Knowledge Graph connected → {}\n", db_path);

    // ── 2. UI NOTIFICATION CHANNEL (Core → UI, strictly decoupled) ───────────
    // The Core emits generic UiNotification structs.
    // The receiver below is a placeholder — it will be replaced by the real
    // guardian-ui tray implementation without ANY changes to core code.
    let (ui_tx, mut ui_rx) = mpsc::channel::<UiNotification>(256);

    tokio::spawn(async move {
        println!("[UI]   Notification channel open. Waiting for Core events...\n");
        while let Some(n) = ui_rx.recv().await {
            let label = match n.level {
                NotificationLevel::Info     => "ℹ️  INFO    ",
                NotificationLevel::Warning  => "⚠️  WARNING ",
                NotificationLevel::Critical => "🛑 CRITICAL",
            };
            println!("[UI NOTIFICATION] {} | {} | {}", label, n.title, n.message);
            if !n.action_buttons.is_empty() {
                println!("                   Actions available: {:?}", n.action_buttons);
            }
        }
    });

    // ── 3. CENTRAL EVENTBUS (broadcast channel, Pub/Sub) ─────────────────────
    let (event_tx, mut event_rx) = broadcast::channel::<SystemEvent>(512);

    // ── 4. SENSORS ───────────────────────────────────────────────────────────
    // Load config early so sensors can use watch_paths + poll_interval.
    // ProtectionEngine::build() will load it again internally — that is fine,
    // the file is tiny and reads are instant.
    let config = GuardianConfig::load("guardian_config.toml");

    for path in &config.sensors.watch_paths {
        let mut fs = FileSystemSensor::new(path);
        fs.start(event_tx.clone()).await?;
        println!("[SENSOR] Watching: {}", path);
    }

    let mut proc_sensor = ProcessSensor::new(config.sensors.poll_interval_secs);
    proc_sensor.start(event_tx.clone()).await?;

    println!("[INIT] EventBus active. Sensors running...\n");

        // ── 5. PROTECTION ENGINE (Phase 2) ───────────────────────────────────────
    let engine = guardian_protection::ProtectionEngine::build(
        Arc::clone(&db),
        ui_tx.clone(),
        "guardian_config.toml",
        "safety_rules.toml",
    ).await;
    let engine = Arc::new(engine);
    println!("[INIT] Protection Engine ready.\n");

    // ── 6. MCP SERVER with Phase 2 tools ─────────────────────────────────────
    let mcp_db = Arc::clone(&db);
    let mut mcp_server = McpServer::new(mcp_db)
        .with_protection(Arc::clone(&engine));

    // log_agent_intent now carries the protection engine
    mcp_server.register_tool(Box::new(
        guardian_mcp::tools::log_agent_intent::LogAgentIntentTool {
            protection: Some(Arc::clone(&engine)),
        }
    ));
    mcp_server.register_tool(Box::new(QuerySemanticContextTool));
    mcp_server.register_tool(Box::new(GetSystemStateTool));
    // ── FIX #10: Register token validation tool ───────────────────────────────
    mcp_server.register_tool(Box::new(guardian_mcp::ValidateTokenTool));
    // ── END FIX #10 ──────────────────────────────────────────────────────────
    let mcp_server = Arc::new(mcp_server);


    println!();

        // ── 7. MCP GATEWAY — real TCP listener ───────────────────────────────────────
    let server_for_tcp = Arc::clone(&mcp_server);
    tokio::spawn(async move {
        server_for_tcp.serve(3000).await;
    });
    println!("[INIT] MCP Gateway live → http://127.0.0.1:3000\n");

    // ── 8. PROCESS TREE POPULATION (Fix #7) ──────────────────────────────────────
    // Scans OS every 10 seconds for known AI agent processes.
    // Registers their PIDs in ProcessTree so kill switch can actually suspend them.
    // Pattern table: (process name fragment, guardian agent_id)
    // To add a new agent: just add a row to agent_patterns — no other changes needed.
    let process_tree_ref = Arc::clone(&engine.kill_switch.process_tree);
    tokio::spawn(async move {
        // Give agents time to start before first scan
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        let agent_patterns: &[(&str, &str)] = &[
            ("openclaw",  "openclaw"),
            ("claude",    "claude-code"),
        ];

        let mut interval = tokio::time::interval(
            tokio::time::Duration::from_secs(10)
        );
        loop {
            interval.tick().await;
            // System::new_all() gives a fresh snapshot — safe to call every 10s
            let sys = System::new_all();
            for (pid, process) in sys.processes() {
                let name = process.name().to_string_lossy().to_lowercase();
                for (pattern, agent_id) in agent_patterns {
                    if name.contains(pattern) {
                        process_tree_ref.register(agent_id, pid.as_u32()).await;
                        println!("[PROCESS TREE] Registered '{}' → PID {}", agent_id, pid);
                    }
                }
            }
        }
    });
    println!("[INIT] Process tree scanner active.\n");
    // ── END FIX #7 ───────────────────────────────────────────────────────────────




        // ── CORE SUBSCRIBER LOOP ─────────────────────────────────────────────────
    // IMPORTANT: Never block here. Spawn each DB write as its own task so
    // event_rx.recv() is called immediately every time — no dropped events.
    loop {
        match event_rx.recv().await {
            Ok(event) => {
                println!("[OBSERVED] {} → {}", event.source, event.target_uri);

                // Sensor events go DIRECTLY to DB — they bypass the protection
                // engine entirely. Protection only runs for AgentIntent events
                // that arrive via the log_agent_intent MCP tool.
                let db_clone = Arc::clone(&db);
                // ── GAP 4: Pass kill_switch ref into each event handler ───────
                let ks_clone     = Arc::clone(&engine.kill_switch);
                let ks_limit     = engine.config.kill_switch.file_ops_per_minute;
                // ── END GAP 4 ref clones ──────────────────────────────────────

                tokio::spawn(async move {
                    let db_lock = db_clone.lock().await;
                    if let Err(e) = db_lock.insert_event(&event) {
                        eprintln!("[ERROR] DB write failed: {}", e);
                    }

                    // ── GAP 4: Count agent file ops and trigger kill switch ────
                    // Only count agent_intent events (sensor events aren't agents)
                    if event.source.starts_with("agent_intent.") {
                        let agent_id = event.source
                            .strip_prefix("agent_intent.")
                            .unwrap_or("unknown");
                        let recent = db_lock
                            .count_recent_agent_events(agent_id, 60)
                            .unwrap_or(0) as u32;
                        drop(db_lock); // Release lock before async call
                        // recent = ops in last 60 seconds → multiply by 1 for per-minute rate
                        ks_clone.check_anomaly(agent_id, recent, ks_limit).await;
                    } else {
                        drop(db_lock);
                    }
                    // ── END GAP 4 ─────────────────────────────────────────────
                                        
                    // lock released automatically here (drop at end of scope)
                });
            }

            Err(broadcast::error::RecvError::Lagged(missed)) => {
                eprintln!("[WARN] EventBus lagged — missed {} events", missed);
            }

            Err(broadcast::error::RecvError::Closed) => {
                eprintln!("[CRITICAL] EventBus closed unexpectedly.");
                break;
            }
        }
    }

    Ok(())
}
