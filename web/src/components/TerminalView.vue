<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { Button } from '@/components/ui/button'
import { streamUrl } from '@/lib/api'

const props = defineProps<{ sessionId: string; cwd: string; active: boolean }>()
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

function measure() {
  if (!props.active || !host.value?.clientWidth || !host.value.clientHeight) return
  fit?.fit()
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
      ? '无法连接终端。请从本机网页打开，并确认 API 启动时设置了 ASHIRA_WEB_TERMINAL=1。'
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
    fontFamily: '"CaskaydiaMono NFM", "CaskaydiaMono Nerd Font Mono", "Cascadia Mono", Consolas, Menlo, "DejaVu Sans Mono", "Noto Sans Mono CJK SC", "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", monospace',
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
  <section aria-label="终端" class="flex min-h-0 min-w-0 flex-1 flex-col bg-[#101012]">
    <div class="flex shrink-0 items-center gap-3 border-b px-4 py-2 text-xs text-muted-foreground">
      <span class="min-w-0 flex-1 truncate font-mono" :title="cwd">{{ cwd }}</span>
      <span v-if="state === 'connecting'" role="status">连接终端中…</span>
      <Button v-if="state === 'closed'" size="sm" variant="outline" type="button" @click="connect">重新打开终端</Button>
    </div>
    <p v-if="error" role="status" class="shrink-0 px-4 py-2 text-sm text-muted-foreground">{{ error }}</p>
    <div ref="host" class="min-h-0 flex-1 overflow-hidden px-3 py-2" />
  </section>
</template>
