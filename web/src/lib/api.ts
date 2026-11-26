import type { InjectionKey, Ref } from 'vue'

const serverKey = 'ashira.server'
const tokenKey = 'ashira.token'
/** Build-time default; empty means the page's own origin (dev uses the Vite `/v1` proxy). */
const builtServer = (import.meta.env.VITE_ASHIRA_API as string | undefined) ?? ''

export class ApiError extends Error {
  readonly status: number
  constructor(message: string, status: number) {
    super(message)
    this.status = status
  }
}

function stored(key: string): string | null {
  try { return localStorage.getItem(key) } catch { return null }
}

function store(key: string, value: string) {
  try {
    if (value) localStorage.setItem(key, value)
    else localStorage.removeItem(key)
  } catch { /* storage may be blocked */ }
}

const trimServer = (url: string) => url.trim().replace(/\/+$/, '')

/** Where the page talks to: what you saved, else the build default, else this origin. */
export function connection(): { server: string; token: string } {
  return { server: trimServer(stored(serverKey) ?? builtServer), token: stored(tokenKey) || '' }
}

/** Saved in this browser; the page reloads afterwards so open streams reconnect. */
export function setConnection(server: string, token: string) {
  store(serverKey, trimServer(server))
  store(tokenKey, token.trim())
}

/** Checks a server address and token without saving them; resolves to an error message or ''. */
export async function probeConnection(server: string, token: string): Promise<string> {
  try {
    const response = await fetch(`${trimServer(server)}/v1`, { headers: token.trim() ? { authorization: `Bearer ${token.trim()}` } : {} })
    if (response.status === 401) return 'Token 不正确'
    if (!response.ok) return `服务返回 ${response.status}`
    const body: unknown = await response.json().catch(() => null)
    return body && typeof body === 'object' && 'ok' in body ? '' : '这个地址不是 Ashira API'
  } catch {
    return '连接不上，检查地址、端口和服务端是否已启动'
  }
}

/** Absolute URL of an API path, with `token` for transports that cannot send headers (EventSource, WebSocket). */
export function streamUrl(path: string): URL {
  const { server, token } = connection()
  const url = new URL(`${server}${path}`, window.location.href)
  if (token) url.searchParams.set('token', token)
  return url
}

/** Session whose workspace images (`![](path)`, `generate_image` results) load through `rawUrl`. */
export const workspaceSessionKey: InjectionKey<Ref<string>> = Symbol('workspaceSession')

/** A workspace image of the session as an `<img src>`; the token rides in the query. */
export function rawUrl(sessionId: string, path: string): string {
  const url = streamUrl(`${sessionPath(sessionId)}/raw`)
  url.searchParams.set('path', path)
  return url.toString()
}

/** A Markdown image target that names a workspace file: no scheme, not protocol-relative. */
export function isWorkspacePath(src: string): boolean {
  return !!src && !/^[a-z][a-z0-9+.-]*:/i.test(src) && !src.startsWith('//')
}

/** Workspace paths a successful `generate_image` call saved (one `saved: <path>` line each). */
export function generatedImages(tool: { name: string; output: string; error: boolean }): string[] {
  if (tool.name !== 'generate_image' || tool.error) return []
  return tool.output.split('\n').flatMap((line) => line.startsWith('saved: ') ? [line.slice(7).trim()] : [])
}

export type UsageStatistics = {
  usage: { input: number; output: number; cacheRead: number; cacheWrite: number }
  cost: { main: number; child: number; mainUnknown: boolean; childUnknown: boolean }
  requests: { main: number; child: number }
}

/** One UTC hour of one model across all sessions; `hour` is unix seconds / 3600. `timedOutput`
 *  and `durationMs` cover the requests that logged a duration, for TPS. */
export type UsageBucket = { hour: number; model: string; input: number; output: number; cacheRead: number; cacheWrite: number; cost: number; unknown: number; requests: number; timedOutput: number; durationMs: number }
export type UsageReport = { sessions: number; buckets: UsageBucket[] }

export type ReplyMetrics = {
  model: string | null
  usage: UsageStatistics['usage'] | null
  rate: number | null
  cost: number | null
  durationMs: number | null
}

export type ApprovalMode = 'manual' | 'auto' | 'full'
/** config.yml `approval:` and `proactiveQuestions`: the mode sessions start in, whether the chat hides
 *  its switch, and whether the model asks about open choices before it works. `forced`: `--yes`. */
export type ApprovalSettings = { default: ApprovalMode; hidden: boolean; forced: boolean; questions: boolean }

function approvalMode(value: unknown): ApprovalMode | undefined {
  return value === 'manual' || value === 'auto' || value === 'full' ? value : undefined
}

function memoryReport(body: unknown): MemoryReport {
  const value = record(body)
  const entries = Array.isArray(value.entries) ? value.entries.flatMap((item): MemoryEntry[] => {
    if (!item || typeof item !== 'object') return []
    const row = record(item)
    return [{ key: text(row.key), value: text(row.value) }]
  }) : []
  return { path: text(value.path), auto: value.auto === true, proactive: value.proactive === true, entries }
}

function approvalSettings(body: unknown): ApprovalSettings {
  const value = record(body)
  return { default: approvalMode(value.default) ?? 'manual', hidden: value.hidden === true, forced: value.forced === true, questions: value.questions !== false }
}

export type Status = UsageStatistics & {
  ok: boolean
  /** The server's ashira version, and a newer release tag it can update to (null when current). */
  version: string
  update: string | null
  /** `ashira update` replaced the program while it runs; a restart loads the new build. */
  restartPending: boolean
  /** Running from the Docker image: updated by pulling the image, never in place. */
  container: boolean
  /** Set when the server hosts this page with `--web`: the release the folder holds (null if copied by hand). */
  web: { version: string | null } | null
  cwd: string
  sessionId: string
  model: string | null
  modelName: string | null
  busy: boolean
  fast: boolean
  approvalMode: ApprovalMode
  /** 设置 → 通用 hid the chat's approval switch: sessions run in the default mode. */
  approvalHidden: boolean
  context: number
  window: number | null
}

export type SessionItem = { id: string; prompt: string; modified: number; renamed: boolean }
export type ArchivedSession = SessionItem & { cwd: string }
export type Archive = { sessions: ArchivedSession[]; folders: { name: string; path: string }[] }
/** `folded`: collapsed in the session list, kept on the server so every page and App agrees. */
export type SessionNode = { name: string; path: string; folded: boolean; sessions: SessionItem[]; children: SessionNode[] }
export type FileEntry = { name: string; dir: boolean; size: number | null }
export type FileListing = { path: string; entries: FileEntry[] }
export type BrowseEntry = { name: string; path: string }
export type BrowseListing = { path: string; parent: string | null; entries: BrowseEntry[] }
export type McpTool = { name: string; description: string }
export type McpServer = { name: string; status: string; kind: string; source: string; target: string; tools: McpTool[] }
export type MemoryEntry = { key: string; value: string }
/** 设置 → 记忆: the saved entries; `auto` (`memory.auto`), consolidated after every turn; `proactive`
 *  (`memory.proactive`), the replying model saves durable facts unasked. */
export type MemoryReport = { path: string; auto: boolean; proactive: boolean; entries: MemoryEntry[] }
export type McpForm = { name: string; type: 'stdio' | 'http' | 'sse'; command: string; args: string; url: string; headers: string }
export type ModelItem = { selector: string; provider: string; id: string; name: string | null; context: number; input: number | null; output: number | null; efforts: string[]; fast: boolean }
/** A server update's step, and while a file downloads, bytes so far and its size when known. */
export type UpdateProgress = { stage: 'checking' | 'downloading' | 'installing' | 'web' | null; received: number; total: number | null }
/** `source`: where the running shell came from (`env` = ASHIRA_SHELL, `config` = config.yml, `auto`). */
export type ShellStatus = { current: string; source: 'env' | 'config' | 'auto'; configured: string | null; envOverride: string | null; problem: string | null; restartNeeded: boolean }

function shellStatus(body: unknown): ShellStatus {
  const value = record(body)
  const source = value.source === 'env' || value.source === 'config' ? value.source : 'auto'
  const optional = (item: unknown) => (typeof item === 'string' && item ? item : null)
  return { current: text(value.current), source, configured: optional(value.configured), envOverride: optional(value.envOverride), problem: optional(value.problem), restartNeeded: value.restartNeeded === true }
}
/** How a provider gets its key; a stored literal key is never sent to the page. */
export type ProviderKey =
  | { kind: 'env'; value: string; unset?: boolean }
  | { kind: 'command'; value: string }
  | { kind: 'literal' }
  | { kind: 'none' }
  | { kind: 'missing' }
export type ProviderStatus = { state: 'ready' | 'pending' | 'failed'; detail: string | null; models: number }
export type ProviderConfig = { id: string; api: string; baseUrl: string; key: ProviderKey; discovery: boolean; models: string[]; /** This provider's own image model; '' uses modelRoles.image. */ imageModel: string; status: ProviderStatus | null }
/** `config.yml` `modelRoles` the settings page edits (same list as the server's). */
export const MODEL_ROLES = ['default', 'task', 'compact', 'title', 'memory', 'image'] as const
export type ModelRole = typeof MODEL_ROLES[number]
export type ModelConfig = { apis: string[]; providers: ProviderConfig[]; roles: Record<ModelRole, string>; loadError: string | null; warnings: string[] }
/** `key.value` empty keeps the stored key of that provider. */
export type ProviderForm = { id: string; api: string; baseUrl: string; key: { kind: 'env' | 'literal' | 'command' | 'none'; value: string }; discovery: boolean; models: string[]; imageModel: string }

export type ConfirmRequest = { id: number; name: string; command: string; reason: string; always: string }
export type QuestionOption = { label: string; description: string }
export type QuestionItem = { id: string; question: string; type: 'single' | 'multi' | 'text'; options: QuestionOption[]; default: string[] }
export type QuestionRequest = { id: number; questions: QuestionItem[] }
/** Per question id: text, chosen options, or null when unanswered. */
export type QuestionAnswers = Record<string, string | string[] | null>

/** `messages` are history blocks `[start, start + messages.length)` of `total`. */
export type SessionView = Status & { messages: unknown; start: number; total: number; turnId: string | null; confirm: ConfirmRequest | null; question: QuestionRequest | null }

function sessionPath(id: string) {
  if (!id) throw new Error('请先选择会话')
  return `/v1/sessions/${encodeURIComponent(id)}`
}

function turnPath(id: string, turnId: string) {
  return `${sessionPath(id)}/turns/${encodeURIComponent(turnId)}`
}
export type AgentEvent = {
  type: string
  sessionId?: string
  turnId?: string
  message?: unknown
  /** `steered` / `steer_returned`: user blocks that joined the running turn, or came back unread. */
  messages?: unknown
  text?: string
  name?: string
  intent?: string
  summary?: string
  display?: string
  output?: string
  isError?: boolean
  /** `tool_end` of `read`: the picture attached for the model, as a data URL. */
  image?: string
  /** `tool_end` of `write`/`edit`: the file change, when the server computed one. */
  diff?: FileDiff
  id?: number
  command?: string
  reason?: string
  always?: string
  question?: QuestionRequest
  ok?: boolean
  /** `titling`: generation finished; `title` is the new name when it succeeded. */
  done?: boolean
  title?: string
  error?: string
  status?: string
  from?: string
  body?: string
  tokens?: number
  statistics?: UsageStatistics
  agent?: string | null
  /** `subagent`: the finished job's kind, `task`, `scout` or `shell` (a background command). */
  role?: string
  approvalMode?: ApprovalMode
}

/** A file change on a `write`/`edit` card. Rows of a replayed `edit` have no line numbers. */
export type DiffRow = { kind: 'context' | 'added' | 'removed' | 'hunk' | 'note'; text: string; old?: number; new?: number }
export type FileDiff = { added: number | null; removed: number | null; rows: DiffRow[]; omitted: number; notice: string }

const DIFF_KINDS: DiffRow['kind'][] = ['context', 'added', 'removed', 'hunk', 'note']

export function parseDiff(value: unknown): FileDiff | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined
  const body = value as Record<string, unknown>
  const count = (item: unknown) => typeof item === 'number' && Number.isInteger(item) && item >= 0 ? item : null
  const rows = Array.isArray(body.rows) ? body.rows.flatMap((item): DiffRow[] => {
    if (!item || typeof item !== 'object') return []
    const row = item as Record<string, unknown>
    const kind = DIFF_KINDS.find((name) => name === row.kind)
    if (!kind || typeof row.text !== 'string') return []
    return [{ kind, text: row.text, old: count(row.old) ?? undefined, new: count(row.new) ?? undefined }]
  }) : []
  return { added: count(body.added), removed: count(body.removed), rows, omitted: count(body.omitted) ?? 0, notice: typeof body.notice === 'string' ? body.notice : '' }
}

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('响应不是对象')
  const result: Record<string, unknown> = {}
  for (const [key, item] of Object.entries(value)) result[key] = item
  return result
}

function text(value: unknown, fallback = ''): string {
  return typeof value === 'string' ? value : fallback
}

function number(value: unknown): number {
  return typeof value === 'number' ? value : 0
}

function errorText(body: unknown, fallback: string): string {
  if (body && typeof body === 'object' && 'error' in body && typeof body.error === 'string') return body.error
  return fallback
}

async function request(path: string, init?: RequestInit): Promise<unknown> {
  const { server, token } = connection()
  const response = await fetch(`${server}${path}`, {
    ...init,
    headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}), ...init?.headers },
  })
  const raw = await response.text()
  let body: unknown = {}
  try {
    if (raw) body = JSON.parse(raw)
  } catch {
    if (response.ok) throw new Error('服务返回了无法解析的内容')
  }
  if (!response.ok) throw new ApiError(errorText(body, `请求失败（${response.status} ${response.statusText}）`.trim()), response.status)
  return body
}

function metric(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null
}

export function parseReplyMetrics(data: unknown): ReplyMetrics | null {
  if (!data || typeof data !== 'object' || Array.isArray(data)) return null
  const value = record(data)
  const usage = value.usage == null ? null : record(value.usage)
  return {
    model: text(value.model) || null,
    usage: usage && { input: number(usage.input), output: number(usage.output), cacheRead: number(usage.cacheRead), cacheWrite: number(usage.cacheWrite) },
    rate: metric(value.rate), cost: metric(value.cost), durationMs: metric(value.durationMs),
  }
}

function statistics(value: Record<string, unknown>): UsageStatistics {
  const usage = record(value.usage ?? {})
  const cost = record(value.cost ?? {})
  const requests = record(value.requests ?? {})
  return {
    usage: { input: number(usage.input), output: number(usage.output), cacheRead: number(usage.cacheRead), cacheWrite: number(usage.cacheWrite) },
    cost: { main: number(cost.main), child: number(cost.child), mainUnknown: cost.mainUnknown === true, childUnknown: cost.childUnknown === true },
    requests: { main: number(requests.main), child: number(requests.child) },
  }
}

function status(body: unknown): Status {
  const value = record(body)
  return {
    ok: value.ok === true,
    version: text(value.version),
    update: typeof value.update === 'string' ? value.update : null,
    restartPending: value.restartPending === true,
    container: value.container === true,
    web: value.web && typeof value.web === 'object' ? { version: text(record(value.web).version) || null } : null,
    cwd: text(value.cwd),
    sessionId: text(value.sessionId),
    model: typeof value.model === 'string' ? value.model : null,
    modelName: typeof value.modelName === 'string' ? value.modelName : null,
    busy: value.busy === true,
    fast: value.fast === true,
    approvalMode: approvalMode(value.approvalMode) ?? 'manual',
    approvalHidden: value.approvalHidden === true,
    context: number(value.context),
    ...statistics(value),
    window: metric(value.window),
  }
}

function sessions(body: unknown): SessionNode[] {
  const value = record(body)
  if (!Array.isArray(value.roots)) return []
  return value.roots.flatMap((item) => {
    const node = sessionNode(item)
    return node ? [node] : []
  })
}

function sessionNode(value: unknown): SessionNode | null {
  if (!value || typeof value !== 'object') return null
  const row = record(value)
  const sessions = Array.isArray(row.sessions) ? row.sessions.flatMap((item): SessionItem[] => {
    if (!item || typeof item !== 'object') return []
    const session = record(item)
    const id = text(session.id)
    if (!id) return []
    return [{ id, prompt: text(session.prompt), modified: number(session.modified), renamed: session.renamed === true }]
  }) : []
  const children = Array.isArray(row.children) ? row.children.flatMap((item) => {
    const child = sessionNode(item)
    return child ? [child] : []
  }) : []
  const path = text(row.path)
  if (!path) return null
  return { name: text(row.name) || path, path, folded: row.folded === true, sessions, children }
}

function archive(value: unknown): Archive {
  if (!value || typeof value !== 'object') return { sessions: [], folders: [] }
  const body = record(value)
  const sessions = Array.isArray(body.sessions) ? body.sessions.flatMap((item): ArchivedSession[] => {
    if (!item || typeof item !== 'object') return []
    const row = record(item)
    return text(row.id) ? [{ id: text(row.id), prompt: text(row.prompt), modified: number(row.modified), renamed: row.renamed === true, cwd: text(row.cwd) }] : []
  }) : []
  const folders = Array.isArray(body.folders) ? body.folders.flatMap((item) => {
    if (!item || typeof item !== 'object') return []
    const row = record(item)
    return text(row.path) ? [{ name: text(row.name) || text(row.path), path: text(row.path) }] : []
  }) : []
  return { sessions, folders }
}

function files(body: unknown): FileListing {
  const value = record(body)
  const entries = Array.isArray(value.entries) ? value.entries.flatMap((item): FileEntry[] => {
    if (!item || typeof item !== 'object') return []
    const row = record(item)
    return [{ name: text(row.name), dir: row.dir === true, size: typeof row.size === 'number' ? row.size : null }]
  }) : []
  return { path: text(value.path), entries }
}

function browseListing(body: unknown): BrowseListing {
  const value = record(body)
  const entries = Array.isArray(value.entries) ? value.entries.flatMap((item): BrowseEntry[] => {
    if (!item || typeof item !== 'object') return []
    const row = record(item)
    const path = text(row.path)
    if (!path) return []
    return [{ name: text(row.name), path }]
  }) : []
  return { path: text(value.path), parent: typeof value.parent === 'string' ? value.parent : null, entries }
}

export function parseEvent(raw: string): AgentEvent | null {
  let body: unknown
  try { body = JSON.parse(raw) } catch { return null }
  if (!body || typeof body !== 'object' || !('type' in body) || typeof body.type !== 'string') return null
  const value = record(body)
  return {
    type: body.type,
    sessionId: text(value.sessionId) || undefined,
    turnId: text(value.turnId) || undefined,
    message: value.message,
    messages: value.messages,
    text: typeof value.text === 'string' ? value.text : undefined,
    name: typeof value.name === 'string' ? value.name : undefined,
    summary: typeof value.summary === 'string' ? value.summary : undefined,
    display: typeof value.display === 'string' ? value.display : undefined,
    output: typeof value.output === 'string' ? value.output : undefined,
    isError: typeof value.isError === 'boolean' ? value.isError : undefined,
    image: typeof value.image === 'string' && value.image.startsWith('data:image/') ? value.image : undefined,
    diff: body.type === 'tool_end' ? parseDiff(value.diff) : undefined,
    id: typeof value.id === 'number' ? value.id : undefined,
    command: typeof value.command === 'string' ? value.command : undefined,
    reason: typeof value.reason === 'string' ? value.reason : undefined,
    always: typeof value.always === 'string' ? value.always : undefined,
    ok: typeof value.ok === 'boolean' ? value.ok : undefined,
    done: typeof value.done === 'boolean' ? value.done : undefined,
    title: typeof value.title === 'string' ? value.title : undefined,
    error: typeof value.error === 'string' ? value.error : undefined,
    status: typeof value.status === 'string' ? value.status : undefined,
    from: typeof value.from === 'string' ? value.from : undefined,
    body: typeof value.body === 'string' ? value.body : undefined,
    tokens: typeof value.tokens === 'number' ? value.tokens : undefined,
    statistics: body.type === 'statistics' ? statistics(value) : undefined,
    question: body.type === 'question' ? questionRequest(value) ?? undefined : undefined,
    agent: typeof value.agent === 'string' ? value.agent : typeof value.id === 'string' ? value.id : null,
    role: typeof value.role === 'string' ? value.role : undefined,
    approvalMode: approvalMode(value.approvalMode),
  }
}

function questionRequest(value: Record<string, unknown>): QuestionRequest | null {
  if (typeof value.id !== 'number' || !Array.isArray(value.questions)) return null
  const strings = (item: unknown) => (Array.isArray(item) ? item : [item]).filter((entry): entry is string => typeof entry === 'string' && entry !== '')
  const questions = value.questions.flatMap((item): QuestionItem[] => {
    if (!item || typeof item !== 'object') return []
    const row = record(item)
    const question = text(row.question)
    const type = row.type === 'multi' || row.type === 'text' ? row.type : 'single'
    if (!question || !text(row.id)) return []
    const options = Array.isArray(row.options) ? row.options.flatMap((option): QuestionOption[] => {
      if (typeof option === 'string') return option ? [{ label: option, description: '' }] : []
      if (!option || typeof option !== 'object') return []
      const value = record(option)
      return text(value.label) ? [{ label: text(value.label), description: text(value.description) }] : []
    }) : []
    return [{ id: text(row.id), question, type, options, default: strings(row.default) }]
  })
  return questions.length ? { id: value.id, questions } : null
}

export const api = {
  status: async () => status(await request('/v1')),
  /** Session tree plus every session with a turn running, and those waiting on an answer. */
  sessions: async () => {
    const body = await request('/v1/sessions')
    const value = record(body)
    const ids = (list: unknown) => Array.isArray(list) ? list.filter((id): id is string => typeof id === 'string') : []
    return { tree: sessions(body), running: ids(value.running), asking: ids(value.asking), archive: archive(value.archived) }
  },
  alias: (path: string, alias: string) => request('/v1/alias', { method: 'POST', body: JSON.stringify({ path, alias }) }),
  /** An empty title restores the first-message title. */
  title: (id: string, title: string) => request('/v1/title', { method: 'POST', body: JSON.stringify({ id, title }) }),
  archive: (kind: 'session' | 'folder', key: string, archived: boolean) => request('/v1/archive', { method: 'POST', body: JSON.stringify({ kind, key, archived }) }),
  fold: (path: string, folded: boolean) => request('/v1/fold', { method: 'POST', body: JSON.stringify({ path, folded }) }),
  newSession: async (cwd?: string) => text(record(await request('/v1/sessions', { method: 'POST', body: JSON.stringify(cwd ? { cwd } : {}) })).sessionId),
  /** `before` pages back: the `limit` blocks before that index; without it, the newest. */
  session: async (id: string, page: { before?: number; limit?: number } = {}): Promise<SessionView> => {
    const query = new URLSearchParams()
    if (page.before !== undefined) query.set('before', String(page.before))
    if (page.limit !== undefined) query.set('limit', String(page.limit))
    const body = record(await request(`${sessionPath(id)}${query.size ? `?${query}` : ''}`))
    const confirm = body.confirm ? record(body.confirm) : null
    return {
      ...status(body), messages: body.messages, start: number(body.start), total: number(body.total), turnId: text(body.turnId) || null,
      confirm: confirm && typeof confirm.id === 'number' ? {
        id: confirm.id, name: text(confirm.name), command: text(confirm.command), reason: text(confirm.reason), always: text(confirm.always),
      } : null,
      question: body.question ? questionRequest(record(body.question)) : null,
    }
  },
  /** `steered`: the reply was running, so the message joined it instead of starting a turn. */
  message: async (id: string, text: string, images: { mediaType: string; data: string }[] = [], files: { name: string; text: string }[] = []) => {
    const body = record(await request(`${sessionPath(id)}/messages`, { method: 'POST', body: JSON.stringify({ text, images, files }) }))
    return { turnId: typeof body.turnId === 'string' ? body.turnId : null, steered: body.steered === true }
  },
  /** Asks the model again after a failed reply, from the history as it stands. */
  retry: async (id: string) => {
    const body = record(await request(`${sessionPath(id)}/retry`, { method: 'POST', body: '{}' }))
    return typeof body.turnId === 'string' ? body.turnId : null
  },
  interrupt: (id: string, turn: string) => request(`${turnPath(id, turn)}/interrupt`, { method: 'POST', body: '{}' }),
  confirm: (id: string, turn: string, confirmId: number, decision: 'yes' | 'no' | 'always') => request(`${turnPath(id, turn)}/confirm`, { method: 'POST', body: JSON.stringify({ id: confirmId, decision }) }),
  answer: (id: string, turn: string, questionId: number, answers: QuestionAnswers | null) => request(`${turnPath(id, turn)}/answer`, { method: 'POST', body: JSON.stringify({ id: questionId, answers }) }),
  model: (id: string, selector: string) => request(`${sessionPath(id)}/model`, { method: 'POST', body: JSON.stringify({ selector }) }),
  models: async () => {
    const body = record(await request('/v1/models'))
    const models = Array.isArray(body.models) ? body.models.flatMap((item): ModelItem[] => {
      if (!item || typeof item !== 'object') return []
      const row = record(item)
      const efforts = Array.isArray(row.efforts) ? row.efforts.filter((effort): effort is string => typeof effort === 'string') : []
      return [{ selector: text(row.selector), provider: text(row.provider), id: text(row.id), name: typeof row.name === 'string' ? row.name : null, context: number(row.context), input: typeof row.input === 'number' ? row.input : null, output: typeof row.output === 'number' ? row.output : null, efforts, fast: row.fast === true }]
    }) : []
    const levels = Array.isArray(body.levels) ? body.levels.filter((level): level is string => typeof level === 'string') : []
    return { current: typeof body.current === 'string' ? body.current : null, levels, models }
  },
  fast: (id: string, enabled: boolean) => request(`${sessionPath(id)}/fast`, { method: 'POST', body: JSON.stringify({ enabled }) }),
  setApprovalMode: async (id: string, mode: ApprovalMode): Promise<ApprovalMode> => {
    const body = record(await request(`${sessionPath(id)}/approval`, { method: 'POST', body: JSON.stringify({ mode }) }))
    const confirmed = approvalMode(body.approvalMode)
    if (!confirmed) throw new Error('服务端未返回有效的审批模式')
    return confirmed
  },
  files: async (id: string, path = '') => files(await request(`${sessionPath(id)}/files?path=${encodeURIComponent(path)}`)),
  createEntry: (id: string, path: string, kind: 'file' | 'directory') => request(`${sessionPath(id)}/files`, { method: 'POST', body: JSON.stringify({ path, kind }) }),
  readFile: async (id: string, path: string, offset = 0): Promise<{ path: string; kind: 'text' | 'large' | 'binary'; text: string; nextOffset: number | null }> => {
    const body = record(await request(`${sessionPath(id)}/file?${new URLSearchParams({ path, offset: String(offset) })}`))
    return { path: text(body.path) || path, kind: body.kind === 'large' || body.kind === 'binary' ? body.kind : 'text', text: text(body.text), nextOffset: metric(body.nextOffset) }
  },
  writeFile: (id: string, path: string, text: string) => request(`${sessionPath(id)}/file`, { method: 'POST', body: JSON.stringify({ path, text }) }),
  browse: async (path = '') => browseListing(await request(`/v1/browse?path=${encodeURIComponent(path)}`)),
  subagent: async (id: string) => {
    const body = record(await request(`/v1/subagents?id=${encodeURIComponent(id)}`))
    return { id: text(body.id), report: text(body.report), messages: body.messages }
  },
  plugins: async () => text(record(await request('/v1/plugins')).text),
  mcp: async () => {
    const body = record(await request('/v1/mcp'))
    const servers = Array.isArray(body.servers) ? body.servers.flatMap((item): McpServer[] => {
      if (!item || typeof item !== 'object') return []
      const row = record(item)
      const tools = Array.isArray(row.tools) ? row.tools.flatMap((tool): McpTool[] => {
        if (!tool || typeof tool !== 'object') return []
        const entry = record(tool)
        return [{ name: text(entry.name), description: text(entry.description) }]
      }) : []
      return [{ name: text(row.name), status: text(row.status), kind: text(row.kind), source: text(row.source), target: text(row.target), tools }]
    }) : []
    const warnings = Array.isArray(body.warnings) ? body.warnings.filter((item): item is string => typeof item === 'string') : []
    return { servers, warnings }
  },
  mcpAdd: (body: McpForm) => request('/v1/mcp', { method: 'POST', body: JSON.stringify({ action: 'add', scope: 'user', ...body }) }),
  mcpUpdate: (previous: string, body: McpForm) => request('/v1/mcp', { method: 'POST', body: JSON.stringify({ action: 'update', previous, ...body }) }),
  mcpRemove: (name: string) => request('/v1/mcp', { method: 'POST', body: JSON.stringify({ action: 'remove', name }) }),
  mcpConfig: async (name: string): Promise<McpForm & { source: string }> => {
    const body = record(await request(`/v1/mcp/config?name=${encodeURIComponent(name)}`))
    const type = body.type === 'http' || body.type === 'sse' ? body.type : 'stdio'
    return { name: text(body.name), type, command: text(body.command), args: text(body.args), url: text(body.url), headers: text(body.headers), source: text(body.source) }
  },
  /** `ashira update` on the server: the latest release replaces its program; a restart switches to it. */
  updateServer: async () => text(record(await request('/v1/update', { method: 'POST' })).message),
  /** The running update's step and download (`total` null when the size is unknown), else null. */
  updateProgress: async (): Promise<UpdateProgress | null> => {
    const body = record(await request('/v1/update'))
    const stage = body.stage === 'checking' || body.stage === 'downloading' || body.stage === 'installing' || body.stage === 'web' ? body.stage : null
    const download = body.download && typeof body.download === 'object' ? record(body.download) : null
    if (!stage && !download) return null
    return { stage, received: download ? number(download.received) : 0, total: download ? metric(download.total) : null }
  },
  /** The bash tool's shell: running now, saved in config.yml, and what a restart would use. */
  shell: async (): Promise<ShellStatus> => shellStatus(await request('/v1/shell')),
  /** Saves config.yml `shell:` after a test run; empty goes back to finding it automatically. */
  setShell: async (shell: string): Promise<ShellStatus> => shellStatus(await request('/v1/shell', { method: 'POST', body: JSON.stringify({ shell }) })),
  approval: async (): Promise<ApprovalSettings> => approvalSettings(await request('/v1/approval')),
  setApproval: async (value: Pick<ApprovalSettings, 'default' | 'hidden' | 'questions'>): Promise<ApprovalSettings> =>
    approvalSettings(await request('/v1/approval', { method: 'POST', body: JSON.stringify(value) })),
  /** The server re-launches its program (the build `ashira update` installed). */
  restart: async () => { await request('/v1/restart', { method: 'POST' }) },
  usage: async (): Promise<UsageReport> => {
    const body = record(await request('/v1/usage'))
    const buckets = Array.isArray(body.buckets) ? body.buckets.flatMap((item): UsageBucket[] => {
      if (!item || typeof item !== 'object') return []
      const row = record(item)
      return [{ hour: number(row.hour), model: text(row.model) || '未知模型', input: number(row.input), output: number(row.output), cacheRead: number(row.cacheRead), cacheWrite: number(row.cacheWrite), cost: number(row.cost), unknown: number(row.unknown), requests: number(row.requests), timedOutput: number(row.timedOutput), durationMs: number(row.durationMs) }]
    }) : []
    return { sessions: number(body.sessions), buckets }
  },
  memory: async () => memoryReport(await request('/v1/memory')),
  /** config.yml `memory.auto` (consolidate after every turn) and `memory.proactive` (the replying model saves unasked). */
  setMemory: async (change: Partial<Pick<MemoryReport, 'auto' | 'proactive'>>) => memoryReport(await request('/v1/memory', { method: 'POST', body: JSON.stringify(change) })),
  modelConfig: async (): Promise<ModelConfig> => {
    const body = record(await request('/v1/model-config'))
    const providers = Array.isArray(body.providers) ? body.providers.flatMap((item): ProviderConfig[] => {
      if (!item || typeof item !== 'object') return []
      const row = record(item)
      const key = row.key && typeof row.key === 'object' ? row.key as ProviderKey : { kind: 'missing' as const }
      const status = row.status && typeof row.status === 'object' ? row.status as ProviderStatus : null
      const models = Array.isArray(row.models) ? row.models.filter((model): model is string => typeof model === 'string') : []
      return [{ id: text(row.id), api: text(row.api), baseUrl: text(row.baseUrl), key, discovery: row.discovery === true, models, imageModel: text(row.imageModel), status }]
    }) : []
    const strings = (value: unknown) => Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []
    const rawRoles = record(body.roles)
    const roles = Object.fromEntries(MODEL_ROLES.map((role) => [role, text(rawRoles[role])])) as Record<ModelRole, string>
    return { apis: strings(body.apis), providers, roles, loadError: typeof body.loadError === 'string' ? body.loadError : null, warnings: strings(body.warnings) }
  },
  /** Create (`previous` empty) or replace a provider. */
  saveProvider: (previous: string, provider: ProviderForm) => request('/v1/model-config', { method: 'POST', body: JSON.stringify({ action: 'save', previous, provider }) }),
  removeProvider: (id: string) => request('/v1/model-config', { method: 'POST', body: JSON.stringify({ action: 'remove', id }) }),
  /** Model ids the form's connection offers; `previous` lets an empty key field use the stored key. */
  probeModels: async (previous: string, provider: ProviderForm): Promise<string[]> => {
    const body = record(await request('/v1/model-config', { method: 'POST', body: JSON.stringify({ action: 'probe', previous, provider }) }))
    return Array.isArray(body.models) ? body.models.filter((model): model is string => typeof model === 'string') : []
  },
  /** Saves every role given; an empty selector clears it. */
  saveRoles: (roles: Partial<Record<ModelRole, string>>) => request('/v1/model-config', { method: 'POST', body: JSON.stringify({ action: 'roles', roles }) }),
  events: (id: string) => new EventSource(streamUrl(`${sessionPath(id)}/events`)),
}
