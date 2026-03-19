//! Schedule management tools for the LLM.
//!
//! These tools allow the LLM to create, update, delete, and query schedules
//! via natural conversation.

use std::sync::Arc;

use serde_json::{Value, json};

use super::Tool;
use crate::scheduler::{
    CreateScheduleParams, Schedule, ScheduleStatus, Scheduler, UpdateScheduleParams,
};

/// Get all schedule tool definitions.
pub fn definitions() -> Vec<Tool> {
    vec![
        create_schedule_definition(),
        list_schedules_definition(),
        update_schedule_definition(),
        delete_schedule_definition(),
        run_schedule_now_definition(),
        get_schedule_history_definition(),
    ]
}

fn create_schedule_definition() -> Tool {
    Tool {
        name: "create_schedule".to_string(),
        description: "Create a new scheduled task. Use this when the user wants to set up \
                      recurring automated tasks. Ask one question at a time to gather: \
                      name, timing (cron expression), what to do (prompt), and notification preferences."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Human-readable name for the schedule (e.g., 'Daily task summary')"
                },
                "prompt": {
                    "type": "string",
                    "description": "The task to execute. This exact prompt will be sent to the LLM when the schedule runs."
                },
                "cron_expression": {
                    "type": "string",
                    "description": "Standard cron expression (5 fields: minute hour day month weekday). Examples: '0 14 * * *' (daily at 2pm), '0 9 * * 1-5' (weekdays at 9am), '0 17 * * 5' (Fridays at 5pm)"
                },
                "notify_before": {
                    "type": "boolean",
                    "description": "Send notification before execution (default: false)"
                },
                "notify_after": {
                    "type": "boolean",
                    "description": "Send notification/result after execution (default: true)"
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional tags for organizing schedules (e.g., ['work', 'daily'])"
                },
                "priority": {
                    "type": "integer",
                    "description": "Priority level (higher runs first when concurrent, default: 0)"
                },
                "max_runs": {
                    "type": "integer",
                    "description": "Maximum number of times to run (0 = unlimited, default: 0)"
                }
            },
            "required": ["name", "prompt", "cron_expression"]
        }),
    }
}

fn list_schedules_definition() -> Tool {
    Tool {
        name: "list_schedules".to_string(),
        description: "List all scheduled tasks. Can filter by status (active, paused, completed) \
                      or by tag."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["active", "paused", "completed", "all"],
                    "description": "Filter by status (default: all)"
                },
                "tag": {
                    "type": "string",
                    "description": "Filter by tag"
                }
            }
        }),
    }
}

fn update_schedule_definition() -> Tool {
    Tool {
        name: "update_schedule".to_string(),
        description: "Update an existing schedule. Use the schedule ID or name to identify it. \
                      Can update name, prompt, timing, status (pause/resume), or notification settings."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Schedule ID (UUID) - use this or name"
                },
                "name_search": {
                    "type": "string",
                    "description": "Search for schedule by name (partial match) - use this or id"
                },
                "name": {
                    "type": "string",
                    "description": "New name for the schedule"
                },
                "prompt": {
                    "type": "string",
                    "description": "New prompt/task to execute"
                },
                "cron_expression": {
                    "type": "string",
                    "description": "New cron expression for timing"
                },
                "status": {
                    "type": "string",
                    "enum": ["active", "paused"],
                    "description": "New status - use 'paused' to temporarily stop, 'active' to resume"
                },
                "notify_before": {
                    "type": "boolean",
                    "description": "Whether to notify before execution"
                },
                "notify_after": {
                    "type": "boolean",
                    "description": "Whether to notify after execution"
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "New tags (replaces existing)"
                },
                "priority": {
                    "type": "integer",
                    "description": "New priority level"
                },
                "max_runs": {
                    "type": "integer",
                    "description": "New maximum runs limit"
                }
            }
        }),
    }
}

fn delete_schedule_definition() -> Tool {
    Tool {
        name: "delete_schedule".to_string(),
        description:
            "Permanently delete a scheduled task. Use the schedule ID or name to identify it."
                .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Schedule ID (UUID) - use this or name_search"
                },
                "name_search": {
                    "type": "string",
                    "description": "Search for schedule by name (partial match) - use this or id"
                }
            }
        }),
    }
}

fn run_schedule_now_definition() -> Tool {
    Tool {
        name: "run_schedule_now".to_string(),
        description: "Execute a scheduled task immediately, outside its normal schedule. \
                      Use the schedule ID or name to identify it."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Schedule ID (UUID) - use this or name_search"
                },
                "name_search": {
                    "type": "string",
                    "description": "Search for schedule by name (partial match) - use this or id"
                }
            }
        }),
    }
}

fn get_schedule_history_definition() -> Tool {
    Tool {
        name: "get_schedule_history".to_string(),
        description: "Get execution history for schedules. Can query a specific schedule or \
                      all schedules. Shows when tasks ran, their results, and any errors."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Schedule ID (UUID) to get history for - optional, omit for all schedules"
                },
                "name_search": {
                    "type": "string",
                    "description": "Search for schedule by name (partial match) - optional"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of history entries to return (default: 10)"
                }
            }
        }),
    }
}

/// Execute a schedule tool.
///
/// Returns the result as a string suitable for the LLM.
pub fn execute(name: &str, input: &Value, scheduler: &Arc<Scheduler>, user_id: i64) -> String {
    match name {
        "create_schedule" => execute_create_schedule(input, scheduler, user_id),
        "list_schedules" => execute_list_schedules(input, scheduler, user_id),
        "update_schedule" => execute_update_schedule(input, scheduler, user_id),
        "delete_schedule" => execute_delete_schedule(input, scheduler, user_id),
        "run_schedule_now" => execute_run_schedule_now(input, scheduler, user_id),
        "get_schedule_history" => execute_get_schedule_history(input, scheduler, user_id),
        _ => format!("Unknown schedule tool: {name}"),
    }
}

fn execute_create_schedule(input: &Value, scheduler: &Arc<Scheduler>, user_id: i64) -> String {
    let Some(name) = input.get("name").and_then(|v| v.as_str()) else {
        return "Error: 'name' is required".to_string();
    };
    let Some(prompt) = input.get("prompt").and_then(|v| v.as_str()) else {
        return "Error: 'prompt' is required".to_string();
    };
    let Some(cron_expression) = input.get("cron_expression").and_then(|v| v.as_str()) else {
        return "Error: 'cron_expression' is required".to_string();
    };

    let notify_before = input
        .get("notify_before")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let notify_after = input
        .get("notify_after")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let tags: Vec<String> = input
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    #[allow(clippy::cast_possible_truncation)]
    let priority = input
        .get("priority")
        .and_then(Value::as_i64)
        .map_or(0, |v| v as i32);
    let max_runs = input.get("max_runs").and_then(Value::as_i64).unwrap_or(0);

    match scheduler.create(CreateScheduleParams {
        user_id,
        name: name.to_string(),
        prompt: prompt.to_string(),
        cron_expression: cron_expression.to_string(),
        timezone: None,
        notify_before,
        notify_after,
        tags,
        priority,
        max_runs,
    }) {
        Ok(schedule) => format_schedule_created(&schedule),
        Err(e) => format!("Error creating schedule: {e}"),
    }
}

fn execute_list_schedules(input: &Value, sched: &Arc<Scheduler>, user_id: i64) -> String {
    let status_filter = input
        .get("status")
        .and_then(Value::as_str)
        .and_then(|s| match s {
            "active" => Some(ScheduleStatus::Active),
            "paused" => Some(ScheduleStatus::Paused),
            "completed" => Some(ScheduleStatus::Completed),
            _ => None,
        });

    let tag_filter = input.get("tag").and_then(Value::as_str);

    match sched.list(user_id, status_filter, tag_filter) {
        Ok(list) => {
            if list.is_empty() {
                "No schedules found.".to_string()
            } else {
                format_schedule_list(&list)
            }
        }
        Err(e) => format!("Error listing schedules: {e}"),
    }
}

fn execute_update_schedule(input: &Value, sched: &Arc<Scheduler>, user_id: i64) -> String {
    // Find the schedule by ID or name
    let Some(schedule) = find_schedule(input, sched, user_id) else {
        return "Error: Schedule not found. Provide 'id' or 'name_search'.".to_string();
    };

    let mut update_params = UpdateScheduleParams::default();

    if let Some(name) = input.get("name").and_then(Value::as_str) {
        update_params.name = Some(name.to_string());
    }
    if let Some(prompt) = input.get("prompt").and_then(Value::as_str) {
        update_params.prompt = Some(prompt.to_string());
    }
    if let Some(cron) = input.get("cron_expression").and_then(Value::as_str) {
        update_params.cron_expression = Some(cron.to_string());
    }
    if let Some(status) = input.get("status").and_then(Value::as_str) {
        update_params.status = match status {
            "active" => Some(ScheduleStatus::Active),
            "paused" => Some(ScheduleStatus::Paused),
            _ => None,
        };
    }
    if let Some(notify_before) = input.get("notify_before").and_then(Value::as_bool) {
        update_params.notify_before = Some(notify_before);
    }
    if let Some(notify_after) = input.get("notify_after").and_then(Value::as_bool) {
        update_params.notify_after = Some(notify_after);
    }
    if let Some(tags) = input.get("tags").and_then(Value::as_array) {
        update_params.tags = Some(
            tags.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        );
    }
    if let Some(priority) = input.get("priority").and_then(Value::as_i64) {
        #[allow(clippy::cast_possible_truncation)]
        {
            update_params.priority = Some(priority as i32);
        }
    }
    if let Some(max_runs) = input.get("max_runs").and_then(Value::as_i64) {
        update_params.max_runs = Some(max_runs);
    }

    match sched.update(&schedule.id, update_params) {
        Ok(Some(updated)) => format_schedule_updated(&updated),
        Ok(None) => "Error: Schedule not found after update.".to_string(),
        Err(e) => format!("Error updating schedule: {e}"),
    }
}

fn execute_delete_schedule(input: &Value, sched: &Arc<Scheduler>, user_id: i64) -> String {
    let Some(schedule) = find_schedule(input, sched, user_id) else {
        return "Error: Schedule not found. Provide 'id' or 'name_search'.".to_string();
    };

    match sched.delete(&schedule.id) {
        Ok(true) => format!("Deleted schedule '{}'.", schedule.name),
        Ok(false) => "Error: Schedule not found.".to_string(),
        Err(e) => format!("Error deleting schedule: {e}"),
    }
}

fn execute_run_schedule_now(input: &Value, sched: &Arc<Scheduler>, user_id: i64) -> String {
    let Some(schedule) = find_schedule(input, sched, user_id) else {
        return "Error: Schedule not found. Provide 'id' or 'name_search'.".to_string();
    };

    // Return a special marker that the bot can detect to trigger immediate execution
    format!("SCHEDULE_RUN_NOW:{}:{}", schedule.id, schedule.name)
}

fn execute_get_schedule_history(input: &Value, sched: &Arc<Scheduler>, user_id: i64) -> String {
    let schedule_id = input
        .get("id")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            input
                .get("name_search")
                .and_then(Value::as_str)
                .and_then(|name| sched.find_by_name(user_id, name).ok().flatten())
                .map(|s| s.id)
        });

    #[allow(clippy::cast_possible_truncation)]
    let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;

    match sched.get_history(schedule_id.as_deref(), user_id, limit) {
        Ok(runs) => {
            if runs.is_empty() {
                "No execution history found.".to_string()
            } else {
                format_run_history(&runs)
            }
        }
        Err(e) => format!("Error getting history: {e}"),
    }
}

/// Find a schedule by ID or name search.
fn find_schedule(input: &Value, sched: &Arc<Scheduler>, user_id: i64) -> Option<Schedule> {
    if let Some(id) = input.get("id").and_then(Value::as_str) {
        return sched.get(id).ok().flatten();
    }

    if let Some(name) = input.get("name_search").and_then(Value::as_str) {
        return sched.find_by_name(user_id, name).ok().flatten();
    }

    None
}

fn format_schedule_created(schedule: &Schedule) -> String {
    let timing = crate::scheduler::describe_cron(&schedule.cron_expression);
    let next = schedule.next_run.map_or_else(
        || "not scheduled".to_string(),
        crate::scheduler::format_relative_time,
    );

    let notifications = match (schedule.notify_before, schedule.notify_after) {
        (true, true) => "before and after",
        (true, false) => "before only",
        (false, true) => "after only",
        (false, false) => "silent",
    };

    let name = &schedule.name;
    let id = &schedule.id;
    format!(
        "Created schedule '{name}'\n\
         Timing: {timing}\n\
         Next run: {next}\n\
         Notifications: {notifications}\n\
         ID: {id}"
    )
}

fn format_schedule_updated(schedule: &Schedule) -> String {
    let timing = crate::scheduler::describe_cron(&schedule.cron_expression);
    let next = schedule.next_run.map_or_else(
        || "not scheduled".to_string(),
        crate::scheduler::format_relative_time,
    );
    let status = schedule.status.as_str();
    let name = &schedule.name;

    format!(
        "Updated schedule '{name}'\n\
         Status: {status}\n\
         Timing: {timing}\n\
         Next run: {next}"
    )
}

fn format_schedule_list(list: &[Schedule]) -> String {
    use std::fmt::Write;

    let mut output = format!("Found {} schedule(s):\n\n", list.len());

    for schedule in list {
        let timing = crate::scheduler::describe_cron(&schedule.cron_expression);
        let status = schedule.status.as_str();
        let last_run = schedule.last_run.map_or_else(
            || "never".to_string(),
            crate::scheduler::format_relative_time,
        );
        let next_run = schedule
            .next_run
            .map_or_else(|| "-".to_string(), crate::scheduler::format_relative_time);
        let name = &schedule.name;
        let run_count = schedule.run_count;
        let id = &schedule.id;

        let _ = writeln!(
            output,
            "- {name} [{status}]\n  Timing: {timing}\n  Last: {last_run} | Next: {next_run}\n  Runs: {run_count} | ID: {id}\n"
        );
    }

    output
}

fn format_run_history(runs: &[crate::scheduler::ScheduleRun]) -> String {
    use std::fmt::Write;

    let mut output = format!("Execution history ({} entries):\n\n", runs.len());

    for run in runs {
        let started = crate::scheduler::format_relative_time(run.started_at);
        let duration = run.completed_at.map_or_else(
            || "running".to_string(),
            |end| {
                let secs = end.signed_duration_since(run.started_at).num_seconds();
                if secs < 60 {
                    format!("{secs}s")
                } else {
                    let mins = secs / 60;
                    let rem = secs % 60;
                    format!("{mins}m {rem}s")
                }
            },
        );
        let status = run.status.as_str();
        let result = run
            .result_summary
            .as_ref()
            .map_or_else(|| "-".to_string(), |s| truncate_string(s, 100));
        let error = run.error_message.as_ref().map_or_else(String::new, |s| {
            let truncated = truncate_string(s, 100);
            format!("\n  Error: {truncated}")
        });

        let _ = writeln!(
            output,
            "- {started} [{status}] ({duration})\n  Result: {result}{error}"
        );
    }

    output
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
