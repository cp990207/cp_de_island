# Memo Pill

**English** | [中文](README.md)

A lightweight "Dynamic Island" style floating widget for Windows, inspired by Apple's Dynamic Island design. Built with [Dioxus](https://dioxuslabs.com/).

## Current Status

This project is in early development — a personal-use utility.

## Features

- **Three-state island**
  - Rests as a 48px circle at the top center of the screen
  - Smoothly expands into a 280px pill on hover, showing memo count and last-updated time
  - Click the pill to open the 400px memo panel; click again to collapse
  - Automatically shrinks back to the circle 1s after the cursor leaves
- **Faces & clock** — a CSS-drawn blinking face (8 expressions in random rotation, including heart eyes, sleepy breathing and dizzy) alternating with an HH:MM clock every 5s
- **Quick notes** — add / edit / delete short memos, Enter to save, persisted automatically
- **Smooth animation** — asymmetric spring expand/collapse curves, jelly bounce, cross-fading content
- **Flicker-free rendering** — the window has a fixed size and is never resized (avoiding WebView2 resize flashes); transparent areas are click-through and never block apps underneath

### Interactions

| Action | Result |
| --- | --- |
| Hover | Circle expands into the pill |
| Move cursor away | Shrinks back to the circle after 1s |
| Left click | Open / collapse the memo panel |
| Shift + left-drag | Move the window |
| Right click | Quit the app |

## Planned Features

- **Life Planning** — Daily/weekly planning overview accessible from the pill
- **LLM Token Plan Usage Tracker** — Monitor your AI model (e.g. GPT, Claude) token consumption and plan quotas in real time

## Tech Stack

- **Language**: Rust
- **UI Framework**: Dioxus 0.7.9 (desktop mode)
- **Async timers**: tokio
- **Local time**: time
- **Win32 interaction**: windows-sys (click-through / hot-region hit testing)
- **Serialization**: serde + serde_json
- **ID Generation**: uuid v4

## Build & Run

```bash
cargo build
cargo run
```

> On Windows + Rust (windows-gnu) toolchain, make sure you have MinGW in your PATH and `libshlwapi.a` available. See `AGENTS.md` for details on common build issues.

## Data Storage

Memos are persisted as JSON to `%APPDATA%/MemoPill/memos.json`.

## License

[MIT](LICENSE)
