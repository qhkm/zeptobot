//! Agent tools wrapping the macOS Accessibility API.
//!
//! These tools let the LLM agent query UI element trees, find buttons/fields
//! by name, click elements, and set text values — all without relying on
//! fragile coordinate-based clicking or keyboard simulation.

use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::info;
use zeptoclaw::tools::ToolOutput;
use zeptoclaw::{Result as ZeptoResult, Tool, ToolCategory, ToolContext};

use super::ax;

// ---------------------------------------------------------------------------
// GetUIElementsTool — list all UI elements for an app
// ---------------------------------------------------------------------------

pub struct GetUIElementsTool;

#[async_trait]
impl Tool for GetUIElementsTool {
    fn name(&self) -> &str {
        "get_ui_elements"
    }

    fn description(&self) -> &str {
        "Get the accessibility tree of a running app. Returns all UI elements (buttons, text \
         fields, menus, etc.) with their roles, titles, values, and positions. Each element has \
         an index number you can use with click_element or set_value. \
         Provide either an app name (e.g. 'WhatsApp') or 'frontmost' for the active app."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "App name (e.g. 'WhatsApp', 'Safari') or 'frontmost' for the active app"
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Max depth to traverse (default: 5, max: 10). Lower = faster but less detail."
                }
            },
            "required": ["app"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        if !ax::is_trusted() {
            ax::is_trusted_with_prompt(true);
            return Ok(ToolOutput::error(
                "Accessibility permission not granted. A system dialog should appear — \
                 please grant permission in System Settings > Privacy & Security > Accessibility, \
                 then try again.",
            ));
        }

        let app_name = match args.get("app").and_then(Value::as_str) {
            Some(n) => n,
            None => return Ok(ToolOutput::error("Missing 'app' parameter")),
        };

        let max_depth = args
            .get("max_depth")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .min(10) as usize;

        let pid = if app_name.eq_ignore_ascii_case("frontmost") {
            match ax::frontmost_app_pid() {
                Some(p) => p,
                None => return Ok(ToolOutput::error("Could not determine frontmost app PID")),
            }
        } else {
            match ax::app_pid(app_name) {
                Some(p) => p,
                None => {
                    return Ok(ToolOutput::error(format!(
                        "App '{app_name}' not found or not running"
                    )));
                }
            }
        };

        info!(
            "[AX] Getting UI tree for {} (pid={}, depth={})",
            app_name, pid, max_depth
        );

        let elements = tokio::task::spawn_blocking(move || ax::get_ui_tree(pid, max_depth))
            .await
            .unwrap_or_default();

        if elements.is_empty() {
            return Ok(ToolOutput::llm_only(format!(
                "No UI elements found for '{app_name}' (pid={pid}). \
                 The app may not expose accessibility data, or the window may be minimized."
            )));
        }

        // Format as compact text for the LLM
        let mut lines = Vec::with_capacity(elements.len() + 1);
        lines.push(format!(
            "UI elements for '{}' (pid={}, {} elements):",
            app_name,
            pid,
            elements.len()
        ));
        for el in &elements {
            lines.push(format!("  {el}"));
        }

        Ok(ToolOutput::llm_only(lines.join("\n")))
    }
}

// ---------------------------------------------------------------------------
// FindElementTool — search for specific UI elements
// ---------------------------------------------------------------------------

pub struct FindElementTool;

#[async_trait]
impl Tool for FindElementTool {
    fn name(&self) -> &str {
        "find_element"
    }

    fn description(&self) -> &str {
        "Search for UI elements in an app that match a query string. Searches element roles, \
         titles, values, and descriptions. Returns matching elements with their index numbers \
         for use with click_element or set_value. Much faster than get_ui_elements for targeted \
         searches."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "App name or 'frontmost'"
                },
                "query": {
                    "type": "string",
                    "description": "Text to search for (e.g. 'Send', 'search', 'message input', 'AXTextField')"
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Max depth to traverse (default: 8)"
                }
            },
            "required": ["app", "query"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        if !ax::is_trusted() {
            return Ok(ToolOutput::error("Accessibility permission not granted."));
        }

        let app_name = match args.get("app").and_then(Value::as_str) {
            Some(n) => n,
            None => return Ok(ToolOutput::error("Missing 'app' parameter")),
        };

        let query = match args.get("query").and_then(Value::as_str) {
            Some(q) => q.to_string(),
            None => return Ok(ToolOutput::error("Missing 'query' parameter")),
        };

        let max_depth = args
            .get("max_depth")
            .and_then(Value::as_u64)
            .unwrap_or(8)
            .min(10) as usize;

        let pid = match resolve_pid(app_name) {
            Some(p) => p,
            None => {
                return Ok(ToolOutput::error(format!(
                    "App '{app_name}' not found or not running"
                )));
            }
        };

        info!(
            "[AX] Finding '{}' in {} (pid={})",
            query, app_name, pid
        );

        let q = query.clone();
        let elements =
            tokio::task::spawn_blocking(move || ax::find_elements(pid, &q, max_depth))
                .await
                .unwrap_or_default();

        if elements.is_empty() {
            return Ok(ToolOutput::llm_only(format!(
                "No elements matching '{query}' found in '{app_name}'."
            )));
        }

        let mut lines = Vec::with_capacity(elements.len() + 1);
        lines.push(format!(
            "Found {} element(s) matching '{}' in '{}':",
            elements.len(),
            query,
            app_name
        ));
        for el in &elements {
            lines.push(format!("  {el}"));
        }

        Ok(ToolOutput::llm_only(lines.join("\n")))
    }
}

// ---------------------------------------------------------------------------
// ClickElementTool — click/press a UI element by index
// ---------------------------------------------------------------------------

pub struct ClickElementTool;

#[async_trait]
impl Tool for ClickElementTool {
    fn name(&self) -> &str {
        "click_element"
    }

    fn description(&self) -> &str {
        "Click/press a UI element by its index number (from get_ui_elements or find_element). \
         This uses the Accessibility API to perform a programmatic press action — much more \
         reliable than coordinate-based clicking. Works even if the element is partially hidden."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "App name or 'frontmost'"
                },
                "index": {
                    "type": "integer",
                    "description": "Element index from get_ui_elements or find_element output"
                }
            },
            "required": ["app", "index"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        if !ax::is_trusted() {
            return Ok(ToolOutput::error("Accessibility permission not granted."));
        }

        let app_name = match args.get("app").and_then(Value::as_str) {
            Some(n) => n,
            None => return Ok(ToolOutput::error("Missing 'app' parameter")),
        };

        let index = match args.get("index").and_then(Value::as_u64) {
            Some(i) => i as usize,
            None => return Ok(ToolOutput::error("Missing 'index' parameter")),
        };

        let pid = match resolve_pid(app_name) {
            Some(p) => p,
            None => {
                return Ok(ToolOutput::error(format!(
                    "App '{app_name}' not found or not running"
                )));
            }
        };

        info!("[AX] Clicking element {} in {} (pid={})", index, app_name, pid);

        match tokio::task::spawn_blocking(move || ax::press_element(pid, index))
            .await
            .unwrap_or(Err("Task panicked".into()))
        {
            Ok(()) => Ok(ToolOutput::llm_only(format!(
                "Clicked element at index {index} in '{app_name}'"
            ))),
            Err(e) => Ok(ToolOutput::error(format!(
                "Failed to click element {index}: {e}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// SetValueTool — set text in a UI element
// ---------------------------------------------------------------------------

pub struct SetValueTool;

#[async_trait]
impl Tool for SetValueTool {
    fn name(&self) -> &str {
        "set_value"
    }

    fn description(&self) -> &str {
        "Set the text value of a UI element (text field, search box, etc.) by its index number. \
         This directly sets the element's value via Accessibility API — no keyboard simulation \
         needed. Much more reliable than type_text for filling in specific fields."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "App name or 'frontmost'"
                },
                "index": {
                    "type": "integer",
                    "description": "Element index from get_ui_elements or find_element output"
                },
                "value": {
                    "type": "string",
                    "description": "The text value to set"
                }
            },
            "required": ["app", "index", "value"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        if !ax::is_trusted() {
            return Ok(ToolOutput::error("Accessibility permission not granted."));
        }

        let app_name = match args.get("app").and_then(Value::as_str) {
            Some(n) => n,
            None => return Ok(ToolOutput::error("Missing 'app' parameter")),
        };

        let index = match args.get("index").and_then(Value::as_u64) {
            Some(i) => i as usize,
            None => return Ok(ToolOutput::error("Missing 'index' parameter")),
        };

        let new_value = match args.get("value").and_then(Value::as_str) {
            Some(v) => v.to_string(),
            None => return Ok(ToolOutput::error("Missing 'value' parameter")),
        };

        let pid = match resolve_pid(app_name) {
            Some(p) => p,
            None => {
                return Ok(ToolOutput::error(format!(
                    "App '{app_name}' not found or not running"
                )));
            }
        };

        info!(
            "[AX] Setting value on element {} in {} (pid={})",
            index, app_name, pid
        );

        let val = new_value.clone();
        match tokio::task::spawn_blocking(move || ax::set_element_value(pid, index, &val))
            .await
            .unwrap_or(Err("Task panicked".into()))
        {
            Ok(()) => {
                let preview = if new_value.len() > 40 {
                    format!("{}...", &new_value[..37])
                } else {
                    new_value
                };
                Ok(ToolOutput::llm_only(format!(
                    "Set value \"{preview}\" on element {index} in '{app_name}'"
                )))
            }
            Err(e) => Ok(ToolOutput::error(format!(
                "Failed to set value on element {index}: {e}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// ReadValueTool — read the value/title of a UI element
// ---------------------------------------------------------------------------

pub struct ReadValueTool;

#[async_trait]
impl Tool for ReadValueTool {
    fn name(&self) -> &str {
        "read_value"
    }

    fn description(&self) -> &str {
        "Read the current value and attributes of a UI element at a given index. \
         Useful for checking what text is in a field, whether a checkbox is checked, etc."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "App name or 'frontmost'"
                },
                "index": {
                    "type": "integer",
                    "description": "Element index from get_ui_elements or find_element"
                }
            },
            "required": ["app", "index"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        if !ax::is_trusted() {
            return Ok(ToolOutput::error("Accessibility permission not granted."));
        }

        let app_name = match args.get("app").and_then(Value::as_str) {
            Some(n) => n,
            None => return Ok(ToolOutput::error("Missing 'app' parameter")),
        };

        let index = match args.get("index").and_then(Value::as_u64) {
            Some(i) => i as usize,
            None => return Ok(ToolOutput::error("Missing 'index' parameter")),
        };

        let pid = match resolve_pid(app_name) {
            Some(p) => p,
            None => {
                return Ok(ToolOutput::error(format!(
                    "App '{app_name}' not found or not running"
                )));
            }
        };

        let elements =
            tokio::task::spawn_blocking(move || ax::get_ui_tree(pid, 10))
                .await
                .unwrap_or_default();

        match elements.into_iter().find(|el| el.index == index) {
            Some(el) => {
                let info = json!({
                    "role": el.role,
                    "title": el.title,
                    "value": el.value,
                    "description": el.description,
                    "position": el.position.map(|(x,y)| format!("({x:.0}, {y:.0})")),
                    "size": el.size.map(|(w,h)| format!("{w:.0}x{h:.0}")),
                    "focused": el.focused,
                    "enabled": el.enabled,
                    "children_count": el.children_count,
                });
                Ok(ToolOutput::llm_only(info.to_string()))
            }
            None => Ok(ToolOutput::error(format!(
                "Element at index {index} not found in '{app_name}'"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// ElementAtPositionTool — identify element at screen coordinates
// ---------------------------------------------------------------------------

pub struct ElementAtPositionTool;

#[async_trait]
impl Tool for ElementAtPositionTool {
    fn name(&self) -> &str {
        "element_at_position"
    }

    fn description(&self) -> &str {
        "Identify what UI element is at specific screen coordinates. \
         Useful after taking a screenshot to identify what element is at a particular location."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "App name or 'frontmost'"
                },
                "x": {
                    "type": "number",
                    "description": "X screen coordinate"
                },
                "y": {
                    "type": "number",
                    "description": "Y screen coordinate"
                }
            },
            "required": ["app", "x", "y"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        if !ax::is_trusted() {
            return Ok(ToolOutput::error("Accessibility permission not granted."));
        }

        let app_name = match args.get("app").and_then(Value::as_str) {
            Some(n) => n,
            None => return Ok(ToolOutput::error("Missing 'app' parameter")),
        };

        let x = match args.get("x").and_then(Value::as_f64) {
            Some(v) => v as f32,
            None => return Ok(ToolOutput::error("Missing 'x' parameter")),
        };

        let y = match args.get("y").and_then(Value::as_f64) {
            Some(v) => v as f32,
            None => return Ok(ToolOutput::error("Missing 'y' parameter")),
        };

        let pid = match resolve_pid(app_name) {
            Some(p) => p,
            None => {
                return Ok(ToolOutput::error(format!(
                    "App '{app_name}' not found or not running"
                )));
            }
        };

        match tokio::task::spawn_blocking(move || ax::element_at_position(pid, x, y))
            .await
            .unwrap_or(None)
        {
            Some(el) => Ok(ToolOutput::llm_only(format!("{el}"))),
            None => Ok(ToolOutput::llm_only(format!(
                "No element found at ({x}, {y}) in '{app_name}'"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn resolve_pid(app_name: &str) -> Option<i32> {
    if app_name.eq_ignore_ascii_case("frontmost") {
        ax::frontmost_app_pid()
    } else {
        ax::app_pid(app_name)
    }
}
