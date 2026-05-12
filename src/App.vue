<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

interface DockerMatch {
  name: string
  host_port: number
  container_port: number | null
  proto: string
}

interface PortProxyRule {
  listen_addr: string
  listen_port: string
  connect_addr: string
  connect_port: string
  win_open: boolean
  wsl_running: boolean
  docker_matches: DockerMatch[]
}

interface DockerPort {
  host_ip: string
  host_port: number
  container_port: number | null
  proto: string
}

interface DockerContainer {
  id: string
  name: string
  ports: DockerPort[]
}

interface AllData {
  rules: PortProxyRule[]
  wsl_ports: number[]
  docker_containers: DockerContainer[]
  errors: string[]
  debug_log: string[]
}

const rules = ref<PortProxyRule[]>([])
const wslPorts = ref<number[]>([])
const dockerContainers = ref<DockerContainer[]>([])
const dataErrors = ref<string[]>([])
const debugLog = ref<string[]>([])

const wslIp = ref('')
const adminStatus = ref(false)
const loading = ref(false)
const statusMsg = ref('Ready')
const statusError = ref(false)
const formError = ref('')

const listenAddr = ref('0.0.0.0')
const listenPort = ref('')
const connectAddr = ref('')
const connectPort = ref('')

function onListenPortChange() {
  if (!connectPort.value) {
    connectPort.value = listenPort.value
  }
}

const selectedRules = ref<Set<number>>(new Set())
const activeTab = ref<'diagnostics' | 'docker'>('diagnostics')
const selectedContainer = ref<DockerContainer | null>(null)
const autoRefresh = ref(false)
let autoTimer: ReturnType<typeof setInterval> | null = null

async function detectIp() {
  try {
    const ip = await invoke<string>('get_wsl_ip')
    wslIp.value = ip
    connectAddr.value = ip
  } catch {
    statusMsg.value = 'Could not detect WSL IP'
  }
}

async function refresh() {
  if (loading.value) return
  loading.value = true
  statusMsg.value = 'Refreshing…'
  try {
    const data = await invoke<AllData>('get_all_data')
    rules.value = data.rules
    wslPorts.value = data.wsl_ports
    dockerContainers.value = data.docker_containers
    dataErrors.value = data.errors
    debugLog.value = data.debug_log

    if (selectedContainer.value) {
      selectedContainer.value =
        data.docker_containers.find(c => c.id === selectedContainer.value!.id) ?? null
    }

    statusMsg.value = `Updated ${new Date().toLocaleTimeString()}`
    statusError.value = false
  } catch (e) {
    statusMsg.value = `Refresh failed: ${e}`
  } finally {
    loading.value = false
  }
}

async function addRule() {
  formError.value = ''
  if (!listenAddr.value || !listenPort.value || !connectAddr.value || !connectPort.value) {
    formError.value = 'All fields are required'
    return
  }
  const lp = Number(listenPort.value)
  const cp = Number(connectPort.value)
  if (!lp || !cp || lp < 1 || lp > 65535 || cp < 1 || cp > 65535) {
    formError.value = 'Ports must be between 1 and 65535'
    return
  }
  try {
    await invoke('add_rule', {
      la: listenAddr.value,
      lp: listenPort.value,
      ca: connectAddr.value,
      cp: connectPort.value,
    })
    listenPort.value = ''
    connectPort.value = ''
    connectAddr.value = wslIp.value
    statusError.value = false
    await refresh()
  } catch (e) {
    const msg = String(e)
    formError.value = msg
    statusMsg.value = `Add rule failed: ${msg}`
    statusError.value = true
  }
}

async function deleteSelected() {
  if (selectedRules.value.size === 0) return
  const toDelete = [...selectedRules.value].map(i => rules.value[i])
  for (const rule of toDelete) {
    try {
      await invoke('delete_rule', { la: rule.listen_addr, lp: rule.listen_port })
    } catch (e) {
      statusMsg.value = `Delete failed: ${e}`
    }
  }
  selectedRules.value = new Set()
  await refresh()
}

async function forwardPort(port: number) {
  if (!wslIp.value) {
    statusMsg.value = 'WSL IP not set'
    return
  }
  try {
    await invoke('forward_port', { hostPort: port, wslIp: wslIp.value })
    await refresh()
  } catch (e) {
    statusMsg.value = `Forward failed: ${e}`
  }
}

async function forwardAllPorts(container: DockerContainer) {
  for (const p of container.ports) {
    await forwardPort(p.host_port)
  }
}

function toggleRow(i: number) {
  const next = new Set(selectedRules.value)
  next.has(i) ? next.delete(i) : next.add(i)
  selectedRules.value = next
}

function toggleAutoRefresh() {
  if (autoRefresh.value) {
    autoTimer = setInterval(refresh, 30000)
  } else {
    if (autoTimer) clearInterval(autoTimer)
    autoTimer = null
  }
}

onMounted(async () => {
  adminStatus.value = await invoke<boolean>('is_admin')
  await detectIp()
  await refresh()
})

onUnmounted(() => {
  if (autoTimer) clearInterval(autoTimer)
})
</script>

<template>
  <div class="app">

    <header class="header">
      <span class="app-title">WSLForward</span>
      <div class="header-right">
        <span :class="['badge', adminStatus ? 'ok' : 'warn']">
          {{ adminStatus ? '● Administrator' : '⚠ Not Administrator' }}
        </span>
        <label class="check-label">
          <input type="checkbox" v-model="autoRefresh" @change="toggleAutoRefresh" />
          Auto 30s
        </label>
        <button class="btn" :disabled="loading" @click="refresh">
          {{ loading ? '…' : 'Refresh' }}
        </button>
      </div>
    </header>

    <div class="bar">
      <span class="label">WSL IP</span>
      <input v-model="wslIp" class="input-ip" placeholder="172.x.x.x" />
      <button class="btn-sm" @click="detectIp">Detect</button>
    </div>

    <div class="bar add-bar">
      <span class="label">Add Rule</span>
      <input v-model="listenAddr" class="input-addr" placeholder="Listen Addr" title="Listen Address" />
      <input v-model="listenPort" class="input-port" placeholder="Port" @input="onListenPortChange" @keyup.enter="addRule" />
      <span class="arrow">→</span>
      <input v-model="connectAddr" class="input-addr" placeholder="Connect Addr" title="Connect Address" />
      <input v-model="connectPort" class="input-port" placeholder="Port" @keyup.enter="addRule" />
      <button class="btn-sm" @click="addRule">Add</button>
      <span v-if="formError" class="form-error">{{ formError }}</span>
    </div>

    <div class="table-wrap">
      <table class="rules-table">
        <thead>
          <tr>
            <th class="th-check"></th>
            <th>Listen Addr</th>
            <th>Port</th>
            <th>Connect Addr</th>
            <th>Port</th>
            <th>Windows</th>
            <th>WSL</th>
            <th>Docker</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="(rule, i) in rules"
            :key="i"
            :class="{ 'row-selected': selectedRules.has(i) }"
            @click="toggleRow(i)"
          >
            <td class="th-check">
              <input type="checkbox" :checked="selectedRules.has(i)" @click.stop="toggleRow(i)" />
            </td>
            <td>{{ rule.listen_addr }}</td>
            <td class="port-cell">{{ rule.listen_port }}</td>
            <td>{{ rule.connect_addr }}</td>
            <td class="port-cell">{{ rule.connect_port }}</td>
            <td>
              <span :class="['badge', rule.win_open ? 'ok' : 'err']">
                {{ rule.win_open ? 'OPEN' : 'CLOSED' }}
              </span>
            </td>
            <td>
              <span :class="['badge', rule.wsl_running ? 'ok' : 'off']">
                {{ rule.wsl_running ? 'RUNNING' : 'NOT' }}
              </span>
            </td>
            <td>
              <span
                v-for="dm in rule.docker_matches"
                :key="dm.name"
                class="badge docker"
                :title="`${dm.host_port} → ${dm.container_port ?? '?'} / ${dm.proto}`"
              >
                {{ dm.name }}
              </span>
            </td>
          </tr>
          <tr v-if="rules.length === 0">
            <td colspan="8" class="empty-row">No portproxy rules — add one above</td>
          </tr>
        </tbody>
      </table>
      <div class="table-footer">
        <button
          class="btn-danger"
          :disabled="selectedRules.size === 0"
          @click="deleteSelected"
        >
          Delete Selected{{ selectedRules.size > 0 ? ` (${selectedRules.size})` : '' }}
        </button>
      </div>
    </div>

    <div class="panel">
      <div class="tab-bar">
        <button
          :class="['tab', { active: activeTab === 'diagnostics' }]"
          @click="activeTab = 'diagnostics'"
        >Diagnostics</button>
        <button
          :class="['tab', { active: activeTab === 'docker' }]"
          @click="activeTab = 'docker'"
        >Docker ({{ dockerContainers.length }})</button>
      </div>

      <div v-if="activeTab === 'diagnostics'" class="tab-body">
        <div class="diag-row">
          <strong>WSL Listening Ports:</strong>
          <span v-if="wslPorts.length === 0" class="muted">None detected</span>
          <span v-else class="port-list">{{ wslPorts.join(', ') }}</span>
        </div>
        <div v-if="dataErrors.length > 0" class="errors-block">
          <div v-for="e in dataErrors" :key="e" class="error-line">{{ e }}</div>
        </div>
        <details class="debug-block" v-if="debugLog.length > 0">
          <summary>Debug Log ({{ debugLog.length }} entries)</summary>
          <pre v-for="(line, i) in debugLog" :key="i" class="debug-line">{{ line }}</pre>
        </details>
      </div>

      <div v-if="activeTab === 'docker'" class="tab-body docker-layout">
        <div class="docker-sidebar">
          <div
            v-for="c in dockerContainers"
            :key="c.id"
            :class="['container-row', { active: selectedContainer?.id === c.id }]"
            @click="selectedContainer = c"
          >
            {{ c.name }}
          </div>
          <div v-if="dockerContainers.length === 0" class="muted">No containers running</div>
        </div>

        <div class="docker-detail" v-if="selectedContainer">
          <table class="inner-table">
            <thead>
              <tr>
                <th>Host Port</th>
                <th>Container Port</th>
                <th>Proto</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="p in selectedContainer.ports" :key="p.host_port">
                <td>{{ p.host_port }}</td>
                <td>{{ p.container_port ?? '—' }}</td>
                <td>{{ p.proto }}</td>
                <td>
                  <button class="btn-sm" @click="forwardPort(p.host_port)">Forward</button>
                </td>
              </tr>
            </tbody>
          </table>
          <button class="btn" @click="forwardAllPorts(selectedContainer)">Forward All</button>
        </div>
        <div v-else class="docker-detail muted">Select a container</div>
      </div>
    </div>

    <footer :class="['status-bar', { 'status-err': statusError }]">{{ statusMsg }}</footer>
  </div>
</template>

<style>
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

:root {
  --bg: #f4f4f5;
  --surface: #ffffff;
  --border: #d4d4d8;
  --text: #18181b;
  --muted: #71717a;
  --accent: #2563eb;
  --ok: #16a34a;
  --err: #dc2626;
  --warn: #d97706;
  --off: #a1a1aa;
  --docker-bg: #1d4ed8;
  --row-hover: #f1f5f9;
  --row-sel: #dbeafe;
  font-family: 'Segoe UI', system-ui, sans-serif;
  font-size: 13px;
  color: var(--text);
  background: var(--bg);
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #18181b;
    --surface: #27272a;
    --border: #3f3f46;
    --text: #f4f4f5;
    --muted: #71717a;
    --row-hover: #3f3f46;
    --row-sel: #1e3a5f;
  }
}

.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
}

.header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.app-title { font-size: 15px; font-weight: 600; }
.header-right { display: flex; align-items: center; gap: 8px; }

.bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  flex-wrap: wrap;
}
.add-bar { background: var(--bg); }
.label { font-weight: 500; min-width: 60px; color: var(--muted); }
.arrow { color: var(--muted); padding: 0 2px; }

.input-ip  { width: 140px; }
.input-addr { width: 140px; }
.input-port { width: 60px; }

input[type="text"], input:not([type="checkbox"]) {
  height: 26px;
  padding: 0 6px;
  border: 1px solid var(--border);
  border-radius: 4px;
  background: var(--surface);
  color: var(--text);
  font-size: 12px;
  outline: none;
}
input:focus { border-color: var(--accent); }

.form-error { color: var(--err); font-size: 11px; }

.btn, .btn-sm, .btn-danger {
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
  background: var(--surface);
  color: var(--text);
  white-space: nowrap;
}
.btn:hover, .btn-sm:hover { border-color: var(--accent); color: var(--accent); }
.btn:disabled, .btn-sm:disabled, .btn-danger:disabled {
  opacity: 0.4; cursor: default;
}
.btn-danger { color: var(--err); border-color: var(--err); }
.btn-danger:hover:not(:disabled) { background: var(--err); color: #fff; }
.check-label { display: flex; align-items: center; gap: 4px; font-size: 12px; cursor: pointer; }

.table-wrap {
  flex-shrink: 0;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  overflow-y: auto;
  max-height: 40vh;
}
.rules-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.rules-table th {
  text-align: left;
  padding: 6px 8px;
  background: var(--bg);
  border-bottom: 1px solid var(--border);
  font-weight: 600;
  position: sticky;
  top: 0;
  z-index: 1;
}
.rules-table td { padding: 5px 8px; border-bottom: 1px solid var(--border); }
.rules-table tbody tr:hover { background: var(--row-hover); cursor: pointer; }
.row-selected td { background: var(--row-sel) !important; }
.th-check { width: 28px; }
.port-cell { font-variant-numeric: tabular-nums; }
.docker-col { max-width: 200px; }
.empty-row { text-align: center; color: var(--muted); padding: 16px; }

.table-footer {
  padding: 6px 10px;
  border-top: 1px solid var(--border);
}

.badge {
  display: inline-block;
  padding: 1px 6px;
  border-radius: 3px;
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.03em;
  margin-right: 2px;
}
.badge.ok     { background: #dcfce7; color: var(--ok); }
.badge.err    { background: #fee2e2; color: var(--err); }
.badge.warn   { background: #fef3c7; color: var(--warn); }
.badge.off    { background: #f4f4f5; color: var(--off); }
.badge.docker { background: #dbeafe; color: var(--docker-bg); cursor: default; }

@media (prefers-color-scheme: dark) {
  .badge.ok     { background: #14532d; color: #86efac; }
  .badge.err    { background: #7f1d1d; color: #fca5a5; }
  .badge.warn   { background: #78350f; color: #fcd34d; }
  .badge.off    { background: #3f3f46; color: var(--off); }
  .badge.docker { background: #1e3a5f; color: #93c5fd; }
}

.panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--surface);
}
.tab-bar {
  display: flex;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.tab {
  padding: 6px 14px;
  border: none;
  background: none;
  cursor: pointer;
  font-size: 12px;
  color: var(--muted);
  border-bottom: 2px solid transparent;
}
.tab.active { color: var(--accent); border-bottom-color: var(--accent); }
.tab:hover { color: var(--text); }

.tab-body { flex: 1; overflow-y: auto; padding: 10px 12px; }

.diag-row { display: flex; gap: 8px; align-items: baseline; flex-wrap: wrap; margin-bottom: 8px; }
.port-list { color: var(--muted); font-variant-numeric: tabular-nums; }
.errors-block { margin-top: 6px; }
.error-line {
  color: var(--err);
  font-size: 11px;
  padding: 2px 0;
}
.muted { color: var(--muted); font-size: 12px; }

.docker-layout { display: flex; gap: 0; padding: 0; overflow: hidden; }
.docker-sidebar {
  width: 180px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  overflow-y: auto;
  padding: 4px;
}
.container-row {
  padding: 5px 8px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.container-row:hover { background: var(--row-hover); }
.container-row.active { background: var(--row-sel); color: var(--accent); }
.docker-detail {
  flex: 1;
  padding: 10px 12px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.inner-table { width: 100%; border-collapse: collapse; font-size: 12px; }
.inner-table th {
  text-align: left;
  padding: 4px 8px;
  border-bottom: 1px solid var(--border);
  font-weight: 600;
  background: var(--bg);
}
.inner-table td { padding: 4px 8px; border-bottom: 1px solid var(--border); }

.status-bar {
  padding: 4px 12px;
  font-size: 11px;
  color: var(--muted);
  background: var(--bg);
  border-top: 1px solid var(--border);
  flex-shrink: 0;
}
.status-bar.status-err {
  color: var(--err);
  background: #fee2e2;
}
@media (prefers-color-scheme: dark) {
  .status-bar.status-err { background: #7f1d1d; }
}
.debug-block {
  margin-top: 10px;
  font-size: 11px;
}
.debug-block summary {
  cursor: pointer;
  color: var(--muted);
  margin-bottom: 4px;
}
.debug-line {
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 3px;
  padding: 4px 8px;
  margin-bottom: 4px;
  white-space: pre-wrap;
  word-break: break-all;
  font-family: monospace;
  font-size: 11px;
  color: var(--text);
}
</style>
