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
  if (!connectPort.value) connectPort.value = listenPort.value
}

const selectedRules = ref<Set<number>>(new Set())
const activeTab = ref<'diagnostics' | 'docker' | 'console'>('diagnostics')
const selectedContainer = ref<DockerContainer | null>(null)
const autoRefresh = ref(false)
let autoTimer: ReturnType<typeof setInterval> | null = null

interface ConsoleEntry { time: string; level: 'info' | 'error' | 'warn'; msg: string }
const consoleLogs = ref<ConsoleEntry[]>([])

function clog(msg: string, level: ConsoleEntry['level'] = 'info') {
  consoleLogs.value.push({ time: new Date().toLocaleTimeString(), level, msg })
}

async function detectIp() {
  clog('Detecting WSL IP…')
  try {
    const ip = await invoke<string>('get_wsl_ip')
    wslIp.value = ip
    connectAddr.value = ip
    clog(`WSL IP detected: ${ip}`)
  } catch (e) {
    statusMsg.value = 'Could not detect WSL IP'
    clog(`WSL IP detection failed: ${e}`, 'error')
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
    if (selectedContainer.value)
      selectedContainer.value = data.docker_containers.find(c => c.id === selectedContainer.value!.id) ?? null
    data.errors.forEach(e => clog(e, 'error'))
    data.debug_log.forEach(l => clog(l))
    clog(`Refresh OK — ${data.rules.length} rules, ${data.wsl_ports.length} WSL ports, ${data.docker_containers.length} containers`)
    statusMsg.value = `Updated ${new Date().toLocaleTimeString()}`
    statusError.value = false
  } catch (e) {
    statusMsg.value = `Refresh failed: ${e}`
    clog(`Refresh failed: ${e}`, 'error')
  } finally {
    loading.value = false
  }
}

async function addRule() {
  formError.value = ''
  statusError.value = false
  const missing: string[] = []
  if (!listenAddr.value) missing.push('Listen Addr')
  if (!listenPort.value) missing.push('Listen Port')
  if (!connectAddr.value) missing.push('Connect Addr (WSL IP not detected — click Detect)')
  if (!connectPort.value) missing.push('Connect Port')
  if (missing.length > 0) {
    const msg = `Missing: ${missing.join(', ')}`
    formError.value = msg; statusMsg.value = msg; statusError.value = true
    clog(`Add rule validation failed: ${msg}`, 'warn')
    return
  }
  const lp = Number(listenPort.value), cp = Number(connectPort.value)
  if (!lp || !cp || lp < 1 || lp > 65535 || cp < 1 || cp > 65535) {
    const msg = 'Ports must be between 1 and 65535'
    formError.value = msg; statusMsg.value = msg; statusError.value = true
    clog(`Add rule validation failed: ${msg}`, 'warn')
    return
  }
  clog(`Adding rule: ${listenAddr.value}:${listenPort.value} → ${connectAddr.value}:${connectPort.value}`)
  try {
    await invoke('add_rule', { la: listenAddr.value, lp: listenPort.value, ca: connectAddr.value, cp: connectPort.value })
    clog('Rule added successfully')
    listenPort.value = ''; connectPort.value = ''; connectAddr.value = wslIp.value
    statusError.value = false
    await refresh()
  } catch (e) {
    const msg = String(e)
    formError.value = msg; statusMsg.value = `Add rule failed: ${msg}`; statusError.value = true
    clog(`Add rule failed: ${msg}`, 'error')
  }
}

async function deleteSelected() {
  if (selectedRules.value.size === 0) return
  const toDelete = [...selectedRules.value].map(i => rules.value[i])
  for (const rule of toDelete) {
    clog(`Deleting rule: ${rule.listen_addr}:${rule.listen_port}`)
    try {
      await invoke('delete_rule', { la: rule.listen_addr, lp: rule.listen_port })
      clog(`Deleted: ${rule.listen_addr}:${rule.listen_port}`)
    } catch (e) {
      statusMsg.value = `Delete failed: ${e}`
      clog(`Delete failed: ${e}`, 'error')
    }
  }
  selectedRules.value = new Set()
  await refresh()
}

async function forwardPort(port: number) {
  if (!wslIp.value) { statusMsg.value = 'WSL IP not set'; clog('Forward failed: WSL IP not set', 'warn'); return }
  clog(`Forwarding port ${port} → ${wslIp.value}:${port}`)
  try {
    await invoke('forward_port', { hostPort: port, wslIp: wslIp.value })
    clog(`Port ${port} forwarded`)
    await refresh()
  } catch (e) {
    statusMsg.value = `Forward failed: ${e}`
    clog(`Forward port ${port} failed: ${e}`, 'error')
  }
}

async function forwardAllPorts(container: DockerContainer) {
  for (const p of container.ports) await forwardPort(p.host_port)
}

function toggleRow(i: number) {
  const next = new Set(selectedRules.value)
  next.has(i) ? next.delete(i) : next.add(i)
  selectedRules.value = next
}

function toggleAutoRefresh() {
  if (autoRefresh.value) { autoTimer = setInterval(refresh, 30000) }
  else { if (autoTimer) clearInterval(autoTimer); autoTimer = null }
}

const errorCount = () => consoleLogs.value.filter(l => l.level === 'error').length

onMounted(async () => {
  adminStatus.value = await invoke<boolean>('is_admin')
  await detectIp()
  await refresh()
})
onUnmounted(() => { if (autoTimer) clearInterval(autoTimer) })
</script>

<template>
  <div class="flex flex-col h-screen overflow-hidden bg-zinc-100 dark:bg-zinc-900 text-zinc-900 dark:text-zinc-100 text-[13px] font-[Segoe_UI,system-ui,sans-serif]">

    <!-- Header -->
    <header class="flex items-center justify-between px-3 py-2 bg-white dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700 shrink-0">
      <span class="font-semibold text-[15px]">WSLForward</span>
      <div class="flex items-center gap-2">
        <span :class="adminStatus ? 'bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300' : 'bg-amber-100 text-amber-700 dark:bg-amber-900 dark:text-amber-300'"
          class="px-2 py-0.5 rounded text-[11px] font-semibold">
          {{ adminStatus ? '● Administrator' : '⚠ Not Administrator' }}
        </span>
        <label class="flex items-center gap-1 text-[12px] cursor-pointer">
          <input type="checkbox" v-model="autoRefresh" @change="toggleAutoRefresh" />
          Auto 30s
        </label>
        <button :disabled="loading" @click="refresh"
          class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 hover:border-blue-500 hover:text-blue-500 disabled:opacity-40 cursor-pointer">
          {{ loading ? '…' : 'Refresh' }}
        </button>
      </div>
    </header>

    <!-- WSL IP -->
    <div class="flex items-center gap-2 px-3 py-1.5 bg-white dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700 shrink-0 flex-wrap">
      <span class="text-[12px] text-zinc-500 min-w-[60px] font-medium">WSL IP</span>
      <input v-model="wslIp" placeholder="172.x.x.x"
        class="h-[26px] px-2 w-36 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 outline-none focus:border-blue-500" />
      <button @click="detectIp"
        class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 hover:border-blue-500 hover:text-blue-500 cursor-pointer">
        Detect
      </button>
    </div>

    <!-- Add Rule -->
    <div class="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-100 dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-700 shrink-0 flex-wrap">
      <span class="text-[12px] text-zinc-500 min-w-[60px] font-medium">Add Rule</span>
      <input v-model="listenAddr" placeholder="Listen Addr" title="Listen Address"
        class="h-[26px] px-2 w-32 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 outline-none focus:border-blue-500" />
      <input v-model="listenPort" placeholder="Port" @input="onListenPortChange" @keyup.enter="addRule"
        class="h-[26px] px-2 w-16 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 outline-none focus:border-blue-500" />
      <span class="text-zinc-400">→</span>
      <input v-model="connectAddr" placeholder="Connect Addr" title="Connect Address"
        class="h-[26px] px-2 w-32 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 outline-none focus:border-blue-500" />
      <input v-model="connectPort" placeholder="Port" @keyup.enter="addRule"
        class="h-[26px] px-2 w-16 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 outline-none focus:border-blue-500" />
      <button @click="addRule"
        class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 hover:border-blue-500 hover:text-blue-500 cursor-pointer">
        Add
      </button>
      <span v-if="formError" class="text-[11px] text-red-500">{{ formError }}</span>
    </div>

    <!-- Rules Table -->
    <div class="flex-1 min-h-[120px] overflow-y-auto bg-white dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
      <table class="w-full border-collapse text-[12px]">
        <thead>
          <tr class="bg-zinc-100 dark:bg-zinc-900 sticky top-0 z-10">
            <th class="w-7 px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700"></th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">Listen Addr</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">Port</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">Connect Addr</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">Port</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">Windows</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">WSL</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">Docker</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(rule, i) in rules" :key="i"
            :class="selectedRules.has(i) ? 'bg-blue-50 dark:bg-blue-950' : 'hover:bg-zinc-50 dark:hover:bg-zinc-700'"
            class="cursor-pointer border-b border-zinc-100 dark:border-zinc-700"
            @click="toggleRow(i)">
            <td class="px-2 py-1.5 w-7">
              <input type="checkbox" :checked="selectedRules.has(i)" @click.stop="toggleRow(i)" />
            </td>
            <td class="px-2 py-1.5">{{ rule.listen_addr }}</td>
            <td class="px-2 py-1.5 tabular-nums">{{ rule.listen_port }}</td>
            <td class="px-2 py-1.5">{{ rule.connect_addr }}</td>
            <td class="px-2 py-1.5 tabular-nums">{{ rule.connect_port }}</td>
            <td class="px-2 py-1.5">
              <span :class="rule.win_open ? 'bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300' : 'bg-red-100 text-red-700 dark:bg-red-900 dark:text-red-300'"
                class="px-1.5 py-0.5 rounded text-[10px] font-semibold">
                {{ rule.win_open ? 'OPEN' : 'CLOSED' }}
              </span>
            </td>
            <td class="px-2 py-1.5">
              <span :class="rule.wsl_running ? 'bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300' : 'bg-zinc-100 text-zinc-500 dark:bg-zinc-700 dark:text-zinc-400'"
                class="px-1.5 py-0.5 rounded text-[10px] font-semibold">
                {{ rule.wsl_running ? 'RUNNING' : 'NOT' }}
              </span>
            </td>
            <td class="px-2 py-1.5">
              <span v-for="dm in rule.docker_matches" :key="dm.name"
                class="px-1.5 py-0.5 rounded text-[10px] font-semibold bg-blue-100 text-blue-700 dark:bg-blue-900 dark:text-blue-300 mr-1"
                :title="`${dm.host_port} → ${dm.container_port ?? '?'} / ${dm.proto}`">
                {{ dm.name }}
              </span>
            </td>
          </tr>
          <tr v-if="rules.length === 0">
            <td colspan="8" class="px-3 py-4 text-center text-zinc-400">No portproxy rules — add one above</td>
          </tr>
        </tbody>
      </table>
      <div class="px-2.5 py-1.5 border-t border-zinc-100 dark:border-zinc-700">
        <button :disabled="selectedRules.size === 0" @click="deleteSelected"
          class="h-[26px] px-3 text-[12px] border border-red-400 text-red-500 rounded hover:bg-red-500 hover:text-white disabled:opacity-30 disabled:cursor-default cursor-pointer">
          Delete Selected{{ selectedRules.size > 0 ? ` (${selectedRules.size})` : '' }}
        </button>
      </div>
    </div>

    <!-- Tabs -->
    <div class="flex-none h-[200px] flex flex-col bg-white dark:bg-zinc-800">
      <div class="flex border-b border-zinc-200 dark:border-zinc-700 shrink-0">
        <button v-for="tab in (['diagnostics', 'docker', 'console'] as const)" :key="tab"
          :class="activeTab === tab ? 'text-blue-500 border-b-2 border-blue-500' : 'text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200'"
          class="px-3.5 py-1.5 text-[12px] border-b-2 border-transparent -mb-px"
          @click="activeTab = tab">
          <span class="capitalize">{{ tab }}</span>
          <span v-if="tab === 'console' && errorCount() > 0"
            class="ml-1 px-1.5 py-0.5 rounded-full bg-red-500 text-white text-[10px]">
            {{ errorCount() }}
          </span>
        </button>
        <button v-if="activeTab === 'console'" @click="consoleLogs = []"
          class="ml-auto px-3 text-[11px] text-zinc-400 hover:text-zinc-600 cursor-pointer">✕ Clear</button>
      </div>

      <!-- Diagnostics -->
      <div v-if="activeTab === 'diagnostics'" class="flex-1 overflow-y-auto p-3 text-[12px]">
        <div class="flex gap-2 flex-wrap mb-2">
          <span class="font-semibold">WSL Listening Ports:</span>
          <span v-if="wslPorts.length === 0" class="text-zinc-400">None detected</span>
          <span v-else class="text-zinc-500 tabular-nums">{{ wslPorts.join(', ') }}</span>
        </div>
        <div v-for="e in dataErrors" :key="e" class="text-red-500 text-[11px] mb-1">{{ e }}</div>
        <details v-if="debugLog.length > 0" class="mt-2">
          <summary class="text-zinc-400 cursor-pointer text-[11px]">Debug Log ({{ debugLog.length }})</summary>
          <pre v-for="(l, i) in debugLog" :key="i"
            class="mt-1 p-2 bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-700 rounded text-[11px] whitespace-pre-wrap break-all font-mono">{{ l }}</pre>
        </details>
      </div>

      <!-- Docker -->
      <div v-if="activeTab === 'docker'" class="flex-1 flex overflow-hidden">
        <div class="w-44 shrink-0 border-r border-zinc-200 dark:border-zinc-700 overflow-y-auto p-1">
          <div v-for="c in dockerContainers" :key="c.id"
            :class="selectedContainer?.id === c.id ? 'bg-blue-50 dark:bg-blue-950 text-blue-600' : 'hover:bg-zinc-50 dark:hover:bg-zinc-700'"
            class="px-2 py-1.5 rounded text-[12px] truncate cursor-pointer"
            @click="selectedContainer = c">
            {{ c.name }}
          </div>
          <div v-if="dockerContainers.length === 0" class="px-2 py-2 text-zinc-400 text-[12px]">No containers running</div>
        </div>
        <div v-if="selectedContainer" class="flex-1 flex flex-col gap-2 p-3 overflow-y-auto">
          <table class="w-full border-collapse text-[12px]">
            <thead>
              <tr class="bg-zinc-50 dark:bg-zinc-900">
                <th class="px-2 py-1 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">Host Port</th>
                <th class="px-2 py-1 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">Container Port</th>
                <th class="px-2 py-1 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">Proto</th>
                <th class="px-2 py-1 border-b border-zinc-200 dark:border-zinc-700"></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="p in selectedContainer.ports" :key="p.host_port" class="border-b border-zinc-100 dark:border-zinc-700">
                <td class="px-2 py-1 tabular-nums">{{ p.host_port }}</td>
                <td class="px-2 py-1 tabular-nums">{{ p.container_port ?? '—' }}</td>
                <td class="px-2 py-1">{{ p.proto }}</td>
                <td class="px-2 py-1">
                  <button @click="forwardPort(p.host_port)"
                    class="h-[22px] px-2 text-[11px] border border-zinc-300 dark:border-zinc-600 rounded hover:border-blue-500 hover:text-blue-500 cursor-pointer">
                    Forward
                  </button>
                </td>
              </tr>
            </tbody>
          </table>
          <button @click="forwardAllPorts(selectedContainer)"
            class="self-start h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded hover:border-blue-500 hover:text-blue-500 cursor-pointer">
            Forward All
          </button>
        </div>
        <div v-else class="flex-1 flex items-center justify-center text-zinc-400 text-[12px]">Select a container</div>
      </div>

      <!-- Console -->
      <div v-if="activeTab === 'console'" class="flex-1 overflow-y-auto p-2 font-mono text-[11px]">
        <div v-if="consoleLogs.length === 0" class="text-zinc-400 p-1">No logs yet.</div>
        <div v-for="(entry, i) in consoleLogs" :key="i"
          :class="entry.level === 'error' ? 'text-red-500' : entry.level === 'warn' ? 'text-amber-500' : 'text-zinc-600 dark:text-zinc-300'"
          class="flex gap-2 py-0.5 px-1 rounded hover:bg-zinc-50 dark:hover:bg-zinc-700">
          <span class="text-zinc-400 shrink-0">{{ entry.time }}</span>
          <span class="whitespace-pre-wrap break-all">{{ entry.msg }}</span>
        </div>
      </div>
    </div>

    <!-- Status bar -->
    <footer :class="statusError ? 'bg-red-50 dark:bg-red-950 text-red-500' : 'bg-zinc-100 dark:bg-zinc-900 text-zinc-400'"
      class="shrink-0 px-3 py-1 text-[11px] border-t border-zinc-200 dark:border-zinc-700">
      {{ statusMsg }}
    </footer>
  </div>
</template>
