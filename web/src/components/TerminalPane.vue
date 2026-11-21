<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { Button } from '@/components/ui/button'
import { streamUrl } from '@/lib/api'
import { NO_WRAP_COLS, wrapTerminal } from '@/lib/display'

/** One shell: its own WebSocket, so its own PTY on the server. */
const props = defineProps<{ sessionId: string; active: boolean }>()
const host = ref<HTMLDivElement | null>(null)
const state = ref<'connecting' | 'open' | 'closed'>('connecting')
const error = ref('')
let terminal: Terminal | undefined
let fit: FitAddon | undefined
let socket: WebSocket | undefined
let resize: ResizeObserver | undefined
let sizeSent = ''
const encoder = new TextEncoder()

function sendSize() {
  if (!terminal || socket?.readyState !== WebSocket.OPEN) return
  const size = JSON.stringify({ cols: terminal.cols, rows: terminal.rows })
  if (size !== sizeSent) {
    socket.send(size)
    sizeSent = size
  }
}

/** Wrapping: columns fit the pane. Off: at least `NO_WRAP_COLS`, scrolled sideways, so the
 *  shell lays long lines out on one row; a row is kept back for the horizontal scrollbar. */
function measure() {
  if (!props.active || !host.value?.clientWidth || !host.value.clientHeight || !terminal || !fit) return
  if (wrapTerminal.value) fit.fit()
  else {
    const fitted = fit.proposeDimensions()
    if (fitted && Number.isFinite(fitted.cols) && Number.isFinite(fitted.rows)) {
      terminal.resize(Math.max(fitted.cols, NO_WRAP_COLS), Math.max(1, fitted.rows - 1))
    }
  }
  sendSize()
}

function connect() {
  if (!terminal || (socket && socket.readyState < WebSocket.CLOSING)) return
  terminal.reset()
  state.value = 'connecting'
  error.value = ''
  sizeSent = ''
  const url = streamUrl(`/v1/sessions/${encodeURIComponent(props.sessionId)}/terminal`)
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  const conn = new WebSocket(url)
  socket = conn
  conn.binaryType = 'arraybuffer'
  conn.onopen = () => {
    if (socket !== conn) return
    state.value = 'open'
    measure()
    sendSize()
    if (props.active) terminal?.focus()
  }
  conn.onmessage = (event) => {
    if (socket === conn && event.data instanceof ArrayBuffer) terminal?.write(new Uint8Array(event.data))
  }
  conn.onclose = (event) => {
    if (socket !== conn) return
    error.value = event.reason || (state.value === 'connecting'
      ? '无法连接终端。请确认 API 启动时设置了 ASHIRA_WEB_TERMINAL=1。'
      : '终端连接已关闭。')
    state.value = 'closed'
    socket = undefined
  }
}

onMounted(() => {
  if (!host.value) return
  terminal = new Terminal({
    cursorBlink: true,
    scrollback: 5000,
    fontFamily: '"Ashira Terminal", "Cascadia Mono", Consolas, Menlo, "DejaVu Sans Mono", "Noto Sans Mono CJK SC", "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", monospace',
    lineHeight: 1.2,
    theme: { background: '#101012', foreground: '#e4e4e7', cursor: '#d4d4d8' },
  })
  fit = new FitAddon()
  terminal.loadAddon(fit)
  terminal.open(host.value)
  terminal.onData((text) => {
    if (socket?.readyState !== WebSocket.OPEN) return
    const data = encoder.encode(text)
    for (let start = 0; start < data.length; start += 16384) {
      socket.send(data.subarray(start, start + 16384))
    }
  })
  resize = new ResizeObserver(measure)
  resize.observe(host.value)
  measure()
  connect()
})

watch(() => props.active, async (active) => {
  if (!active) return
  await nextTick()
  if (!props.active) return
  measure()
  if (state.value === 'open') terminal?.focus()
})

watch(wrapTerminal, async () => {
  await nextTick()
  measure()
})

onBeforeUnmount(() => {
  resize?.disconnect()
  const conn = socket
  socket = undefined
  conn?.close()
  terminal?.dispose()
  terminal = undefined
})
</script>

<template>
  <div class="flex min-h-0 min-w-0 flex-1 flex-col">
    <div v-if="state !== 'open'" class="flex shrink-0 items-center gap-3 px-4 py-2 text-sm text-muted-foreground">
      <span v-if="state === 'connecting'" role="status">连接终端中…</span>
      <p v-else role="status" class="min-w-0 flex-1">{{ error }}</p>
      <Button v-if="state === 'closed'" size="sm" variant="outline" type="button" @click="connect">重新连接</Button>
    </div>
    <div ref="host" class="min-h-0 flex-1 px-3 py-2" :class="wrapTerminal ? 'overflow-hidden' : 'overflow-x-auto overflow-y-hidden'" />
  </div>
</template>
