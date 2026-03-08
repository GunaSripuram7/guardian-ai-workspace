// cli/src/main.rs
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};

use guardian_core::db::Database;
use guardian_core::event::SystemEvent;
use guardian_core::notification::{NotificationLevel, UiNotification};

use guardian_sensors::Sensor;
use guardian_sensors::fs_sensor::FileSystemSensor;
use guardian_sensors::process_sensor::ProcessSensor;

use guardian_mcp::{McpRequest, McpServer, QuerySemanticContextTool, GetSystemStateTool};

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
    // Change this path to wherever you want to monitor on your machine.
    let mut fs_sensor   = FileSystemSensor::new("C:\\Users\\chand\\test_for_gAI");
    let mut proc_sensor = ProcessSensor::new(3); // polls every 3 seconds

    fs_sensor.start(event_tx.clone()).await?;
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
    let mcp_server = Arc::new(mcp_server);


    println!();

    // ── 7. MOCK AGENT INTERACTION (demonstrates the full flow) ──────────────
    let test_server = Arc::clone(&mcp_server);
    let ui_tx_mcp   = ui_tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;

        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("[MOCK AGENT] Connecting to Guardian MCP Gateway...");

        // Test 1: tools/list
        let list_req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id:      Some(serde_json::json!(1)),
            method:  "tools/list".to_string(),
            params:  None,
        };
        println!("[MOCK AGENT] → tools/list");
        let res = test_server.process_request(list_req).await;
        println!("[GUARDIAN]   ← {:?}\n", res.result);

        // Test 2: log_agent_intent (broad: any action, any target)
        let intent_req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id:      Some(serde_json::json!(2)),
            method:  "tools/call".to_string(),
            params:  Some(serde_json::json!({
                "name": "log_agent_intent",
                "arguments": {
                    "agent_id":   "mock-agent-v1",
                    "action":     "read",
                    "target_uri": "file://C:/Users/chand/test_for_gAI/sample.txt",
                    "metadata":   { "reason": "user asked to summarise this file" }
                }
            })),
        };
        println!("[MOCK AGENT] → log_agent_intent (action=read)");
        let res = test_server.process_request(intent_req).await;
        println!("[GUARDIAN]   ← {:?}\n", res.result);
        let _ = ui_tx_mcp.send(UiNotification::info(
            "Agent Intent Logged",
            "mock-agent-v1 declared intent to READ sample.txt",
        )).await;

        // Test 3: get_system_state (all sources, last 5 events)
        let state_req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id:      Some(serde_json::json!(3)),
            method:  "tools/call".to_string(),
            params:  Some(serde_json::json!({
                "name": "get_system_state",
                "arguments": { "limit": 5 }
            })),
        };
        println!("[MOCK AGENT] → get_system_state (limit=5, all sources)");
        let res = test_server.process_request(state_req).await;
        println!("[GUARDIAN]   ← {:?}", res.result);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    });

    // ── 8. CORE SUBSCRIBER LOOP (EventBus → DB + UI notifications) ───────────
    // This is Phase 1: observe silently, log everything, surface Info notifications.
    // Phase 2 will add risk scoring here before the UI emit step.
        // ── CORE SUBSCRIBER LOOP ─────────────────────────────────────────────────
    // IMPORTANT: Never block here. Spawn each DB write as its own task so
    // event_rx.recv() is called immediately every time — no dropped events.
    loop {
        match event_rx.recv().await {
            Ok(event) => {
                println!("[OBSERVED] {} → {}", event.source, event.target_uri);

                // Spawn DB write independently — never block the receiver loop
                let db_clone    = Arc::clone(&db);
                let ui_tx_clone = ui_tx.clone();
                tokio::spawn(async move {
                    let db_lock = db_clone.lock().await;
                    if let Err(e) = db_lock.insert_event(&event) {
                        eprintln!("[ERROR] DB write failed: {}", e);
                    }
                    drop(db_lock);
                    let notif = UiNotification::info(
                        &format!("Event: {}", event.source),
                        &format!("{:?} → {}", event.event_type, event.target_uri),
                    );
                    let _ = ui_tx_clone.send(notif).await;
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
