use serde::{Deserialize, Serialize};

use crate::services::agent::AgentService;
use crate::services::automation::AutomationService;

/// A single chat message exchanged between the user and assistant.
///
/// Used by the frontend to render conversation history.
/// Not referenced in Rust commands yet, but exposed for Tauri IPC serialization.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BotStatus {
    pub listening: bool,
    pub agent_ready: bool,
    pub automation_available: bool,
}

/// Send a user message and receive an assistant response.
///
/// Currently delegates to the placeholder `AgentService`. Once ZeptoClaw
/// is integrated, this will run through the full agent loop with tools.
#[tauri::command]
pub async fn send_message(message: String) -> Result<String, String> {
    let agent = AgentService::new();
    agent.chat(&message).await
}

/// Return the current status of the bot subsystems.
#[tauri::command]
pub async fn get_status() -> Result<BotStatus, String> {
    Ok(BotStatus {
        listening: false,
        agent_ready: true,
        automation_available: true,
    })
}

/// Execute a desktop automation action (mouse, keyboard, screen queries).
///
/// Delegates to `AutomationService` which wraps autopilot-rs.
#[tauri::command]
pub async fn execute_automation(
    action: String,
    params: serde_json::Value,
) -> Result<String, String> {
    let automation = AutomationService::new();
    // autopilot calls are synchronous -- run on the blocking pool
    // to avoid holding the async executor.
    tokio::task::spawn_blocking(move || automation.execute(&action, &params))
        .await
        .map_err(|e| format!("Automation task panicked: {e}"))?
}
