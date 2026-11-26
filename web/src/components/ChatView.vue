<script setup lang="ts">
import { ChartNoAxesColumn, ChevronDown, ChevronsRight, CircleAlert, CornerDownLeft, Folder, LoaderCircle, Menu, MessageSquare, PanelRightClose, PanelRightOpen, Paperclip, RotateCw, Send, Square, Terminal, X } from '@lucide/vue'
import { TabsContent, TabsList, TabsRoot, TabsTrigger } from 'reka-ui'
import Transcript, { type Block, type TextFile } from '@/components/Transcript.vue'
import ContextRing from '@/components/ContextRing.vue'
import TextFileCard from '@/components/TextFileCard.vue'
import AgentView from '@/components/AgentView.vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import QuestionForm from '@/components/QuestionForm.vue'
import { workspaceSessionKey, type ApprovalMode, type QuestionAnswers, type QuestionRequest } from '@/lib/api'
import { approvalOptions } from '@/lib/approval'
import { computed, defineAsyncComponent, provide, ref, toRef, watch } from 'vue'

const props = defineProps<{
  cwd: string
  sessionName: string
  /** The AI title for this session is being generated. */
  naming: boolean
  blocks: Block[]
  draft: string
  model: string
  approvalMode: ApprovalMode | null
  approvalEnabled: boolean
  /** 设置 → 通用 hid the switch: the session runs in the default mode. */
  approvalHidden: boolean
  approvalSubmitting: boolean
  busy: boolean
  hasReplyText: boolean
  sessionId: string
  send: (images: { mediaType: string; data: string }[], files: { name: string; text: string }[]) => Promise<boolean>
  connected: boolean
  loading: boolean
  filesOpen: boolean
  /** Desktop only: the session list is folded away, so the header offers to bring it back. */
  sessionsCollapsed: boolean
  /** The session's current context use, shown as a ring by the model button. */
  contextTokens: number
  contextWindow: number | null
  question: QuestionRequest | null
  /** The last turn's failure; `retry` when asking the model again can help. */
  turnError: { message: string; retry: boolean } | null
  /** Sent while the reply ran, not read by the model yet. */
  pendingSteer: string[]
  /** History paging: blocks before this index are not loaded yet. */
  historyStart: number
  loadingEarlier: boolean
  /** 新会话: the first message is creating the session. */
  creating: boolean
}>()

// Workspace images in replies and tool cards load from this session.
provide(workspaceSessionKey, toRef(props, 'sessionId'))

const emit = defineEmits<{
  'update:draft': [value: string]
  interrupt: []
  openModels: []
  approvalMode: [mode: ApprovalMode]
  openUsage: []
  openSessions: []
  expandSessions: []
  toggleFiles: []
  /** 新会话: change the directory the session will start in. */
  pickDirectory: []
  answer: [answers: QuestionAnswers | null]
  retry: []
  dismissError: []
  loadEarlier: []
}>()

const confirmFull = ref(false)

function chooseApproval(mode: unknown) {
  if (!canChooseApproval.value || mode === props.approvalMode) return
  if (mode === 'full') confirmFull.value = true
  else if (mode === 'manual' || mode === 'auto') emit('approvalMode', mode)
}

function enableFullApproval() {
  confirmFull.value = false
  if (canChooseApproval.value) emit('approvalMode', 'full')
}

const agentId = ref('')
const file = ref<HTMLInputElement | null>(null)
const imageTypes: Record<string, string> = { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp' }
const allowedImageTypes = new Set(Object.values(imageTypes))
const maxImages = 4
const maxImageBytes = 1_500_000 // 2,000,000 base64 characters in the API.
const images = ref<{ id: number; name: string; mediaType: string; data: string; url: string }[]>([])
const readingImages = ref(false)
const attachmentError = ref('')
let nextImageId = 0
// Long pastes and attached text files travel as files: the model gets the full
// text, the input box and transcript stay short.
const textFiles = ref<(TextFile & { id: number })[]>([])
const pasteLines = 12
const pasteChars = 1200
const maxTextBytes = 8 * 1024 * 1024 // the API's request body limit.
let nextTextId = 0
let pasteCount = 0
const sending = ref(false)
const approvalLabel = computed(() => approvalOptions.find((option) => option.value === props.approvalMode)?.label ?? '审批模式')
const canChooseApproval = computed(() => props.approvalEnabled && !sending.value)
watch(canChooseApproval, (enabled) => { if (!enabled) confirmFull.value = false })
const TerminalView = defineAsyncComponent(() => import('@/components/TerminalView.vue'))
const hasSession = computed(() => Boolean(props.sessionId))
const view = ref<'chat' | 'terminal'>('chat')
const terminalOpened = ref(false)

const viewTabClass = 'inline-flex h-6 items-center justify-center gap-1 rounded-[5px] border border-transparent px-2 text-[0.8rem] font-medium whitespace-nowrap text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 data-[state=active]:border-border data-[state=active]:bg-background data-[state=active]:text-foreground max-sm:size-7 max-sm:p-0 [&>svg]:size-3.5'

function selectView(value: string | number | undefined) {
  if (value === 'chat') view.value = 'chat'
  else if (value === 'terminal' && hasSession.value) {
    terminalOpened.value = true
    view.value = 'terminal'
  }
}
const activity = computed(() => props.question ? '' : props.busy ? (props.hasReplyText ? '' : '正在回复…') : sending.value ? '正在发送…' : '')
/** Long errors fold to three lines; the full text is a click away. */
const errorOpen = ref(false)
watch(() => props.turnError, () => { errorOpen.value = false })


function onKey(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229) {
    event.preventDefault()
    submit()
  }
}

async function submit() {
  // While the reply runs, sending is allowed: the message joins that turn.
  if (props.approvalSubmitting || props.creating || sending.value || readingImages.value || !props.connected || (!props.draft.trim() && !images.value.length && !textFiles.value.length)) return
  const attached = images.value.slice()
  const files = textFiles.value.slice()
  sending.value = true
  try {
    if (await props.send(attached, files.map(({ name, text }) => ({ name, text })))) {
      images.value = images.value.filter((image) => !attached.includes(image))
      textFiles.value = textFiles.value.filter((file) => !files.includes(file))
    }
  } finally {
    sending.value = false
  }
}

function addText(name: string, text: string) {
  const normalized = text.replace(/\r\n?/g, '\n')
  textFiles.value.push({ id: nextTextId++, name, text: normalized, lines: normalized.split('\n').length })
}

function onPaste(event: ClipboardEvent) {
  const clipboard = event.clipboardData
  if (!clipboard) return
  const isImage = (file: File) => file.type.startsWith('image/') || Boolean(extensionType(file))
  const selected = [...clipboard.files].filter(isImage)
  const files = selected.length ? selected : [...clipboard.items].flatMap((item) => {
    const file = item.getAsFile()
    return file && isImage(file) ? [file] : []
  })
  if (files.length) {
    event.preventDefault()
    void addFiles(files)
    return
  }
  const text = clipboard.getData('text/plain')
  if (text.length > pasteChars || text.split('\n', pasteLines + 1).length > pasteLines) {
    event.preventDefault()
    addText(`粘贴文本 ${++pasteCount}.txt`, text)
  }
}

function onFiles(event: Event) {
  const input = event.target as HTMLInputElement
  const files = [...input.files ?? []]
  input.value = ''
  void addFiles(files)
}

function extensionType(file: File) {
  const extension = /\.([^.]+)$/.exec(file.name)?.[1]?.toLowerCase()
  return extension && Object.hasOwn(imageTypes, extension) ? imageTypes[extension] : ''
}

function readImage(file: File, mediaType: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => typeof reader.result === 'string' ? resolve(reader.result) : reject(new Error('无法读取图片'))
    reader.onerror = reader.onabort = () => reject(new Error('无法读取图片'))
    reader.readAsDataURL(file.type === mediaType ? file : file.slice(0, file.size, mediaType))
  })
}

async function addFiles(files: File[]) {
  if (!files.length) return
  if (readingImages.value) {
    attachmentError.value = '正在读取上一批文件，请稍后再添加。'
    return
  }
  readingImages.value = true
  attachmentError.value = ''
  try {
    for (const item of files) {
      const mediaType = item.type && item.type !== 'application/octet-stream' ? item.type : extensionType(item)
      if (!mediaType.startsWith('image/')) {
        await addTextFile(item)
        continue
      }
      const name = item.name || '剪贴板图片'
      if (images.value.length >= maxImages) {
        attachmentError.value ||= `最多添加 ${maxImages} 张图片。`
        continue
      }
      if (!allowedImageTypes.has(mediaType)) {
        attachmentError.value ||= `「${name}」格式不支持，请选择 PNG、JPEG、GIF 或 WebP。`
        continue
      }
      if (item.size > maxImageBytes) {
        attachmentError.value ||= `「${name}」超过单张 1.5 MB 的限制。`
        continue
      }
      try {
        const url = await readImage(item, mediaType)
        const preview = new Image()
        preview.src = url
        await preview.decode()
        images.value.push({ id: nextImageId++, name, mediaType, data: url.slice(url.indexOf(',') + 1), url })
      } catch {
        attachmentError.value ||= `无法读取「${name}」，请检查图片是否损坏。`
      }
    }
  } finally {
    readingImages.value = false
  }
}

async function addTextFile(item: File) {
  const name = item.name || '文本.txt'
  if (item.size > maxTextBytes) {
    attachmentError.value ||= `「${name}」超过 8 MB 的请求上限。`
    return
  }
  const bytes = new Uint8Array(await item.arrayBuffer())
  // NUL bytes mean a binary file; the model would only see mojibake.
  if (bytes.subarray(0, 8192).includes(0)) {
    attachmentError.value ||= `「${name}」不是文本文件，只能添加文本或图片。`
    return
  }
  const text = new TextDecoder().decode(bytes)
  if (!text.trim()) {
    attachmentError.value ||= `「${name}」是空文件。`
    return
  }
  addText(name, text)
}

function removeImage(index: number) {
  images.value.splice(index, 1)
  attachmentError.value = ''
}

function removeText(index: number) {
  textFiles.value.splice(index, 1)
}
</script>

<template>
  <TabsRoot as="section" class="flex min-w-0 flex-1 flex-col" :model-value="view" activation-mode="manual" :unmount-on-hide="false" @update:model-value="selectView">
    <header class="flex h-12 shrink-0 items-center gap-1 border-b px-2 sm:gap-2 sm:px-6">
      <Button type="button" size="icon-sm" variant="ghost" class="size-9 shrink-0 lg:hidden" aria-label="打开会话列表" aria-haspopup="dialog" @click="emit('openSessions')">
        <Menu aria-hidden="true" />
      </Button>
      <Button v-if="sessionsCollapsed" type="button" size="icon-sm" variant="ghost" class="-ml-2 size-9 shrink-0 max-lg:hidden" aria-label="展开会话列表" title="展开会话列表" @click="emit('expandSessions')">
        <ChevronsRight aria-hidden="true" />
      </Button>
      <p class="flex min-w-0 flex-1 items-center gap-2 text-sm font-medium" :title="sessionName">
        <span class="truncate">{{ sessionName }}</span>
        <LoaderCircle v-if="naming" class="size-3.5 shrink-0 text-muted-foreground motion-safe:animate-spin" role="status" aria-label="正在生成标题" />
      </p>
      <TabsList aria-label="会话视图" class="inline-flex shrink-0 items-center gap-0.5 rounded-md bg-muted p-0.5">
        <TabsTrigger value="chat" :class="viewTabClass" aria-label="对话" title="对话">
          <MessageSquare aria-hidden="true" /><span class="hidden sm:inline">对话</span>
        </TabsTrigger>
        <TabsTrigger value="terminal" :class="viewTabClass" aria-label="终端" title="终端" :disabled="!hasSession">
          <Terminal aria-hidden="true" /><span class="hidden sm:inline">终端</span>
        </TabsTrigger>
      </TabsList>
      <Button type="button" size="icon-sm" variant="ghost" class="shrink-0 max-sm:size-9" aria-label="用量" title="用量" :disabled="!hasSession" @click="emit('openUsage')">
        <ChartNoAxesColumn aria-hidden="true" />
      </Button>
      <Button type="button" size="icon-sm" variant="ghost" class="shrink-0 max-sm:size-9" :aria-label="filesOpen ? '收起文件' : '展开文件'" :title="filesOpen ? '收起文件' : '展开文件'" :disabled="!hasSession" @click="emit('toggleFiles')">
        <PanelRightClose v-if="filesOpen" />
        <PanelRightOpen v-else />
      </Button>
    </header>
    <TabsContent value="chat" class="flex min-h-0 min-w-0 flex-1 flex-col data-[state=inactive]:hidden">
      <!-- Opening a session: stay blank until its history arrives (usually instant). -->
      <div v-if="loading" class="min-h-0 flex-1" aria-busy="true" />
      <!-- 新会话: the input box sits mid-page; the first message creates the session. -->
      <div v-else-if="!hasSession" class="min-h-0 flex-[1_1_0] max-h-[38%]" />
      <div v-else-if="blocks.length === 0 && !activity" class="flex min-h-0 flex-1 flex-col items-center justify-center px-6 text-center">
        <p class="text-lg font-medium">这个目录里要做什么？</p>
        <p class="mt-2 w-full max-w-md truncate px-3 text-sm text-muted-foreground" :title="cwd">{{ cwd }}</p>
      </div>
      <div v-else class="min-h-0 flex-1">
        <Transcript :key="sessionId" :blocks="blocks" :activity="activity" :start="historyStart" :pending="pendingSteer" :has-earlier="historyStart > 0" :loading-earlier="loadingEarlier" @open-agent="agentId = $event" @load-earlier="emit('loadEarlier')" />
        <AgentView :id="agentId" @close="agentId = ''" />
      </div>
      <div v-if="!loading && !hasSession" class="shrink-0 px-6 pb-6 text-center">
        <p class="text-2xl font-medium">要做什么？</p>
        <button type="button" class="mx-auto mt-2 flex max-w-md items-center gap-1.5 rounded-md px-2 py-1 text-sm text-muted-foreground hover:bg-muted hover:text-foreground disabled:pointer-events-none" :title="`在 ${cwd} 中新建会话，点击更换目录`" :disabled="creating" @click="emit('pickDirectory')">
          <Folder aria-hidden="true" class="size-4 shrink-0" />
          <span class="truncate">{{ cwd }}</span>
          <ChevronDown aria-hidden="true" class="size-4 shrink-0" />
        </button>
      </div>
      <form class="shrink-0 px-2 pb-[max(0.5rem,env(safe-area-inset-bottom))] sm:px-6 sm:pb-6" :class="!loading && !hasSession && 'mb-auto'" @submit.prevent="submit">
        <div v-if="turnError" role="alert" class="mx-auto mb-2 flex max-w-3xl items-start gap-2.5 rounded-xl border border-destructive/40 bg-destructive/10 px-3 py-2.5 text-sm">
          <CircleAlert aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-destructive" />
          <div class="min-w-0 flex-1">
            <p class="font-medium text-destructive">{{ turnError.retry ? '回复失败' : '发送失败' }}</p>
            <p class="mt-0.5 break-words text-muted-foreground select-text" :class="!errorOpen && 'line-clamp-3'">{{ turnError.message }}</p>
            <button v-if="turnError.message.length > 160" type="button" class="mt-1 text-xs text-muted-foreground underline-offset-2 hover:underline" @click="errorOpen = !errorOpen">{{ errorOpen ? '收起' : '展开' }}</button>
          </div>
          <Button v-if="turnError.retry" type="button" size="sm" variant="outline" class="shrink-0" :disabled="busy || !connected" @click="emit('retry')">
            <RotateCw /> 重试
          </Button>
          <Button type="button" size="icon-sm" variant="ghost" class="-mr-1 shrink-0 text-muted-foreground" aria-label="关闭错误提示" @click="emit('dismissError')">
            <X />
          </Button>
        </div>
        <div v-if="question" class="mx-auto max-w-3xl rounded-2xl border border-primary/60 bg-card p-2 shadow-sm">
          <QuestionForm :request="question" @answer="emit('answer', $event)" @interrupt="emit('interrupt')" />
        </div>
        <div v-else class="mx-auto max-w-3xl rounded-2xl border bg-card p-2 shadow-sm">
          <ul v-if="images.length || textFiles.length" aria-label="待发送附件" class="flex items-stretch gap-2 overflow-x-auto px-1 pb-2 sm:flex-wrap">
            <li v-for="(image, index) in images" :key="`image-${image.id}`" class="relative w-24 shrink-0 rounded-md border bg-muted">
              <img :src="image.url" :alt="image.name" class="h-16 w-full rounded-t-md object-contain" />
              <span class="absolute top-1 left-1 rounded bg-background px-1 text-xs font-medium text-foreground">图片 {{ index + 1 }}</span>
              <p :title="image.name" class="truncate px-1.5 py-1 text-xs text-muted-foreground">{{ image.name }}</p>
              <Button type="button" size="icon-xs" variant="secondary" class="absolute top-1 right-1" :aria-label="`移除图片 ${index + 1}（${image.name}）`" @click="removeImage(index)">
                <X />
              </Button>
            </li>
            <li v-for="(textFile, index) in textFiles" :key="`text-${textFile.id}`" class="flex shrink-0">
              <TextFileCard :file="textFile" removable @remove="removeText(index)" />
            </li>
          </ul>
          <p v-if="readingImages" role="status" class="px-1 pb-2 text-xs text-muted-foreground">正在读取文件…</p>
          <p v-if="attachmentError" role="alert" class="px-1 pb-2 text-xs text-destructive">{{ attachmentError }}</p>
          <Textarea
            :model-value="draft"
            class="max-h-[40dvh] min-h-16 resize-none border-0 text-base shadow-none transition-none focus-visible:ring-0 sm:text-sm"
            :placeholder="creating ? '正在新建会话…' : busy ? '补充说明，模型会在下一步读到' : '描述任务，Enter 发送，Shift+Enter 换行'"
            :disabled="sending || creating || !connected"
            @update:model-value="emit('update:draft', String($event))"
            @keydown="onKey"
            @paste="onPaste"
          />
          <div class="flex items-center gap-2 px-1 pt-1">
            <input ref="file" type="file" multiple class="hidden" @change="onFiles" />
            <Button type="button" size="icon-sm" variant="ghost" class="shrink-0 max-sm:size-9" aria-label="添加图片或文本文件" title="添加图片（单张不超过 1.5 MB）或文本文件" :disabled="sending || creating || readingImages || !connected" @click="file?.click()">
              <Paperclip />
            </Button>
            <Button type="button" size="sm" variant="outline" class="min-w-0 max-w-72 shrink max-sm:h-9" :title="model" :disabled="busy || creating || !connected" @click="emit('openModels')">
              <span class="truncate">{{ model || '选择模型' }}</span>
              <ChevronDown />
            </Button>
            <Select v-if="!approvalHidden" :model-value="approvalMode ?? ''" :disabled="!canChooseApproval" @update:model-value="chooseApproval">
              <SelectTrigger aria-label="审批模式" :aria-busy="approvalSubmitting" class="shrink-0 max-sm:h-9">
                <SelectValue placeholder="审批模式">{{ approvalLabel }}</SelectValue>
              </SelectTrigger>
              <SelectContent position="popper" align="end" class="w-60">
                <SelectItem v-for="option in approvalOptions" :key="option.value" :value="option.value">
                  <span class="flex flex-col items-start gap-0.5 whitespace-normal">
                    <span>{{ option.label }}</span>
                    <span class="text-xs text-muted-foreground">{{ option.detail }}</span>
                  </span>
                </SelectItem>
              </SelectContent>
            </Select>
            <div class="ml-auto flex shrink-0 items-center gap-1">
              <ContextRing v-if="hasSession && contextTokens > 0" :context="contextTokens" :window="contextWindow" />
              <Button v-if="busy" type="button" size="sm" variant="destructive" class="shrink-0 max-sm:h-9" @click="emit('interrupt')">
                <Square /> 停止
              </Button>
              <!-- While the reply runs this adds to it rather than starting a new turn. -->
              <Button v-if="!busy || draft.trim() || images.length || textFiles.length" type="submit" size="sm" class="shrink-0 max-sm:h-9" :title="busy ? '模型会在下一步读到这条消息' : undefined" :disabled="(!draft.trim() && images.length === 0 && textFiles.length === 0) || !connected || creating || sending || readingImages || approvalSubmitting">
                <component :is="busy ? Send : CornerDownLeft" /> {{ busy ? '补充' : '发送' }}
              </Button>
            </div>
          </div>
        </div>
      </form>
    </TabsContent>
    <TabsContent value="terminal" class="flex min-h-0 min-w-0 flex-1 flex-col data-[state=inactive]:hidden">
      <TerminalView v-if="terminalOpened" :session-id="sessionId" :active="view === 'terminal'" />
    </TabsContent>
    <Dialog v-model:open="confirmFull">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>启用完整权限？</DialogTitle>
          <DialogDescription>此会话将跳过工具审批，允许执行危险 Shell 命令和调用未授权的 MCP 工具。这不会增加 Ashira 的系统权限。</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button type="button" variant="outline" @click="confirmFull = false">取消</Button>
          <Button type="button" variant="destructive" :disabled="!canChooseApproval" @click="enableFullApproval">启用完整权限</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </TabsRoot>
</template>
