# Memo Pill

**English** | [中文](README.md)

A lightweight "Dynamic Island" style floating widget for Windows, inspired by Apple's Dynamic Island design. Built with [Dioxus](https://dioxuslabs.com/).

## Screenshots

| Task panel | Kimi usage monitor |
| --- | --- |
| ![Task panel](docs/screenshots/memo-panel.png) | ![Kimi usage monitor](docs/screenshots/kimi-usage.png) |

## Current Status

v1.0.0 stable release — a personal-use desktop utility. Everything planned for the first release (quick tasks, Coding Plan usage tracking) is now implemented.

## Features

- **Two-sided island**
  - Rests as a 48px circle at the top center of the screen
  - Hover the left side to expand the task pill (280px), showing the most urgent task's text (ellipsized when long, full text on hover); scroll the wheel to cycle through tasks, with All done / No tasks as the empty fallback
  - Hover the right side to expand the Coding Plan pill, rotating through each provider's balance / quota; scroll the wheel to cycle providers; the left↔right cross-switch shares the same spring animation as the circle expanding
  - Click a pill to open its panel (400px task panel / Coding Plan panel); click again to collapse
  - Automatically shrinks back to the circle 1s after the cursor leaves
- **Quick tasks** — press Enter to capture; every attribute is optional: due time (Today / Tomorrow / +7d / custom date & time) and a ⚑ three-level priority flag (Low / Medium / High, click to cycle); click the circle on a task to complete it — it strikes through and sinks into the Completed group, click again to restore; overdue tasks turn red, and 10 minutes before a due time the island pops open and flashes the task as a visual reminder; edit mode can change due and priority too; deletions can be undone within 6s, search appears with 5+ tasks, hover a row to see its full text
- **Coding Plan usage tracking** — supports Kimi / DeepSeek / MiniMax / GLM; enter an API key in the panel and the balance & quota usage is fetched automatically, with manual refresh available; Kimi additionally shows the plan name with 5-hour / weekly quota progress, a usage history bar chart (hover a bar for its cost and token totals), and estimates token cost from local session logs; keys are stored locally only
- **Faces & clock** — a CSS-drawn blinking face (8 expressions in random rotation, including heart eyes, sleepy breathing and dizzy) alternating with an HH:MM clock every 5s
- **CSS-drawn monochrome icons** — the sticky note and quota icons on the pill and the calendar due-date button are pure CSS shapes in the grayscale design language; the calendar follows the button's state colors (hover / due set)
- **Smooth animation** — asymmetric spring expand/collapse curves, jelly bounce, cross-fading content
- **Flicker-free rendering** — the window has a fixed size and is never resized (avoiding WebView2 resize flashes); transparent areas are click-through and never block apps underneath

### Interactions

| Action | Result |
| --- | --- |
| Hover the left / right side | Circle expands into the task pill / Coding Plan pill |
| Sweep the cursor across | One side shrinks while the other expands, springy cross-switch |
| Move cursor away | Shrinks back to the circle after 1s |
| Left click a pill | Open / collapse its panel (tasks / Coding Plan) |
| Mouse wheel on a pill | Cycle which task / provider the pill shows |
| Esc / click the margin outside the panel | Collapse the panel |
| Circle on a task | Complete / restore the task |
| Calendar / ⚑ buttons next to the input | Set due time / priority for the new task |
| Chips row while editing | Change that task's due time and priority |
| Coding Plan panel | Pick a provider, enter an API key, refresh balances |
| Shift + left-drag | Move the window (position persists across restarts) |
| Right click (on the circle / pill) | Quit the app |

## Download

Windows 10 / 11 (x64) users can grab `memo-pill-vX.Y.Z-windows-x86_64.zip` from [GitHub Releases](https://github.com/cp990207/cp_de_island/releases). Keep `memo-pill.exe` and `WebView2Loader.dll` in the same folder after extracting, then run `memo-pill.exe`.

- If SmartScreen blocks the first launch, choose "More info → Run anyway" (the exe is not code-signed).
- Requires the WebView2 Runtime (preinstalled on Windows 11 and recent Windows 10; otherwise install the [Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)).

## Tech Stack

- **Language**: Rust
- **UI Framework**: Dioxus 0.7.9 (desktop mode)
- **Async timers**: tokio
- **Local time**: time
- **HTTP**: reqwest
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

Tasks are persisted as JSON to `%APPDATA%/MemoPill/memos.json`. Writes use a temp file + atomic replace; a corrupt data file is backed up as `memos.corrupt-<timestamp>.json` instead of being overwritten. Data files from the older notes-only version load and upgrade automatically. The app runs as a single instance so concurrent processes cannot clobber each other's data. The window position (after a Shift+drag) is saved to `settings.json` in the same directory and restored on restart; if the saved spot no longer overlaps any monitor (e.g. the display layout changed), the window falls back to top center. Coding Plan API keys are saved to `providers.json` in the same directory, and Kimi quota history is kept in `kimi-quota-history.json`; everything stays on your machine.

## License

[MIT](LICENSE)
