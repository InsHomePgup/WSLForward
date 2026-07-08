# WSLForward

> 图形化管理 WSL2 端口转发规则（`netsh portproxy`）的 Windows 桌面工具 —— 无需命令行，即可查看、添加、删除转发规则。

一款基于 Tauri + Vue 构建的轻量级 Windows 桌面应用，告别繁琐的 `netsh interface portproxy` 命令。

[English](README.en.md)

---

## 背景

WSL2 运行在独立的虚拟机中，拥有每次重启后都会变化的 IP 地址。要从局域网或其他设备访问 WSL2 内的服务，需要在 Windows 侧配置 `netsh portproxy` 转发规则——手动操作繁琐且容易出错。

WSLForward 提供图形界面，让你能够：
- 一览所有活跃的端口转发规则
- 无需命令行即可添加、删除规则
- 一键开放/关闭 Windows 防火墙，允许局域网设备访问转发端口
- 查看 WSL2 内实际处于监听状态的端口
- 检测并集成在 WSL2 中运行的 Docker 容器
- 非管理员运行时，一键切换为管理员重启

## 系统要求

- Windows 10 / 11
- WSL2
- **需以管理员身份运行**才能操作 portproxy 规则

## 安装

从 [Releases](https://github.com/InsHomePgup/WSLForward/releases) 页面下载最新版本：

- `WSLForward_x.x.x_x64-setup.exe` — 安装版（推荐）
- `WSLForward_x.x.x_x64.msi` — MSI 安装包

无需额外运行时，WebView2 在 Windows 10/11 上已预装。

## 使用方法

1. **以管理员身份运行**（右键 → "以管理员身份运行"），或在应用内点击 **以管理员运行** 按钮
2. 应用启动时自动检测 WSL2 IP 地址
3. **添加规则** — 填写监听地址、监听端口、连接地址、连接端口，点击添加
4. 点击规则行可选中，使用 **删除所选** 按钮批量删除
5. 在 **局域网** 列点击 **允许局域网** 可为该端口开放防火墙规则，供局域网内其他设备访问；点击 **禁止局域网** 可撤销
6. **诊断** 标签页展示 WSL 监听端口及调试日志
7. **Docker** 标签页列出运行中的容器，支持一键转发端口
8. 拖动规则表格与底部面板之间的分隔条可调整布局
9. 点击右上角语言按钮可在 **English** 和 **中文** 之间切换

## 从源码构建

**前置条件：** Rust（stable）、Node.js 24+、pnpm

```bash
git clone https://github.com/InsHomePgup/WSLForward.git
cd WSLForward
pnpm install --ignore-scripts
pnpm tauri build
```

构建产物输出至 `src-tauri/target/release/bundle/`。

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | Vue 3 + TypeScript + Vite |
| 样式 | UnoCSS（兼容 Tailwind） |
| 后端 | Rust（Tauri v2） |
| 打包 | Tauri bundler（NSIS / MSI） |
| CI | GitHub Actions（windows-latest） |
