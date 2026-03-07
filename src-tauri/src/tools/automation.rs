//! Desktop automation tools wrapping autopilot-rs.
//!
//! Each struct implements the ZeptoClaw `Tool` trait so the agent can
//! control the mouse, keyboard, and query screen state.

use std::process::Command;

use async_trait::async_trait;
use autopilot::geometry::Point;
use autopilot::key::{self, Character, Code, Flag, KeyCode};
use autopilot::mouse::{self, Button};
use autopilot::screen;
use serde_json::{json, Value};
use zeptoclaw::tools::ToolOutput;
use zeptoclaw::{Result as ZeptoResult, Tool, ToolCategory, ToolContext};

// ---------------------------------------------------------------------------
// MoveMouseTool
// ---------------------------------------------------------------------------

pub struct MoveMouseTool;

#[async_trait]
impl Tool for MoveMouseTool {
    fn name(&self) -> &str {
        "move_mouse"
    }

    fn description(&self) -> &str {
        "Move the mouse cursor to absolute screen coordinates"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "x": { "type": "number", "description": "X coordinate (pixels from left)" },
                "y": { "type": "number", "description": "Y coordinate (pixels from top)" }
            },
            "required": ["x", "y"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let x = match args.get("x").and_then(Value::as_f64) {
            Some(v) => v,
            None => return Ok(ToolOutput::error("Missing or invalid 'x' parameter")),
        };
        let y = match args.get("y").and_then(Value::as_f64) {
            Some(v) => v,
            None => return Ok(ToolOutput::error("Missing or invalid 'y' parameter")),
        };

        match mouse::move_to(Point::new(x, y)) {
            Ok(()) => Ok(ToolOutput::llm_only(format!("Mouse moved to ({x}, {y})"))),
            Err(e) => Ok(ToolOutput::error(format!(
                "Failed to move mouse to ({x}, {y}): {e}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// ClickTool
// ---------------------------------------------------------------------------

pub struct ClickTool;

#[async_trait]
impl Tool for ClickTool {
    fn name(&self) -> &str {
        "click"
    }

    fn description(&self) -> &str {
        "Click the mouse at the current cursor position. Optionally specify button (left/right/middle) and click count."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "button": {
                    "type": "string",
                    "enum": ["left", "right", "middle"],
                    "description": "Mouse button to click (default: left)"
                },
                "count": {
                    "type": "integer",
                    "description": "Number of clicks (default: 1, use 2 for double-click)"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let button_str = args.get("button").and_then(Value::as_str).unwrap_or("left");

        let button = match button_str {
            "left" => Button::Left,
            "right" => Button::Right,
            "middle" => Button::Middle,
            other => {
                return Ok(ToolOutput::error(format!(
                    "Unknown button '{other}'. Use left, right, or middle."
                )));
            }
        };

        let count = args
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1);

        for _ in 0..count {
            mouse::click(button, None);
        }

        let label = if count == 1 {
            format!("{button_str} click")
        } else {
            format!("{count}x {button_str} click")
        };
        Ok(ToolOutput::llm_only(format!("Performed {label}")))
    }
}

// ---------------------------------------------------------------------------
// TypeTextTool
// ---------------------------------------------------------------------------

pub struct TypeTextTool;

#[async_trait]
impl Tool for TypeTextTool {
    fn name(&self) -> &str {
        "type_text"
    }

    fn description(&self) -> &str {
        "Type text using simulated keystrokes. Types the given string as if the user typed it on the keyboard."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The text to type"
                }
            },
            "required": ["text"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let text = match args.get("text").and_then(Value::as_str) {
            Some(t) => t,
            None => return Ok(ToolOutput::error("Missing or invalid 'text' parameter")),
        };

        key::type_string(text, &[], 50.0, 0.0);

        let preview = if text.len() > 60 {
            format!("{}...", &text[..57])
        } else {
            text.to_string()
        };
        Ok(ToolOutput::llm_only(format!(
            "Typed {len} characters: \"{preview}\"",
            len = text.len()
        )))
    }
}

// ---------------------------------------------------------------------------
// ScreenInfoTool
// ---------------------------------------------------------------------------

pub struct ScreenInfoTool;

#[async_trait]
impl Tool for ScreenInfoTool {
    fn name(&self) -> &str {
        "screen_info"
    }

    fn description(&self) -> &str {
        "Get information about the screen and current mouse position. Returns screen dimensions and cursor coordinates."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let size = screen::size();
        let pos = mouse::location();
        let info = json!({
            "screen": {
                "width": size.width,
                "height": size.height
            },
            "mouse": {
                "x": pos.x,
                "y": pos.y
            }
        });
        Ok(ToolOutput::llm_only(info.to_string()))
    }
}

// ---------------------------------------------------------------------------
// KeyPressTool
// ---------------------------------------------------------------------------

pub struct KeyPressTool;

#[async_trait]
impl Tool for KeyPressTool {
    fn name(&self) -> &str {
        "key_press"
    }

    fn description(&self) -> &str {
        "Press a keyboard key or key combination. Supports modifier keys (cmd, ctrl, alt, shift) with regular keys."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Key to press (e.g. 'return', 'tab', 'escape', 'a', '1', 'f5')"
                },
                "modifiers": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Modifier keys (e.g. ['cmd'], ['cmd', 'shift'])"
                }
            },
            "required": ["key"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let key_str = match args.get("key").and_then(Value::as_str) {
            Some(k) => k,
            None => return Ok(ToolOutput::error("Missing or invalid 'key' parameter")),
        };

        let flags: Vec<Flag> = args
            .get("modifiers")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().and_then(parse_flag))
                    .collect()
            })
            .unwrap_or_default();

        // Try as a named KeyCode first, then fall back to a single character.
        if let Some(code) = parse_key_code(key_str) {
            key::tap(&Code(code), &flags, 0, 0);
        } else {
            let ch = if key_str.len() == 1 {
                key_str.chars().next().unwrap()
            } else {
                return Ok(ToolOutput::error(format!(
                    "Unknown key '{key_str}'. Use a single character or a named key \
                     (return, tab, escape, space, backspace, delete, up, down, left, right, \
                     home, end, pageup, pagedown, f1-f24)."
                )));
            };
            key::tap(&Character(ch), &flags, 0, 0);
        }

        let mod_label = if flags.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> = flags.iter().map(flag_name).collect();
            format!("{} + ", names.join(" + "))
        };
        Ok(ToolOutput::llm_only(format!(
            "Pressed {mod_label}{key_str}"
        )))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a modifier string to an autopilot `Flag`.
fn parse_flag(s: &str) -> Option<Flag> {
    match s.to_ascii_lowercase().as_str() {
        "shift" => Some(Flag::Shift),
        "control" | "ctrl" => Some(Flag::Control),
        "alt" | "option" => Some(Flag::Alt),
        "meta" | "cmd" | "command" | "win" | "super" => Some(Flag::Meta),
        _ => None,
    }
}

/// Human-readable name for a flag (used in output messages).
fn flag_name(f: &Flag) -> &'static str {
    match f {
        Flag::Shift => "Shift",
        Flag::Control => "Ctrl",
        Flag::Alt => "Alt",
        Flag::Meta => "Cmd",
        Flag::Help => "Help",
    }
}

/// Map a key name string to an autopilot `KeyCode`.
fn parse_key_code(s: &str) -> Option<KeyCode> {
    match s.to_ascii_lowercase().as_str() {
        "return" | "enter" => Some(KeyCode::Return),
        "tab" => Some(KeyCode::Tab),
        "escape" | "esc" => Some(KeyCode::Escape),
        "space" => Some(KeyCode::Space),
        "backspace" => Some(KeyCode::Backspace),
        "delete" | "del" => Some(KeyCode::Delete),
        "up" | "uparrow" => Some(KeyCode::UpArrow),
        "down" | "downarrow" => Some(KeyCode::DownArrow),
        "left" | "leftarrow" => Some(KeyCode::LeftArrow),
        "right" | "rightarrow" => Some(KeyCode::RightArrow),
        "home" => Some(KeyCode::Home),
        "end" => Some(KeyCode::End),
        "pageup" => Some(KeyCode::PageUp),
        "pagedown" => Some(KeyCode::PageDown),
        "capslock" => Some(KeyCode::CapsLock),
        "printscreen" => Some(KeyCode::PrintScreen),
        "scrolllock" => Some(KeyCode::ScrollLock),
        "pause" => Some(KeyCode::Pause),
        "f1" => Some(KeyCode::F1),
        "f2" => Some(KeyCode::F2),
        "f3" => Some(KeyCode::F3),
        "f4" => Some(KeyCode::F4),
        "f5" => Some(KeyCode::F5),
        "f6" => Some(KeyCode::F6),
        "f7" => Some(KeyCode::F7),
        "f8" => Some(KeyCode::F8),
        "f9" => Some(KeyCode::F9),
        "f10" => Some(KeyCode::F10),
        "f11" => Some(KeyCode::F11),
        "f12" => Some(KeyCode::F12),
        "f13" => Some(KeyCode::F13),
        "f14" => Some(KeyCode::F14),
        "f15" => Some(KeyCode::F15),
        "f16" => Some(KeyCode::F16),
        "f17" => Some(KeyCode::F17),
        "f18" => Some(KeyCode::F18),
        "f19" => Some(KeyCode::F19),
        "f20" => Some(KeyCode::F20),
        "f21" => Some(KeyCode::F21),
        "f22" => Some(KeyCode::F22),
        "f23" => Some(KeyCode::F23),
        "f24" => Some(KeyCode::F24),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// OpenAppTool
// ---------------------------------------------------------------------------

pub struct OpenAppTool;

#[async_trait]
impl Tool for OpenAppTool {
    fn name(&self) -> &str {
        "open_app"
    }

    fn description(&self) -> &str {
        "Open a macOS application by name. This is the most reliable way to launch apps."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Application name (e.g. 'Google Chrome', 'Safari', 'Terminal', 'Notes')"
                }
            },
            "required": ["name"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let app_name = match args.get("name").and_then(Value::as_str) {
            Some(n) => n,
            None => return Ok(ToolOutput::error("Missing or invalid 'name' parameter")),
        };

        // Use `open -a` which is the standard macOS way to launch apps
        let output = Command::new("open")
            .arg("-a")
            .arg(app_name)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                Ok(ToolOutput::llm_only(format!("Opened {app_name}")))
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                Ok(ToolOutput::error(format!(
                    "Failed to open '{app_name}': {stderr}"
                )))
            }
            Err(e) => Ok(ToolOutput::error(format!(
                "Failed to run open command: {e}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// RunAppleScriptTool
// ---------------------------------------------------------------------------

pub struct RunAppleScriptTool;

#[async_trait]
impl Tool for RunAppleScriptTool {
    fn name(&self) -> &str {
        "run_applescript"
    }

    fn description(&self) -> &str {
        "Run an AppleScript command. Useful for interacting with macOS apps, getting window info, clicking UI elements via accessibility, etc."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "script": {
                    "type": "string",
                    "description": "The AppleScript code to execute"
                }
            },
            "required": ["script"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let script = match args.get("script").and_then(Value::as_str) {
            Some(s) => s,
            None => return Ok(ToolOutput::error("Missing or invalid 'script' parameter")),
        };

        // Support multi-line scripts by passing each line as a separate -e flag
        let mut cmd = Command::new("osascript");
        for line in script.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                cmd.arg("-e").arg(trimmed);
            }
        }
        let output = cmd.output();

        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                Ok(ToolOutput::llm_only(if stdout.is_empty() {
                    "AppleScript executed successfully".to_string()
                } else {
                    stdout
                }))
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                Ok(ToolOutput::error(format!("AppleScript error: {stderr}")))
            }
            Err(e) => Ok(ToolOutput::error(format!("Failed to run osascript: {e}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// ActivateAppTool
// ---------------------------------------------------------------------------

pub struct ActivateAppTool;

#[async_trait]
impl Tool for ActivateAppTool {
    fn name(&self) -> &str {
        "activate_app"
    }

    fn description(&self) -> &str {
        "Bring a running application to the foreground (make it the active window on top). Use this before interacting with an app that's already open."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Application name (e.g. 'Google Chrome', 'WhatsApp', 'Notes')"
                }
            },
            "required": ["name"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let app_name = match args.get("name").and_then(Value::as_str) {
            Some(n) => n,
            None => return Ok(ToolOutput::error("Missing or invalid 'name' parameter")),
        };

        let script = format!(
            r#"tell application "{}" to activate"#,
            app_name.replace('"', "\\\"")
        );

        let output = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                Ok(ToolOutput::llm_only(format!("{app_name} is now in the foreground")))
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                Ok(ToolOutput::error(format!(
                    "Failed to activate '{app_name}': {stderr}"
                )))
            }
            Err(e) => Ok(ToolOutput::error(format!("Failed to run osascript: {e}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// OpenUrlTool
// ---------------------------------------------------------------------------

pub struct OpenUrlTool;

#[async_trait]
impl Tool for OpenUrlTool {
    fn name(&self) -> &str {
        "open_url"
    }

    fn description(&self) -> &str {
        "Open a URL in the default browser or a specific browser. The browser will be brought to the foreground."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to open (e.g. 'https://facebook.com')"
                },
                "browser": {
                    "type": "string",
                    "description": "Optional browser name (e.g. 'Google Chrome', 'Safari'). If not specified, uses the default browser."
                }
            },
            "required": ["url"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let url = match args.get("url").and_then(Value::as_str) {
            Some(u) => u,
            None => return Ok(ToolOutput::error("Missing or invalid 'url' parameter")),
        };

        let mut cmd = Command::new("open");
        cmd.arg(url);

        if let Some(browser) = args.get("browser").and_then(Value::as_str) {
            cmd.arg("-a").arg(browser);
        }

        let output = cmd.output();

        match output {
            Ok(o) if o.status.success() => {
                Ok(ToolOutput::llm_only(format!("Opened {url} in browser")))
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                Ok(ToolOutput::error(format!("Failed to open URL: {stderr}")))
            }
            Err(e) => Ok(ToolOutput::error(format!("Failed to run open command: {e}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// WaitTool
// ---------------------------------------------------------------------------

pub struct WaitTool;

#[async_trait]
impl Tool for WaitTool {
    fn name(&self) -> &str {
        "wait"
    }

    fn description(&self) -> &str {
        "Wait/pause for a specified number of milliseconds. Use between actions that need time to take effect (e.g. after opening Raycast, wait 500ms before typing)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ms": {
                    "type": "integer",
                    "description": "Milliseconds to wait (e.g. 500 for half a second)"
                }
            },
            "required": ["ms"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        let ms = args
            .get("ms")
            .and_then(Value::as_u64)
            .unwrap_or(500)
            .min(10_000); // cap at 10 seconds

        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        Ok(ToolOutput::llm_only(format!("Waited {ms}ms")))
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/// Returns all automation tools as boxed trait objects.
pub fn all_automation_tools() -> Vec<Box<dyn Tool>> {
    let (_browser_state, browser_tools) = super::browser::all_browser_tools();

    let mut tools: Vec<Box<dyn Tool>> = vec![
        // Low-level mouse/keyboard (Tier 4 — fallback)
        Box::new(MoveMouseTool),
        Box::new(ClickTool),
        Box::new(TypeTextTool),
        Box::new(ScreenInfoTool),
        Box::new(KeyPressTool),
        Box::new(WaitTool),
        // App management (Tier 1 — AppleScript)
        Box::new(OpenAppTool),
        Box::new(ActivateAppTool),
        Box::new(OpenUrlTool),
        Box::new(RunAppleScriptTool),
        // Vision (Tier 3 — screenshot + GPT-4o-mini)
        Box::new(super::screenshot::ScreenshotTool),
        // Accessibility API (Tier 2 — programmatic UI interaction)
        Box::new(super::ax_tools::GetUIElementsTool),
        Box::new(super::ax_tools::FindElementTool),
        Box::new(super::ax_tools::ClickElementTool),
        Box::new(super::ax_tools::SetValueTool),
        Box::new(super::ax_tools::ReadValueTool),
        Box::new(super::ax_tools::ElementAtPositionTool),
    ];

    // Browser CDP (Tier 1.5 — direct DOM interaction for web apps)
    tools.extend(browser_tools);

    tools
}
