# Memo Pill

**English** | [中文](README.md)

A lightweight "Dynamic Island" style floating widget for Windows, inspired by Apple's Dynamic Island design. Built with [Dioxus](https://dioxuslabs.com/).

## Current Status

This project is in early development — a personal-use utility.

## Planned Features

- **Quick Notes / Memos** — Capture short notes directly from the island widget
- **Life Planning** — Daily/weekly planning overview accessible from the pill
- **LLM Token Plan Usage Tracker** — Monitor your AI model (e.g. GPT, Claude) token consumption and plan quotas in real time

## Tech Stack

- **Language**: Rust
- **UI Framework**: Dioxus 0.7.9 (desktop mode)
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
