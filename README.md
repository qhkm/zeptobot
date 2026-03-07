# ZeptoBot

> **Experimental** — This project is in early development. APIs, architecture, and features may change significantly between commits. Not ready for production use.

Voice-controlled AI desktop assistant. Uses LLM reasoning to drive real desktop automation — clicking buttons, filling forms, navigating apps, and browsing the web.

## Stack

| Layer | Technology |
|-------|-----------|
| Framework | Tauri 2.0 |
| Frontend | React 19 + TypeScript + Vite |
| AI Brain | [ZeptoClaw](https://github.com/qhkm/zeptoclaw) (Rust agent runtime) |
| Desktop Automation | autopilot-rs + macOS Accessibility API |
| Browser Automation | Chrome extension bridge / CDP / agent-browser |
| Vision | GPT-4o (screenshot analysis) |

## What It Can Do (So Far)

- Chat with an AI agent that has access to desktop tools
- Open, activate, and control macOS apps (open_app, activate_app, AppleScript)
- Inspect and interact with any app's UI via Accessibility API (find, click, set value, read)
- Automate Chrome — navigate, click, type, read pages, list elements, execute JS, wait for content
- Take screenshots and understand what's on screen via GPT-4o vision
- Mouse/keyboard control (move, click, type, key combos)
- Open URLs in any browser

## Roadmap

- [x] Phase 0: Project scaffold (Tauri + React + autopilot-rs)
- [x] Phase 1: Text-based agent with ZeptoClaw brain
- [x] Phase 2: AX tools, browser automation, screenshot + vision
- [ ] Phase 3: Voice input (cpal + whisper-rs)
- [ ] Phase 4: Wake word + TTS (Porcupine + speech synthesis)
- [ ] Phase 5: Automation builder (visual workflow editor)
- [ ] Phase 6: Polish (auto-update, onboarding, distribution)

## TODO

### Near-term
- [ ] Persist conversation history across app restarts
- [ ] Add settings UI for API keys (currently env vars only)
- [ ] Improve AX tree performance for large apps (lazy loading, caching)
- [ ] Add keyboard shortcut support (hotkey to activate, global trigger)
- [ ] Error recovery — retry failed tool calls, better error messages to agent

### Browser
- [ ] Auto-detect and connect to Chrome extension on startup
- [ ] Tab management (switch tabs, open/close)
- [ ] Form filling workflows (multi-step)
- [ ] Cookie/session awareness for the agent

### Voice (Phase 3)
- [ ] Audio capture with cpal
- [ ] On-device STT with whisper-rs (MLX backend on Apple Silicon)
- [ ] Streaming transcription for real-time response
- [ ] Push-to-talk and continuous listening modes

### Agent Intelligence
- [ ] Multi-step planning with tool chaining
- [ ] Memory — remember user preferences and past tasks
- [ ] Task templates (e.g., "send WhatsApp to X saying Y")
- [ ] Safety guardrails — confirmation before destructive actions

### Polish
- [ ] System tray status indicator (tray exists, needs dynamic state)
- [ ] Onboarding flow (permissions, API key setup)
- [ ] Auto-update via Sparkle
- [ ] DMG packaging and notarization

## Development

```bash
# Prerequisites: Rust, Node.js, pnpm

# Install dependencies
pnpm install

# Run in dev mode (frontend + Rust hot-reload)
pnpm tauri dev

# Build release
pnpm tauri build
```

### Environment Variables

```bash
export ANTHROPIC_API_KEY="sk-..."   # or
export OPENAI_API_KEY="sk-..."      # for agent + vision
```

### Chrome Extension (Optional)

For browser automation with your existing Chrome (preserves logins):

1. Open `chrome://extensions`
2. Enable "Developer mode"
3. Click "Load unpacked" and select the `extension/` folder
4. The extension connects to ZeptoBot via WebSocket on port 3847

## License

MIT
