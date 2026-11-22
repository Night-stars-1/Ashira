import { computed, onMounted, onUnmounted, reactive, ref } from 'vue'
import { api, ApiError, connection, parseEvent, parseReplyMetrics, type AgentEvent, type Archive, type ConfirmRequest, type QuestionAnswers, type QuestionRequest, type SessionNode, type Status, type UpdateProgress } from './api'
import { toast } from './toast'
import type { Block, ReplyPart, TextFile } from '@/components/Transcript.vue'

const viewKey = 'ashira.session'

export function useAshira() {
  const status = ref<Status | null>(null)
  /** The server's version and newer release, from `GET /v1`; session views don't carry them. */
  const server = ref<{ version: string; update: string | null; restartPending: boolean; container: boolean; web: Status['web'] }>({ version: '', update: null, restartPending: false, container: false, web: null })
  const restarting = ref(false)
  const updating = ref(false)
  /** The server update's download while it runs, polled for the progress bar. */
  const updateProgress = ref<UpdateProgress | null>(null)
  /** Has the server replace its program with the latest release; its restart button follows. */
  async function updateServer() {
    updating.value = true
    // Between files (checksum, unpack, replace) nothing downloads: keep the last one shown.
    const poll = setInterval(() => { api.updateProgress().then((progress) => { if (updating.value && progress) updateProgress.value = progress }, () => {}) }, 500)
    try {
      await api.updateServer()
      await refreshServer()
    } catch (reason) {
      fail(reason, '更新服务端失败')
    } finally {
      clearInterval(poll)
      updating.value = false
      updateProgress.value = null
    }
  }
  /** Asks the server to re-launch its updated program, waits until it answers again, then
   *  reloads so the page (and a `--web` build the new core just installed) is current. */
  async function restartServer() {
    restarting.value = true
    try {
      await api.restart()
      const before = server.value.version
      for (let tries = 0; tries < 60; tries++) {
        await new Promise((resolve) => setTimeout(resolve, 1000))
        const status = await api.status().catch(() => null)
        if (status && (status.version !== before || !status.restartPending)) { location.reload(); return }
      }
      throw new Error('服务端没有在 60 秒内恢复，请检查服务器')
    } catch (reason) {
      restarting.value = false
      fail(reason, '重启失败')
    }
  }
  /** Server-wide facts from `GET /v1`, re-read on every stream (re)connect so an
   *  `ashira update` on the server shows its restart button on pages already open. */
  async function refreshServer() {
    const service = await api.status()
    server.value = { version: service.version, update: service.update, restartPending: service.restartPending, container: service.container, web: service.web }
  }
  const tree = ref<SessionNode[]>([])
  // Sessions with a turn running on the server, from the last list refresh.
  const listedRunning = ref<string[]>([])
  // A session is being opened; the transcript is empty because it hasn't arrived yet.
  // Starts true: the first paint shows the skeleton until a session (or none) is known.
  const loading = ref(true)
  // Of those, the ones waiting on an `ask_user` answer.
  const listedAsking = ref<string[]>([])
  const archive = ref<Archive>({ sessions: [], folders: [] })
  // Sessions whose AI title is being generated right now.
  const titling = reactive(new Set<string>())
  const naming = computed(() => titling.has(shownId.value))
  const blocks = ref<Block[]>([])
  const draft = ref('')
  const model = ref('')
  const modelName = ref('')
  const fast = ref(false)
  const connected = ref(false)
  const confirm = ref<ConfirmRequest | null>(null)
  const question = ref<QuestionRequest | null>(null)
  const shownId = ref('')
  const initialCwd = ref('')
  const turnId = ref<string | null>(null)
  const textTurnId = ref<string | null>(null)
  const hasReplyText = computed(() => turnId.value !== null && textTurnId.value === turnId.value)
  const busy = computed(() => turnId.value !== null)
  const shownCwd = computed(() => status.value?.cwd || initialCwd.value)
  let source: EventSource | undefined
  let view = 0
  let revision = 0
  async function refresh() { ({ tree: tree.value, running: listedRunning.value, asking: listedAsking.value, archive: archive.value } = await api.sessions()) }

  /** Applies a change to the list right away, then confirms it with the server. */
  async function editList(local: () => void, remote: () => Promise<unknown>, failed: string) {
    local()
    try { await remote() } catch (reason) { fail(reason, failed) }
    await refresh().catch(() => {})
  }

  function eachSession(visit: (item: { id: string; prompt: string; renamed: boolean }) => void) {
    const walk = (nodes: SessionNode[]) => nodes.forEach((node) => { node.sessions.forEach(visit); walk(node.children) })
    walk(tree.value)
    archive.value.sessions.forEach(visit)
  }

  function renameSession(id: string, title: string) {
    return editList(
      () => eachSession((item) => { if (item.id === id && title) { item.prompt = title; item.renamed = true } }),
      () => api.title(id, title),
      '重命名失败',
    )
  }

  /** Folds a folder at once here; the server keeps it and tells every other page and App. */
  function setFolded(path: string, folded: boolean) {
    return editList(() => {
      const node = tree.value.find((item) => item.path === path)
      if (node) node.folded = folded
    }, () => api.fold(path, folded), '折叠状态保存失败')
  }
  /** Archiving only hides it from the list; an open session stays open. */
  function setArchived(kind: 'session' | 'folder', key: string, archived: boolean) {
    return editList(() => {
      if (!archived && kind === 'session') archive.value.sessions = archive.value.sessions.filter((item) => item.id !== key)
      else if (!archived) archive.value.folders = archive.value.folders.filter((folder) => folder.path !== key)
      else if (kind === 'folder') {
        // Shown in Settings right away, before the server confirms.
        const node = tree.value.find((item) => item.path === key)
        if (node) archive.value.folders = [...archive.value.folders, { name: node.name, path: node.path }]
        tree.value = tree.value.filter((item) => item.path !== key)
      } else {
        const drop = (nodes: SessionNode[]) => nodes.forEach((node) => {
          const item = node.sessions.find((session) => session.id === key)
          if (item) archive.value.sessions = [...archive.value.sessions, { ...item, cwd: node.path }]
          node.sessions = node.sessions.filter((session) => session.id !== key)
          drop(node.children)
        })
        drop(tree.value)
      }
    }, () => api.archive(kind, key, archived), archived ? '归档失败' : '还原失败')
  }
  // Every session with a turn running. The open session's live turn is fresher than the
  // last list refresh, so its own state comes from this page's event stream.
  const runningIds = computed(() => {
    const ids = new Set(listedRunning.value.filter((id) => id !== shownId.value))
    if (turnId.value && shownId.value) ids.add(shownId.value)
    return ids
  })
  // Of those, the ones waiting on an `ask_user` answer.
  const askingIds = computed(() => {
    const ids = new Set(listedAsking.value.filter((id) => id !== shownId.value))
    if (turnId.value && shownId.value && question.value !== null) ids.add(shownId.value)
    return ids
  })
  // The server announces every list change (`sessions_changed`), including other pages'
  // and other devices' edits; a burst of them refreshes once.
  let listTimer: ReturnType<typeof setTimeout> | undefined
  function listChanged() {
    clearTimeout(listTimer)
    listTimer = setTimeout(() => { void refresh().catch(() => {}) }, 150)
  }
  const modelTitle = computed(() => titled(model.value, modelName.value))
  const sessionName = computed(() => shownId.value ? (sessionPrompt(tree.value, shownId.value) || '空会话') : '新会话')

  function setStatus(value: Status) {
    status.value = value
    model.value = value.model || ''
    modelName.value = value.modelName || ''
    fast.value = value.fast
  }

  /** Set when the API rejected the token or could not be reached at all. */
  const needsConnection = ref(false)

  function fail(reason: unknown, fallback: string) {
    if (reason instanceof ApiError && reason.status === 401) {
      needsConnection.value = true
      // No token yet is the normal first visit: the form is enough, no error.
      if (!connection().token) return
    }
    toast(reason instanceof Error ? reason.message : fallback)
  }

  async function syncHistory() {
    const mine = view
    const snapshot = revision
    const id = shownId.value
    if (!id) return
    const body = await api.session(id)
    if (mine !== view || revision !== snapshot) return
    setStatus(body)
    blocks.value = historyBlocks(body)
    turnId.value = body.turnId
    confirm.value = body.confirm
    question.value = body.question
  }


  function assistant(): Extract<Block, { kind: 'assistant' }> {
    const last = blocks.value.at(-1)
    if (last?.kind === 'assistant' && last.live) return last
    const created = reactive<Extract<Block, { kind: 'assistant' }>>({ kind: 'assistant', parts: [], live: true })
    blocks.value.push(created)
    return created
  }

  function appendText(type: 'text' | 'reasoning', text: string) {
    if (!text) return
    const parts = assistant().parts
    const last = parts.at(-1)
    if (last?.type === type) last.text += text
    else parts.push({ type, text })
  }

  function onEvent(event: AgentEvent) {
    if (event.sessionId !== shownId.value) return
    if (event.type === 'turn_start' && event.turnId) {
      revision++
      turnId.value = event.turnId
      confirm.value = null
      question.value = null
      return
    }
    if (event.turnId && turnId.value && event.turnId !== turnId.value) return
    if (event.turnId) {
      revision++
      turnId.value = event.turnId
    }
    if (event.type === 'user_message') {
      blocks.value.push(...historyBlocks({ messages: [event.message] }))
    } else if (event.type === 'text') {
      const text = event.text ?? ''
      appendText('text', text)
      if (text.trim() && event.turnId) textTurnId.value = event.turnId
    } else if (event.type === 'reasoning') appendText('reasoning', event.text ?? '')
    else if (event.type === 'tool_start') {
      if (!coordination(event.name)) assistant().parts.push({ type: 'tool', name: event.name || 'tool', summary: event.summary || event.intent || '', output: '', error: false, done: false })
    } else if (event.type === 'tool_end') {
      if (coordination(event.name)) {
        if (event.isError) blocks.value.push({ kind: 'note', text: event.output || '子任务失败', error: true })
      } else {
        const tools = assistant().parts.filter((part): part is Extract<ReplyPart, { type: 'tool' }> => part.type === 'tool')
        const tool = [...tools].reverse().find((item) => item.name === (event.name || item.name) && !item.done) ?? tools.at(-1)
        if (tool) {
          tool.output = event.output ?? ''
          tool.error = event.isError === true
          if (event.image) tool.image = event.image
          tool.done = true
        }
      }
    } else if (event.type === 'notice') blocks.value.push({ kind: 'note', text: event.text ?? '', error: false })
    else if (event.type === 'confirm' && typeof event.id === 'number') {
      confirm.value = { id: event.id, name: event.name || '命令', command: event.command || '', reason: event.reason || '', always: event.always || '始终允许' }
    } else if (event.type === 'confirm_cancelled' && confirm.value?.id === event.id) confirm.value = null
    else if (event.type === 'question' && event.question) question.value = event.question
    else if (event.type === 'question_cancelled' && question.value?.id === event.id) question.value = null
    else if (event.type === 'subagent') blocks.value.push({ kind: 'note', text: `${event.name || '未命名'}：${jobStatus(event.status)}${event.error ? `\n${event.error}` : ''}`, error: Boolean(event.error), agent: event.agent || undefined, role: event.role })
    else if (event.type === 'peer_message') blocks.value.push({ kind: 'note', text: `${event.from || '代理'}：${event.body || ''}`, error: false })
    else if (event.type === 'done') {
      turnId.value = null
      confirm.value = null
      question.value = null
      const current = blocks.value.at(-1)
      if (current?.kind === 'assistant') current.live = false
      if (event.ok === false && event.error !== 'cancelled') toast(event.error || '回合失败')
      const mine = view
      void syncHistory().catch((reason: unknown) => { if (mine === view) fail(reason, '无法加载会话') })
      void refresh().catch(() => {})
    } else if (event.type === 'statistics' && event.statistics && status.value) {
      status.value = { ...status.value, ...event.statistics }
    } else if (event.type === 'context' && event.tokens !== undefined && status.value) {
      status.value = { ...status.value, context: event.tokens }
    } else if (event.type === 'model' && event.name) {
      model.value = event.name
      modelName.value = event.display || event.name
      if (status.value) status.value = { ...status.value, model: event.name, modelName: event.display || event.name }
    }
  }

  function connect(id: string) {
    const stream = api.events(id)
    source = stream
    stream.onopen = () => {
      if (source !== stream) return
      connected.value = true
      void refreshServer().catch(() => {})
    }
    stream.onerror = () => {
      if (source !== stream) return
      // EventSource retries by itself; announce the drop once, not every retry.
      if (connected.value) toast('与 Ashira 的连接已断开，正在重连…')
      connected.value = false
    }
    stream.onmessage = (message) => {
      if (source !== stream) return
      const event = parseEvent(message.data)
      if (event?.type === 'ready' || event?.type === 'lag') {
        void syncHistory().catch((reason: unknown) => { if (source === stream) fail(reason, '无法加载会话') })
      } else if (event?.type === 'titling' && event.sessionId) {
        const id = event.sessionId
        if (!event.done) titling.add(id)
        else {
          // Same tick, same render: the new title appears as the spinner goes.
          if (event.title) eachSession((item) => { if (item.id === id && !item.renamed) item.prompt = event.title! })
          titling.delete(id)
        }
      } else if (event?.type === 'sessions_changed') listChanged()
      else if (event) onEvent(event)
    }
  }

  async function send(images: { mediaType: string; data: string }[] = [], files: { name: string; text: string }[] = []): Promise<boolean> {
    const text = draft.value
    const id = shownId.value
    const mine = view
    const atSend = revision
    if (!id || !connected.value || busy.value || (!text.trim() && images.length === 0 && files.length === 0)) return false
    try {
      const accepted = await api.message(id, text.trim(), images.map(({ mediaType, data }) => ({ mediaType, data })), files)
      if (mine === view) {
        // A stream event can arrive before the POST response, including done.
        if (accepted && revision === atSend) {
          revision++
          turnId.value = accepted
        }
        if (draft.value === text) draft.value = ''
      }
      return true
    } catch (reason) {
      if (mine === view) fail(reason, '发送失败')
      return false
    }
  }

  async function openSession(create: boolean, id?: string, cwd?: string) {
    const mine = ++view
    source?.close()
    source = undefined
    loading.value = true
    turnId.value = null
    confirm.value = null
    question.value = null
    status.value = null
    blocks.value = []
    // Known target: the title updates at once instead of passing through "新会话".
    shownId.value = create ? '' : id ?? ''
    draft.value = ''
    try {
      const selected = create ? await api.newSession(cwd) : id
      if (mine !== view || !selected) return
      const body = await api.session(selected)
      if (mine !== view) return
      shownId.value = selected
      setStatus(body)
      blocks.value = historyBlocks(body)
      turnId.value = body.turnId
      confirm.value = body.confirm
      question.value = body.question
      try { sessionStorage.setItem(viewKey, JSON.stringify({ id: selected })) } catch { /* storage may be blocked */ }
      loading.value = false
      connect(selected)
      // Also picks up turns running in other sessions (shown in the list).
      await refresh()
    } catch (reason) {
      if (mine === view) fail(reason, '会话操作失败')
    } finally {
      if (mine === view) loading.value = false
    }
  }

  async function decide(decision: 'yes' | 'no' | 'always') {
    const pending = confirm.value
    const turn = turnId.value
    if (!pending || !turn || !shownId.value) return
    await run(() => api.confirm(shownId.value, turn, pending.id, decision))
  }

  /** `null` skips the whole group; the model then decides on its own. */
  let answering: number | null = null
  async function answer(answers: QuestionAnswers | null) {
    const pending = question.value
    const turn = turnId.value
    if (!pending || !turn || !shownId.value || answering === pending.id) return
    answering = pending.id
    try {
      await run(async () => {
        await api.answer(shownId.value, turn, pending.id, answers)
        if (question.value?.id === pending.id) question.value = null
      })
    } finally { answering = null }
  }

  async function interrupt() {
    const turn = turnId.value
    if (turn && shownId.value) await run(() => api.interrupt(shownId.value, turn))
  }

  async function run(action: () => Promise<unknown>) {
    const mine = view
    try { await action() } catch (reason) {
      if (mine === view) fail(reason, '操作失败')
    }
  }

  async function updateSettings(action: (id: string) => Promise<unknown>) {
    const id = shownId.value
    const mine = view
    if (!id) return
    await run(async () => {
      await action(id)
      const body = await api.session(id)
      if (mine === view) setStatus(body)
    })
  }

  function savedId(): string {
    try {
      const item = JSON.parse(sessionStorage.getItem(viewKey) || 'null')
      return typeof item?.id === 'string' ? item.id : ''
    } catch { return '' }
  }

  onMounted(async () => {
    const mine = view
    // The server always requires a token, so without one there is nothing to try yet.
    if (!connection().token) {
      loading.value = false
      needsConnection.value = true
      return
    }
    try {
      const [service, listing] = await Promise.all([api.status(), api.sessions()])
      tree.value = listing.tree
      listedRunning.value = listing.running
      listedAsking.value = listing.asking
      archive.value = listing.archive
      initialCwd.value = service.cwd
      server.value = { version: service.version, update: service.update, restartPending: service.restartPending, container: service.container, web: service.web }
      if (mine !== view) return
      const ids = listing.tree.flatMap(function collect(node): string[] {
        return [...node.sessions.map((item) => item.id), ...node.children.flatMap(collect)]
      })
      const previous = savedId()
      const selected = ids.includes(previous) ? previous : ids[0]
      if (selected) await openSession(false, selected)
      else {
        connected.value = true
        loading.value = false
      }
    } catch (reason) {
      if (mine === view) {
        loading.value = false
        // fetch throws TypeError when the address is wrong or the server is down.
        if (!(reason instanceof ApiError)) needsConnection.value = true
        fail(reason, '无法连接 Ashira')
      }
    }
  })
  onUnmounted(() => { source?.close(); clearTimeout(listTimer) })

  return { status, server, refreshServer, restarting, restartServer, updating, updateProgress, updateServer, tree, archive, blocks, draft, model, modelTitle, fast, connected, loading, naming, confirm, question,
    busy, hasReplyText, shownId, shownCwd, sessionName, needsConnection, runningIds, askingIds, send, openSession, decide, answer, interrupt, refresh, updateSettings, renameSession, setArchived, setFolded }
}

const LEVELS = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max']

function titled(selector: string, name: string) {
  const base = name || selector
  const split = selector.lastIndexOf(':')
  if (!base || split <= 0) return base
  const level = selector.slice(split + 1)
  if (!LEVELS.includes(level)) return base
  const label = level.charAt(0).toUpperCase() + level.slice(1)
  return base.endsWith(` ${label}`) ? base : `${base} ${label}`
}

function sessionPrompt(nodes: SessionNode[], id: string): string | undefined {
  for (const node of nodes) {
    const item = node.sessions.find((session) => session.id === id)
    if (item) return item.prompt
    const nested = sessionPrompt(node.children, id)
    if (nested !== undefined) return nested
  }
}

export function historyBlocks(body: unknown): Block[] {
  if (!body || typeof body !== 'object' || !('messages' in body) || !Array.isArray(body.messages)) return []
  return body.messages.flatMap((item): Block[] => {
    if (!item || typeof item !== 'object') return []
    const row = item as Record<string, unknown>
    const value = (key: string) => typeof row[key] === 'string' ? row[key] : ''
    if (row.kind === 'user') {
      const files = Array.isArray(row.files) ? row.files.flatMap((file): TextFile[] => {
        if (!file || typeof file !== 'object') return []
        const entry = file as Record<string, unknown>
        return typeof entry.text === 'string' ? [{ name: typeof entry.name === 'string' ? entry.name : '附件', text: entry.text, lines: typeof entry.lines === 'number' ? entry.lines : entry.text.split('\n').length }] : []
      }) : []
      return [{ kind: 'user', text: value('text'), images: Array.isArray(row.images) ? row.images.filter((item): item is string => typeof item === 'string') : [], files }]
    }
    if (row.kind === 'note') return [{ kind: 'note', text: value('text'), error: row.error === true, agent: typeof row.agent === 'string' && row.agent ? row.agent : undefined, role: typeof row.role === 'string' ? row.role : undefined }]
    if (row.kind !== 'assistant') return []
    const parts = Array.isArray(row.parts) ? row.parts.flatMap((item): ReplyPart[] => {
      if (!item || typeof item !== 'object') return []
      const part = item as Record<string, unknown>
      const text = (key: string) => typeof part[key] === 'string' ? part[key] : ''
      if (part.type === 'text' || part.type === 'reasoning') return text('text') ? [{ type: part.type, text: text('text') }] : []
      if (part.type !== 'tool') return []
      const image = text('image')
      return [{ type: 'tool', name: text('name') || 'tool', summary: text('summary'), output: text('output'), error: part.error === true, done: part.done !== false, image: image.startsWith('data:image/') ? image : undefined }]
    }) : []
    return [{ kind: 'assistant', parts, live: false, metrics: parseReplyMetrics(row.metrics) }]
  })
}

/** The same words the server uses for results in history (`push_subagent_results`). */
function jobStatus(status: string | undefined): string {
  if (status === 'completed') return '已完成'
  if (status === 'failed') return '失败'
  if (status === 'cancelled') return '已取消'
  return status || '已结束'
}

function coordination(name: string | undefined): boolean {
  return name === 'task' || name === 'wait' || name === 'task_status' || name === 'task_cancel'
}
