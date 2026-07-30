# Memo Pill

**English** | [中文](README.md)

A lightweight "Dynamic Island" style floating widget for Windows, inspired by Apple's Dynamic Island design. Built with [Dioxus](https://dioxuslabs.com/).

## Current Status

This project is in early development — a personal-use utility.

## Features

- **Three-state island**
  - Rests as a 48px circle at the top center of the screen
  - Smoothly expands into a 280px pill on hover, showing the most urgent task's text (ellipsized when long, full text on hover); scroll the wheel to cycle through tasks, with All done / No tasks as the empty fallback
  - Click the pill to open the 400px task panel; click again to collapse
  - Automatically shrinks back to the circle 1s after the cursor leaves
- **Faces & clock** — a CSS-drawn blinking face (8 expressions in random rotation, including heart eyes, sleepy breathing and dizzy) alternating with an HH:MM clock every 5s
- **Quick tasks** — press Enter to capture; every attribute is optional: due time (Today / Tomorrow / +7d / custom date & time) and a ⚑ three-level priority flag (Low / Medium / High, click to cycle); click the circle on a task to complete it — it strikes through and sinks into the Completed group, click again to restore; overdue tasks turn red, and 10 minutes before a due time the island pops open and flashes the task as a visual reminder; edit mode can change due and priority too; deletions can be undone within 6s, search appears with 5+ tasks, hover a row to see its full text
- **CSS-drawn monochrome icons** — the sticky note on the pill and the calendar due-date button are pure CSS shapes in the grayscale design language; the calendar follows the button's state colors (hover / due set)
- **Smooth animation** — asymmetric spring expand/collapse curves, jelly bounce, cross-fading content
- **Flicker-free rendering** — the window has a fixed size and is never resized (avoiding WebView2 resize flashes); transparent areas are click-through and never block apps underneath

### Interactions

| Action | Result |
| --- | --- |
| Hover | Circle expands into the pill |
| Move cursor away | Shrinks back to the circle after 1s |
| Left click | Open / collapse the task panel |
| Mouse wheel on the pill | Cycle which task the pill shows |
| Esc / click the margin outside the panel | Collapse the task panel |
| Circle on a task | Complete / restore the task |
| Calendar / ⚑ buttons next to the input | Set due time / priority for the new task |
| Chips row while editing | Change that task's due time and priority |
| Shift + left-drag | Move the window (position persists across restarts) |
| Right click (on the circle / pill) | Quit the app |

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

Tasks are persisted as JSON to `%APPDATA%/MemoPill/memos.json`. Writes use a temp file + atomic replace; a corrupt data file is backed up as `memos.corrupt-<timestamp>.json` instead of being overwritten. Data files from the older notes-only version load and upgrade automatically. The app runs as a single instance so concurrent processes cannot clobber each other's data. The window position (after a Shift+drag) is saved to `settings.json` in the same directory and restored on restart; if the saved spot no longer overlaps any monitor (e.g. the display layout changed), the window falls back to top center.

## License

[MIT](LICENSE)
