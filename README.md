# WSLForward

> GUI tool to manage `netsh portproxy` port forwarding rules for WSL2 on Windows — view, add, and delete rules without touching the command line.

A lightweight Windows desktop app (built with Tauri + Vue) for managing WSL2 port forwarding. No more manual `netsh interface portproxy` commands.

[中文文档](README.zh-CN.md)

---

## Why

WSL2 runs in a virtual machine with its own IP address that changes on every restart. To access WSL2 services from the local network or other devices, you need Windows `netsh portproxy` rules — and managing them manually is tedious.

WSLForward gives you a GUI to:
- View all active portproxy rules at a glance
- Add and delete rules without touching the command line
- See which WSL ports are actually listening
- Detect and integrate with Docker containers running inside WSL2
- Re-launch as Administrator with one click if needed

## Requirements

- Windows 10 / 11
- WSL2
- **Must be run as Administrator** to manage portproxy rules

## Installation

Download the latest release from the [Releases](https://github.com/InsHomePgup/WSLForward/releases) page:

- `WSLForward_x.x.x_x64-setup.exe` — installer (recommended)
- `WSLForward_x.x.x_x64.msi` — MSI package

No additional runtime required — WebView2 is pre-installed on Windows 10/11.

## Usage

1. **Run as Administrator** (right-click → "Run as administrator"), or click the **Run as Admin** button inside the app
2. The app auto-detects your WSL2 IP on startup
3. **Add Rule** — enter listen address, listen port, connect address, connect port, then click Add
4. Click any rule row to select it; use **Delete Selected** to remove
5. The **Diagnostics** tab shows WSL listening ports and debug output
6. The **Docker** tab lists running containers and lets you forward their ports in one click
7. Use the resize handle between the rules table and the bottom panel to adjust layout
8. Switch between **English** and **中文** with the language button in the header

## Build from Source

**Prerequisites:** Rust (stable), Node.js 24+, pnpm

```bash
git clone https://github.com/InsHomePgup/WSLForward.git
cd WSLForward
pnpm install --ignore-scripts
pnpm tauri build
```

The installer will be output to `src-tauri/target/release/bundle/`.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Frontend | Vue 3 + TypeScript + Vite |
| Styling | UnoCSS (Tailwind-compatible) |
| Backend | Rust (Tauri v2) |
| Packaging | Tauri bundler (NSIS / MSI) |
| CI | GitHub Actions (windows-latest) |
