<script setup lang="ts">
import { ChartNoAxesColumn, ChevronDown, CornerDownLeft, LoaderCircle, Menu, MessageSquare, PanelRightClose, PanelRightOpen, Paperclip, Square, Terminal, X } from '@lucide/vue'
import { TabsContent, TabsList, TabsRoot, TabsTrigger } from 'reka-ui'
import Transcript, { type Block, type TextFile } from '@/components/Transcript.vue'
import TextFileCard from '@/components/TextFileCard.vue'
import AgentView from '@/components/AgentView.vue'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import QuestionForm from '@/components/QuestionForm.vue'
import type { QuestionAnswers, QuestionRequest } from '@/lib/api'
import { computed, defineAsyncComponent, ref } from 'vue'

const props = defineProps<{
  cwd: string
  sessionName: string
  /** The AI title for this session is being generated. */
  naming: boolean
  blocks: Block[]
  draft: string
  model: string
  busy: boolean
  hasReplyText: boolean
  sessionId: string
  send: (images: { mediaType: string; data: string }[], files: { name: string; text: string }[]) => Promise<boolean>
  connected: boolean
  loading: boolean
  filesOpen: boolean
  question: QuestionRequest | null
}>()

const emit = defineEmits<{
  'update:draft': [value: string]
  interrupt: []
  openModels: []
  openUsage: []
  openSessions: []
  toggleFiles: []
  answer: [answers: QuestionAnswers | null]
}>()

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


function onKey(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229) {
    event.preventDefault()
    submit()
  }
}

async function submit() {
  if (props.busy || sending.value || readingImages.value || !props.connected || !props.sessionId || (!props.draft.trim() && !images.value.length && !textFiles.value.length)) return
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
      <div v-else-if="blocks.length === 0 && !activity" class="flex min-h-0 flex-1 flex-col items-center justify-center px-6 text-center">
        <p class="text-lg font-medium">{{ hasSession ? '这个目录里要做什么？' : '新建一个会话开始' }}</p>
        <p class="mt-2 w-full max-w-md truncate px-3 text-sm text-muted-foreground" :title="cwd">{{ cwd }}</p>
      </div>
      <div v-else class="min-h-0 flex-1">
        <Transcript :key="sessionId" :blocks="blocks" :activity="activity" @open-agent="agentId = $event" />
        <AgentView :id="agentId" @close="agentId = ''" />
      </div>
      <form class="shrink-0 px-2 pb-[max(0.5rem,env(safe-area-inset-bottom))] sm:px-6 sm:pb-6" @submit.prevent="submit">
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
            :placeholder="hasSession || !connected ? '描述任务，Enter 发送，Shift+Enter 换行' : '请先选择或新建会话'"
            :disabled="busy || sending || !connected || !hasSession"
            @update:model-value="emit('update:draft', String($event))"
            @keydown="onKey"
            @paste="onPaste"
          />
          <div class="flex items-center gap-2 px-1 pt-1">
            <input ref="file" type="file" multiple class="hidden" @change="onFiles" />
            <Button type="button" size="icon-sm" variant="ghost" class="shrink-0 max-sm:size-9" aria-label="添加图片或文本文件" title="添加图片（单张不超过 1.5 MB）或文本文件" :disabled="busy || sending || readingImages || !connected || !hasSession" @click="file?.click()">
              <Paperclip />
            </Button>
            <Button type="button" size="sm" variant="outline" class="min-w-0 max-w-72 max-sm:h-9" :title="model" :disabled="busy || !hasSession" @click="emit('openModels')">
              <span class="truncate">{{ model || '选择模型' }}</span>
              <ChevronDown />
            </Button>
            <Button v-if="busy" type="button" size="sm" variant="destructive" class="ml-auto shrink-0 max-sm:h-9" @click="emit('interrupt')">
              <Square /> 停止
            </Button>
            <Button v-else type="submit" size="sm" class="ml-auto shrink-0 max-sm:h-9" :disabled="(!draft.trim() && images.length === 0 && textFiles.length === 0) || !connected || !hasSession || sending || readingImages">
              <CornerDownLeft /> 发送
            </Button>
          </div>
        </div>
      </form>
    </TabsContent>
    <TabsContent value="terminal" class="flex min-h-0 min-w-0 flex-1 flex-col data-[state=inactive]:hidden">
      <TerminalView v-if="terminalOpened" :session-id="sessionId" :cwd="cwd" :active="view === 'terminal'" />
    </TabsContent>
  </TabsRoot>
</template>
