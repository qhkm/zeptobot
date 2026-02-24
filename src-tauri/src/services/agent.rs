//! Agent service powered by ZeptoClaw's `ZeptoAgent` facade.
//!
//! Wraps `ZeptoAgent` in Tauri managed state so conversation history
//! persists across `send_message` invocations.

use zeptoclaw::agent::ZeptoAgent;
use zeptoclaw::{ClaudeProvider, OpenAIProvider};

use crate::tools::all_automation_tools;

/// System prompt that tells the LLM what it can do.
const SYSTEM_PROMPT: &str = "\
You are ZeptoBot, a helpful AI assistant that can control the user's Mac computer. \
You have tools to move the mouse, click, type text, press keyboard keys, and get \
screen information. When the user asks you to perform an action on their computer, \
use the appropriate tools. Be concise in your responses. Describe what you did after \
performing actions.\n\n\
User environment:\n\
- macOS with Raycast (Cmd+Space) instead of Spotlight. To launch apps, press Cmd+Space \
to open Raycast, then type the app name and press Return.\n\
- When asked to open an app, use key_press with cmd+space, then type_text the app name, \
then key_press Return.";

/// Build a `ZeptoAgent` from environment variables.
///
/// Checks `ANTHROPIC_API_KEY` first, then `OPENAI_API_KEY`.
pub fn build_agent() -> Result<ZeptoAgent, String> {
    let mut builder = ZeptoAgent::builder()
        .tools(all_automation_tools())
        .system_prompt(SYSTEM_PROMPT);

    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        builder = builder.provider(ClaudeProvider::new(&key));
    } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        builder = builder.provider(OpenAIProvider::new(&key));
    } else {
        return Err("No API key found. Set ANTHROPIC_API_KEY or OPENAI_API_KEY".into());
    }

    builder.build().map_err(|e| format!("{e}"))
}

/// Returns `true` when an API key is available in the environment.
pub fn has_api_key() -> bool {
    std::env::var("ANTHROPIC_API_KEY").is_ok() || std::env::var("OPENAI_API_KEY").is_ok()
}
