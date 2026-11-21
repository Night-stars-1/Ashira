<script setup lang="ts">
import { computed, ref } from 'vue'
import { Ban, Check, Circle, CircleDot, CircleHelp, CornerDownRight, Minus } from '@lucide/vue'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import type { ToolCard } from '@/components/Transcript.vue'
import WorkspaceImage from '@/components/WorkspaceImage.vue'
import { generatedImages } from '@/lib/api'
import { wrapToolOutput } from '@/lib/display'

type TodoTask = { content: string; status: string; reason: string }
type TodoPhase = { name: string; tasks: TodoTask[] }

const props = defineProps<{ tool: ToolCard }>()
const todo = computed(() => props.tool.name === 'todo' ? todoBoard(props.tool.output) : null)
const asked = computed(() => props.tool.name === 'ask_user' ? askResult(props.tool.output) : null)
const images = computed(() => generatedImages(props.tool))
const numbered = computed(() => todo.value || asked.value || !props.tool.output ? null : numberedLines(props.tool.output))
/** The full-size view of the picture `read` gave the model. */
const zoomed = ref(false)

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
    <div class="flex items-center gap-2 px-3 py-2 text-sm">
      <Badge variant="secondary">{{ tool.name === 'todo' ? '待办' : tool.name === 'ask_user' ? '提问' : tool.name === 'generate_image' ? '生成图片' : tool.name }}</Badge>
      <span v-if="tool.name !== 'todo'" class="min-w-0 flex-1 truncate text-muted-foreground">{{ tool.summary }}</span>
      <span v-else class="min-w-0 flex-1" />
      <span v-if="!tool.done" class="text-xs text-muted-foreground">运行中</span>
      <span v-else-if="tool.error" class="text-xs text-destructive">失败</span>
    </div>
    <div v-if="images.length" class="flex flex-wrap gap-2 border-t px-3 py-2">
      <WorkspaceImage v-for="path in images" :key="path" :path="path" size="thumb" />
    </div>
    <!-- The picture `read` gave the model; clicking shows it full size. -->
    <div v-if="tool.image" class="border-t px-3 py-2">
      <button type="button" class="inline-block max-w-full cursor-zoom-in" :title="tool.summary" @click="zoomed = true">
        <img :src="tool.image" :alt="tool.summary || '读取的图片'" class="max-h-64 max-w-full rounded-lg border object-contain" />
      </button>
      <Dialog v-model:open="zoomed">
        <DialogContent class="max-h-[92dvh] w-auto max-w-[min(92vw,1600px)] p-2 sm:max-w-[min(92vw,1600px)]" :aria-describedby="undefined">
          <DialogTitle class="sr-only">{{ tool.summary || '读取的图片' }}</DialogTitle>
          <img :src="tool.image" :alt="tool.summary || '读取的图片'" class="max-h-[calc(92dvh-1rem)] max-w-full rounded-md object-contain" />
        </DialogContent>
      </Dialog>
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
    <!-- 设置 → 显示: unwrapped lines keep their width and scroll sideways. -->
    <!-- A picture `read` returned is the result; its "attached for you to view" note is for the model. -->
    <ScrollArea v-else-if="tool.output && !tool.image" class="border-t" viewport-class="max-h-64" :scroll-x="!wrapToolOutput">
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
  </div>
</template>
