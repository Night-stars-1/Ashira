import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { api, ApiError, connection, parseDiff, parseEvent, parseReplyMetrics, streamUrl, type AgentEvent, type ApprovalMode, type Archive, type ConfirmRequest, type QuestionAnswers, type QuestionRequest, type SessionNode, type SessionView, type Status, type UpdateProgress } from './api'
import { toast } from './toast'
import type { Block, ReplyPart, TextFile } from '@/components/Transcript.vue'

const viewKey = 'ashira.session'

export function useAshira() {
  const status = ref<Status | null>(null)
  /** The server's version and newer release, from `GET /v1`; session views don't carry them. */
  const server = ref<{ version: string; update: string | null; restartPending: boolean; container: boolean; web: Status['web']; approvalHidden: boolean }>({ version: '', update: null, restartPending: false, container: false, web: null, approvalHidden: false })
  const restarting = ref(false)
  const updating = ref(false)
  /** The server update's download while it runs, polled for the progress bar. */
  const updateProgress = ref<UpdateProgress | null>(null)
  /** Has the server replace its program with the latest release; its restart button follows. */
  async function updateServer() {
    updating.value = true
    // Polled for the step and download shown under the server row.
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
    server.value = serverFacts(service)
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
  /** The last turn's failure, shown above the composer with 重试; `retry` false for other errors. */
  const turnError = ref<{ message: string; retry: boolean } | null>(null)
  /** Messages sent while the reply ran, until the model reads them (`steered`) or hands them back. */
  const pendingSteer = ref<string[]>([])
  /** History paging: the index of the first block shown, and how many the session has. */
  const historyStart = ref(0)
  const historyTotal = ref(0)
  const loadingEarlier = ref(false)
  const approvalRequests = reactive(new Set<string>())
  const approvalSubmitting = computed(() => approvalRequests.has(shownId.value))
  /** 新会话's start page (no session shown): the session is created by its first message, in
   *  `cwd`, with the model, fast and approval chosen there. `defaults` is what a new session
   *  gets anyway, so only changes are sent. */
  const fresh = reactive({ cwd: '', approval: 'manual' as ApprovalMode, forced: false, defaults: { model: '', fast: false, approval: 'manual' as ApprovalMode } })
  const creating = ref(false)
  const shownCwd = computed(() => status.value?.cwd || fresh.cwd || initialCwd.value)
  const approvalMode = computed(() => shownId.value ? status.value?.approvalMode ?? null : fresh.approval)
  let source: EventSource | undefined
  let view = 0
  let revision = 0
  // Approval changes run outside turns, so they must not invalidate an in-flight send.
  let approvalRevision = 0
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

  function setStatus(value: Status, approvalSnapshot = approvalRevision) {
    const currentMode = status.value?.approvalMode
    status.value = currentMode && approvalSnapshot !== approvalRevision ? { ...value, approvalMode: currentMode } : value
    model.value = value.model || ''
    modelName.value = value.modelName || ''
    fast.value = value.fast
  }

  /** Shows 新会话's start page for `cwd`, with the server's defaults preselected. */
  async function startNew(cwd = shownCwd.value) {
    const mine = resetView()
    shownId.value = ''
    fresh.cwd = cwd
    try { sessionStorage.removeItem(viewKey) } catch { /* storage may be blocked */ }
    try {
      const [service, approval] = await Promise.all([api.status(), api.approval()])
      if (mine !== view) return
      model.value = service.model || ''
      modelName.value = service.modelName || ''
      fast.value = service.fast
      const mode = approval.forced ? 'full' : approval.default
      fresh.approval = mode
      fresh.forced = approval.forced
      fresh.defaults = { model: model.value, fast: fast.value, approval: mode }
      connected.value = true
    } catch (reason) {
      if (mine === view) fail(reason, '无法连接 Ashira')
    } finally {
      if (mine === view) loading.value = false
    }
  }

  /** The model picker: the open session switches; the start page only remembers the choice. */
  async function chooseModel(selector: string) {
    if (shownId.value) return updateSettings((id) => api.model(id, selector))
    model.value = selector
    const split = selector.lastIndexOf(':')
    const base = split > 0 && LEVELS.includes(selector.slice(split + 1)) ? selector.slice(0, split) : selector
    modelName.value = ''
    const item = await api.models().then((catalog) => catalog.models.find((entry) => entry.selector === base), () => undefined)
    if (model.value === selector) modelName.value = item?.name || item?.id || ''
  }

  function chooseFast(enabled: boolean) {
    if (shownId.value) return updateSettings((id) => api.fast(id, enabled))
    fast.value = enabled
  }

  /** Set when the API rejected the token or could not be reached at all. */
  const needsConnection = ref(false)
  // The start page sets the mode the session will be created with; `--yes` fixes it at full.
  const canSetApprovalMode = computed(() => shownId.value
    ? Boolean(status.value && connected.value && !needsConnection.value && !loading.value && !busy.value && !approvalSubmitting.value)
    : connected.value && !loading.value && !creating.value && !fresh.forced)

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
    const approvalSnapshot = approvalRevision
    const id = shownId.value
    if (!id) return
    // Keep the pages already scrolled back to: re-read from the same first block.
    const body = await api.session(id, { limit: Math.max(HISTORY_PAGE, historyTotal.value - historyStart.value + 50) })
    if (mine !== view || revision !== snapshot) return
    setStatus(body, approvalSnapshot)
    showHistory(body)
    turnId.value = body.turnId
    confirm.value = body.confirm
    question.value = body.question
  }

  function showHistory(body: SessionView) {
    blocks.value = historyBlocks(body)
    historyStart.value = body.start
    historyTotal.value = body.total
  }

  /** The page before the first block shown; prepended, so the reader stays where they were. */
  async function loadEarlier() {
    const id = shownId.value
    const mine = view
    if (!id || loadingEarlier.value || historyStart.value === 0) return
    loadingEarlier.value = true
    try {
      const body = await api.session(id, { before: historyStart.value, limit: HISTORY_PAGE })
      if (mine !== view) return
      blocks.value = [...historyBlocks(body), ...blocks.value]
      historyStart.value = body.start
    } catch (reason) {
      if (mine === view) fail(reason, '无法加载更早的消息')
    } finally {
      if (mine === view) loadingEarlier.value = false
    }
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
    if (event.type === 'approval_mode') {
      if (event.approvalMode && status.value) {
        approvalRevision++
        status.value = { ...status.value, approvalMode: event.approvalMode }
      }
      return
    }
    if (event.type === 'turn_start' && event.turnId) {
      revision++
      turnId.value = event.turnId
      turnError.value = null
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
    } else if (event.type === 'steered') {
      // Shown where the model read them; the pending copies go as they arrive.
      const read = historyBlocks({ messages: event.messages })
      pendingSteer.value = pendingSteer.value.slice(read.length)
      const current = blocks.value.at(-1)
      if (current?.kind === 'assistant') current.live = false
      blocks.value.push(...read)
    } else if (event.type === 'steer_returned') {
      // The turn ended before reading them: back into the composer to send next.
      const texts = historyBlocks({ messages: event.messages }).flatMap((block) => block.kind === 'user' && block.text ? [block.text] : [])
      pendingSteer.value = []
      if (texts.length) draft.value = [...texts, draft.value].filter(Boolean).join('\n\n')
    } else if (event.type === 'text') {
      const text = event.text ?? ''
      appendText('text', text)
      if (text.trim() && event.turnId) textTurnId.value = event.turnId
    } else if (event.type === 'reasoning') appendText('reasoning', event.text ?? '')
    else if (event.type === 'tool_start') {
      if (!coordination(event.name)) assistant().parts.push({ type: 'tool', name: event.name || 'tool', summary: event.summary || event.intent || '', output: '', error: false, done: false })
    } else if (event.type === 'tool_end') {
      if (coordination(event.name)) {
        // A stop the user asked for interrupts `wait`; that is not a failure.
        if (event.isError && event.output !== 'cancelled') blocks.value.push({ kind: 'note', text: event.output || '子任务失败', error: true })
      } else {
        const tools = assistant().parts.filter((part): part is Extract<ReplyPart, { type: 'tool' }> => part.type === 'tool')
        const tool = [...tools].reverse().find((item) => item.name === (event.name || item.name) && !item.done) ?? tools.at(-1)
        if (tool) {
          tool.output = event.output ?? ''
          tool.error = event.isError === true
          if (event.image) tool.image = event.image
          if (event.diff) tool.diff = event.diff
          tool.done = true
        }
      }
    } else if (event.type === 'notice') blocks.value.push({ kind: 'note', text: event.text ?? '', error: false })
    else if (event.type === 'confirm' && typeof event.id === 'number') {
      confirm.value = { id: event.id, name: event.name || '命令', command: event.command || '', reason: event.reason || '', always: event.always || '始终允许' }
    } else if (event.type === 'confirm_cancelled' && confirm.value?.id === event.id) confirm.value = null
    else if (event.type === 'question' && event.question) question.value = event.question
    else if (event.type === 'question_cancelled' && question.value?.id === event.id) question.value = null
    else if (event.type === 'subagent') blocks.value.push({ kind: 'note', text: `${event.name || '未命名'}：${jobStatus(event.status)}${event.error ? `\n${event.error}` : ''}`, error: Boolean(event.error), agent: event.agent || undefined, role: event.role, output: event.output || undefined })
    else if (event.type === 'peer_message') blocks.value.push({ kind: 'note', text: `${event.from || '代理'}：${event.body || ''}`, error: false })
    else if (event.type === 'done') {
      turnId.value = null
      // Unread steers came back in `steer_returned` just before; any copy left is stale
      // (its POST answered after the model had already read it).
      pendingSteer.value = []
      confirm.value = null
      question.value = null
      const current = blocks.value.at(-1)
      if (current?.kind === 'assistant') current.live = false
      if (event.ok === false && event.error !== 'cancelled') turnError.value = { message: event.error || '回合失败', retry: true }
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
        // Re-read on the first ready too: settings may have changed before the stream subscribed.
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
    if (!text.trim() && images.length === 0 && files.length === 0) return false
    if (!shownId.value) return sendFirst(text, images, files)
    if (!connected.value || approvalSubmitting.value) return false
    return post(shownId.value, text, images, files)
  }

  async function post(id: string, text: string, images: { mediaType: string; data: string }[], files: { name: string; text: string }[]): Promise<boolean> {
    const mine = view
    const atSend = revision
    try {
      const accepted = await api.message(id, text.trim(), images.map(({ mediaType, data }) => ({ mediaType, data })), files)
      if (mine === view) {
        if (accepted.steered) pendingSteer.value = [...pendingSteer.value, text.trim() || '（附件）']
        // A stream event can arrive before the POST response, including done.
        else if (accepted.turnId && revision === atSend) {
          revision++
          turnId.value = accepted.turnId
          turnError.value = null
        }
        if (draft.value === text) draft.value = ''
      }
      return true
    } catch (reason) {
      if (mine === view) {
        turnError.value = { message: reason instanceof Error ? reason.message : '发送失败', retry: false }
        // The first message of a new session: its box was emptied when the session opened.
        if (!draft.value) draft.value = text
      }
      return false
    }
  }

  /** The start page's first message: creates the session with what was chosen there, opens
   *  it, and sends once its event stream is listening, so the reply streams in. */
  async function sendFirst(text: string, images: { mediaType: string; data: string }[], files: { name: string; text: string }[]): Promise<boolean> {
    if (creating.value || !connected.value) return false
    const mine = view
    const chosen = { model: model.value, fast: fast.value, approval: fresh.approval }
    creating.value = true
    turnError.value = null
    try {
      const id = await api.newSession(fresh.cwd || undefined)
      if (chosen.model && chosen.model !== fresh.defaults.model) await api.model(id, chosen.model)
      if (chosen.fast !== fresh.defaults.fast) await api.fast(id, chosen.fast)
      if (!server.value.approvalHidden && chosen.approval !== fresh.defaults.approval) await api.setApprovalMode(id, chosen.approval)
      if (mine !== view) return false
      await openSession(id)
      if (shownId.value !== id) return false
      if (!await streamOpen(id)) {
        // Opening cleared the box; give the message back with the reason.
        if (shownId.value === id) {
          turnError.value = { message: '会话已新建，但连接没有就绪，消息未发送', retry: false }
          if (!draft.value) draft.value = text
        }
        return false
      }
      return post(id, text, images, files)
    } catch (reason) {
      if (mine === view) turnError.value = { message: reason instanceof Error ? reason.message : '新建会话失败', retry: false }
      return false
    } finally {
      creating.value = false
    }
  }

  /** Resolves once the shown session's event stream is open, or false after `ms`. */
  function streamOpen(id: string, ms = 10_000): Promise<boolean> {
    if (connected.value) return Promise.resolve(true)
    return new Promise((resolve) => {
      const done = (open: boolean) => { stop(); clearTimeout(timer); resolve(open && shownId.value === id) }
      const stop = watch(connected, (open) => { if (open) done(true) })
      const timer = setTimeout(() => done(false), ms)
    })
  }

  /** 重试 on a failed turn: the model answers again from the history as it stands. */
  async function retry() {
    const id = shownId.value
    const mine = view
    const atSend = revision
    if (!id || busy.value) return
    turnError.value = null
    try {
      const accepted = await api.retry(id)
      if (mine === view && accepted && revision === atSend) {
        revision++
        turnId.value = accepted
      }
    } catch (reason) {
      if (mine === view) turnError.value = { message: reason instanceof Error ? reason.message : '重试失败', retry: true }
    }
  }

  /** Leaves whatever is shown: closes its stream and clears its state; the new view's token. */
  function resetView() {
    const mine = ++view
    source?.close()
    source = undefined
    connected.value = false
    loading.value = true
    turnId.value = null
    turnError.value = null
    pendingSteer.value = []
    confirm.value = null
    question.value = null
    status.value = null
    blocks.value = []
    historyStart.value = 0
    historyTotal.value = 0
    draft.value = ''
    return mine
  }

  async function openSession(id: string) {
    const mine = resetView()
    // Known target: the title updates at once instead of passing through "新会话".
    shownId.value = id
    try {
      const body = await api.session(id, { limit: HISTORY_PAGE })
      if (mine !== view) return
      setStatus(body)
      showHistory(body)
      turnId.value = body.turnId
      confirm.value = body.confirm
      question.value = body.question
      try { sessionStorage.setItem(viewKey, JSON.stringify({ id })) } catch { /* storage may be blocked */ }
      loading.value = false
      connect(id)
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
      const approvalSnapshot = approvalRevision
      const body = await api.session(id)
      if (mine === view) setStatus(body, approvalSnapshot)
    })
  }

  async function setApprovalMode(mode: ApprovalMode) {
    if (!canSetApprovalMode.value || mode === approvalMode.value) return
    if (!shownId.value) { fresh.approval = mode; return }
    const id = shownId.value
    const mine = view
    const approvalSnapshot = approvalRevision
    approvalRequests.add(id)
    try {
      const saved = await api.setApprovalMode(id, mode)
      // A live update received during the request may be newer than its response.
      if (mine === view && approvalRevision === approvalSnapshot && status.value) {
        approvalRevision++
        status.value = { ...status.value, approvalMode: saved }
      }
    } catch (reason) {
      if (mine === view) fail(reason, '切换审批模式失败')
    } finally {
      approvalRequests.delete(id)
    }
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
      server.value = serverFacts(service)
      if (mine !== view) return
      const ids = listing.tree.flatMap(function collect(node): string[] {
        return [...node.sessions.map((item) => item.id), ...node.children.flatMap(collect)]
      })
      const previous = savedId()
      const selected = ids.includes(previous) ? previous : ids[0]
      if (selected) await openSession(selected)
      // No session yet: the start page, in the server's working directory.
      else await startNew(service.cwd)
    } catch (reason) {
      if (mine === view) {
        loading.value = false
        // fetch throws TypeError when the address is wrong or the server is down.
        if (!(reason instanceof ApiError)) needsConnection.value = true
        fail(reason, '无法连接 Ashira')
      }
    }
  })
  onUnmounted(() => { view++; source?.close(); clearTimeout(listTimer) })

  return { status, server, refreshServer, restarting, restartServer, updating, updateProgress, updateServer, tree, archive, blocks, draft, model, modelTitle, fast, connected, loading, naming, confirm, question,
    busy, hasReplyText, shownId, shownCwd, sessionName, needsConnection, runningIds, askingIds, send, openSession, startNew, chooseModel, chooseFast, creating, approvalMode, decide, answer, interrupt, refresh, updateSettings, renameSession, setArchived, setFolded,
    canSetApprovalMode, approvalSubmitting, setApprovalMode, turnError, retry, pendingSteer, historyStart, loadingEarlier, loadEarlier }
}

function serverFacts(service: Status) {
  return { version: service.version, update: service.update, restartPending: service.restartPending, container: service.container, web: service.web, approvalHidden: service.approvalHidden }
}

/** Blocks a history page holds: a long session opens with its end and pages back. */
const HISTORY_PAGE = 200

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
      return [{ kind: 'user', text: value('text'), images: Array.isArray(row.images) ? row.images.flatMap((item) => { const src = imageSrc(item); return src ? [src] : [] }) : [], files }]
    }
    if (row.kind === 'note') return [{ kind: 'note', text: value('text'), error: row.error === true, agent: typeof row.agent === 'string' && row.agent ? row.agent : undefined, role: typeof row.role === 'string' ? row.role : undefined, output: value('output') || undefined }]
    if (row.kind !== 'assistant') return []
    const parts = Array.isArray(row.parts) ? row.parts.flatMap((item): ReplyPart[] => {
      if (!item || typeof item !== 'object') return []
      const part = item as Record<string, unknown>
      const text = (key: string) => typeof part[key] === 'string' ? part[key] : ''
      if (part.type === 'text' || part.type === 'reasoning') return text('text') ? [{ type: part.type, text: text('text') }] : []
      if (part.type !== 'tool') return []
      return [{ type: 'tool', name: text('name') || 'tool', summary: text('summary'), output: text('output'), error: part.error === true, done: part.done !== false, image: imageSrc(part.image), diff: parseDiff(part.diff) }]
    }) : []
    return [{ kind: 'assistant', parts, live: false, metrics: parseReplyMetrics(row.metrics) }]
  })
}

/** A picture in a history: a data URL as sent, or a server path (`/v1/...`) loaded on demand. */
function imageSrc(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined
  if (value.startsWith('data:image/')) return value
  return value.startsWith('/v1/') ? streamUrl(value).toString() : undefined
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
