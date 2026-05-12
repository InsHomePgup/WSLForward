# TODO

## 1. 多语言支持

- 支持语言：英语（en）、简体中文（zh-CN）
- 界面右上角添加语言切换按钮（点击在 EN ↔ 中文 之间切换）
- 所有 UI 文本提取为语言包（Header、标签、按钮、提示、错误信息等）
- 默认根据系统语言自动选择，支持手动切换并持久化到 localStorage

### 实现方案

**新建 `src/i18n/index.ts`**
- 模块级单例 `locale` ref，读取 `localStorage('wslforward-locale')`，fallback 用 `navigator.language`
- 导出 `useI18n()` → `{ t, locale, setLocale }`，`t` 是 `ComputedRef<Messages>`，包含 en / zh-CN 两套字符串

**修改 `src/App.vue`**
- 顶部引入并调用 `useI18n()`
- 所有模板硬编码文字替换为 `t.xxx`
- 脚本内状态消息（`statusMsg.value = ...`）替换为 `t.value.xxx`
- Header 添加语言切换按钮

---

## 2. 管理员权限提升按钮

- 当检测到当前不是管理员时，在 Header 的 `⚠ Not Administrator` 徽标旁边显示一个 `以管理员运行` 按钮
- 点击后通过 PowerShell `Start-Process -FilePath '...' -Verb RunAs` 重新以 elevated 权限启动
- 新进程启动后当前实例自动调用 `app.exit(0)` 退出
- 管理员状态下按钮隐藏

### 实现方案

**修改 `src-tauri/src/lib.rs`**
- 新增 Tauri 命令 `restart_as_admin(app: AppHandle)`：
  ```rust
  let exe = std::env::current_exe()?;
  Command::new("powershell")
      .args(["-WindowStyle", "Hidden", "-Command",
             &format!("Start-Process -FilePath '{}' -Verb RunAs", exe)])
      .spawn()?;
  app.exit(0);
  ```
- 注册到 `generate_handler![]`

**修改 `src/App.vue`**
- Header 中 `v-if="!adminStatus"` 显示按钮（amber 色边框样式）
- 点击调用 `invoke('restart_as_admin')`
