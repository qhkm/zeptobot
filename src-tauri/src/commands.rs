use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use zeptoclaw::agent::ZeptoAgent;

use crate::services::agent::has_api_key;
use crate::services::automation::AutomationService;

/// Shared agent state managed by Tauri. `None` when no API key is configured.
pub struct AgentState(pub Option<ZeptoAgent>);

/// Holds the cancellation token for the current generation.
pub struct CancelState(pub Arc<Mutex<Option<CancellationToken>>>);

#[derive(Debug, Serialize, Deserialize)]
pub struct BotStatus {
    pub listening: bool,
    pub agent_ready: bool,
    pub automation_available: bool,
}

/// Payload for agent step events sent to the frontend.
#[derive(Clone, Serialize)]
pub struct AgentStep {
    pub tool: String,
    pub message: String,
}

/// Send a user message through the ZeptoAgent and return the response.
///
/// Conversation history is maintained across calls via the managed state.
/// Emits `agent-step` events to the frontend for live progress updates.
/// The request can be cancelled via `stop_generation`.
#[tauri::command]
pub async fn send_message(
    message: String,
    app: AppHandle,
    agent: State<'_, AgentState>,
    cancel: State<'_, CancelState>,
) -> Result<String, String> {
    let agent = agent
        .0
        .as_ref()
        .ok_or_else(|| "No API key configured. Set ANTHROPIC_API_KEY or OPENAI_API_KEY and restart.".to_string())?;

    // Create a new cancellation token for this request
    let token = CancellationToken::new();
    {
        let mut current = cancel.0.lock().await;
        *current = Some(token.clone());
    }

    let result = tokio::select! {
        res = agent.chat_with_callback(&message, |tool, msg| {
            let _ = app.emit("agent-step", AgentStep {
                tool: tool.to_string(),
                message: msg.to_string(),
            });
        }) => {
            res.map_err(|e| format!("{e}"))
        }
        _ = token.cancelled() => {
            // Clean up dangling tool_calls from history so the next
            // message doesn't trigger OpenAI's "tool_call_ids did not
            // have response messages" error.
            agent.repair_history().await;
            Err("Generation stopped by user".into())
        }
    };

    // Clear the token
    {
        let mut current = cancel.0.lock().await;
        *current = None;
    }

    result
}

/// Stop the current generation.
#[tauri::command]
pub async fn stop_generation(cancel: State<'_, CancelState>) -> Result<(), String> {
    let current = cancel.0.lock().await;
    if let Some(token) = current.as_ref() {
        token.cancel();
    }
    Ok(())
}

/// Clear the conversation history.
#[tauri::command]
pub async fn clear_history(agent: State<'_, AgentState>) -> Result<(), String> {
    match agent.0.as_ref() {
        Some(a) => {
            a.clear_history().await;
            Ok(())
        }
        None => Err("Agent not available".into()),
    }
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
