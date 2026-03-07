// ZeptoBot Chrome Extension — Bridge between ZeptoBot and Chrome tabs.
// Connects via WebSocket to ZeptoBot's local server and executes DOM commands.

const WS_URL = "ws://127.0.0.1:3847";
let ws = null;
let connected = false;
let reconnectTimer = null;

// ---------------------------------------------------------------------------
// WebSocket connection
// ---------------------------------------------------------------------------

function connect() {
  if (ws && ws.readyState === WebSocket.OPEN) return;

  try {
    ws = new WebSocket(WS_URL);
  } catch (e) {
    scheduleReconnect();
    return;
  }

  ws.onopen = () => {
    connected = true;
    console.log("[ZeptoBot] Connected to ZeptoBot server");
    // Send hello so the server knows we're here
    ws.send(JSON.stringify({ type: "hello", version: "1.0.0" }));
    broadcastStatus();
  };

  ws.onmessage = async (event) => {
    let cmd;
    try {
      cmd = JSON.parse(event.data);
    } catch {
      return;
    }
    if (cmd.type === "ping") {
      ws.send(JSON.stringify({ type: "pong" }));
      return;
    }
    const result = await handleCommand(cmd);
    ws.send(JSON.stringify(result));
  };

  ws.onclose = () => {
    connected = false;
    ws = null;
    console.log("[ZeptoBot] Disconnected");
    broadcastStatus();
    scheduleReconnect();
  };

  ws.onerror = () => {
    // onclose will fire after this
  };
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connect();
  }, 3000);
}

// Keep-alive: retry connection every 5s via alarm
chrome.alarms.create("zeptobot-keepalive", { periodInMinutes: 0.08 }); // ~5s
chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === "zeptobot-keepalive" && !connected) {
    connect();
  }
});

// Start on load
connect();

// ---------------------------------------------------------------------------
// Status broadcast to popup
// ---------------------------------------------------------------------------

function broadcastStatus() {
  chrome.runtime.sendMessage({ type: "status", connected }).catch(() => {});
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === "get_status") {
    sendResponse({ connected });
  }
});

// ---------------------------------------------------------------------------
// Command handler
// ---------------------------------------------------------------------------

async function handleCommand(cmd) {
  const { id, action, tabId, params = {} } = cmd;
  try {
    // Resolve target tab
    let tab;
    if (tabId) {
      tab = await chrome.tabs.get(tabId);
    } else {
      const [activeTab] = await chrome.tabs.query({
        active: true,
        currentWindow: true,
      });
      tab = activeTab;
    }

    switch (action) {
      case "get_tabs":
        return await handleGetTabs(id);
      case "navigate":
        return await handleNavigate(id, tab, params);
      case "click":
        return await handleClick(id, tab, params);
      case "type":
        return await handleType(id, tab, params);
      case "read":
        return await handleRead(id, tab, params);
      case "list_elements":
        return await handleListElements(id, tab, params);
      case "execute_js":
        return await handleExecuteJs(id, tab, params);
      case "wait_for":
        return await handleWaitFor(id, tab, params);
      default:
        return { id, success: false, error: `Unknown action: ${action}` };
    }
  } catch (e) {
    return { id, success: false, error: e.message || String(e) };
  }
}

// ---------------------------------------------------------------------------
// Action handlers
// ---------------------------------------------------------------------------

async function handleGetTabs(id) {
  const tabs = await chrome.tabs.query({});
  const list = tabs.map((t) => ({
    id: t.id,
    title: t.title,
    url: t.url,
    active: t.active,
  }));
  return { id, success: true, result: JSON.stringify(list) };
}

async function handleNavigate(id, tab, params) {
  const { url } = params;
  if (!url) return { id, success: false, error: "Missing 'url' parameter" };

  await chrome.tabs.update(tab.id, { url });

  // Wait for page to finish loading (max 15s)
  await new Promise((resolve) => {
    const listener = (updatedTabId, changeInfo) => {
      if (updatedTabId === tab.id && changeInfo.status === "complete") {
        chrome.tabs.onUpdated.removeListener(listener);
        resolve();
      }
    };
    chrome.tabs.onUpdated.addListener(listener);
    setTimeout(() => {
      chrome.tabs.onUpdated.removeListener(listener);
      resolve();
    }, 15000);
  });

  const updated = await chrome.tabs.get(tab.id);
  return {
    id,
    success: true,
    result: `Navigated to ${url} — title: "${updated.title}"`,
  };
}

async function handleClick(id, tab, params) {
  const { selector, text } = params;
  if (!selector && !text)
    return { id, success: false, error: "Provide 'selector' or 'text'" };

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: (sel, txt) => {
      if (sel) {
        const el = document.querySelector(sel);
        if (el) {
          el.click();
          return "clicked: " + sel;
        }
        return "not_found: " + sel;
      }
      if (txt) {
        const lower = txt.toLowerCase();
        // Try XPath text search
        const xpath = `//*[contains(translate(text(),'ABCDEFGHIJKLMNOPQRSTUVWXYZ','abcdefghijklmnopqrstuvwxyz'), '${lower.replace(/'/g, "\\'")}')]`;
        const xr = document.evaluate(
          xpath,
          document,
          null,
          XPathResult.FIRST_ORDERED_NODE_TYPE,
          null
        );
        if (xr.singleNodeValue) {
          xr.singleNodeValue.click();
          return "clicked text: " + txt;
        }
        // Try aria-label
        const ariaEl = document.querySelector(
          `[aria-label*="${txt}" i], [data-tooltip*="${txt}" i]`
        );
        if (ariaEl) {
          ariaEl.click();
          return "clicked aria: " + txt;
        }
        // Try role=button with text
        const btns = document.querySelectorAll(
          'button, [role="button"], a, [role="link"]'
        );
        for (const b of btns) {
          if (b.textContent.toLowerCase().includes(lower)) {
            b.click();
            return "clicked button: " + txt;
          }
        }
        return "not_found: " + txt;
      }
    },
    args: [selector || null, text || null],
    world: "MAIN",
  });

  const result = results[0]?.result || "no result";
  if (result.startsWith("not_found"))
    return { id, success: false, error: result };
  return { id, success: true, result };
}

async function handleType(id, tab, params) {
  const { selector, text, value, clear_first = true } = params;
  if (!value)
    return { id, success: false, error: "Missing 'value' parameter" };

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: (sel, label, val, clear) => {
      let el = null;

      if (sel) {
        el = document.querySelector(sel);
      } else if (label) {
        // Find by label text
        for (const l of document.querySelectorAll("label")) {
          if (l.textContent.toLowerCase().includes(label.toLowerCase())) {
            el = l.htmlFor
              ? document.getElementById(l.htmlFor)
              : l.querySelector("input,textarea,select");
            if (el) break;
          }
        }
        // Try placeholder
        if (!el)
          el = document.querySelector(
            `input[placeholder*="${label}" i], textarea[placeholder*="${label}" i]`
          );
        // Try aria-label
        if (!el)
          el = document.querySelector(
            `[aria-label*="${label}" i][contenteditable], input[aria-label*="${label}" i], textarea[aria-label*="${label}" i]`
          );
      } else {
        // Use focused element
        el = document.activeElement;
      }

      if (
        !el ||
        !(
          el.tagName === "INPUT" ||
          el.tagName === "TEXTAREA" ||
          el.tagName === "SELECT" ||
          el.isContentEditable
        )
      ) {
        return "not_found: no suitable input element";
      }

      el.focus();
      if (el.isContentEditable) {
        if (clear) el.textContent = "";
        el.textContent = val;
        el.dispatchEvent(new Event("input", { bubbles: true }));
      } else {
        if (clear) el.value = "";
        el.value = val;
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.dispatchEvent(new Event("change", { bubbles: true }));
      }
      return "typed";
    },
    args: [selector || null, text || null, value, clear_first],
    world: "MAIN",
  });

  const result = results[0]?.result || "no result";
  if (result.startsWith("not_found"))
    return { id, success: false, error: result };
  return { id, success: true, result };
}

async function handleRead(id, tab, params) {
  const { selector, page_text = false } = params;

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: (sel, fullPage) => {
      if (sel) {
        const el = document.querySelector(sel);
        if (!el) return "Element not found: " + sel;
        return (el.innerText || el.textContent || el.value || "").substring(
          0,
          3000
        );
      }
      if (fullPage) {
        return (document.body.innerText || "").substring(0, 5000);
      }
      return JSON.stringify({
        title: document.title,
        url: window.location.href,
      });
    },
    args: [selector || null, page_text],
    world: "MAIN",
  });

  return { id, success: true, result: results[0]?.result || "" };
}

async function handleListElements(id, tab, params) {
  const { filter = "" } = params;

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: (filterText) => {
      const elements = [];
      const all = document.querySelectorAll(
        'a, button, input, textarea, select, [role="button"], [role="link"], [role="tab"], [onclick], [aria-label]'
      );
      for (let i = 0; i < Math.min(all.length, 100); i++) {
        const el = all[i];
        const text = (
          el.innerText ||
          el.value ||
          el.placeholder ||
          el.getAttribute("aria-label") ||
          ""
        )
          .trim()
          .substring(0, 80);
        if (
          filterText &&
          !text.toLowerCase().includes(filterText.toLowerCase())
        )
          continue;
        const tag = el.tagName.toLowerCase();
        const type = el.type || el.getAttribute("role") || "";
        const elId = el.id ? "#" + el.id : "";
        const cls =
          el.className && typeof el.className === "string"
            ? "." +
              el.className
                .split(" ")
                .filter(Boolean)
                .slice(0, 2)
                .join(".")
            : "";
        elements.push({ tag, text, type, selector: tag + elId + cls });
      }
      return JSON.stringify(elements);
    },
    args: [filter],
    world: "MAIN",
  });

  const raw = results[0]?.result || "[]";
  return { id, success: true, result: raw };
}

async function handleExecuteJs(id, tab, params) {
  const { code } = params;
  if (!code) return { id, success: false, error: "Missing 'code' parameter" };

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: (codeStr) => {
      try {
        const fn = new Function("return (" + codeStr + ")");
        const result = fn();
        if (result === undefined || result === null)
          return String(result);
        return typeof result === "object"
          ? JSON.stringify(result)
          : String(result);
      } catch (e) {
        return "Error: " + e.message;
      }
    },
    args: [code],
    world: "MAIN",
  });

  return { id, success: true, result: results[0]?.result || "undefined" };
}

async function handleWaitFor(id, tab, params) {
  const { selector, text, timeout_ms = 5000 } = params;
  if (!selector && !text)
    return { id, success: false, error: "Provide 'selector' or 'text'" };

  const results = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: (sel, txt, timeout) => {
      return new Promise((resolve) => {
        const deadline = Date.now() + timeout;
        const check = () => {
          if (sel) {
            const el = document.querySelector(sel);
            if (el) return resolve(el.innerText || el.textContent || "found");
          }
          if (txt) {
            const lower = txt.toLowerCase();
            const walker = document.createTreeWalker(
              document.body,
              NodeFilter.SHOW_TEXT
            );
            while (walker.nextNode()) {
              if (walker.currentNode.textContent.toLowerCase().includes(lower)) {
                return resolve(
                  walker.currentNode.parentElement.innerText.substring(0, 200)
                );
              }
            }
          }
          if (Date.now() >= deadline) return resolve(null);
          setTimeout(check, 300);
        };
        check();
      });
    },
    args: [selector || null, text || null, timeout_ms],
    world: "MAIN",
  });

  const result = results[0]?.result;
  if (result) return { id, success: true, result };
  return {
    id,
    success: false,
    error: `Timeout: not found after ${timeout_ms}ms`,
  };
}
