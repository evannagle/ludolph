//! Schedule management for automated tasks.
//!
//! Provides persistent storage and execution tracking for scheduled tasks.
//! Schedules are managed via conversation with the LLM using schedule tools.

#![allow(clippy::significant_drop_tightening)]

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cron::Schedule as CronSchedule;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::SchedulerConfig;

/// A scheduled task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    /// Unique identifier (UUID)
    pub id: String,
    /// Telegram user ID who created this schedule
    pub user_id: i64,
    /// Human-readable name
    pub name: String,
    /// The prompt to send to the LLM when executing
    pub prompt: String,
    /// Cron expression for timing
    pub cron_expression: String,
    /// Timezone for the cron expression (default: local)
    pub timezone: String,
    /// Next scheduled run time
    pub next_run: Option<DateTime<Utc>>,
    /// Current status
    pub status: ScheduleStatus,
    /// When the schedule was created
    pub created_at: DateTime<Utc>,
    /// When the schedule was last updated
    pub updated_at: DateTime<Utc>,
    /// Last execution time
    pub last_run: Option<DateTime<Utc>>,
    /// Last execution result summary
    pub last_result: Option<String>,
    /// Notify before execution (1 = yes, 0 = no)
    pub notify_before: bool,
    /// Notify after execution (1 = yes, 0 = no)
    pub notify_after: bool,
    /// Tags for filtering (JSON array)
    pub tags: Vec<String>,
    /// Priority (higher runs first when concurrent)
    pub priority: i32,
    /// Number of times this schedule has run
    pub run_count: i64,
    /// Maximum runs (0 = unlimited)
    pub max_runs: i64,
}

/// Schedule status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleStatus {
    Active,
    Paused,
    Completed,
}

impl ScheduleStatus {
    fn from_str(s: &str) -> Self {
        match s {
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            _ => Self::Active,
        }
    }

    /// Get the string representation of the status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
        }
    }
}

/// A record of a schedule execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRun {
    pub id: i64,
    pub schedule_id: String,
    pub user_id: i64,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: RunStatus,
    pub result_summary: Option<String>,
    pub error_message: Option<String>,
}

/// Status of a schedule run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Success,
    Error,
    Cancelled,
}

impl RunStatus {
    fn from_str(s: &str) -> Self {
        match s {
            "success" => Self::Success,
            "error" => Self::Error,
            "cancelled" => Self::Cancelled,
            _ => Self::Running,
        }
    }

    /// Get the string representation of the run status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Parameters for creating a new schedule.
#[derive(Debug, Clone)]
pub struct CreateScheduleParams {
    pub user_id: i64,
    pub name: String,
    pub prompt: String,
    pub cron_expression: String,
    pub timezone: Option<String>,
    pub notify_before: bool,
    pub notify_after: bool,
    pub tags: Vec<String>,
    pub priority: i32,
    pub max_runs: i64,
}

/// Parameters for updating a schedule.
#[derive(Debug, Clone, Default)]
pub struct UpdateScheduleParams {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub cron_expression: Option<String>,
    pub timezone: Option<String>,
    pub status: Option<ScheduleStatus>,
    pub notify_before: Option<bool>,
    pub notify_after: Option<bool>,
    pub tags: Option<Vec<String>>,
    pub priority: Option<i32>,
    pub max_runs: Option<i64>,
}

/// Scheduler backed by `SQLite`.
///
/// Thread-safe via internal Mutex.
pub struct Scheduler {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    config: SchedulerConfig,
}

impl Scheduler {
    /// Open or create the scheduler database.
    pub fn open(db_path: &Path, config: &SchedulerConfig) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open database: {}", db_path.display()))?;

        // Initialize schema
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schedules (
                id TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                prompt TEXT NOT NULL,
                cron_expression TEXT NOT NULL,
                timezone TEXT DEFAULT 'local',
                next_run TEXT,
                status TEXT DEFAULT 'active',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_run TEXT,
                last_result TEXT,
                notify_before INTEGER DEFAULT 0,
                notify_after INTEGER DEFAULT 1,
                tags TEXT,
                priority INTEGER DEFAULT 0,
                run_count INTEGER DEFAULT 0,
                max_runs INTEGER DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_schedules_user ON schedules(user_id);
            CREATE INDEX IF NOT EXISTS idx_schedules_status ON schedules(status);
            CREATE INDEX IF NOT EXISTS idx_schedules_next_run ON schedules(next_run);

            CREATE TABLE IF NOT EXISTS schedule_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                schedule_id TEXT NOT NULL,
                user_id INTEGER NOT NULL,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                status TEXT NOT NULL,
                result_summary TEXT,
                error_message TEXT,
                FOREIGN KEY (schedule_id) REFERENCES schedules(id)
            );

            CREATE INDEX IF NOT EXISTS idx_runs_schedule ON schedule_runs(schedule_id);
            CREATE INDEX IF NOT EXISTS idx_runs_user ON schedule_runs(user_id);",
        )
        .context("Failed to initialize scheduler database schema")?;

        Ok(Self {
            conn: Mutex::new(conn),
            config: config.clone(),
        })
    }

    /// Create a new schedule.
    pub fn create(&self, params: CreateScheduleParams) -> Result<Schedule> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let timezone = params.timezone.unwrap_or_else(|| "local".to_string());

        // Parse cron expression to validate and calculate next run
        let next_run = calculate_next_run(&params.cron_expression)?;

        let tags_json = serde_json::to_string(&params.tags)?;

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        conn.execute(
            "INSERT INTO schedules (
                id, user_id, name, prompt, cron_expression, timezone,
                next_run, status, created_at, updated_at,
                notify_before, notify_after, tags, priority, max_runs
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                id,
                params.user_id,
                params.name,
                params.prompt,
                params.cron_expression,
                timezone,
                next_run.map(|dt| dt.to_rfc3339()),
                ScheduleStatus::Active.as_str(),
                now.to_rfc3339(),
                now.to_rfc3339(),
                i32::from(params.notify_before),
                i32::from(params.notify_after),
                tags_json,
                params.priority,
                params.max_runs,
            ],
        )
        .context("Failed to create schedule")?;

        Ok(Schedule {
            id,
            user_id: params.user_id,
            name: params.name,
            prompt: params.prompt,
            cron_expression: params.cron_expression,
            timezone,
            next_run,
            status: ScheduleStatus::Active,
            created_at: now,
            updated_at: now,
            last_run: None,
            last_result: None,
            notify_before: params.notify_before,
            notify_after: params.notify_after,
            tags: params.tags,
            priority: params.priority,
            run_count: 0,
            max_runs: params.max_runs,
        })
    }

    /// Get a schedule by ID.
    pub fn get(&self, id: &str) -> Result<Option<Schedule>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let mut stmt = conn.prepare(
            "SELECT id, user_id, name, prompt, cron_expression, timezone,
                    next_run, status, created_at, updated_at, last_run, last_result,
                    notify_before, notify_after, tags, priority, run_count, max_runs
             FROM schedules WHERE id = ?1",
        )?;

        let schedule = stmt
            .query_row(params![id], |row| Ok(row_to_schedule(row)))
            .optional()?;

        Ok(schedule)
    }

    /// List schedules for a user.
    pub fn list(
        &self,
        user_id: i64,
        status_filter: Option<ScheduleStatus>,
        tag_filter: Option<&str>,
    ) -> Result<Vec<Schedule>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let mut sql = String::from(
            "SELECT id, user_id, name, prompt, cron_expression, timezone,
                    next_run, status, created_at, updated_at, last_run, last_result,
                    notify_before, notify_after, tags, priority, run_count, max_runs
             FROM schedules WHERE user_id = ?1",
        );

        if let Some(status) = status_filter {
            let _ = write!(sql, " AND status = '{}'", status.as_str());
        }

        if let Some(tag) = tag_filter {
            // Search for tag in JSON array
            let _ = write!(sql, " AND tags LIKE '%\"{tag}%'");
        }

        sql.push_str(" ORDER BY priority DESC, created_at DESC");

        let mut stmt = conn.prepare(&sql)?;
        let schedules = stmt
            .query_map(params![user_id], |row| Ok(row_to_schedule(row)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(schedules)
    }

    /// Update a schedule.
    #[allow(clippy::needless_pass_by_value)]
    pub fn update(&self, id: &str, params: UpdateScheduleParams) -> Result<Option<Schedule>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        // Build dynamic update query
        let mut updates = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(name) = &params.name {
            updates.push("name = ?");
            values.push(Box::new(name.clone()));
        }
        if let Some(prompt) = &params.prompt {
            updates.push("prompt = ?");
            values.push(Box::new(prompt.clone()));
        }
        if let Some(cron_expression) = &params.cron_expression {
            // Validate and update next_run
            let next_run = calculate_next_run(cron_expression)?;
            updates.push("cron_expression = ?");
            values.push(Box::new(cron_expression.clone()));
            updates.push("next_run = ?");
            values.push(Box::new(next_run.map(|dt| dt.to_rfc3339())));
        }
        if let Some(timezone) = &params.timezone {
            updates.push("timezone = ?");
            values.push(Box::new(timezone.clone()));
        }
        if let Some(status) = params.status {
            updates.push("status = ?");
            values.push(Box::new(status.as_str().to_string()));
        }
        if let Some(notify_before) = params.notify_before {
            updates.push("notify_before = ?");
            values.push(Box::new(i32::from(notify_before)));
        }
        if let Some(notify_after) = params.notify_after {
            updates.push("notify_after = ?");
            values.push(Box::new(i32::from(notify_after)));
        }
        if let Some(tags) = &params.tags {
            updates.push("tags = ?");
            values.push(Box::new(serde_json::to_string(tags)?));
        }
        if let Some(priority) = params.priority {
            updates.push("priority = ?");
            values.push(Box::new(priority));
        }
        if let Some(max_runs) = params.max_runs {
            updates.push("max_runs = ?");
            values.push(Box::new(max_runs));
        }

        if updates.is_empty() {
            return self.get(id);
        }

        updates.push("updated_at = ?");
        values.push(Box::new(Utc::now().to_rfc3339()));

        let sql = format!("UPDATE schedules SET {} WHERE id = ?", updates.join(", "));

        values.push(Box::new(id.to_string()));

        let params: Vec<&dyn rusqlite::ToSql> = values.iter().map(AsRef::as_ref).collect();
        conn.execute(&sql, params.as_slice())?;

        drop(conn);
        self.get(id)
    }

    /// Delete a schedule.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        // Delete associated runs first
        conn.execute(
            "DELETE FROM schedule_runs WHERE schedule_id = ?1",
            params![id],
        )?;

        let rows = conn.execute("DELETE FROM schedules WHERE id = ?1", params![id])?;

        Ok(rows > 0)
    }

    /// Get schedules that are due to run.
    pub fn get_due_schedules(&self) -> Result<Vec<Schedule>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let now = Utc::now().to_rfc3339();

        let mut stmt = conn.prepare(
            "SELECT id, user_id, name, prompt, cron_expression, timezone,
                    next_run, status, created_at, updated_at, last_run, last_result,
                    notify_before, notify_after, tags, priority, run_count, max_runs
             FROM schedules
             WHERE status = 'active'
               AND next_run IS NOT NULL
               AND next_run <= ?1
             ORDER BY priority DESC, next_run ASC",
        )?;

        let schedules = stmt
            .query_map(params![now], |row| Ok(row_to_schedule(row)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(schedules)
    }

    /// Record the start of a schedule run.
    pub fn record_run_start(&self, schedule_id: &str, user_id: i64) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO schedule_runs (schedule_id, user_id, started_at, status)
             VALUES (?1, ?2, ?3, ?4)",
            params![schedule_id, user_id, now, RunStatus::Running.as_str()],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Record the completion of a schedule run.
    pub fn record_run_complete(
        &self,
        run_id: i64,
        schedule_id: &str,
        status: RunStatus,
        result_summary: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let now = Utc::now();
        let now_str = now.to_rfc3339();

        // Update run record
        conn.execute(
            "UPDATE schedule_runs SET completed_at = ?1, status = ?2,
                result_summary = ?3, error_message = ?4 WHERE id = ?5",
            params![
                now_str,
                status.as_str(),
                result_summary,
                error_message,
                run_id
            ],
        )?;

        // Update schedule with run result
        let next_run = {
            let schedule = self.get_internal(&conn, schedule_id)?;
            schedule.and_then(|s| calculate_next_run(&s.cron_expression).ok().flatten())
        };

        // Truncate result summary for storage
        let truncated_result = result_summary.map(|s| {
            if s.len() > 500 {
                format!("{}...", &s[..497])
            } else {
                s.to_string()
            }
        });

        conn.execute(
            "UPDATE schedules SET
                last_run = ?1,
                last_result = ?2,
                next_run = ?3,
                run_count = run_count + 1,
                updated_at = ?4
             WHERE id = ?5",
            params![
                now_str,
                truncated_result,
                next_run.map(|dt| dt.to_rfc3339()),
                now_str,
                schedule_id
            ],
        )?;

        // Check if max_runs reached
        conn.execute(
            "UPDATE schedules SET status = 'completed'
             WHERE id = ?1 AND max_runs > 0 AND run_count >= max_runs",
            params![schedule_id],
        )?;

        Ok(())
    }

    /// Get run history for a schedule.
    pub fn get_history(
        &self,
        schedule_id: Option<&str>,
        user_id: i64,
        limit: usize,
    ) -> Result<Vec<ScheduleRun>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let sql = schedule_id.map_or_else(
            || {
                format!(
                    "SELECT id, schedule_id, user_id, started_at, completed_at,
                            status, result_summary, error_message
                     FROM schedule_runs
                     WHERE user_id = {user_id}
                     ORDER BY started_at DESC
                     LIMIT {limit}"
                )
            },
            |id| {
                format!(
                    "SELECT id, schedule_id, user_id, started_at, completed_at,
                            status, result_summary, error_message
                     FROM schedule_runs
                     WHERE schedule_id = '{id}' AND user_id = {user_id}
                     ORDER BY started_at DESC
                     LIMIT {limit}"
                )
            },
        );

        let mut stmt = conn.prepare(&sql)?;
        let runs = stmt
            .query_map([], |row| {
                let started_at_str: String = row.get(3)?;
                let completed_at_str: Option<String> = row.get(4)?;
                let status_str: String = row.get(5)?;

                Ok(ScheduleRun {
                    id: row.get(0)?,
                    schedule_id: row.get(1)?,
                    user_id: row.get(2)?,
                    started_at: parse_datetime(&started_at_str),
                    completed_at: completed_at_str.map(|s| parse_datetime(&s)),
                    status: RunStatus::from_str(&status_str),
                    result_summary: row.get(6)?,
                    error_message: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(runs)
    }

    /// Get all active schedules for system prompt inclusion.
    pub fn get_active_schedules(&self, user_id: i64) -> Result<Vec<Schedule>> {
        self.list(user_id, Some(ScheduleStatus::Active), None)
    }

    /// Find a schedule by name (case-insensitive partial match).
    pub fn find_by_name(&self, user_id: i64, name: &str) -> Result<Option<Schedule>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let mut stmt = conn.prepare(
            "SELECT id, user_id, name, prompt, cron_expression, timezone,
                    next_run, status, created_at, updated_at, last_run, last_result,
                    notify_before, notify_after, tags, priority, run_count, max_runs
             FROM schedules
             WHERE user_id = ?1 AND LOWER(name) LIKE LOWER(?2)
             ORDER BY created_at DESC
             LIMIT 1",
        )?;

        let pattern = format!("%{name}%");
        let schedule = stmt
            .query_row(params![user_id, pattern], |row| Ok(row_to_schedule(row)))
            .optional()?;

        Ok(schedule)
    }

    /// Internal helper to get schedule with an existing connection.
    #[allow(clippy::unused_self)]
    fn get_internal(&self, conn: &Connection, id: &str) -> Result<Option<Schedule>> {
        let mut stmt = conn.prepare(
            "SELECT id, user_id, name, prompt, cron_expression, timezone,
                    next_run, status, created_at, updated_at, last_run, last_result,
                    notify_before, notify_after, tags, priority, run_count, max_runs
             FROM schedules WHERE id = ?1",
        )?;

        let schedule = stmt
            .query_row(params![id], |row| Ok(row_to_schedule(row)))
            .optional()?;

        Ok(schedule)
    }
}

/// Convert a database row to a Schedule struct.
fn row_to_schedule(row: &rusqlite::Row<'_>) -> Schedule {
    let next_run_str: Option<String> = row.get(6).unwrap_or(None);
    let last_run_str: Option<String> = row.get(10).unwrap_or(None);
    let status_str: String = row.get(7).unwrap_or_else(|_| "active".to_string());
    let tags_json: Option<String> = row.get(14).unwrap_or(None);

    Schedule {
        id: row.get(0).unwrap_or_default(),
        user_id: row.get(1).unwrap_or(0),
        name: row.get(2).unwrap_or_default(),
        prompt: row.get(3).unwrap_or_default(),
        cron_expression: row.get(4).unwrap_or_default(),
        timezone: row.get(5).unwrap_or_else(|_| "local".to_string()),
        next_run: next_run_str.map(|s| parse_datetime(&s)),
        status: ScheduleStatus::from_str(&status_str),
        created_at: parse_datetime(&row.get::<_, String>(8).unwrap_or_default()),
        updated_at: parse_datetime(&row.get::<_, String>(9).unwrap_or_default()),
        last_run: last_run_str.map(|s| parse_datetime(&s)),
        last_result: row.get(11).unwrap_or(None),
        notify_before: row.get::<_, i32>(12).unwrap_or(0) != 0,
        notify_after: row.get::<_, i32>(13).unwrap_or(1) != 0,
        tags: tags_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        priority: row.get(15).unwrap_or(0),
        run_count: row.get(16).unwrap_or(0),
        max_runs: row.get(17).unwrap_or(0),
    }
}

/// Parse a datetime string.
fn parse_datetime(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).map_or_else(|_| Utc::now(), |dt| dt.with_timezone(&Utc))
}

/// Calculate the next run time for a cron expression.
fn calculate_next_run(cron_expression: &str) -> Result<Option<DateTime<Utc>>> {
    // Standard cron has 5 fields (minute hour day month weekday)
    // cron crate expects 7 fields (second minute hour day month weekday year)
    // We add "0" for seconds and "*" for year
    let full_cron = if cron_expression.split_whitespace().count() == 5 {
        format!("0 {cron_expression} *")
    } else {
        cron_expression.to_string()
    };

    let schedule: CronSchedule = full_cron
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid cron expression: {e}"))?;

    let next = schedule.upcoming(Utc).next();
    Ok(next)
}

/// Format schedules for inclusion in system prompt.
pub fn format_schedule_context(schedules: &[Schedule]) -> String {
    if schedules.is_empty() {
        return String::new();
    }

    let mut output = String::from("\n\n## Active Schedules\n\n");
    let _ = writeln!(
        output,
        "You have {} active scheduled task{}:",
        schedules.len(),
        if schedules.len() == 1 { "" } else { "s" }
    );

    for schedule in schedules {
        let timing = describe_cron(&schedule.cron_expression);
        let last_status = schedule.last_run.map_or_else(
            || "never run".to_string(),
            |dt| {
                let rel = format_relative_time(dt);
                format!("last: {rel}")
            },
        );
        let next_status = schedule.next_run.map_or_else(String::new, |dt| {
            let rel = format_relative_time(dt);
            format!(", next: {rel}")
        });
        let name = &schedule.name;

        let _ = writeln!(
            output,
            "- \"{name}\" ({timing}) - {last_status}{next_status}"
        );
    }

    output.push_str("\nWhen users ask about schedules, use the schedule tools.\n");
    output.push_str("Ask one question at a time when creating new schedules.\n");

    output
}

/// Describe a cron expression in human-readable form.
pub fn describe_cron(cron: &str) -> String {
    // Simple descriptions for common patterns
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return cron.to_string();
    }

    let (minute, hour, day, _month, weekday) = (parts[0], parts[1], parts[2], parts[3], parts[4]);

    // Daily at specific time
    if day == "*" && weekday == "*" {
        if let (Ok(h), Ok(m)) = (hour.parse::<u32>(), minute.parse::<u32>()) {
            return format!("daily at {h}:{m:02}");
        }
    }

    // Weekdays
    if day == "*" && weekday == "1-5" {
        if let (Ok(h), Ok(m)) = (hour.parse::<u32>(), minute.parse::<u32>()) {
            return format!("weekdays at {h}:{m:02}");
        }
    }

    // Weekly on specific day
    if day == "*" && weekday.len() == 1 {
        if let (Ok(h), Ok(m)) = (hour.parse::<u32>(), minute.parse::<u32>()) {
            let day_name = match weekday {
                "0" | "7" => "Sun",
                "1" => "Mon",
                "2" => "Tue",
                "3" => "Wed",
                "4" => "Thu",
                "5" => "Fri",
                "6" => "Sat",
                _ => weekday,
            };
            return format!("{day_name} at {h}:{m:02}");
        }
    }

    // Fallback to showing the cron expression
    cron.to_string()
}

/// Format a datetime as relative time.
pub fn format_relative_time(dt: DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = now.signed_duration_since(dt);

    let secs = diff.num_seconds();

    if secs < 0 {
        // Future
        let future_secs = -secs;
        if future_secs < 60 {
            return "in a moment".to_string();
        }
        let future_mins = -diff.num_minutes();
        if future_mins < 60 {
            return format!("in {future_mins} min");
        }
        let future_hours = -diff.num_hours();
        if future_hours < 24 {
            return format!("in {future_hours}h");
        }
        let future_days = -diff.num_days();
        return format!("in {future_days}d");
    }

    // Past
    if secs < 60 {
        return "just now".to_string();
    }
    let mins = diff.num_minutes();
    if mins < 60 {
        return format!("{mins} min ago");
    }
    let hours = diff.num_hours();
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = diff.num_days();
    format!("{days}d ago")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_scheduler() -> (Scheduler, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_scheduler.db");
        let config = SchedulerConfig::default();
        let scheduler = Scheduler::open(&db_path, &config).unwrap();
        (scheduler, dir)
    }

    #[test]
    fn create_and_retrieve_schedule() {
        let (scheduler, _dir) = test_scheduler();

        let schedule = scheduler
            .create(CreateScheduleParams {
                user_id: 12345,
                name: "Test Schedule".to_string(),
                prompt: "Do something".to_string(),
                cron_expression: "0 14 * * *".to_string(),
                timezone: None,
                notify_before: false,
                notify_after: true,
                tags: vec!["test".to_string()],
                priority: 0,
                max_runs: 0,
            })
            .unwrap();

        assert_eq!(schedule.name, "Test Schedule");
        assert_eq!(schedule.status, ScheduleStatus::Active);
        assert!(schedule.next_run.is_some());

        let retrieved = scheduler.get(&schedule.id).unwrap().unwrap();
        assert_eq!(retrieved.name, "Test Schedule");
    }

    #[test]
    fn list_schedules_by_user() {
        let (scheduler, _dir) = test_scheduler();

        scheduler
            .create(CreateScheduleParams {
                user_id: 100,
                name: "User A Schedule".to_string(),
                prompt: "Task A".to_string(),
                cron_expression: "0 9 * * *".to_string(),
                timezone: None,
                notify_before: false,
                notify_after: true,
                tags: vec![],
                priority: 0,
                max_runs: 0,
            })
            .unwrap();

        scheduler
            .create(CreateScheduleParams {
                user_id: 200,
                name: "User B Schedule".to_string(),
                prompt: "Task B".to_string(),
                cron_expression: "0 10 * * *".to_string(),
                timezone: None,
                notify_before: false,
                notify_after: true,
                tags: vec![],
                priority: 0,
                max_runs: 0,
            })
            .unwrap();

        let user_a_schedules = scheduler.list(100, None, None).unwrap();
        assert_eq!(user_a_schedules.len(), 1);
        assert_eq!(user_a_schedules[0].name, "User A Schedule");

        let user_b_schedules = scheduler.list(200, None, None).unwrap();
        assert_eq!(user_b_schedules.len(), 1);
    }

    #[test]
    fn update_schedule() {
        let (scheduler, _dir) = test_scheduler();

        let schedule = scheduler
            .create(CreateScheduleParams {
                user_id: 12345,
                name: "Original Name".to_string(),
                prompt: "Original prompt".to_string(),
                cron_expression: "0 14 * * *".to_string(),
                timezone: None,
                notify_before: false,
                notify_after: true,
                tags: vec![],
                priority: 0,
                max_runs: 0,
            })
            .unwrap();

        let updated = scheduler
            .update(
                &schedule.id,
                UpdateScheduleParams {
                    name: Some("New Name".to_string()),
                    status: Some(ScheduleStatus::Paused),
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();

        assert_eq!(updated.name, "New Name");
        assert_eq!(updated.status, ScheduleStatus::Paused);
    }

    #[test]
    fn delete_schedule() {
        let (scheduler, _dir) = test_scheduler();

        let schedule = scheduler
            .create(CreateScheduleParams {
                user_id: 12345,
                name: "To Delete".to_string(),
                prompt: "Delete me".to_string(),
                cron_expression: "0 14 * * *".to_string(),
                timezone: None,
                notify_before: false,
                notify_after: true,
                tags: vec![],
                priority: 0,
                max_runs: 0,
            })
            .unwrap();

        assert!(scheduler.delete(&schedule.id).unwrap());
        assert!(scheduler.get(&schedule.id).unwrap().is_none());
    }

    #[test]
    fn find_by_name_partial_match() {
        let (scheduler, _dir) = test_scheduler();

        scheduler
            .create(CreateScheduleParams {
                user_id: 12345,
                name: "Daily Task Summary".to_string(),
                prompt: "Summarize tasks".to_string(),
                cron_expression: "0 14 * * *".to_string(),
                timezone: None,
                notify_before: false,
                notify_after: true,
                tags: vec![],
                priority: 0,
                max_runs: 0,
            })
            .unwrap();

        let found = scheduler.find_by_name(12345, "task summary").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Daily Task Summary");
    }

    #[test]
    fn cron_expression_validation() {
        let result = calculate_next_run("0 14 * * *");
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());

        let invalid = calculate_next_run("invalid cron");
        assert!(invalid.is_err());
    }

    #[test]
    fn describe_cron_common_patterns() {
        assert_eq!(describe_cron("0 14 * * *"), "daily at 14:00");
        assert_eq!(describe_cron("30 9 * * 1-5"), "weekdays at 9:30");
        assert_eq!(describe_cron("0 17 * * 5"), "Fri at 17:00");
    }
}
