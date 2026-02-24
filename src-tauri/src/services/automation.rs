use serde_json::Value;

/// Wraps autopilot-rs for macOS desktop automation.
///
/// Provides mouse control, keyboard input, and screen queries
/// that the AI agent can invoke through the `execute_automation` command.
pub struct AutomationService;

impl AutomationService {
    pub fn new() -> Self {
        Self
    }

    /// Move the mouse cursor to absolute screen coordinates.
    pub fn move_mouse(&self, x: f64, y: f64) -> Result<(), String> {
        autopilot::mouse::move_to(autopilot::geometry::Point::new(x, y))
            .map_err(|e| format!("Failed to move mouse: {e}"))
    }

    /// Left-click at the current cursor position.
    pub fn click(&self) -> Result<(), String> {
        autopilot::mouse::click(autopilot::mouse::Button::Left, None);
        Ok(())
    }

    /// Type a string of text using simulated keystrokes.
    pub fn type_text(&self, text: &str) -> Result<(), String> {
        autopilot::key::type_string(text, &[], 0.0, 0.0);
        Ok(())
    }

    /// Return the screen dimensions as `(width, height)`.
    pub fn screen_size(&self) -> (f64, f64) {
        let size = autopilot::screen::size();
        (size.width, size.height)
    }

    /// Return the current mouse cursor position as `(x, y)`.
    pub fn mouse_position(&self) -> (f64, f64) {
        let point = autopilot::mouse::location();
        (point.x, point.y)
    }

    /// Dispatch an automation action by name with JSON parameters.
    ///
    /// Supported actions:
    /// - `move_mouse` — requires `x` and `y` (f64)
    /// - `click` — no params, clicks at current position
    /// - `type` — requires `text` (string)
    /// - `screen_size` — returns `"WxH"`
    /// - `mouse_position` — returns `"(x, y)"`
    pub fn execute(&self, action: &str, params: &Value) -> Result<String, String> {
        match action {
            "move_mouse" => {
                let x = params
                    .get("x")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| "move_mouse requires numeric 'x' param".to_string())?;
                let y = params
                    .get("y")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| "move_mouse requires numeric 'y' param".to_string())?;
                self.move_mouse(x, y)?;
                Ok(format!("Moved mouse to ({x}, {y})"))
            }
            "click" => {
                self.click()?;
                Ok("Clicked at current position".to_string())
            }
            "type" => {
                let text = params
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "type requires string 'text' param".to_string())?;
                self.type_text(text)?;
                Ok(format!("Typed: {text}"))
            }
            "screen_size" => {
                let (w, h) = self.screen_size();
                Ok(format!("{w}x{h}"))
            }
            "mouse_position" => {
                let (x, y) = self.mouse_position();
                Ok(format!("({x}, {y})"))
            }
            _ => Err(format!("Unknown automation action: {action}")),
        }
    }
}

impl Default for AutomationService {
    fn default() -> Self {
        Self::new()
    }
}
