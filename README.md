# Memo Pill

[English](README-en.md) | **中文**

一个轻量级的 Windows "灵动岛"风格浮动组件，灵感来自 Apple 的 Dynamic Island 设计。使用 [Dioxus](https://dioxuslabs.com/) 构建。

## 项目状态

本项目处于早期开发阶段——一个面向个人使用的桌面工具。

## 规划功能

- **快速笔记** — 直接从灵动岛组件捕获简短笔记，随时记录灵感
- **生活规划** — 通过灵动岛访问每日/每周规划概览
- **大模型 Token 用量追踪** — 实时监控 AI 模型（如 GPT、Claude 等）的 Token 消耗与套餐配额

## 技术栈

- **语言**: Rust
- **UI 框架**: Dioxus 0.7.9（桌面模式）
- **序列化**: serde + serde_json
- **ID 生成**: uuid v4

## 构建与运行

```bash
cargo build
cargo run
```

> 在 Windows + Rust (windows-gnu) 工具链下，请确保 MinGW 已加入 PATH 并可用，同时 `libshlwapi.a` 文件可用。常见构建问题详见项目根目录的 `AGENTS.md`。

## 数据存储

笔记数据以 JSON 格式持久化存储于 `%APPDATA%/MemoPill/memos.json`。

## 许可证

[MIT](LICENSE)
