use rusqlite::{Connection, Result};
use std::path::Path;
use std::time::SystemTime;
use crate::event::SystemEvent;

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens or creates the SQLite database and ensures our schemas exist
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// The Broad, Abstract Schema exactly as specified in the architecture
    fn init_schema(&self) -> Result<()> {
        // 1. Raw Events Log (Stores the normalized SystemEvent stream)
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS raw_events_log (
                id TEXT PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                event_source TEXT NOT NULL,
                event_type TEXT NOT NULL,
                target_uri TEXT NOT NULL,
                metadata_json TEXT NOT NULL
            )",
            [],
        )?;

        // 2. Semantic Entities (Maps URIs to abstract roles via tags)
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS semantic_entities (
                uri TEXT PRIMARY KEY,
                semantic_tags TEXT NOT NULL,
                confidence_score REAL NOT NULL,
                last_observed INTEGER NOT NULL
            )",
            [],
        )?;

        // 3. Agent Registry (Stores external agents like OpenClaw)
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_registry (
                agent_id TEXT PRIMARY KEY,
                trust_level INTEGER NOT NULL,
                permissions_json TEXT NOT NULL
            )",
            [],
        )?;

	        // 4. Permission Tokens — short-lived tokens issued after gate approval
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS permission_tokens (
                token_id      TEXT PRIMARY KEY,
                agent_id      TEXT NOT NULL,
                intent_hash   TEXT NOT NULL,
                issued_at     INTEGER NOT NULL,
                expires_at    INTEGER NOT NULL,
                is_used       INTEGER DEFAULT 0,
                gate_decision TEXT NOT NULL
            )",
            [],
        )?;

        // 5. Rollback Snapshots — vault records for undo capability
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS rollback_snapshots (
                snapshot_id      TEXT PRIMARY KEY,
                agent_id         TEXT NOT NULL,
                permission_token TEXT NOT NULL,
                resource_uri     TEXT NOT NULL,
                strategy_used    TEXT NOT NULL,
                vault_path       TEXT,
                metadata_json    TEXT,
                created_at       INTEGER NOT NULL,
                expires_at       INTEGER NOT NULL,
                is_restored      INTEGER DEFAULT 0
            )",
            [],
        )?;

        // 6. Safety Rules — user constraints stored OUTSIDE agent memory
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS safety_rules (
                rule_id           TEXT PRIMARY KEY,
                rule_type         TEXT NOT NULL,
                scope_tags        TEXT NOT NULL,
                applies_to_agent  TEXT,
                is_active         INTEGER DEFAULT 1,
                created_at        INTEGER NOT NULL,
                created_by        TEXT NOT NULL
            )",
            [],
        )?;


        Ok(())
    }

    /// Inserts a normalized event into the raw log
    pub fn insert_event(&self, event: &SystemEvent) -> Result<()> {
        // Convert the SystemTime to a Unix timestamp integer for SQLite
        let timestamp = event.timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        
        let metadata_str = event.metadata.to_string();
        let event_type_str = format!("{:?}", event.event_type);

        self.conn.execute(
            "INSERT INTO raw_events_log (id, timestamp, event_source, event_type, target_uri, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                &event.id,
                timestamp,
                &event.source,
                &event_type_str,
                &event.target_uri,
                &metadata_str,
            ),
        )?;
        
        Ok(())
    }

    // ── BROAD QUERY 1 ────────────────────────────────────────────────────────
    // Resolves ANY abstract tags into physical URIs.
    // The caller decides what tags mean ("role:work_documents", "context:private",
    // "type:config") — this function never hard-codes a category.
    pub fn query_entities_by_tags(&self, tags: &[String]) -> Result<Vec<serde_json::Value>> {
        if tags.is_empty() {
            return Ok(vec![]);
        }
        // Build a dynamic WHERE clause: one LIKE condition per tag, OR-joined.
        // SQLite stores tags as a JSON array string: ["role:user_media","context:work"]
        let conditions: Vec<String> = (0..tags.len())
            .map(|_| "semantic_tags LIKE '%' || ? || '%'".to_string())
            .collect();
        let sql = format!(
            "SELECT uri, semantic_tags, confidence_score, last_observed \
             FROM semantic_entities WHERE {}",
            conditions.join(" OR ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(tags.iter()), |row| {
            Ok(serde_json::json!({
                "uri":              row.get::<_, String>(0)?,
                "semantic_tags":    row.get::<_, String>(1)?,
                "confidence_score": row.get::<_, f64>(2)?,
                "last_observed":    row.get::<_, i64>(3)?,
            }))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── BROAD QUERY 2 ────────────────────────────────────────────────────────
    // Returns the N most recent events from ALL sensors, no filter.
    // Broad: not "get recent file events" — returns everything, sorted newest-first.
    pub fn get_recent_events(&self, limit: usize) -> Result<Vec<serde_json::Value>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, event_source, event_type, target_uri, metadata_json
             FROM raw_events_log
             ORDER BY timestamp DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(serde_json::json!({
                "id":           row.get::<_, String>(0)?,
                "timestamp":    row.get::<_, i64>(1)?,
                "event_source": row.get::<_, String>(2)?,
                "event_type":   row.get::<_, String>(3)?,
                "target_uri":   row.get::<_, String>(4)?,
                "metadata":     row.get::<_, String>(5)?,
            }))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── BROAD QUERY 3 ────────────────────────────────────────────────────────
    // Same as above but filtered by source string.
    // The CALLER decides what source to filter on — "sensor.process", "sensor.fs",
    // "agent_intent.claude-code" — this function never hard-codes a sensor name.
    pub fn get_recent_events_by_source(
        &self,
        source: &str,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, event_source, event_type, target_uri, metadata_json
             FROM raw_events_log
             WHERE event_source = ?1
             ORDER BY timestamp DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![source, limit as i64], |row| {
            Ok(serde_json::json!({
                "id":           row.get::<_, String>(0)?,
                "timestamp":    row.get::<_, i64>(1)?,
                "event_source": row.get::<_, String>(2)?,
                "event_type":   row.get::<_, String>(3)?,
                "target_uri":   row.get::<_, String>(4)?,
                "metadata":     row.get::<_, String>(5)?,
            }))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── PHASE 2 QUERIES ──────────────────────────────────────────────────────

    /// Get the semantic classification of any resource URI.
    /// Returns None if Guardian has never observed this resource.
    pub fn get_entity_by_uri(&self, uri: &str) -> Result<Option<crate::types::SemanticEntity>> {
        let mut stmt = self.conn.prepare(
            "SELECT uri, semantic_tags, confidence_score, last_observed
             FROM semantic_entities WHERE uri = ?1",
        )?;
        let mut rows = stmt.query([uri])?;
        if let Some(row) = rows.next()? {
            let tags_json: String = row.get(1)?;
            let semantic_tags: Vec<String> =
                serde_json::from_str(&tags_json).unwrap_or_default();
            Ok(Some(crate::types::SemanticEntity {
                uri:              row.get(0)?,
                semantic_tags,
                confidence_score: row.get(2)?,
                last_observed:    row.get(3)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the trust level (0–100) of a registered agent. None = unregistered.
    pub fn get_agent_trust_level(&self, agent_id: &str) -> Result<Option<i32>> {
        let mut stmt = self.conn.prepare(
            "SELECT trust_level FROM agent_registry WHERE agent_id = ?1",
        )?;
        let mut rows = stmt.query([agent_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Count how many agent_intent events an agent emitted in the last N seconds.
    /// Used by the AnomalyDetector to detect runaway agents.
    pub fn count_recent_agent_events(&self, agent_id: &str, seconds: i64) -> Result<usize> {
        let cutoff = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64 - seconds;
        let pattern = format!("agent_intent.{}%", agent_id);
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM raw_events_log
             WHERE event_source LIKE ?1 AND timestamp > ?2",
            rusqlite::params![pattern, cutoff],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Load all active safety rules. Called at startup and cached.
    pub fn get_active_safety_rules(&self) -> Result<Vec<crate::types::SafetyRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT rule_id, rule_type, scope_tags, applies_to_agent, created_by
             FROM safety_rules WHERE is_active = 1",
        )?;
        let rows = stmt.query_map([], |row| {
            let rule_type_str: String = row.get(1)?;
            let tags_json: String     = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                rule_type_str,
                tags_json,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut rules = Vec::new();
        for row in rows.filter_map(|r| r.ok()) {
            let rule_type = match row.1.as_str() {
                "always_block"                => crate::types::RuleType::AlwaysBlock,
                "always_require_confirmation" => crate::types::RuleType::AlwaysRequireConfirmation,
                "never_allow_scope"           => crate::types::RuleType::NeverAllowScope,
                _                             => crate::types::RuleType::AlwaysBlock,
            };
            let scope_tags: Vec<String> =
                serde_json::from_str(&row.2).unwrap_or_default();
            rules.push(crate::types::SafetyRule {
                rule_id:          row.0,
                rule_type,
                scope_tags,
                applies_to_agent: row.3,
                created_by:       row.4,
            });
        }
        Ok(rules)
    }

    /// Insert a permission token after gate approval.
    pub fn insert_permission_token(
        &self,
        token_id: &str, agent_id: &str, intent_hash: &str,
        issued_at: i64, expires_at: i64, gate_decision: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO permission_tokens
             (token_id, agent_id, intent_hash, issued_at, expires_at, gate_decision)
             VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![token_id, agent_id, intent_hash, issued_at, expires_at, gate_decision],
        )?;
        Ok(())
    }

    /// Validate and consume a permission token. Returns true if valid + unused + not expired.
    pub fn validate_and_consume_token(&self, token_id: &str, agent_id: &str) -> Result<bool> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let rows_affected = self.conn.execute(
            "UPDATE permission_tokens SET is_used = 1
             WHERE token_id = ?1 AND agent_id = ?2
               AND is_used = 0 AND expires_at > ?3",
            rusqlite::params![token_id, agent_id, now],
        )?;
        Ok(rows_affected > 0)
    }

    /// Insert a rollback snapshot record.
    pub fn insert_rollback_snapshot(
        &self,
        snapshot_id: &str, agent_id: &str, permission_token: &str,
        resource_uri: &str, strategy_used: &str,
        vault_path: Option<&str>, metadata_json: Option<&str>,
        created_at: i64, expires_at: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO rollback_snapshots
             (snapshot_id, agent_id, permission_token, resource_uri, strategy_used,
              vault_path, metadata_json, created_at, expires_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                snapshot_id, agent_id, permission_token, resource_uri, strategy_used,
                vault_path, metadata_json, created_at, expires_at
            ],
        )?;
        Ok(())
    }

    /// Purge expired rollback snapshots. Called by background cleanup task.
    pub fn purge_expired_snapshots(&self) -> Result<Vec<String>> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let mut stmt = self.conn.prepare(
            "SELECT vault_path FROM rollback_snapshots
             WHERE expires_at < ?1 AND is_restored = 0 AND vault_path IS NOT NULL",
        )?;
        let paths: Vec<String> = stmt
            .query_map([now], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        self.conn.execute(
            "DELETE FROM rollback_snapshots WHERE expires_at < ?1",
            [now],
        )?;
        Ok(paths)
    }

        /// Insert a single safety rule. Used when seeding from safety_rules.toml.
    pub fn insert_safety_rule(
        &self,
        rule_id: &str,
        rule_type: &str,
        scope_tags: &str,
        applies_to_agent: Option<&str>,
        created_at: i64,
        created_by: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO safety_rules
             (rule_id, rule_type, scope_tags, applies_to_agent, created_at, created_by)
             VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                rule_id, rule_type, scope_tags,
                applies_to_agent, created_at, created_by
            ],
        )?;
        Ok(())
    }


}
