//! Agent service that drives the LLM tool-execution loop.
//!
//! Uses ZeptoClaw's real LLM providers and tool execution to process user
//! messages. The agent loop sends messages to the LLM, executes any tool
//! calls it requests, feeds results back, and repeats until the LLM returns
//! a plain text response (or a safety cap of 10 iterations is reached).

use std::sync::Arc;

use serde_json::Value;
use zeptoclaw::providers::{ChatOptions, LLMProvider, ToolDefinition};
use zeptoclaw::session::{Message, ToolCall};
use zeptoclaw::tools::{Tool, ToolContext};
use zeptoclaw::{ClaudeProvider, OpenAIProvider};

/// Maximum number of LLM round-trips before we stop the loop.
const MAX_ITERATIONS: usize = 10;

/// System prompt that tells the LLM what it can do.
const SYSTEM_PROMPT: &str = "\
You are ZeptoBot, a helpful AI assistant that can control the user's Mac computer. \
You have tools to move the mouse, click, type text, press keyboard keys, and get \
screen information. When the user asks you to perform an action on their computer, \
use the appropriate tools. Be concise in your responses. Describe what you did after \
performing actions.";

/// The core agent service. Holds a provider and a set of tools.
pub struct AgentService {
    provider: Arc<dyn LLMProvider>,
    tools: Vec<Box<dyn Tool>>,
}

impl AgentService {
    /// Create a new `AgentService` with the given tools.
    ///
    /// Provider is resolved from environment variables:
    /// 1. `ANTHROPIC_API_KEY` -- uses Claude
    /// 2. `OPENAI_API_KEY`   -- uses OpenAI-compatible endpoint
    ///
    /// Returns `Err` if neither key is set.
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Result<Self, String> {
        let provider: Arc<dyn LLMProvider> = if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            Arc::new(ClaudeProvider::new(&key))
        } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            Arc::new(OpenAIProvider::new(&key))
        } else {
            return Err("No API key found. Set ANTHROPIC_API_KEY or OPENAI_API_KEY".into());
        };

        Ok(Self { provider, tools })
    }

    /// Returns `true` when an API key is available in the environment.
    pub fn has_api_key() -> bool {
        std::env::var("ANTHROPIC_API_KEY").is_ok() || std::env::var("OPENAI_API_KEY").is_ok()
    }

    /// Send a user message through the agent loop and return the final text
    /// response.
    ///
    /// The loop:
    /// 1. Build system prompt + user message
    /// 2. Call LLM with tool definitions
    /// 3. If the response contains tool calls, execute them and loop
    /// 4. Return the final text response once no more tool calls are made
    pub async fn chat(&self, user_message: &str) -> Result<String, String> {
        let mut messages = vec![Message::system(SYSTEM_PROMPT), Message::user(user_message)];

        let tool_defs: Vec<ToolDefinition> = self
            .tools
            .iter()
            .map(|t| ToolDefinition::new(t.name(), t.description(), t.parameters()))
            .collect();

        let ctx = ToolContext::default();

        for _ in 0..MAX_ITERATIONS {
            let response = self
                .provider
                .chat(
                    messages.clone(),
                    tool_defs.clone(),
                    None,
                    ChatOptions::new(),
                )
                .await
                .map_err(|e| format!("LLM error: {e}"))?;

            // No tool calls -- we are done.
            if !response.has_tool_calls() {
                return Ok(response.content);
            }

            // Build an assistant message that carries the tool_calls so the
            // provider can correlate subsequent tool-result messages.
            let session_tool_calls: Vec<ToolCall> = response
                .tool_calls
                .iter()
                .map(|tc| ToolCall::new(&tc.id, &tc.name, &tc.arguments))
                .collect();

            messages.push(Message::assistant_with_tools(
                &response.content,
                session_tool_calls,
            ));

            // Execute each tool call and append a tool-result message.
            for tc in &response.tool_calls {
                let args: Value = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);

                let result = if let Some(tool) = self.tools.iter().find(|t| t.name() == tc.name) {
                    match tool.execute(args, &ctx).await {
                        Ok(output) => output.for_llm,
                        Err(e) => format!("Tool error: {e}"),
                    }
                } else {
                    format!("Unknown tool: {}", tc.name)
                };

                messages.push(Message::tool_result(&tc.id, &result));
            }
        }

        // Safety cap reached -- return a generic completion message.
        Ok("I've completed the requested actions.".to_string())
    }
}
