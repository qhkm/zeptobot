# ZeptoBot

Voice-controlled AI desktop assistant. Tauri 2.0 + React frontend, ZeptoClaw brain, autopilot-rs automation.

## Quick Reference

```bash
# Dev (frontend + Rust backend hot-reload)
pnpm tauri dev

# Build release
pnpm tauri build

# Rust check only
cd src-tauri && cargo check

# Rust lint
cd src-tauri && cargo clippy -- -D warnings

# Rust format
cd src-tauri && cargo fmt

# TypeScript check
npx tsc --noEmit

# Full pre-push check
cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo fmt -- --check
```

## Architecture

```
src/                        # React frontend (TypeScript)
├── App.tsx                 # Chat UI (dark theme, message list, input bar)
├── App.css                 # Styles (CSS custom properties, dark theme)
├── main.tsx                # React entry point
└── index.html              # HTML shell

src-tauri/                  # Rust backend (Tauri 2.0)
├── Cargo.toml              # Dependencies: tauri, autopilot, tokio, serde
├── tauri.conf.json         # App config, tray icon, window settings
├── src/
│   ├── main.rs             # Binary entry → lib::run()
│   ├── lib.rs              # Tauri setup: tray icon, commands, plugins
│   ├── commands.rs          # Tauri IPC commands: send_message, get_status, execute_automation
│   └── services/
│       ├── mod.rs           # Service module declarations
│       ├── agent.rs         # ZeptoClaw integration (placeholder → will import crate)
│       └── automation.rs    # autopilot-rs wrapper (mouse, keyboard, screen)
```

## Stack

| Layer | Technology |
|-------|-----------|
| Framework | Tauri 2.0 |
| Frontend | React 19 + TypeScript + Vite |
| AI Brain | ZeptoClaw (crate dependency, planned) |
| Automation | autopilot-rs (mouse, keyboard, screen) |
| Audio | cpal (planned) |
| STT | whisper-rs (planned) |
| Wake Word | Porcupine (planned) |

## Tauri Commands (IPC)

| Command | Params | Returns | Description |
|---------|--------|---------|-------------|
| `send_message` | `message: String` | `String` | Send text to agent, get response |
| `get_status` | none | `BotStatus` | Check subsystem health |
| `execute_automation` | `action: String, params: JSON` | `String` | Run automation action |

## Automation Actions

| Action | Params | Description |
|--------|--------|-------------|
| `move_mouse` | `{ x: f64, y: f64 }` | Move cursor to coordinates |
| `click` | `{}` | Left-click at current position |
| `type` | `{ text: "string" }` | Type text via simulated keystrokes |
| `screen_size` | `{}` | Get screen dimensions |
| `mouse_position` | `{}` | Get current cursor position |

## Phases

- [x] Phase 0: Project scaffold (Tauri + React + autopilot-rs)
- [ ] Phase 1: Text-based agent (ZeptoClaw crate integration)
- [ ] Phase 2: Voice input (cpal + whisper-rs)
- [ ] Phase 3: Wake word + TTS (Porcupine + speech synthesis)
- [ ] Phase 4: Automation builder (visual workflow config)
- [ ] Phase 5: Polish (auto-update, onboarding, distribution)

## Design Decisions

1. **Tauri over Swift** — ZeptoClaw and autopilot-rs are Rust crates. Direct import, no FFI.
2. **autopilot-rs** — 421 stars, active, light footprint, modernized to objc2.
3. **ZeptoClaw as crate** — Tightest integration, shared memory, single binary.
4. **On-device first** — Wake word, STT, TTS all local. Only LLM calls go to cloud.

## Related

- [ZeptoClaw](https://github.com/qhkm/zeptoclaw) — AI agent runtime
- [Issue #1](https://github.com/qhkm/zeptobot/issues/1) — Full planning issue
