import { ref, computed } from 'vue'

export type Locale = 'en' | 'zh-CN'

const en = {
  adminBadge: '● Administrator',
  notAdminBadge: '⚠ Not Administrator',
  runAsAdmin: 'Run as Admin',
  auto30s: 'Auto 30s',
  refresh: 'Refresh',
  loading: '…',
  wslIp: 'WSL IP',
  detect: 'Detect',
  addRule: 'Add Rule',
  listenAddrPlaceholder: 'Listen Addr',
  portPlaceholder: 'Port',
  connectAddrPlaceholder: 'Connect Addr',
  add: 'Add',
  colListenAddr: 'Listen Addr',
  colPort: 'Port',
  colConnectAddr: 'Connect Addr',
  colWindows: 'Windows',
  colWsl: 'WSL',
  colDocker: 'Docker',
  noRules: 'No portproxy rules — add one above',
  statusOpen: 'OPEN',
  statusClosed: 'CLOSED',
  statusRunning: 'RUNNING',
  statusNot: 'NOT',
  deleteSelected: 'Delete Selected',
  tabDiagnostics: 'Diagnostics',
  tabDocker: 'Docker',
  tabConsole: 'Console',
  clearConsole: '✕ Clear',
  wslListeningPorts: 'WSL Listening Ports:',
  noneDetected: 'None detected',
  debugLog: 'Debug Log',
  noContainers: 'No containers running',
  selectContainer: 'Select a container',
  colHostPort: 'Host Port',
  colContainerPort: 'Container Port',
  colProto: 'Proto',
  forward: 'Forward',
  forwardAll: 'Forward All',
  noLogs: 'No logs yet.',
  statusReady: 'Ready',
  statusRefreshing: 'Refreshing…',
  updatedAt: 'Updated',
  wslIpFailed: 'Could not detect WSL IP',
  wslIpNotSet: 'WSL IP not set',
  refreshFailed: 'Refresh failed',
  addFailed: 'Add rule failed',
  deleteFailed: 'Delete failed',
  forwardFailed: 'Forward failed',
  missing: 'Missing',
  fieldListenAddr: 'Listen Addr',
  fieldListenPort: 'Listen Port',
  fieldConnectAddr: 'Connect Addr (WSL IP not detected — click Detect)',
  fieldConnectPort: 'Connect Port',
  portRangeError: 'Ports must be between 1 and 65535',
}

const zhCN: typeof en = {
  adminBadge: '● 管理员',
  notAdminBadge: '⚠ 非管理员',
  runAsAdmin: '以管理员运行',
  auto30s: '自动刷新 30s',
  refresh: '刷新',
  loading: '…',
  wslIp: 'WSL IP',
  detect: '检测',
  addRule: '添加规则',
  listenAddrPlaceholder: '监听地址',
  portPlaceholder: '端口',
  connectAddrPlaceholder: '连接地址',
  add: '添加',
  colListenAddr: '监听地址',
  colPort: '端口',
  colConnectAddr: '连接地址',
  colWindows: 'Windows',
  colWsl: 'WSL',
  colDocker: 'Docker',
  noRules: '暂无转发规则，请在上方添加',
  statusOpen: '开放',
  statusClosed: '关闭',
  statusRunning: '运行中',
  statusNot: '未运行',
  deleteSelected: '删除所选',
  tabDiagnostics: '诊断',
  tabDocker: 'Docker',
  tabConsole: '控制台',
  clearConsole: '✕ 清除',
  wslListeningPorts: 'WSL 监听端口：',
  noneDetected: '未检测到',
  debugLog: '调试日志',
  noContainers: '无运行中的容器',
  selectContainer: '请选择容器',
  colHostPort: '主机端口',
  colContainerPort: '容器端口',
  colProto: '协议',
  forward: '转发',
  forwardAll: '全部转发',
  noLogs: '暂无日志',
  statusReady: '就绪',
  statusRefreshing: '刷新中…',
  updatedAt: '已更新',
  wslIpFailed: '无法检测 WSL IP',
  wslIpNotSet: 'WSL IP 未设置',
  refreshFailed: '刷新失败',
  addFailed: '添加规则失败',
  deleteFailed: '删除失败',
  forwardFailed: '转发失败',
  missing: '缺少',
  fieldListenAddr: '监听地址',
  fieldListenPort: '监听端口',
  fieldConnectAddr: '连接地址（未检测到 WSL IP，请点击检测）',
  fieldConnectPort: '连接端口',
  portRangeError: '端口号必须在 1 到 65535 之间',
}

const messages = { en, 'zh-CN': zhCN }

function detectLocale(): Locale {
  const stored = localStorage.getItem('wslforward-locale') as Locale | null
  if (stored && stored in messages) return stored
  return navigator.language.startsWith('zh') ? 'zh-CN' : 'en'
}

const locale = ref<Locale>(detectLocale())

export function useI18n() {
  const t = computed(() => messages[locale.value])
  function setLocale(l: Locale) {
    locale.value = l
    localStorage.setItem('wslforward-locale', l)
  }
  return { t, locale, setLocale }
}
