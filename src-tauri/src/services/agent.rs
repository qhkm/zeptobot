/// Placeholder for ZeptoClaw agent integration.
///
/// Will be replaced with actual `zeptoclaw` crate integration once the
/// agent service is wired up. For now, returns echo responses so the
/// frontend can develop against a stable API.
pub struct AgentService;

impl AgentService {
    pub fn new() -> Self {
        Self
    }

    /// Send a message to the AI agent and get a response.
    ///
    /// Currently returns a placeholder response. Will be replaced with
    /// a call to the ZeptoClaw agent loop with autopilot tools registered.
    pub async fn chat(&self, message: &str) -> Result<String, String> {
        Ok(format!(
            "I understood: \"{message}\". \
             ZeptoClaw integration coming soon \
             -- I'll be able to control your Mac!"
        ))
    }
}

impl Default for AgentService {
    fn default() -> Self {
        Self::new()
    }
}
