<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue'
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
  firewall_open: boolean
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
const connectPortManuallySet = ref(false)

watch(listenPort, (val) => {
  if (!connectPortManuallySet.value) connectPort.value = val
})

watch(connectPort, (val) => {
  connectPortManuallySet.value = val !== listenPort.value
})

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
    listenPort.value = ''; connectPort.value = ''; connectAddr.value = wslIp.value; connectPortManuallySet.value = false
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

async function toggleFirewall(rule: PortProxyRule) {
  const opening = !rule.firewall_open
  clog(`${opening ? 'Opening' : 'Closing'} LAN access for port ${rule.listen_port}`)
  try {
    await invoke(opening ? 'add_firewall_rule' : 'remove_firewall_rule', { port: rule.listen_port })
    clog(`LAN access ${opening ? 'opened' : 'closed'} for port ${rule.listen_port}`)
    await refresh()
  } catch (e) {
    statusMsg.value = `${t.value.firewallFailed}: ${e}`
    clog(`Firewall toggle failed: ${e}`, 'error')
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
  <div class="flex flex-col h-screen overflow-hidden bg-zinc-100 dark:bg-zinc-950 text-zinc-900 dark:text-zinc-100 text-[13px] font-[Segoe_UI,system-ui,sans-serif]">

    <!-- Header -->
    <header class="flex items-center justify-between px-4 py-2.5 bg-white dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700 shrink-0">
      <span class="font-semibold text-[15px]">WSLForward</span>
      <div class="flex items-center gap-2">
        <span :class="adminStatus ? 'bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300' : 'bg-amber-100 text-amber-700 dark:bg-amber-900 dark:text-amber-300'"
          class="px-2 py-0.5 rounded text-[11px] font-semibold">
          {{ adminStatus ? t.adminBadge : t.notAdminBadge }}
        </span>
        <button v-if="!adminStatus" @click="restartAsAdmin"
          class="h-[26px] px-3 text-[12px] border border-amber-400 text-amber-600 dark:border-amber-500 dark:text-amber-400 rounded hover:bg-amber-500 hover:text-white hover:border-amber-500 cursor-pointer">
          {{ t.runAsAdmin }}
        </button>
        <button @click="setLocale(locale === 'en' ? 'zh-CN' : 'en')"
          class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded hover:border-blue-500 hover:text-blue-500 cursor-pointer">
          {{ locale === 'en' ? '中文' : 'EN' }}
        </button>
        <button :disabled="loading" @click="refresh"
          class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded hover:border-blue-500 hover:text-blue-500 disabled:opacity-40 cursor-pointer">
          {{ loading ? t.loading : t.refresh }}
        </button>
      </div>
    </header>

    <!-- Not-admin warning -->
    <div v-if="!adminStatus"
      class="shrink-0 px-4 py-1 bg-amber-50 dark:bg-amber-950 border-b border-amber-200 dark:border-amber-800 text-amber-600 dark:text-amber-400 text-[11px]">
      {{ t.notAdminHint }}
    </div>

    <!-- Main content -->
    <div class="flex-1 flex flex-col gap-3 p-3 min-h-0 overflow-hidden">

      <!-- Top row: WSL IP card + Add Rule card -->
      <div class="flex gap-3 shrink-0">

        <!-- WSL IP card -->
        <div class="bg-white dark:bg-zinc-800 rounded-lg border border-zinc-200 dark:border-zinc-700 shadow-sm px-4 py-3 flex items-center gap-3">
          <span class="text-[11px] text-zinc-400 font-medium whitespace-nowrap">{{ t.wslIp }}</span>
          <input v-model="wslIp" placeholder="172.x.x.x"
            class="h-[26px] px-2 w-32 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-700 outline-none focus:border-blue-500" />
          <button @click="detectIp"
            class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded hover:border-blue-500 hover:text-blue-500 cursor-pointer whitespace-nowrap">
            {{ t.detect }}
          </button>
        </div>

        <!-- Add Rule card -->
        <div :class="{ 'opacity-40 pointer-events-none': !adminStatus }"
          class="flex-1 bg-white dark:bg-zinc-800 rounded-lg border border-zinc-200 dark:border-zinc-700 shadow-sm px-4 py-3 flex items-center gap-1.5 flex-wrap">
          <span class="text-[11px] text-zinc-400 font-medium mr-1 whitespace-nowrap">{{ t.addRule }}</span>
          <input v-model="listenAddr" :placeholder="t.listenAddrPlaceholder" title="Listen Address"
            class="h-[26px] px-2 w-28 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-700 outline-none focus:border-blue-500" />
          <input v-model="listenPort" :placeholder="t.portPlaceholder" @keyup.enter="addRule"
            class="h-[26px] px-2 w-14 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-700 outline-none focus:border-blue-500" />
          <span class="text-zinc-300 dark:text-zinc-600">→</span>
          <input v-model="connectAddr" :placeholder="t.connectAddrPlaceholder" title="Connect Address"
            class="h-[26px] px-2 w-28 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-700 outline-none focus:border-blue-500" />
          <input v-model="connectPort" :placeholder="t.portPlaceholder" @keyup.enter="addRule"
            class="h-[26px] px-2 w-14 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded bg-white dark:bg-zinc-700 outline-none focus:border-blue-500" />
          <button @click="addRule"
            class="h-[26px] px-3 text-[12px] border border-zinc-300 dark:border-zinc-600 rounded hover:border-blue-500 hover:text-blue-500 cursor-pointer">
            {{ t.add }}
          </button>
          <span v-if="formError" class="text-[11px] text-red-500">{{ formError }}</span>
        </div>
      </div>

      <!-- Rules card -->
      <div class="flex-1 min-h-0 bg-white dark:bg-zinc-800 rounded-lg border border-zinc-200 dark:border-zinc-700 shadow-sm flex flex-col overflow-hidden">
        <div class="flex-1 overflow-y-auto">
          <table class="w-full border-collapse text-[12px]">
            <thead>
              <tr class="bg-zinc-50 dark:bg-zinc-900 sticky top-0 z-10">
                <th class="w-7 px-3 py-2 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700"></th>
                <th class="px-3 py-2 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500 dark:text-zinc-400">{{ t.colListenAddr }}</th>
                <th class="px-3 py-2 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500 dark:text-zinc-400">{{ t.colPort }}</th>
                <th class="px-3 py-2 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500 dark:text-zinc-400">{{ t.colConnectAddr }}</th>
                <th class="px-3 py-2 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500 dark:text-zinc-400">{{ t.colPort }}</th>
                <th class="px-3 py-2 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500 dark:text-zinc-400">{{ t.colWindows }}</th>
                <th class="px-3 py-2 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500 dark:text-zinc-400">{{ t.colLan }}</th>
                <th class="px-3 py-2 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500 dark:text-zinc-400">{{ t.colWsl }}</th>
                <th class="px-3 py-2 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500 dark:text-zinc-400">{{ t.colDocker }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(rule, i) in rules" :key="i"
                :class="selectedRules.has(i) ? 'bg-blue-50 dark:bg-blue-950' : 'hover:bg-zinc-50 dark:hover:bg-zinc-700/40'"
                class="cursor-pointer border-b border-zinc-100 dark:border-zinc-700/60"
                @click="toggleRow(i)">
                <td class="px-3 py-2 w-7">
                  <input type="checkbox" :checked="selectedRules.has(i)" @click.stop="toggleRow(i)" />
                </td>
                <td class="px-3 py-2 text-zinc-500 dark:text-zinc-400 font-mono text-[11px]">{{ rule.listen_addr }}</td>
                <td class="px-3 py-2">
                  <span class="inline-flex items-center px-1.5 py-0.5 rounded bg-blue-50 dark:bg-blue-950 text-blue-600 dark:text-blue-400 text-[11px] font-mono font-semibold border border-blue-200 dark:border-blue-800">
                    {{ rule.listen_port }}
                  </span>
                </td>
                <td class="px-3 py-2 text-zinc-500 dark:text-zinc-400 font-mono text-[11px]">{{ rule.connect_addr }}</td>
                <td class="px-3 py-2">
                  <span class="inline-flex items-center px-1.5 py-0.5 rounded bg-blue-50 dark:bg-blue-950 text-blue-600 dark:text-blue-400 text-[11px] font-mono font-semibold border border-blue-200 dark:border-blue-800">
                    {{ rule.connect_port }}
                  </span>
                </td>
                <td class="px-3 py-2">
                  <span :class="rule.win_open ? 'bg-green-100 text-green-700 dark:bg-green-900/60 dark:text-green-400' : 'bg-red-100 text-red-600 dark:bg-red-900/60 dark:text-red-400'"
                    class="px-1.5 py-0.5 rounded text-[10px] font-semibold">
                    {{ rule.win_open ? t.statusOpen : t.statusClosed }}
                  </span>
                </td>
                <td class="px-3 py-2" @click.stop>
                  <div class="flex items-center gap-1.5">
                    <span :class="rule.firewall_open ? 'bg-green-100 text-green-700 dark:bg-green-900/60 dark:text-green-400' : 'bg-zinc-100 text-zinc-400 dark:bg-zinc-700 dark:text-zinc-500'"
                      class="px-1.5 py-0.5 rounded text-[10px] font-semibold">
                      {{ rule.firewall_open ? t.statusOpen : t.statusClosed }}
                    </span>
                    <button :disabled="!adminStatus" @click="toggleFirewall(rule)"
                      class="h-[20px] px-1.5 text-[10px] border border-zinc-300 dark:border-zinc-600 rounded hover:border-blue-500 hover:text-blue-500 disabled:opacity-40 disabled:cursor-default cursor-pointer whitespace-nowrap">
                      {{ rule.firewall_open ? t.blockLan : t.allowLan }}
                    </button>
                  </div>
                </td>
                <td class="px-3 py-2">
                  <span :class="rule.wsl_running ? 'bg-green-100 text-green-700 dark:bg-green-900/60 dark:text-green-400' : 'bg-zinc-100 text-zinc-400 dark:bg-zinc-700 dark:text-zinc-500'"
                    class="px-1.5 py-0.5 rounded text-[10px] font-semibold">
                    {{ rule.wsl_running ? t.statusRunning : t.statusNot }}
                  </span>
                </td>
                <td class="px-3 py-2">
                  <span v-for="dm in rule.docker_matches" :key="dm.name"
                    class="px-1.5 py-0.5 rounded text-[10px] font-semibold bg-violet-100 text-violet-700 dark:bg-violet-900/60 dark:text-violet-300 mr-1"
                    :title="`${dm.host_port} → ${dm.container_port ?? '?'} / ${dm.proto}`">
                    {{ dm.name }}
                  </span>
                </td>
              </tr>
              <tr v-if="rules.length === 0">
                <td colspan="9" class="px-3 py-8 text-center text-zinc-400">{{ t.noRules }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div class="shrink-0 px-3 py-2 border-t border-zinc-100 dark:border-zinc-700">
          <button :disabled="selectedRules.size === 0 || !adminStatus" @click="deleteSelected"
            class="h-[26px] px-3 text-[12px] border border-red-300 text-red-500 rounded hover:bg-red-500 hover:text-white hover:border-red-500 disabled:opacity-30 disabled:cursor-default cursor-pointer">
            {{ t.deleteSelected }}{{ selectedRules.size > 0 ? ` (${selectedRules.size})` : '' }}
          </button>
        </div>
      </div>

      <!-- Bottom tabs card -->
      <div class="h-52 shrink-0 bg-white dark:bg-zinc-800 rounded-lg border border-zinc-200 dark:border-zinc-700 shadow-sm flex flex-col overflow-hidden">
        <div class="flex border-b border-zinc-200 dark:border-zinc-700 shrink-0">
          <button v-for="tab in (['diagnostics', 'docker', 'console'] as const)" :key="tab"
            :class="activeTab === tab ? 'text-blue-500 border-b-2 border-blue-500' : 'text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200'"
            class="px-3.5 py-1.5 text-[12px] border-b-2 border-transparent -mb-px cursor-pointer"
            @click="activeTab = tab">
            <span>{{ tabLabel(tab) }}</span>
            <span v-if="tab === 'console' && errorCount() > 0"
              class="ml-1 px-1.5 py-0.5 rounded-full bg-red-500 text-white text-[10px]">
              {{ errorCount() }}
            </span>
          </button>
          <button v-if="activeTab === 'console'" @click="consoleLogs = []"
            class="ml-auto px-3 text-[11px] text-zinc-400 hover:text-zinc-600 cursor-pointer">
            {{ t.clearConsole }}
          </button>
        </div>

        <!-- Diagnostics -->
        <div v-if="activeTab === 'diagnostics'" class="flex-1 overflow-y-auto p-3">
          <div class="flex gap-1.5 flex-wrap items-center">
            <span class="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400 mr-1">{{ t.wslListeningPorts }}</span>
            <span v-if="wslPorts.length === 0" class="text-[11px] text-zinc-400">{{ t.noneDetected }}</span>
            <span v-for="port in wslPorts" :key="port"
              class="inline-flex items-center px-1.5 py-0.5 rounded bg-violet-50 dark:bg-violet-950 text-violet-600 dark:text-violet-400 text-[11px] font-mono border border-violet-200 dark:border-violet-800">
              {{ port }}
            </span>
          </div>
          <div v-for="e in dataErrors" :key="e" class="mt-2 text-red-500 text-[11px]">{{ e }}</div>
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
                  <th class="px-2 py-1 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500">{{ t.colHostPort }}</th>
                  <th class="px-2 py-1 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500">{{ t.colContainerPort }}</th>
                  <th class="px-2 py-1 text-left font-semibold border-b border-zinc-200 dark:border-zinc-700 text-zinc-500">{{ t.colProto }}</th>
                  <th class="px-2 py-1 border-b border-zinc-200 dark:border-zinc-700"></th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="p in selectedContainer.ports" :key="p.host_port" class="border-b border-zinc-100 dark:border-zinc-700">
                  <td class="px-2 py-1.5">
                    <span class="inline-flex items-center px-1.5 py-0.5 rounded bg-blue-50 dark:bg-blue-950 text-blue-600 dark:text-blue-400 text-[11px] font-mono border border-blue-200 dark:border-blue-800">
                      {{ p.host_port }}
                    </span>
                  </td>
                  <td class="px-2 py-1.5">
                    <span v-if="p.container_port !== null"
                      class="inline-flex items-center px-1.5 py-0.5 rounded bg-blue-50 dark:bg-blue-950 text-blue-600 dark:text-blue-400 text-[11px] font-mono border border-blue-200 dark:border-blue-800">
                      {{ p.container_port }}
                    </span>
                    <span v-else class="text-zinc-400">—</span>
                  </td>
                  <td class="px-2 py-1.5 text-zinc-500 dark:text-zinc-400">{{ p.proto }}</td>
                  <td class="px-2 py-1.5">
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
            class="flex gap-2 py-0.5 px-1 rounded hover:bg-zinc-50 dark:hover:bg-zinc-700/40">
            <span class="text-zinc-400 shrink-0">{{ entry.time }}</span>
            <span class="whitespace-pre-wrap break-all">{{ entry.msg }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- Status bar -->
    <footer :class="statusError ? 'bg-red-50 dark:bg-red-950 text-red-500' : 'bg-zinc-100 dark:bg-zinc-900 text-zinc-400'"
      class="shrink-0 px-4 py-1 text-[11px] border-t border-zinc-200 dark:border-zinc-700">
      {{ statusMsg }}
    </footer>
  </div>
</template>
