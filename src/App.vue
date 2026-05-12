<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useI18n } from './i18n/index'

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

const { t, locale, setLocale } = useI18n()

const rules = ref<PortProxyRule[]>([])
const wslPorts = ref<number[]>([])
const dockerContainers = ref<DockerContainer[]>([])
const dataErrors = ref<string[]>([])
const debugLog = ref<string[]>([])

const wslIp = ref('')
const adminStatus = ref(false)
const loading = ref(false)
const statusMsg = ref(t.value.statusReady)
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

interface ConsoleEntry { time: string; level: 'info' | 'error' | 'warn'; msg: string }
const consoleLogs = ref<ConsoleEntry[]>([])

function clog(msg: string, level: ConsoleEntry['level'] = 'info') {
  consoleLogs.value.push({ time: new Date().toLocaleTimeString(), level, msg })
}

function tabLabel(tab: 'diagnostics' | 'docker' | 'console'): string {
  if (tab === 'diagnostics') return t.value.tabDiagnostics
  if (tab === 'docker') return t.value.tabDocker
  return t.value.tabConsole
}

async function detectIp() {
  clog('Detecting WSL IP…')
  try {
    const ip = await invoke<string>('get_wsl_ip')
    wslIp.value = ip
    connectAddr.value = ip
    clog(`WSL IP detected: ${ip}`)
  } catch (e) {
    statusMsg.value = t.value.wslIpFailed
    clog(`WSL IP detection failed: ${e}`, 'error')
  }
}

async function refresh() {
  if (loading.value) return
  loading.value = true
  statusMsg.value = t.value.statusRefreshing
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
    statusMsg.value = `${t.value.updatedAt} ${new Date().toLocaleTimeString()}`
    statusError.value = false
  } catch (e) {
    statusMsg.value = `${t.value.refreshFailed}: ${e}`
    clog(`Refresh failed: ${e}`, 'error')
  } finally {
    loading.value = false
  }
}

async function addRule() {
  formError.value = ''
  statusError.value = false
  const missing: string[] = []
  if (!listenAddr.value) missing.push(t.value.fieldListenAddr)
  if (!listenPort.value) missing.push(t.value.fieldListenPort)
  if (!connectAddr.value) missing.push(t.value.fieldConnectAddr)
  if (!connectPort.value) missing.push(t.value.fieldConnectPort)
  if (missing.length > 0) {
    const msg = `${t.value.missing}: ${missing.join(', ')}`
    formError.value = msg; statusMsg.value = msg; statusError.value = true
    clog(`Add rule validation failed: ${msg}`, 'warn')
    return
  }
  const lp = Number(listenPort.value), cp = Number(connectPort.value)
  if (!lp || !cp || lp < 1 || lp > 65535 || cp < 1 || cp > 65535) {
    const msg = t.value.portRangeError
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
    formError.value = msg; statusMsg.value = `${t.value.addFailed}: ${msg}`; statusError.value = true
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
      statusMsg.value = `${t.value.deleteFailed}: ${e}`
      clog(`Delete failed: ${e}`, 'error')
    }
  }
  selectedRules.value = new Set()
  await refresh()
}

async function forwardPort(port: number) {
  if (!wslIp.value) { statusMsg.value = t.value.wslIpNotSet; clog('Forward failed: WSL IP not set', 'warn'); return }
  clog(`Forwarding port ${port} → ${wslIp.value}:${port}`)
  try {
    await invoke('forward_port', { hostPort: port, wslIp: wslIp.value })
    clog(`Port ${port} forwarded`)
    await refresh()
  } catch (e) {
    statusMsg.value = `${t.value.forwardFailed}: ${e}`
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

async function restartAsAdmin() {
  clog('Requesting elevation…')
  try {
    await invoke('restart_as_admin')
  } catch (e) {
    statusMsg.value = String(e)
    clog(`Elevation failed: ${e}`, 'error')
  }
}

const errorCount = () => consoleLogs.value.filter(l => l.level === 'error').length

let unlistenPortProxy: (() => void) | null = null

function onKeyDown(e: KeyboardEvent) {
  if (e.ctrlKey && e.key === 'r') {
    e.preventDefault()
    refresh()
  }
}

const bottomH = ref(200)

function startResize(e: MouseEvent) {
  const startY = e.clientY
  const startH = bottomH.value
  const onMove = (ev: MouseEvent) => {
    bottomH.value = Math.max(80, Math.min(startH + (startY - ev.clientY), window.innerHeight - 180))
  }
  const onUp = () => {
    window.removeEventListener('mousemove', onMove)
    window.removeEventListener('mouseup', onUp)
    document.body.style.userSelect = ''
    document.body.style.cursor = ''
  }
  document.body.style.userSelect = 'none'
  document.body.style.cursor = 'ns-resize'
  window.addEventListener('mousemove', onMove)
  window.addEventListener('mouseup', onUp)
}

onMounted(async () => {
  adminStatus.value = await invoke<boolean>('is_admin')
  await detectIp()
  await refresh()
  unlistenPortProxy = await listen('portproxy-changed', () => refresh())
  window.addEventListener('keydown', onKeyDown)
})
onUnmounted(() => {
  unlistenPortProxy?.()
  window.removeEventListener('keydown', onKeyDown)
})
</script>

<template>
  <div class="flex flex-col h-screen overflow-hidden bg-zinc-100 dark:bg-zinc-900 text-zinc-900 dark:text-zinc-100 text-[13px] font-[Segoe_UI,system-ui,sans-serif]">

    <!-- Header -->
    <header class="flex items-center justify-between px-3 py-2 bg-white dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700 shrink-0">
      <span class="font-semibold text-[15px]">WSLForward</span>
      <div class="flex items-center gap-2">
        <span :class="adminStatus ? 'bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300' : 'bg-amber-100 text-amber-700 dark:bg-amber-900 dark:text-amber-300'"
          class="px-2 py-0.5 rounded text-[11px] font-semibold">
          {{ adminStatus ? t.adminBadge : t.notAdminBadge }}
        </span>
        <button v-if="!adminStatus" @click="restartAsAdmin"
          class="h-[26px] px-3 text-[12px] border border-amber-400 text-amber-600 dark:border-amber-500 dark:text-amber-400 rounded bg-white dark:bg-zinc-800 hover:bg-amber-500 hover:text-white hover:border-amber-500 cursor-pointer">
          {{ t.runAsAdmin }}
        </button>
        <button @click="setLocale(locale === 'en' ? 'zh-CN' : 'en')"
          class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 hover:border-blue-500 hover:text-blue-500 cursor-pointer">
          {{ locale === 'en' ? '中文' : 'EN' }}
        </button>
        <button :disabled="loading" @click="refresh"
          class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 hover:border-blue-500 hover:text-blue-500 disabled:opacity-40 cursor-pointer">
          {{ loading ? t.loading : t.refresh }}
        </button>
      </div>
    </header>

    <!-- Not-admin warning -->
    <div v-if="!adminStatus"
      class="shrink-0 px-3 py-1 bg-amber-50 dark:bg-amber-950 border-b border-amber-200 dark:border-amber-800 text-amber-600 dark:text-amber-400 text-[11px]">
      {{ t.notAdminHint }}
    </div>

    <!-- WSL IP -->
    <div class="flex items-center gap-2 px-3 py-1.5 bg-white dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700 shrink-0 flex-wrap">
      <span class="text-[12px] text-zinc-500 min-w-[60px] font-medium">{{ t.wslIp }}</span>
      <input v-model="wslIp" placeholder="172.x.x.x"
        class="h-[26px] px-2 w-36 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 outline-none focus:border-blue-500" />
      <button @click="detectIp"
        class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 hover:border-blue-500 hover:text-blue-500 cursor-pointer">
        {{ t.detect }}
      </button>
    </div>

    <!-- Add Rule -->
    <div :class="{ 'opacity-40 pointer-events-none': !adminStatus }"
      class="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-100 dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-700 shrink-0 flex-wrap">
      <span class="text-[12px] text-zinc-500 min-w-[60px] font-medium">{{ t.addRule }}</span>
      <input v-model="listenAddr" :placeholder="t.listenAddrPlaceholder" title="Listen Address"
        class="h-[26px] px-2 w-32 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 outline-none focus:border-blue-500" />
      <input v-model="listenPort" :placeholder="t.portPlaceholder" @input="onListenPortChange" @keyup.enter="addRule"
        class="h-[26px] px-2 w-16 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 outline-none focus:border-blue-500" />
      <span class="text-zinc-400">→</span>
      <input v-model="connectAddr" :placeholder="t.connectAddrPlaceholder" title="Connect Address"
        class="h-[26px] px-2 w-32 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 outline-none focus:border-blue-500" />
      <input v-model="connectPort" :placeholder="t.portPlaceholder" @keyup.enter="addRule"
        class="h-[26px] px-2 w-16 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 outline-none focus:border-blue-500" />
      <button @click="addRule"
        class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-800 hover:border-blue-500 hover:text-blue-500 cursor-pointer">
        {{ t.add }}
      </button>
      <span v-if="formError" class="text-[11px] text-red-500">{{ formError }}</span>
    </div>

    <!-- Rules Table -->
    <div class="flex-1 min-h-[120px] overflow-y-auto bg-white dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
      <table class="w-full border-collapse text-[12px]">
        <thead>
          <tr class="bg-zinc-100 dark:bg-zinc-900 sticky top-0 z-10">
            <th class="w-7 px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700"></th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">{{ t.colListenAddr }}</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">{{ t.colPort }}</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">{{ t.colConnectAddr }}</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">{{ t.colPort }}</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">{{ t.colWindows }}</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">{{ t.colWsl }}</th>
            <th class="px-2 py-1.5 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">{{ t.colDocker }}</th>
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
                {{ rule.win_open ? t.statusOpen : t.statusClosed }}
              </span>
            </td>
            <td class="px-2 py-1.5">
              <span :class="rule.wsl_running ? 'bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300' : 'bg-zinc-100 text-zinc-500 dark:bg-zinc-700 dark:text-zinc-400'"
                class="px-1.5 py-0.5 rounded text-[10px] font-semibold">
                {{ rule.wsl_running ? t.statusRunning : t.statusNot }}
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
            <td colspan="8" class="px-3 py-4 text-center text-zinc-400">{{ t.noRules }}</td>
          </tr>
        </tbody>
      </table>
      <div class="px-2.5 py-1.5 border-t border-zinc-100 dark:border-zinc-700">
        <button :disabled="selectedRules.size === 0 || !adminStatus" @click="deleteSelected"
          class="h-[26px] px-3 text-[12px] border border-red-400 text-red-500 rounded hover:bg-red-500 hover:text-white disabled:opacity-30 disabled:cursor-default cursor-pointer">
          {{ t.deleteSelected }}{{ selectedRules.size > 0 ? ` (${selectedRules.size})` : '' }}
        </button>
      </div>
    </div>

    <!-- Resize handle -->
    <div class="h-[5px] shrink-0 bg-zinc-200 dark:bg-zinc-700 hover:bg-blue-400 dark:hover:bg-blue-500 cursor-ns-resize flex items-center justify-center group"
      @mousedown.prevent="startResize">
      <div class="w-8 h-[2px] rounded-full bg-zinc-300 dark:bg-zinc-600 group-hover:bg-blue-400"></div>
    </div>

    <!-- Tabs -->
    <div class="flex-none flex flex-col bg-white dark:bg-zinc-800" :style="{ height: bottomH + 'px' }">
      <div class="flex border-b border-zinc-200 dark:border-zinc-700 shrink-0">
        <button v-for="tab in (['diagnostics', 'docker', 'console'] as const)" :key="tab"
          :class="activeTab === tab ? 'text-blue-500 border-b-2 border-blue-500' : 'text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200'"
          class="px-3.5 py-1.5 text-[12px] border-b-2 border-transparent -mb-px"
          @click="activeTab = tab">
          <span>{{ tabLabel(tab) }}</span>
          <span v-if="tab === 'console' && errorCount() > 0"
            class="ml-1 px-1.5 py-0.5 rounded-full bg-red-500 text-white text-[10px]">
            {{ errorCount() }}
          </span>
        </button>
        <button v-if="activeTab === 'console'" @click="consoleLogs = []"
          class="ml-auto px-3 text-[11px] text-zinc-400 hover:text-zinc-600 cursor-pointer">{{ t.clearConsole }}</button>
      </div>

      <!-- Diagnostics -->
      <div v-if="activeTab === 'diagnostics'" class="flex-1 overflow-y-auto p-3 text-[12px]">
        <div class="flex gap-2 flex-wrap mb-2">
          <span class="font-semibold">{{ t.wslListeningPorts }}</span>
          <span v-if="wslPorts.length === 0" class="text-zinc-400">{{ t.noneDetected }}</span>
          <span v-else class="text-zinc-500 tabular-nums">{{ wslPorts.join(', ') }}</span>
        </div>
        <div v-for="e in dataErrors" :key="e" class="text-red-500 text-[11px] mb-1">{{ e }}</div>
        <details v-if="debugLog.length > 0" class="mt-2">
          <summary class="text-zinc-400 cursor-pointer text-[11px]">{{ t.debugLog }} ({{ debugLog.length }})</summary>
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
          <div v-if="dockerContainers.length === 0" class="px-2 py-2 text-zinc-400 text-[12px]">{{ t.noContainers }}</div>
        </div>
        <div v-if="selectedContainer" class="flex-1 flex flex-col gap-2 p-3 overflow-y-auto">
          <table class="w-full border-collapse text-[12px]">
            <thead>
              <tr class="bg-zinc-50 dark:bg-zinc-900">
                <th class="px-2 py-1 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">{{ t.colHostPort }}</th>
                <th class="px-2 py-1 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">{{ t.colContainerPort }}</th>
                <th class="px-2 py-1 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700">{{ t.colProto }}</th>
                <th class="px-2 py-1 border-b border-zinc-200 dark:border-zinc-700"></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="p in selectedContainer.ports" :key="p.host_port" class="border-b border-zinc-100 dark:border-zinc-700">
                <td class="px-2 py-1 tabular-nums">{{ p.host_port }}</td>
                <td class="px-2 py-1 tabular-nums">{{ p.container_port ?? '—' }}</td>
                <td class="px-2 py-1">{{ p.proto }}</td>
                <td class="px-2 py-1">
                  <button :disabled="!adminStatus" @click="forwardPort(p.host_port)"
                    class="h-[22px] px-2 text-[11px] border border-zinc-300 dark:border-zinc-600 rounded hover:border-blue-500 hover:text-blue-500 disabled:opacity-40 disabled:cursor-default cursor-pointer">
                    {{ t.forward }}
                  </button>
                </td>
              </tr>
            </tbody>
          </table>
          <button :disabled="!adminStatus" @click="forwardAllPorts(selectedContainer)"
            class="self-start h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded hover:border-blue-500 hover:text-blue-500 disabled:opacity-40 disabled:cursor-default cursor-pointer">
            {{ t.forwardAll }}
          </button>
        </div>
        <div v-else class="flex-1 flex items-center justify-center text-zinc-400 text-[12px]">{{ t.selectContainer }}</div>
      </div>

      <!-- Console -->
      <div v-if="activeTab === 'console'" class="flex-1 overflow-y-auto p-2 font-mono text-[11px]">
        <div v-if="consoleLogs.length === 0" class="text-zinc-400 p-1">{{ t.noLogs }}</div>
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
