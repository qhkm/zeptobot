/**
 * ZeptoBot — chat interface
 */

import { useState, useRef, useEffect, KeyboardEvent, FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

interface ChatMessage {
  id: number;
  role: "user" | "assistant" | "step";
  content: string;
  tool?: string;
}

interface BotStatus {
  listening: boolean;
  agent_ready: boolean;
  automation_available: boolean;
}

interface AgentStep {
  tool: string;
  message: string;
}

let nextId = 1;

function TypingIndicator() {
  return (
    <div className="message-row bot-row">
      <div className="bubble bot-bubble typing-indicator">
        <span />
        <span />
        <span />
      </div>
    </div>
  );
}

function App() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [isConnected, setIsConnected] = useState(true);
  const [agentReady, setAgentReady] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  // Check agent status on mount
  useEffect(() => {
    invoke<BotStatus>("get_status")
      .then((status) => {
        setAgentReady(status.agent_ready);
        setIsConnected(true);
      })
      .catch(() => setIsConnected(false));
  }, []);

  // Listen for agent step events
  useEffect(() => {
    const unlisten = listen<AgentStep>("agent-step", (event) => {
      const step = event.payload;
      setMessages((prev) => {
        // Check if the last message is a step for the same tool starting with "Executing:"
        // If so, replace it with the "Done:" result
        const last = prev[prev.length - 1];
        if (
          last &&
          last.role === "step" &&
          last.tool === step.tool &&
          step.message.startsWith("Done:")
        ) {
          return [
            ...prev.slice(0, -1),
            {
              id: last.id,
              role: "step" as const,
              content: step.message,
              tool: step.tool,
            },
          ];
        }
        return [
          ...prev,
          {
            id: nextId++,
            role: "step" as const,
            content: step.message,
            tool: step.tool,
          },
        ];
      });
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Auto-scroll whenever messages or loading state change
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, isLoading]);

  async function sendMessage() {
    const text = input.trim();
    if (!text || isLoading) return;

    const userMsg: ChatMessage = {
      id: nextId++,
      role: "user",
      content: text,
    };

    setMessages((prev) => [...prev, userMsg]);
    setInput("");
    setIsLoading(true);
    setIsConnected(true);

    try {
      const response = await invoke<string>("send_message", { message: text });
      const botMsg: ChatMessage = {
        id: nextId++,
        role: "assistant",
        content: response,
      };
      setMessages((prev) => [...prev, botMsg]);
    } catch (err) {
      const content =
        typeof err === "string"
          ? err
          : err instanceof Error
            ? err.message
            : JSON.stringify(err);
      // Don't show "stopped" as an error
      if (content === "Generation stopped by user") {
        const stopMsg: ChatMessage = {
          id: nextId++,
          role: "assistant",
          content: "Stopped.",
        };
        setMessages((prev) => [...prev, stopMsg]);
      } else {
        const errMsg: ChatMessage = {
          id: nextId++,
          role: "assistant",
          content,
        };
        setMessages((prev) => [...prev, errMsg]);
        setIsConnected(false);
      }
    } finally {
      setIsLoading(false);
    }
  }

  async function stopGeneration() {
    try {
      await invoke("stop_generation");
    } catch {
      // ignore
    }
  }

  function handleSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    sendMessage();
  }

  function handleKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }

  return (
    <div className="app">
      {/* ── Header ───────────────────────────────────────── */}
      <header className="header">
        <div className="header-left">
          <span className="header-title">ZeptoBot</span>
        </div>
        <div className="header-right">
          <span
            className={`status-dot ${isConnected ? "status-online" : "status-offline"}`}
            title={isConnected ? "Connected" : "Disconnected"}
          />
          <span className="status-label">
            {isConnected ? "Online" : "Offline"}
          </span>
        </div>
      </header>

      {/* ── Messages ─────────────────────────────────────── */}
      <main className="messages-area">
        {messages.length === 0 && (
          <div className="empty-state">
            <p className="empty-title">How can I help you?</p>
            <p className="empty-hint">
              {agentReady
                ? 'I can control your Mac — try "open Google Chrome"'
                : "Set ANTHROPIC_API_KEY or OPENAI_API_KEY to enable the AI agent"}
            </p>
          </div>
        )}

        {messages.map((msg) => (
          <div
            key={msg.id}
            className={`message-row ${
              msg.role === "user"
                ? "user-row"
                : msg.role === "step"
                  ? "step-row"
                  : "bot-row"
            }`}
          >
            <div
              className={`bubble ${
                msg.role === "user"
                  ? "user-bubble"
                  : msg.role === "step"
                    ? "step-bubble"
                    : "bot-bubble"
              }`}
            >
              {msg.role === "step" && (
                <span className="step-tool">{msg.tool}</span>
              )}
              {msg.content}
            </div>
          </div>
        ))}

        {isLoading && <TypingIndicator />}

        <div ref={bottomRef} />
      </main>

      {/* ── Input bar ────────────────────────────────────── */}
      <footer className="input-bar">
        {isLoading ? (
          <div className="stop-row">
            <button className="stop-btn" onClick={stopGeneration}>
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="currentColor"
              >
                <rect x="4" y="4" width="16" height="16" rx="2" />
              </svg>
              Stop
            </button>
          </div>
        ) : (
          <form className="input-form" onSubmit={handleSubmit}>
            <textarea
              className="chat-input"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Message ZeptoBot..."
              rows={1}
              autoFocus
            />
            <button
              type="submit"
              className="send-btn"
              disabled={!input.trim()}
              aria-label="Send message"
            >
              <svg
                width="18"
                height="18"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <line x1="22" y1="2" x2="11" y2="13" />
                <polygon points="22 2 15 22 11 13 2 9 22 2" />
              </svg>
            </button>
          </form>
        )}
        <p className="input-hint">
          {isLoading ? "Processing..." : "Enter to send \u00b7 Shift+Enter for new line"}
        </p>
      </footer>
    </div>
  );
}

export default App;
