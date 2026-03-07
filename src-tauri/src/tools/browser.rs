//! Browser automation tools with triple backend:
//!
//! **Primary**: Chrome extension WebSocket bridge (port 3847).
//!   Works with the user's existing Chrome — no restart, all logins preserved.
//!
//! **Secondary**: agent-browser CLI (Vercel).
//!   Accessibility-tree based browser control. Can use existing Chrome via `--cdp`.
//!
//! **Fallback**: Chrome DevTools Protocol via chromiumoxide.
//!   Launches a dedicated Chrome instance with `--remote-debugging-port=9222`.
//!   Requires a separate profile (`~/.zeptobot/chrome-profile`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chromiumoxide::browser::Browser;
use chromiumoxide::Page;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tracing::info;
use zeptoclaw::tools::ToolOutput;
use zeptoclaw::{Result as ZeptoResult, Tool, ToolCategory, ToolContext};

const WS_PORT: u16 = 3847;
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> String {
    format!("req_{}", REQUEST_ID.fetch_add(1, Ordering::Relaxed))
}

// ===========================================================================
// Active backend
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq)]
enum Backend {
    Extension,
    AgentBrowser,
    Cdp,
}

// ===========================================================================
// Shared browser state
// ===========================================================================

pub struct BrowserState {
    active_backend: Mutex<Option<Backend>>,

    // ---- Extension (WebSocket) state ----
    ws_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    ws_server_started: AtomicBool,
    ws_connected: Arc<AtomicBool>,
    ws_notify: Arc<tokio::sync::Notify>,

    // ---- agent-browser state ----
    ab_session: Mutex<Option<String>>,

    // ---- CDP state ----
    cdp: Mutex<Option<CdpConn>>,
}

struct CdpConn {
    browser: Arc<Browser>,
    _handler: tokio::task::JoinHandle<()>,
}

impl BrowserState {
    pub fn new() -> Self {
        Self {
            active_backend: Mutex::new(None),
            ws_tx: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            ws_server_started: AtomicBool::new(false),
            ws_connected: Arc::new(AtomicBool::new(false)),
            ws_notify: Arc::new(tokio::sync::Notify::new()),
            ab_session: Mutex::new(None),
            cdp: Mutex::new(None),
        }
    }

    // -------------------------------------------------------------------
    // Extension helpers
    // -------------------------------------------------------------------

    async fn ensure_ws_server(&self) -> Result<(), String> {
        if self.ws_server_started.load(Ordering::Relaxed) {
            return Ok(());
        }

        let listener = TcpListener::bind(format!("127.0.0.1:{WS_PORT}"))
            .await
            .map_err(|e| format!("WS bind failed on port {WS_PORT}: {e}"))?;

        self.ws_server_started.store(true, Ordering::Relaxed);
        info!("[WS] Server listening on ws://127.0.0.1:{WS_PORT}");

        let ws_tx_slot = self.ws_tx.clone();
        let pending = self.pending.clone();
        let connected = self.ws_connected.clone();
        let notify = self.ws_notify.clone();

        tokio::spawn(async move {
            loop {
                let Ok((stream, addr)) = listener.accept().await else {
                    continue;
                };
                info!("[WS] Extension connected from {addr}");

                let Ok(ws) = tokio_tungstenite::accept_async(stream).await else {
                    continue;
                };
                let (mut write, mut read) = ws.split();
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

                *ws_tx_slot.lock().await = Some(tx);
                connected.store(true, Ordering::Relaxed);
                notify.notify_waiters();

                let write_task = tokio::spawn(async move {
                    while let Some(msg) = rx.recv().await {
                        if write.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                });

                let p = pending.clone();
                while let Some(Ok(msg)) = read.next().await {
                    let Message::Text(text) = msg else { continue };
                    let text: String = text.into();
                    let Ok(val) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    if val.get("type").and_then(Value::as_str).is_some() {
                        continue;
                    }
                    if let Some(id) = val.get("id").and_then(Value::as_str) {
                        if let Some(sender) = p.lock().await.remove(id) {
                            let _ = sender.send(val);
                        }
                    }
                }

                info!("[WS] Extension disconnected");
                connected.store(false, Ordering::Relaxed);
                *ws_tx_slot.lock().await = None;
                write_task.abort();
            }
        });

        Ok(())
    }

    async fn ws_send(&self, action: &str, params: Value) -> Result<Value, String> {
        let tx = {
            let g = self.ws_tx.lock().await;
            g.as_ref()
                .ok_or("Extension not connected")?
                .clone()
        };

        let id = next_id();
        let cmd = json!({ "id": id, "action": action, "params": params });

        let (resp_tx, resp_rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), resp_tx);

        tx.send(cmd.to_string()).map_err(|_| "WS send failed".to_string())?;

        match tokio::time::timeout(std::time::Duration::from_secs(15), resp_rx).await {
            Ok(Ok(val)) => {
                if val.get("success").and_then(Value::as_bool) == Some(true) {
                    Ok(val)
                } else {
                    Err(val.get("error").and_then(Value::as_str).unwrap_or("error").to_string())
                }
            }
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&id);
                Err("Response channel closed".into())
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err("Extension timeout (15s)".into())
            }
        }
    }

    async fn ws_cmd(&self, action: &str, params: Value) -> Result<String, String> {
        let val = self.ws_send(action, params).await?;
        Ok(val.get("result").map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }).unwrap_or_default())
    }

    // -------------------------------------------------------------------
    // agent-browser helpers
    // -------------------------------------------------------------------

    /// Check if `agent-browser` binary is available.
    fn ab_available() -> bool {
        std::process::Command::new("agent-browser")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Start agent-browser daemon session, connecting to existing Chrome CDP if
    /// available on port 9222, otherwise launching its own browser.
    async fn ab_start(&self) -> Result<String, String> {
        let session = format!("zeptobot-{}", std::process::id());

        // Try connecting to existing Chrome with CDP first
        let mut cmd = tokio::process::Command::new("agent-browser");
        cmd.arg("--session").arg(&session);

        // Check if Chrome is already running with debug port
        if tokio::net::TcpStream::connect("127.0.0.1:9222").await.is_ok() {
            cmd.arg("--cdp").arg("9222");
            info!("[AB] Connecting to existing Chrome CDP on port 9222");
        } else {
            info!("[AB] Launching agent-browser with its own browser");
        }

        cmd.arg("screenshot")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = cmd.output().await
            .map_err(|e| format!("agent-browser launch failed: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("agent-browser failed: {stderr}"));
        }

        *self.ab_session.lock().await = Some(session.clone());
        Ok(session)
    }

    /// Run an agent-browser CLI command and return its stdout.
    async fn ab_exec(&self, args: &[&str]) -> Result<String, String> {
        let session = self.ab_session.lock().await;
        let session = session.as_ref().ok_or("agent-browser not started")?;

        let mut cmd = tokio::process::Command::new("agent-browser");
        cmd.arg("--session").arg(session);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            cmd.output(),
        )
        .await
        .map_err(|_| "agent-browser timeout (30s)".to_string())?
        .map_err(|e| format!("agent-browser exec: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if output.status.success() {
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "agent-browser error: {}",
                if stderr.is_empty() { &stdout } else { stderr.as_ref() }
            ))
        }
    }

    /// Execute an action using agent-browser CLI.
    async fn run_ab(&self, action: &str, params: Value) -> Result<String, String> {
        match action {
            "get_tabs" => {
                // agent-browser doesn't have a tabs list — return session info
                let _ = self.ab_exec(&["screenshot"]).await?;
                Ok(r#"[{"id":0,"title":"agent-browser session","url":""}]"#.to_string())
            }
            "navigate" => {
                let url = params.get("url").and_then(Value::as_str)
                    .ok_or("Missing 'url'")?;
                self.ab_exec(&["navigate", url]).await?;
                // Give it a moment to load
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                Ok(format!("Navigated to {url}"))
            }
            "click" => {
                if let Some(sel) = params.get("selector").and_then(Value::as_str) {
                    self.ab_exec(&["click", sel]).await
                } else if let Some(txt) = params.get("text").and_then(Value::as_str) {
                    self.ab_exec(&["click", txt]).await
                } else {
                    Err("Provide 'selector' or 'text'".into())
                }
            }
            "type" => {
                let value = params.get("value").and_then(Value::as_str)
                    .ok_or("Missing 'value'")?;
                if let Some(sel) = params.get("selector").and_then(Value::as_str) {
                    self.ab_exec(&["click", sel]).await?;
                    self.ab_exec(&["type", value]).await
                } else {
                    self.ab_exec(&["type", value]).await
                }
            }
            "read" => {
                let out = self.ab_exec(&["content"]).await?;
                let truncated = if out.len() > 5000 { &out[..5000] } else { &out };
                Ok(truncated.to_string())
            }
            "list_elements" => {
                // Use observe to get accessibility tree
                let out = self.ab_exec(&["observe"]).await?;
                let truncated = if out.len() > 5000 { &out[..5000] } else { &out };
                Ok(truncated.to_string())
            }
            "execute_js" => {
                let code = params.get("code").and_then(Value::as_str)
                    .ok_or("Missing 'code'")?;
                self.ab_exec(&["evaluate", code]).await
            }
            "wait_for" => {
                let timeout = params.get("timeout_ms").and_then(Value::as_u64).unwrap_or(5000).min(15000);
                let deadline = tokio::time::Instant::now()
                    + std::time::Duration::from_millis(timeout);

                loop {
                    let out = self.ab_exec(&["observe"]).await.unwrap_or_default();
                    if let Some(sel) = params.get("selector").and_then(Value::as_str) {
                        if out.contains(sel) {
                            return Ok(format!("Found: {sel}"));
                        }
                    }
                    if let Some(txt) = params.get("text").and_then(Value::as_str) {
                        let lower = txt.to_lowercase();
                        if out.to_lowercase().contains(&lower) {
                            return Ok(format!("Found text: {txt}"));
                        }
                    }
                    if tokio::time::Instant::now() >= deadline {
                        return Err(format!("Timeout after {timeout}ms"));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
            _ => Err(format!("Unknown agent-browser action: {action}")),
        }
    }

    // -------------------------------------------------------------------
    // CDP helpers
    // -------------------------------------------------------------------

    async fn cdp_connect(&self) -> Result<Arc<Browser>, String> {
        let mut guard = self.cdp.lock().await;
        if let Some(conn) = guard.as_ref() {
            return Ok(conn.browser.clone());
        }

        let url = "http://127.0.0.1:9222";
        info!("[CDP] Connecting to Chrome at {url}...");

        match Browser::connect(url).await {
            Ok((browser, mut handler)) => {
                let h = tokio::spawn(async move {
                    while let Some(_ev) = handler.next().await {}
                });
                let browser = Arc::new(browser);
                *guard = Some(CdpConn { browser: browser.clone(), _handler: h });
                info!("[CDP] Connected");
                Ok(browser)
            }
            Err(e) => Err(format!("CDP connect failed: {e}")),
        }
    }

    async fn cdp_launch_and_connect(&self) -> Result<(Arc<Browser>, usize), String> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let profile = format!("{home}/.zeptobot/chrome-profile");
        let _ = std::fs::create_dir_all(&profile);

        let bins = [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        ];
        let bin = bins.iter().find(|p| std::path::Path::new(p).exists())
            .ok_or("Google Chrome not found")?;

        info!("[CDP] Launching dedicated Chrome at {bin}");
        std::process::Command::new(bin)
            .arg("--remote-debugging-port=9222")
            .arg(format!("--user-data-dir={profile}"))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .spawn()
            .map_err(|e| format!("Chrome launch failed: {e}"))?;

        tokio::time::sleep(std::time::Duration::from_secs(4)).await;

        let browser = self.cdp_connect().await?;
        let pages = browser.pages().await.unwrap_or_default();
        Ok((browser, pages.len()))
    }

    async fn cdp_active_page(&self) -> Result<Page, String> {
        let browser = self.cdp_connect().await?;
        let pages = browser.pages().await.map_err(|e| format!("pages: {e}"))?;
        if pages.is_empty() {
            info!("[CDP] No tabs, creating one...");
            let page = browser.new_page("about:blank").await
                .map_err(|e| format!("new_page: {e}"))?;
            return Ok(page);
        }
        Ok(pages.into_iter().last().unwrap())
    }

    // -------------------------------------------------------------------
    // Unified dispatch
    // -------------------------------------------------------------------

    async fn backend(&self) -> Option<Backend> {
        *self.active_backend.lock().await
    }

    /// Run a command through whichever backend is active.
    async fn run(&self, action: &str, params: Value) -> Result<String, String> {
        match self.backend().await {
            Some(Backend::Extension) => self.ws_cmd(action, params).await,
            Some(Backend::AgentBrowser) => self.run_ab(action, params).await,
            Some(Backend::Cdp) => self.run_cdp(action, params).await,
            None => Err("Not connected. Call browser_connect first.".into()),
        }
    }

    /// CDP fallback: execute an action using chromiumoxide.
    async fn run_cdp(&self, action: &str, params: Value) -> Result<String, String> {
        match action {
            "get_tabs" => {
                let browser = self.cdp_connect().await?;
                let pages = browser.pages().await.unwrap_or_default();
                let tabs: Vec<Value> = pages.iter().enumerate().map(|(i, _)| {
                    json!({ "id": i, "title": "(cdp tab)", "url": "" })
                }).collect();
                Ok(serde_json::to_string(&tabs).unwrap_or_default())
            }
            "navigate" => {
                let url = params.get("url").and_then(Value::as_str)
                    .ok_or("Missing 'url'")?;
                let page = self.cdp_active_page().await?;
                page.goto(url).await.map_err(|e| format!("goto: {e}"))?;
                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                let title = page.evaluate("document.title").await
                    .ok().and_then(|v| v.into_value::<String>().ok())
                    .unwrap_or_default();
                Ok(format!("Navigated to {url} — title: \"{title}\""))
            }
            "click" => {
                let page = self.cdp_active_page().await?;
                let selector = params.get("selector").and_then(Value::as_str);
                let text = params.get("text").and_then(Value::as_str);
                if let Some(sel) = selector {
                    let el = page.find_element(sel).await.map_err(|e| format!("not found: {e}"))?;
                    el.click().await.map_err(|e| format!("click: {e}"))?;
                    Ok(format!("clicked: {sel}"))
                } else if let Some(txt) = text {
                    let js = format!(
                        r#"(() => {{
                            const lower = '{lower}';
                            const xpath = `//*[contains(translate(text(),'ABCDEFGHIJKLMNOPQRSTUVWXYZ','abcdefghijklmnopqrstuvwxyz'), '${{lower}}')]`;
                            const r = document.evaluate(xpath, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
                            if (r.singleNodeValue) {{ r.singleNodeValue.click(); return 'clicked text: {txt}'; }}
                            const a = document.querySelector('[aria-label*="{txt}" i]');
                            if (a) {{ a.click(); return 'clicked aria: {txt}'; }}
                            return 'not_found: {txt}';
                        }})()"#,
                        lower = txt.to_lowercase().replace('\'', "\\'"),
                        txt = txt.replace('"', "\\\"")
                    );
                    let val = page.evaluate(js).await.map_err(|e| format!("js: {e}"))?;
                    let r = val.into_value::<String>().unwrap_or_default();
                    if r.starts_with("not_found") { Err(r) } else { Ok(r) }
                } else {
                    Err("Provide 'selector' or 'text'".into())
                }
            }
            "type" => {
                let page = self.cdp_active_page().await?;
                let value = params.get("value").and_then(Value::as_str)
                    .ok_or("Missing 'value'")?;
                if let Some(sel) = params.get("selector").and_then(Value::as_str) {
                    let el = page.find_element(sel).await.map_err(|e| format!("not found: {e}"))?;
                    let _ = el.click().await;
                    if params.get("clear_first").and_then(Value::as_bool).unwrap_or(true) {
                        let _ = page.evaluate(format!(
                            "document.querySelector('{}').value = ''",
                            sel.replace('\'', "\\'")
                        )).await;
                    }
                    el.type_str(value).await.map_err(|e| format!("type: {e}"))?;
                    Ok(format!("typed into {sel}"))
                } else {
                    // Type into focused element using native setter (React-compatible)
                    let js = format!(
                        r#"(() => {{
                            const el = document.activeElement;
                            if (!el || !(el.tagName==='INPUT'||el.tagName==='TEXTAREA'||el.isContentEditable)) {{
                                return 'no focused input';
                            }}
                            if (el.isContentEditable) {{
                                el.textContent = '{val}';
                                el.dispatchEvent(new Event('input', {{bubbles:true}}));
                            }} else {{
                                // Use native setter to trigger React/Vue/Angular state updates
                                const proto = el.tagName === 'TEXTAREA'
                                    ? HTMLTextAreaElement.prototype
                                    : HTMLInputElement.prototype;
                                const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
                                setter.call(el, '{val}');
                                el.dispatchEvent(new Event('input', {{bubbles:true}}));
                                el.dispatchEvent(new Event('change', {{bubbles:true}}));
                            }}
                            return 'typed';
                        }})()"#,
                        val = value.replace('\'', "\\'").replace('\\', "\\\\")
                    );
                    let val = page.evaluate(js).await.map_err(|e| format!("js: {e}"))?;
                    Ok(val.into_value::<String>().unwrap_or_default())
                }
            }
            "read" => {
                let page = self.cdp_active_page().await?;
                let selector = params.get("selector").and_then(Value::as_str);
                let full = params.get("page_text").and_then(Value::as_bool).unwrap_or(false);
                let js = if let Some(sel) = selector {
                    format!(
                        "(() => {{ const e = document.querySelector('{}'); return e ? (e.innerText||e.textContent||'').substring(0,3000) : 'not found'; }})()",
                        sel.replace('\'', "\\'")
                    )
                } else if full {
                    "(document.body.innerText||'').substring(0,5000)".into()
                } else {
                    r#"JSON.stringify({title:document.title,url:location.href})"#.into()
                };
                let val = page.evaluate(js).await.map_err(|e| format!("js: {e}"))?;
                Ok(val.into_value::<String>().unwrap_or_default())
            }
            "list_elements" => {
                let page = self.cdp_active_page().await?;
                let filter = params.get("filter").and_then(Value::as_str).unwrap_or("");
                let js = format!(
                    r#"(() => {{
                        const els = []; const f = '{}';
                        for (const el of document.querySelectorAll('a,button,input,textarea,select,[role="button"],[role="link"],[aria-label]')) {{
                            const t = (el.innerText||el.value||el.placeholder||el.getAttribute('aria-label')||'').trim().substring(0,80);
                            if (f && !t.toLowerCase().includes(f.toLowerCase())) continue;
                            const tag = el.tagName.toLowerCase();
                            const id = el.id ? '#'+el.id : '';
                            const cls = el.className && typeof el.className==='string' ? '.'+el.className.split(' ').filter(Boolean).slice(0,2).join('.') : '';
                            els.push({{ tag, text: t, type: el.type||el.getAttribute('role')||'', selector: tag+id+cls }});
                            if (els.length >= 100) break;
                        }}
                        return JSON.stringify(els);
                    }})()"#,
                    filter.replace('\'', "\\'")
                );
                let val = page.evaluate(js).await.map_err(|e| format!("js: {e}"))?;
                Ok(val.into_value::<String>().unwrap_or_default())
            }
            "execute_js" => {
                let page = self.cdp_active_page().await?;
                let code = params.get("code").and_then(Value::as_str)
                    .ok_or("Missing 'code'")?;
                let val = page.evaluate(code).await.map_err(|e| format!("js: {e}"))?;
                Ok(val.into_value::<serde_json::Value>()
                    .map(|v| match v { Value::String(s) => s, o => o.to_string() })
                    .unwrap_or_else(|_| "undefined".into()))
            }
            "wait_for" => {
                let page = self.cdp_active_page().await?;
                let selector = params.get("selector").and_then(Value::as_str);
                let text = params.get("text").and_then(Value::as_str);
                let timeout = params.get("timeout_ms").and_then(Value::as_u64).unwrap_or(5000).min(15000);

                let js = if let Some(sel) = selector {
                    format!(
                        "(() => {{ const e = document.querySelector('{}'); return e ? (e.innerText||e.textContent||'found') : null; }})()",
                        sel.replace('\'', "\\'")
                    )
                } else if let Some(txt) = text {
                    format!(
                        r#"(() => {{
                            const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
                            while (w.nextNode()) {{ if (w.currentNode.textContent.toLowerCase().includes('{}')) return w.currentNode.parentElement.innerText.substring(0,200); }}
                            return null;
                        }})()"#,
                        txt.to_lowercase().replace('\'', "\\'")
                    )
                } else {
                    return Err("Provide 'selector' or 'text'".into());
                };

                let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout);
                loop {
                    if let Ok(val) = page.evaluate(js.as_str()).await {
                        if let Ok(result) = val.into_value::<String>() {
                            return Ok(result);
                        }
                    }
                    if tokio::time::Instant::now() >= deadline {
                        return Err(format!("Timeout after {timeout}ms"));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
            }
            _ => Err(format!("Unknown CDP action: {action}")),
        }
    }
}

// ===========================================================================
// BrowserConnectTool
// ===========================================================================

pub struct BrowserConnectTool { pub state: Arc<BrowserState> }

#[async_trait]
impl Tool for BrowserConnectTool {
    fn name(&self) -> &str { "browser_connect" }

    fn description(&self) -> &str {
        "Connect to Chrome for web page automation. Tries: (1) ZeptoBot Bridge extension \
         (existing Chrome, all logins preserved), (2) agent-browser CLI (accessibility-tree \
         based), (3) CDP with dedicated Chrome instance. Call this before other browser_ tools."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    fn category(&self) -> ToolCategory { ToolCategory::Shell }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        // ------- Try 1: Extension bridge -------
        let _ = self.state.ensure_ws_server().await;

        if self.state.ws_connected.load(Ordering::Relaxed) {
            if let Ok(tabs) = self.state.ws_cmd("get_tabs", json!({})).await {
                let n: Vec<Value> = serde_json::from_str(&tabs).unwrap_or_default();
                *self.state.active_backend.lock().await = Some(Backend::Extension);
                return Ok(ToolOutput::llm_only(format!(
                    "Connected to Chrome via extension bridge. {} tab(s) open. \
                     All logins preserved. Use browser_navigate to open URLs.",
                    n.len()
                )));
            }
        }

        // Wait briefly for extension
        info!("[WS] Waiting for extension (3s)...");
        if tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.state.ws_notify.notified(),
        ).await.is_ok() {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if let Ok(tabs) = self.state.ws_cmd("get_tabs", json!({})).await {
                let n: Vec<Value> = serde_json::from_str(&tabs).unwrap_or_default();
                *self.state.active_backend.lock().await = Some(Backend::Extension);
                return Ok(ToolOutput::llm_only(format!(
                    "Connected to Chrome via extension bridge. {} tab(s) open. \
                     All logins preserved. Use browser_navigate to open URLs.",
                    n.len()
                )));
            }
        }

        // ------- Try 2: agent-browser CLI -------
        if BrowserState::ab_available() {
            info!("[AB] agent-browser found, starting session...");
            match self.state.ab_start().await {
                Ok(session) => {
                    *self.state.active_backend.lock().await = Some(Backend::AgentBrowser);
                    return Ok(ToolOutput::llm_only(format!(
                        "Connected via agent-browser (session: {session}). \
                         Accessibility-tree based control. Use browser_navigate to open URLs."
                    )));
                }
                Err(e) => {
                    info!("[AB] agent-browser failed: {e}, trying CDP...");
                }
            }
        }

        // ------- Try 3: CDP on existing debug Chrome -------
        info!("[CDP] Trying CDP fallback...");
        if let Ok(browser) = self.state.cdp_connect().await {
            let pages = browser.pages().await.unwrap_or_default();
            *self.state.active_backend.lock().await = Some(Backend::Cdp);
            return Ok(ToolOutput::llm_only(format!(
                "Connected to Chrome via CDP. {} tab(s) open. \
                 Use browser_navigate to open URLs.",
                pages.len()
            )));
        }

        // ------- Try 4: Launch dedicated Chrome with CDP -------
        info!("[CDP] Launching dedicated Chrome instance...");
        match self.state.cdp_launch_and_connect().await {
            Ok((_browser, n)) => {
                *self.state.active_backend.lock().await = Some(Backend::Cdp);
                Ok(ToolOutput::llm_only(format!(
                    "Launched dedicated Chrome with CDP (fresh profile). {} tab(s). \
                     Note: this is a separate profile — you may need to sign in. \
                     For zero-setup: install the ZeptoBot Bridge extension. \
                     Use browser_navigate to open URLs.",
                    n
                )))
            }
            Err(e) => Ok(ToolOutput::error(format!(
                "All connection methods failed.\n\
                 Best: Install ZeptoBot Bridge extension (chrome://extensions → Load unpacked → zeptobot/extension/).\n\
                 Alt: Install agent-browser (npm install -g agent-browser && agent-browser install).\n\
                 CDP fallback error: {e}"
            ))),
        }
    }
}

// ===========================================================================
// Tool structs (thin wrappers over state.run())
// ===========================================================================

pub struct BrowserClickTool { pub state: Arc<BrowserState> }

#[async_trait]
impl Tool for BrowserClickTool {
    fn name(&self) -> &str { "browser_click" }
    fn description(&self) -> &str {
        "Click an element by CSS selector or text content. \
         Examples: selector='button.compose', text='Compose'"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector" },
                "text": { "type": "string", "description": "Visible text to click" }
            },
            "required": []
        })
    }
    fn category(&self) -> ToolCategory { ToolCategory::Shell }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        match self.state.run("click", args).await {
            Ok(r) => Ok(ToolOutput::llm_only(r)),
            Err(e) => Ok(ToolOutput::error(e)),
        }
    }
}

pub struct BrowserTypeTool { pub state: Arc<BrowserState> }

#[async_trait]
impl Tool for BrowserTypeTool {
    fn name(&self) -> &str { "browser_type" }
    fn description(&self) -> &str {
        "Type text into an input field. Find by CSS selector, label text, or focused element. \
         Examples: selector='input[name=to]' value='john@example.com'"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector for input" },
                "text": { "type": "string", "description": "Find input by label text" },
                "value": { "type": "string", "description": "Text to type" },
                "clear_first": { "type": "boolean", "description": "Clear before typing (default: true)" }
            },
            "required": ["value"]
        })
    }
    fn category(&self) -> ToolCategory { ToolCategory::Shell }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        match self.state.run("type", args).await {
            Ok(r) => Ok(ToolOutput::llm_only(r)),
            Err(e) => Ok(ToolOutput::error(e)),
        }
    }
}

pub struct BrowserReadTool { pub state: Arc<BrowserState> }

#[async_trait]
impl Tool for BrowserReadTool {
    fn name(&self) -> &str { "browser_read" }
    fn description(&self) -> &str {
        "Read text from the page — specific element by selector, full body text, or title/URL."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector to read" },
                "page_text": { "type": "boolean", "description": "Read full page text" }
            },
            "required": []
        })
    }
    fn category(&self) -> ToolCategory { ToolCategory::Shell }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        match self.state.run("read", args).await {
            Ok(r) => Ok(ToolOutput::llm_only(r)),
            Err(e) => Ok(ToolOutput::error(e)),
        }
    }
}

pub struct BrowserListElementsTool { pub state: Arc<BrowserState> }

#[async_trait]
impl Tool for BrowserListElementsTool {
    fn name(&self) -> &str { "browser_list_elements" }
    fn description(&self) -> &str {
        "List interactive elements (buttons, links, inputs) on the page."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filter": { "type": "string", "description": "Only show elements containing this text" }
            },
            "required": []
        })
    }
    fn category(&self) -> ToolCategory { ToolCategory::Shell }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        match self.state.run("list_elements", args).await {
            Ok(raw) => {
                let elements: Vec<Value> = serde_json::from_str(&raw).unwrap_or_default();
                if elements.is_empty() {
                    return Ok(ToolOutput::llm_only("No interactive elements found."));
                }
                let mut lines = vec![format!("{} interactive elements:", elements.len())];
                for el in &elements {
                    let tag = el["tag"].as_str().unwrap_or("");
                    let text = el["text"].as_str().unwrap_or("");
                    let typ = el["type"].as_str().unwrap_or("");
                    let sel = el["selector"].as_str().unwrap_or("");
                    if !text.is_empty() {
                        lines.push(format!("  <{tag}> \"{text}\" type={typ} sel={sel}"));
                    } else {
                        lines.push(format!("  <{tag}> type={typ} sel={sel}"));
                    }
                }
                Ok(ToolOutput::llm_only(lines.join("\n")))
            }
            Err(e) => Ok(ToolOutput::error(e)),
        }
    }
}

pub struct BrowserNavigateTool { pub state: Arc<BrowserState> }

#[async_trait]
impl Tool for BrowserNavigateTool {
    fn name(&self) -> &str { "browser_navigate" }
    fn description(&self) -> &str {
        "Navigate the active Chrome tab to a URL. Waits for load. Use instead of open_url for web apps."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to navigate to" }
            },
            "required": ["url"]
        })
    }
    fn category(&self) -> ToolCategory { ToolCategory::Shell }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        match self.state.run("navigate", args).await {
            Ok(r) => Ok(ToolOutput::llm_only(r)),
            Err(e) => Ok(ToolOutput::error(e)),
        }
    }
}

pub struct BrowserJsTool { pub state: Arc<BrowserState> }

#[async_trait]
impl Tool for BrowserJsTool {
    fn name(&self) -> &str { "browser_js" }
    fn description(&self) -> &str {
        "Execute JavaScript on the active tab. Most powerful browser tool — \
         can do multiple actions in one call. Returns the result."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "JavaScript to execute" }
            },
            "required": ["code"]
        })
    }
    fn category(&self) -> ToolCategory { ToolCategory::Shell }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        match self.state.run("execute_js", args).await {
            Ok(r) => Ok(ToolOutput::llm_only(r)),
            Err(e) => Ok(ToolOutput::error(e)),
        }
    }
}

pub struct BrowserWaitForTool { pub state: Arc<BrowserState> }

#[async_trait]
impl Tool for BrowserWaitForTool {
    fn name(&self) -> &str { "browser_wait_for" }
    fn description(&self) -> &str {
        "Wait for an element to appear on the page. Returns its text when found. \
         Much faster than wait + screenshot."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector to wait for" },
                "text": { "type": "string", "description": "Wait for text to appear" },
                "timeout_ms": { "type": "integer", "description": "Max wait ms (default: 5000)" }
            },
            "required": []
        })
    }
    fn category(&self) -> ToolCategory { ToolCategory::Shell }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ZeptoResult<ToolOutput> {
        match self.state.run("wait_for", args).await {
            Ok(r) => Ok(ToolOutput::llm_only(r)),
            Err(e) => Ok(ToolOutput::error(e)),
        }
    }
}

// ===========================================================================
// Factory
// ===========================================================================

pub fn all_browser_tools() -> (Arc<BrowserState>, Vec<Box<dyn Tool>>) {
    let state = Arc::new(BrowserState::new());
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(BrowserConnectTool { state: state.clone() }),
        Box::new(BrowserClickTool { state: state.clone() }),
        Box::new(BrowserTypeTool { state: state.clone() }),
        Box::new(BrowserReadTool { state: state.clone() }),
        Box::new(BrowserListElementsTool { state: state.clone() }),
        Box::new(BrowserNavigateTool { state: state.clone() }),
        Box::new(BrowserJsTool { state: state.clone() }),
        Box::new(BrowserWaitForTool { state: state.clone() }),
    ];
    (state, tools)
}
