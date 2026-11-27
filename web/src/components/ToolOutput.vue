<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Ban, Check, ChevronDown, Circle, CircleDot, CircleHelp, CornerDownRight, Minus } from '@lucide/vue'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import ImageViewer from '@/components/ImageViewer.vue'
import type { ToolCard } from '@/components/Transcript.vue'
import WorkspaceImage from '@/components/WorkspaceImage.vue'
import { generatedImages, type DiffRow } from '@/lib/api'
import { foldToolOutput, wrapToolOutput } from '@/lib/display'

type TodoTask = { content: string; status: string; reason: string }
type TodoPhase = { name: string; tasks: TodoTask[] }

/** `latest`: the newest tool of the reply still running, which 默认折叠工具输出 leaves open. */
const props = defineProps<{ tool: ToolCard; latest?: boolean }>()
const todo = computed(() => props.tool.name === 'todo' ? todoBoard(props.tool.output) : null)
const asked = computed(() => props.tool.name === 'ask_user' ? askResult(props.tool.output) : null)
const images = computed(() => generatedImages(props.tool))
const numbered = computed(() => todo.value || asked.value || !props.tool.output ? null : numberedLines(props.tool.output))
/** What `write`/`edit` changed; a binary or unreadable file has no counts and shows the text result instead. */
const change = computed(() => !props.tool.error && props.tool.diff?.added != null ? props.tool.diff : null)
/** The diff opens on demand; the header shows the counts. */
const changeOpen = ref(false)
/** Whether the card has a body below its header that 默认折叠工具输出 can hide. */
const hasBody = computed(() => Boolean(images.value.length || props.tool.image || todo.value || asked.value || (props.tool.output && !change.value)))
/** 设置 → 通用 → 显示 → 默认折叠工具输出 folds every tool but the running reply's newest one;
 *  failed tools and the todo board stay open. A write/edit diff already opens on demand, so it
 *  is left alone. */
const foldable = computed(() => foldToolOutput.value && hasBody.value && !change.value && !todo.value && !props.latest && !props.tool.error)
/** This card's own choice once its header is clicked; until then the setting decides. */
const opened = ref<boolean | null>(null)
const bodyOpen = computed(() => !foldable.value || (opened.value ?? false))
// Flipping the setting resets cards opened or closed by hand to the new default.
watch(foldToolOutput, () => { opened.value = null })

function toggleHeader() {
  if (change.value) changeOpen.value = !changeOpen.value
  else if (foldable.value) opened.value = !bodyOpen.value
}
/** The full-size view of the picture `read` gave the model. */
const zoomed = ref(false)

const rowClass: Record<string, string> = {
  added: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400',
  removed: 'bg-destructive/10 text-destructive',
  context: '',
  hunk: 'text-muted-foreground',
  note: 'text-muted-foreground',
}

const markClass: Record<string, string> = {
  added: 'rounded-sm bg-emerald-500/35 text-foreground',
  removed: 'rounded-sm bg-destructive/35 text-foreground',
}

type Segment = { text: string; mark: boolean }

/** A diff row split into the characters that changed and the rest. */
function segments(row: DiffRow): Segment[] {
  const out: Segment[] = []
  let at = 0
  for (const [start, end] of row.marks ?? []) {
    if (start < at) continue
    if (start > at) out.push({ text: row.text.slice(at, start), mark: false })
    out.push({ text: row.text.slice(start, end), mark: true })
    at = end
  }
  if (at < row.text.length) out.push({ text: row.text.slice(at), mark: false })
  return out
}

type Answer = { question: string; answer: string; typed: boolean; skipped: boolean }

/** Parses `question::result_text`; anything else falls back to the raw text. */
function askResult(output: string): { skipped: boolean; answers: Answer[] } | null {
  if (output.startsWith('The user skipped')) return { skipped: true, answers: [] }
  const lines = output.split('\n')
  if (lines[0] !== 'The user answered:') return null
  const answers: Answer[] = []
  for (let i = 1; i + 1 < lines.length; i += 2) {
    const question = /^\d+\. (.*)$/.exec(lines[i]!)?.[1]
    const answer = /^ {3}→ (.*)$/.exec(lines[i + 1]!)?.[1]
    if (question === undefined || answer === undefined) return null
    const skipped = answer.startsWith('(no answer')
    const typed = answer.includes(' (typed by the user instead of an option)')
    answers.push({ question, skipped, typed, answer: skipped ? '' : answer.replaceAll(' (typed by the user instead of an option)', '').replaceAll('; ', '、') })
  }
  return answers.length ? { skipped: false, answers } : null
}

function numberedLines(output: string): { n: string; text: string }[] | null {
  const rows: { n: string; text: string }[] = []
  let saw = false
  for (const line of output.split('\n')) {
    const match = /^ *(\d+)\t(.*)$/.exec(line)
    if (match) {
      saw = true
      rows.push({ n: match[1], text: match[2] })
    } else if (saw && (line === '' || line.startsWith('[lines '))) {
      rows.push({ n: '', text: line })
    } else {
      return null
    }
  }
  return saw ? rows : null
}

function todoBoard(output: string): TodoPhase[] | null {
  const lines = output.split('\n')
  if (!lines[0]?.startsWith('Todo: ')) return null
  const phases: TodoPhase[] = []
  let current: TodoPhase | null = null
  for (const line of lines.slice(1)) {
    if (!line) continue
    if (line.endsWith(':') && !line.startsWith('- ')) {
      current = { name: line.slice(0, -1), tasks: [] }
      phases.push(current)
      continue
    }
    const task = /^- (.+) \[(pending|in_progress|completed|abandoned|blocked)\](?: \((.*)\))?$/.exec(line)
    if (!task || !current) return null
    current.tasks.push({ content: task[1], status: task[2], reason: task[3] ?? '' })
  }
  return phases
}

function todoLabel(status: string) {
  if (status === 'in_progress') return '进行中'
  if (status === 'completed') return '完成'
  if (status === 'abandoned') return '放弃'
  if (status === 'blocked') return '阻塞'
  return '待办'
}
</script>

<template>
  <div class="overflow-hidden rounded-lg border">
    <component
      :is="change || foldable ? 'button' : 'div'"
      v-bind="change ? { type: 'button', 'aria-expanded': changeOpen, title: changeOpen ? '收起改动' : '查看改动' } : foldable ? { type: 'button', 'aria-expanded': bodyOpen, title: bodyOpen ? '收起输出' : '展开输出' } : {}"
      class="flex w-full items-center gap-2 px-3 py-2 text-left text-sm"
      :class="(change || foldable) && 'hover:bg-muted/50'"
      @click="toggleHeader"
    >
      <Badge variant="secondary">{{ tool.name === 'todo' ? '待办' : tool.name === 'ask_user' ? '提问' : tool.name === 'generate_image' ? '生成图片' : tool.name }}</Badge>
      <span v-if="tool.name !== 'todo'" class="min-w-0 flex-1 truncate text-muted-foreground">{{ tool.summary }}</span>
      <span v-else class="min-w-0 flex-1" />
      <span v-if="!tool.done" class="text-xs text-muted-foreground">运行中</span>
      <span v-else-if="tool.error" class="text-xs text-destructive">失败</span>
      <template v-else-if="change">
        <span class="flex shrink-0 gap-1.5 font-mono text-xs tabular-nums" :aria-label="`新增 ${change.added} 行，删除 ${change.removed} 行`">
          <span class="text-emerald-700 dark:text-emerald-400">+{{ change.added }}</span>
          <span class="text-destructive">−{{ change.removed }}</span>
        </span>
        <ChevronDown aria-hidden="true" class="size-4 shrink-0 text-muted-foreground transition-transform motion-reduce:transition-none" :class="changeOpen && 'rotate-180'" />
      </template>
      <ChevronDown v-else-if="foldable" aria-hidden="true" class="size-4 shrink-0 text-muted-foreground transition-transform motion-reduce:transition-none" :class="bodyOpen && 'rotate-180'" />
    </component>
    <!-- Replayed `edit` rows count from the snippet, so they carry no line numbers. -->
    <ScrollArea v-if="change && changeOpen" class="border-t" viewport-class="max-h-96" :scroll-x="!wrapToolOutput">
      <div class="py-1 font-mono text-xs" :class="!wrapToolOutput && 'w-max min-w-full'">
        <template v-for="(row, rowIndex) in change.rows" :key="rowIndex">
          <!-- A replayed `edit` has no `@@` text: a gap between its hunks is just a dashed line. -->
          <template v-if="row.kind === 'hunk'">
            <div v-if="row.text" class="px-3 py-1 text-muted-foreground" :class="rowIndex > 0 && 'mt-1 border-t border-dashed'">{{ row.text }}</div>
            <div v-else-if="rowIndex > 0" class="my-1 border-t border-dashed" />
          </template>
          <div v-else class="flex gap-2 px-3" :class="rowClass[row.kind]">
            <span v-if="row.old !== undefined || row.new !== undefined" class="w-8 shrink-0 text-right opacity-60 tabular-nums select-none">{{ row.kind === 'removed' ? row.old : row.new }}</span>
            <span class="w-3 shrink-0 select-none">{{ row.kind === 'added' ? '+' : row.kind === 'removed' ? '−' : '' }}</span>
            <span class="min-w-0 flex-1 [tab-size:4]" :class="wrapToolOutput ? 'whitespace-pre-wrap' : 'whitespace-pre'"><template v-for="(part, partIndex) in segments(row)" :key="partIndex"><mark v-if="part.mark" :class="markClass[row.kind]">{{ part.text }}</mark><template v-else>{{ part.text }}</template></template></span>
          </div>
        </template>
        <p v-if="change.omitted" class="px-3 pt-1 text-muted-foreground">还有 {{ change.omitted }} 行未显示</p>
      </div>
    </ScrollArea>
    <template v-if="bodyOpen">
    <div v-if="images.length" class="flex flex-wrap gap-2 border-t px-3 py-2">
      <WorkspaceImage v-for="path in images" :key="path" :path="path" size="thumb" />
    </div>
    <!-- The picture `read` gave the model; clicking shows it full size. -->
    <div v-if="tool.image" class="border-t px-3 py-2">
      <button type="button" class="inline-block max-w-full cursor-zoom-in" :title="tool.summary" @click="zoomed = true">
        <img :src="tool.image" :alt="tool.summary || '读取的图片'" class="max-h-64 max-w-full rounded-lg border object-contain" />
      </button>
      <ImageViewer v-model:open="zoomed" :src="tool.image" :alt="tool.summary || '读取的图片'" />
    </div>
    <ScrollArea v-if="todo" class="border-t" viewport-class="max-h-64">
      <div class="flex flex-col gap-3 px-3 py-2 text-sm">
        <section v-for="phase in todo" :key="phase.name" class="flex flex-col gap-1">
          <p class="text-xs font-medium text-muted-foreground">{{ phase.name }}</p>
          <div v-for="task in phase.tasks" :key="task.content" class="flex items-start gap-2">
            <CircleDot v-if="task.status === 'in_progress'" class="mt-0.5 size-3.5 shrink-0" />
            <Check v-else-if="task.status === 'completed'" class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
            <Minus v-else-if="task.status === 'abandoned'" class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
            <Ban v-else-if="task.status === 'blocked'" class="mt-0.5 size-3.5 shrink-0" />
            <Circle v-else class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
            <div class="min-w-0 flex-1">
              <p :class="task.status === 'completed' || task.status === 'abandoned' ? 'text-muted-foreground line-through' : ''">{{ task.content }}</p>
              <p v-if="task.reason" class="text-xs text-muted-foreground">{{ task.reason }}</p>
            </div>
            <span class="shrink-0 text-xs text-muted-foreground">{{ todoLabel(task.status) }}</span>
          </div>
        </section>
      </div>
    </ScrollArea>
    <div v-else-if="asked" class="border-t px-3 py-2 text-sm">
      <p v-if="asked.skipped" class="text-muted-foreground">已跳过，由 AI 自行判断</p>
      <dl v-else class="flex flex-col gap-2.5">
        <div v-for="(item, index) in asked.answers" :key="index" class="flex flex-col gap-1">
          <dt class="flex items-start gap-1.5 text-muted-foreground">
            <CircleHelp aria-hidden="true" class="mt-0.5 size-3.5 shrink-0" />
            <span class="min-w-0">{{ item.question }}</span>
          </dt>
          <dd class="flex items-start gap-1.5 pl-5">
            <CornerDownRight aria-hidden="true" class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
            <span v-if="item.skipped" class="text-muted-foreground">未回答</span>
            <span v-else class="min-w-0 font-medium">{{ item.answer }}<span v-if="item.typed" class="ml-1.5 text-xs font-normal text-muted-foreground">（自填）</span></span>
          </dd>
        </div>
      </dl>
    </div>
    <!-- 设置 → 通用 → 显示: unwrapped lines keep their width and scroll sideways. -->
    <!-- A picture `read` returned is the result; its "attached for you to view" note is for the model. -->
    <ScrollArea v-else-if="tool.output && !tool.image && !change" class="border-t" viewport-class="max-h-64" :scroll-x="!wrapToolOutput">
      <div v-if="numbered" class="px-3 py-2 font-mono text-xs" :class="!wrapToolOutput && 'w-max min-w-full'">
        <template v-for="(row, rowIndex) in numbered" :key="rowIndex">
          <div v-if="row.n" class="flex gap-2">
            <span class="w-6 shrink-0 text-right text-muted-foreground tabular-nums">{{ row.n }}</span>
            <span class="min-w-0 flex-1" :class="wrapToolOutput ? 'whitespace-pre-wrap' : 'whitespace-pre'">{{ row.text }}</span>
          </div>
          <p v-else-if="row.text" class="mt-1 text-muted-foreground" :class="wrapToolOutput ? 'whitespace-pre-wrap' : 'whitespace-pre'">{{ row.text }}</p>
        </template>
      </div>
      <pre v-else class="px-3 py-2 font-mono text-xs" :class="wrapToolOutput ? 'whitespace-pre-wrap' : 'w-max min-w-full whitespace-pre [overflow-wrap:normal]'">{{ tool.output }}</pre>
    </ScrollArea>
    </template>
  </div>
</template>
