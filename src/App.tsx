/**
 * ZeptoBot — chat interface
 *
 * Usage:
 *   The Rust side must expose a `send_message` command:
 *     #[tauri::command]
 *     async fn send_message(message: String) -> Result<String, String> { ... }
 */

import { useState, useRef, useEffect, KeyboardEvent, FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

interface ChatMessage {
  id: number;
  role: "user" | "assistant";
  content: string;
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
  const bottomRef = useRef<HTMLDivElement>(null);

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
      const errMsg: ChatMessage = {
        id: nextId++,
        role: "assistant",
        content:
          err instanceof Error
            ? err.message
            : "Something went wrong. Please try again.",
      };
      setMessages((prev) => [...prev, errMsg]);
      setIsConnected(false);
    } finally {
      setIsLoading(false);
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
            <p className="empty-hint">Type a message below to get started.</p>
          </div>
        )}

        {messages.map((msg) => (
          <div
            key={msg.id}
            className={`message-row ${msg.role === "user" ? "user-row" : "bot-row"}`}
          >
            <div
              className={`bubble ${msg.role === "user" ? "user-bubble" : "bot-bubble"}`}
            >
              {msg.content}
            </div>
          </div>
        ))}

        {isLoading && <TypingIndicator />}

        <div ref={bottomRef} />
      </main>

      {/* ── Input bar ────────────────────────────────────── */}
      <footer className="input-bar">
        <form className="input-form" onSubmit={handleSubmit}>
          <textarea
            className="chat-input"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Message ZeptoBot…"
            rows={1}
            disabled={isLoading}
            autoFocus
          />
          <button
            type="submit"
            className="send-btn"
            disabled={!input.trim() || isLoading}
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
        <p className="input-hint">Enter to send · Shift+Enter for new line</p>
      </footer>
    </div>
  );
}

export default App;
