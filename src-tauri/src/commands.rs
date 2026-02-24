use serde::{Deserialize, Serialize};
use tauri::State;
use zeptoclaw::agent::ZeptoAgent;

use crate::services::agent::has_api_key;
use crate::services::automation::AutomationService;

/// Shared agent state managed by Tauri.
pub struct AgentState(pub ZeptoAgent);

#[derive(Debug, Serialize, Deserialize)]
pub struct BotStatus {
    pub listening: bool,
    pub agent_ready: bool,
    pub automation_available: bool,
}

/// Send a user message through the ZeptoAgent and return the response.
///
/// Conversation history is maintained across calls via the managed state.
#[tauri::command]
pub async fn send_message(message: String, agent: State<'_, AgentState>) -> Result<String, String> {
    agent.0.chat(&message).await.map_err(|e| format!("{e}"))
}

/// Clear the conversation history.
#[tauri::command]
pub async fn clear_history(agent: State<'_, AgentState>) -> Result<(), String> {
    agent.0.clear_history().await;
    Ok(())
}

/// Return the current status of the bot subsystems.
#[tauri::command]
pub async fn get_status() -> Result<BotStatus, String> {
    Ok(BotStatus {
        listening: false,
        agent_ready: has_api_key(),
        automation_available: true,
    })
}

/// Execute a desktop automation action (mouse, keyboard, screen queries).
#[tauri::command]
pub async fn execute_automation(
    action: String,
    params: serde_json::Value,
) -> Result<String, String> {
    let automation = AutomationService::new();
    tokio::task::spawn_blocking(move || automation.execute(&action, &params))
        .await
        .map_err(|e| format!("Automation task panicked: {e}"))?
}
