//! Screenshot tool with GPT-4o vision analysis.
//!
//! Captures the screen using autopilot, sends to OpenAI vision API,
//! and returns a text description of what's on screen.

use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};
use tracing::info;
use zeptoclaw::tools::ToolOutput;
use zeptoclaw::{Result as ZeptoResult, Tool, ToolCategory, ToolContext};

pub struct ScreenshotTool;

#[async_trait]
impl Tool for ScreenshotTool {
    fn name(&self) -> &str {
        "take_screenshot"
    }

    fn description(&self) -> &str {
        "Take a screenshot of the screen and analyze what's visible. Returns a detailed \
         description of all windows, UI elements, text, buttons, and their positions. \
         Use this to see what's on screen before clicking or interacting with UI elements."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "context": {
                    "type": "string",
                    "description": "Optional context about what you're looking for (e.g. 'looking for the send button', 'checking if WhatsApp search results appeared')"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let context = args
            .get("context")
            .and_then(Value::as_str)
            .unwrap_or("Describe everything visible on screen");

        info!("[Screenshot] Capturing screen...");

        // Capture screen using autopilot
        let bitmap = match autopilot::bitmap::capture_screen() {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolOutput::error(format!(
                    "Failed to capture screen: {e}"
                )));
            }
        };

        // Save to temp file then read back (avoids image crate version conflicts)
        let tmp_path = std::env::temp_dir().join("zeptobot_screenshot.png");
        if let Err(e) = bitmap.image.save(&tmp_path) {
            return Ok(ToolOutput::error(format!(
                "Failed to save screenshot: {e}"
            )));
        }

        let png_bytes = match std::fs::read(&tmp_path) {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolOutput::error(format!(
                    "Failed to read screenshot: {e}"
                )));
            }
        };
        let _ = std::fs::remove_file(&tmp_path);

        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
        info!(
            "[Screenshot] Captured, base64 size: {} bytes, sending to vision API...",
            b64.len()
        );

        // Get API key
        let api_key = match std::env::var("OPENAI_API_KEY") {
            Ok(k) => k,
            Err(_) => {
                return Ok(ToolOutput::error(
                    "OPENAI_API_KEY not set — needed for vision analysis",
                ));
            }
        };

        // Call OpenAI vision API
        let prompt = format!(
            "You are a screen reader for a desktop automation assistant. \
             Describe what you see on this macOS screenshot in detail. Include:\n\
             - All visible application windows and which app is in the foreground\n\
             - Any text, buttons, input fields, and their approximate screen positions \
               (top-left, center, bottom-right, etc.)\n\
             - The state of the UI (e.g. search field is active, dialog is open)\n\
             - Any notifications or popups\n\n\
             User context: {context}\n\n\
             Be concise but thorough. Focus on actionable information."
        );

        let body = json!({
            "model": "gpt-4o",
            "max_tokens": 1000,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": prompt
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/png;base64,{b64}"),
                            "detail": "high"
                        }
                    }
                ]
            }]
        });

        let client = reqwest::Client::new();
        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&body)
            .send()
            .await;

        match response {
            Ok(resp) => {
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Ok(ToolOutput::error(format!(
                        "Vision API error ({status}): {text}"
                    )));
                }

                let json: Value = match resp.json().await {
                    Ok(j) => j,
                    Err(e) => {
                        return Ok(ToolOutput::error(format!(
                            "Failed to parse vision response: {e}"
                        )));
                    }
                };

                let description = json["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("No description returned")
                    .to_string();

                info!("[Screenshot] Vision analysis complete");
                Ok(ToolOutput::llm_only(format!(
                    "SCREEN CONTENT:\n{description}"
                )))
            }
            Err(e) => Ok(ToolOutput::error(format!("Vision API request failed: {e}"))),
        }
    }
}
