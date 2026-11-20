<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Button } from '@/components/ui/button'
import { ArrowDown, Bot, ChevronDown, ChevronRight, ChevronUp, LoaderCircle } from '@lucide/vue'
import Markdown from '@/components/Markdown.vue'
import ToolOutput from '@/components/ToolOutput.vue'
import TextFileCard from '@/components/TextFileCard.vue'
import type { ReplyMetrics } from '@/lib/api'

export type ToolCard = { name: string; summary: string; output: string; error: boolean; done: boolean }
export type ReplyPart = { type: 'text'; text: string } | { type: 'reasoning'; text: string } | ({ type: 'tool' } & ToolCard)
export type TextFile = { name: string; text: string; lines: number }
export type Block =
  | { kind: 'user'; text: string; images?: string[]; files?: TextFile[] }
  | { kind: 'assistant'; parts: ReplyPart[]; live: boolean; metrics?: ReplyMetrics | null }
  | { kind: 'note'; text: string; error: boolean; agent?: string }

const props = defineProps<{ blocks: Block[]; activity?: string }>()
const emit = defineEmits<{ openAgent: [id: string] }>()
const content = ref<HTMLElement | null>(null)

// Long pasted messages fold to a preview; the full text stays one click away.
const foldLines = 12
const foldChars = 1200
const expanded = reactive(new Set<number>())
function foldable(text: string) {
  return text.length > foldChars || text.split('\n', foldLines + 1).length > foldLines
}
function preview(text: string) {
  const head = text.split('\n', foldLines).join('\n')
  return (head.length > foldChars ? head.slice(0, foldChars) : head).trimEnd() + '…'
}
// Long reasoning starts collapsed; keys are `${blockIndex}:${partIndex}` the user toggled.
const reasoningLines = 6
const reasoningChars = 400
const toggledReasoning = reactive(new Set<string>())
function longReasoning(text: string) {
  return text.length > reasoningChars || text.split('\n', reasoningLines + 1).length > reasoningLines
}
function reasoningOpen(key: string, text: string) {
  return !longReasoning(text) !== toggledReasoning.has(key)
}
function toggleReasoning(key: string) {
  if (toggledReasoning.has(key)) toggledReasoning.delete(key)
  else toggledReasoning.add(key)
}

type Reply = Extract<Block, { kind: 'assistant' }>
/** One reply between two user messages: every model request's parts, split into the
 *  work (thinking, tools and the text between them) and the answer after the last tool. */
type Turn = { work: ReplyPart[]; answer: string[]; live: boolean; durationMs: number | null; steps: number }
type Row = { key: number; block: Exclude<Block, Reply> } | { key: number; reply: Turn }

function turn(parts: ReplyPart[], live: boolean, durationMs: number | null): Turn {
  let lastTool = -1
  parts.forEach((part, index) => { if (part.type === 'tool') lastTool = index })
  const tail = parts.slice(lastTool + 1)
  // Thinking after the last tool still belongs to the work; only its text is the answer.
  const work = [...parts.slice(0, lastTool + 1), ...tail.filter((part) => part.type === 'reasoning')]
  const answer = tail.flatMap((part) => part.type === 'text' && part.text ? [part.text] : [])
  return { work, answer, live, durationMs, steps: work.filter((part) => part.type !== 'text').length }
}

// Consecutive assistant blocks (one per model request) form one reply.
const rows = computed<Row[]>(() => {
  const out: Row[] = []
  let pending: { key: number; parts: ReplyPart[]; live: boolean; durationMs: number | null } | null = null
  const flush = () => {
    if (pending) out.push({ key: pending.key, reply: turn(pending.parts, pending.live, pending.durationMs) })
    pending = null
  }
  props.blocks.forEach((block, index) => {
    if (block.kind !== 'assistant') {
      flush()
      out.push({ key: index, block })
      return
    }
    pending ??= { key: index, parts: [], live: false, durationMs: null }
    pending.parts.push(...block.parts)
    pending.live ||= block.live
    if (block.metrics?.durationMs != null) pending.durationMs = (pending.durationMs ?? 0) + block.metrics.durationMs
  })
  flush()
  return out
})

// The work is open while it runs and folds once the answer starts or the reply ends;
// a click sticks. Keys are the reply's first block index.
const toggledWork = reactive(new Map<number, boolean>())
function workOpen(key: number, reply: Turn) {
  return toggledWork.get(key) ?? (reply.live && reply.answer.every((text) => !text.trim()))
}
function duration(ms: number) {
  const seconds = Math.round(ms / 1000)
  return seconds >= 60 ? `${Math.floor(seconds / 60)}分${seconds % 60}秒` : `${Math.max(seconds, 1)}秒`
}
// Shown while the reader has scrolled up; clicking jumps back and resumes following.
const awayFromBottom = ref(false)
let toBottom = () => {}

watch(content, (body, _previous, onCleanup) => {
  const viewport = body?.closest<HTMLElement>('[data-slot="scroll-area-viewport"]')
  if (!body || !viewport) return

  const tolerance = 2 // scrollTop can be fractional, unlike scrollHeight.
  const bottom = () => Math.max(0, viewport.scrollHeight - viewport.clientHeight)
  let follow = true
  let lastPinned = 0

  const movedUp = (limit: number) => viewport.scrollTop + tolerance < Math.min(lastPinned, limit)

  const pin = () => {
    if (!follow) return
    const limit = bottom()
    if (movedUp(limit)) {
      follow = false
      return
    }
    viewport.scrollTop = limit
    lastPinned = viewport.scrollTop
  }

  const onScroll = () => {
    const limit = bottom()
    if (viewport.scrollTop + tolerance >= limit) {
      follow = true
      lastPinned = viewport.scrollTop
    } else if (movedUp(limit)) {
      // Growth alone must not turn a pending programmatic scroll into a user scroll.
      follow = false
    }
    awayFromBottom.value = !follow
  }

  toBottom = () => {
    follow = true
    awayFromBottom.value = false
    viewport.scrollTo({ top: bottom(), behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' })
    lastPinned = bottom()
  }

  const observer = new ResizeObserver(pin)
  observer.observe(body)
  observer.observe(viewport)
  viewport.addEventListener('scroll', onScroll, { passive: true })
  pin()
  onCleanup(() => {
    viewport.removeEventListener('scroll', onScroll)
    observer.disconnect()
  })
}, { flush: 'post' })
</script>

<template>
  <div class="relative h-full">
  <ScrollArea class="h-full" viewport-class="[overflow-anchor:none]">
    <div ref="content" class="mx-auto flex max-w-3xl flex-col gap-4 px-3 py-4 [overflow-wrap:anywhere] sm:px-6 sm:py-6">
      <p v-if="blocks.length === 0 && !activity" class="text-sm text-muted-foreground">发送一条消息开始。回复、工具和确认都会出现在这里。</p>
      <article v-for="row in rows" :key="row.key" class="flex flex-col gap-2">
        <div v-if="'block' in row && row.block.kind === 'user'" class="ml-auto flex max-w-[85%] flex-col items-end gap-2">
          <figure v-for="(src, imageIndex) in row.block.images || []" :key="imageIndex" class="relative max-w-full">
            <img :src="src" :alt="`图片 ${imageIndex + 1}`" class="max-h-64 rounded-xl" />
            <figcaption class="absolute top-1 left-1 rounded bg-background px-1.5 py-0.5 text-xs font-medium text-foreground">图片 {{ imageIndex + 1 }}</figcaption>
          </figure>
          <ul v-if="row.block.files?.length" aria-label="附带的文本文件" class="flex max-w-full flex-wrap justify-end gap-2">
            <li v-for="(file, fileIndex) in row.block.files" :key="fileIndex" class="max-w-full"><TextFileCard :file="file" /></li>
          </ul>
          <div v-if="row.block.text" class="flex max-w-full flex-col items-start rounded-2xl bg-primary px-4 py-2 text-sm text-primary-foreground">
            <span class="whitespace-pre-wrap">{{ foldable(row.block.text) && !expanded.has(row.key) ? preview(row.block.text) : row.block.text }}</span>
            <button
              v-if="foldable(row.block.text)"
              type="button"
              class="mt-1 inline-flex items-center gap-1 self-end text-xs opacity-75 hover:opacity-100 focus-visible:underline focus-visible:outline-none"
              :aria-expanded="expanded.has(row.key)"
              @click="expanded.has(row.key) ? expanded.delete(row.key) : expanded.add(row.key)"
            >
              <template v-if="expanded.has(row.key)"><ChevronUp class="size-3.5" aria-hidden="true" />收起</template>
              <template v-else><ChevronDown class="size-3.5" aria-hidden="true" />展开全文（{{ row.block.text.split('\n').length }} 行，{{ row.block.text.length }} 字）</template>
            </button>
          </div>
        </div>
        <Button
          v-else-if="'block' in row && row.block.kind === 'note' && row.block.agent"
          type="button"
          variant="outline"
          class="h-auto w-full justify-start gap-2 whitespace-normal px-3 py-2 text-left font-normal"
          :class="row.block.error ? 'text-destructive' : 'text-muted-foreground'"
          @click="'block' in row && row.block.kind === 'note' && row.block.agent && emit('openAgent', row.block.agent)"
        >
          <Bot class="mt-0.5 size-4 shrink-0" />
          <span class="min-w-0 flex-1 whitespace-pre-wrap">{{ row.block.text }}</span>
          <span class="shrink-0 underline">查看</span>
        </Button>
        <div v-else-if="'block' in row && row.block.kind === 'note'" class="rounded-lg border px-3 py-2 text-sm" :class="row.block.error ? 'border-destructive/40 text-destructive' : 'text-muted-foreground'">
          {{ row.block.text }}
        </div>
        <div v-else-if="'reply' in row" class="flex flex-col gap-2">
          <div v-if="row.reply.work.length" class="overflow-hidden rounded-lg border text-sm">
            <button
              type="button"
              class="flex w-full items-center gap-1.5 px-3 py-2 text-left text-muted-foreground hover:text-foreground focus-visible:text-foreground focus-visible:outline-none"
              :aria-expanded="workOpen(row.key, row.reply)"
              @click="toggledWork.set(row.key, !workOpen(row.key, row.reply))"
            >
              <ChevronRight aria-hidden="true" class="size-3.5 shrink-0 transition-transform motion-reduce:transition-none" :class="workOpen(row.key, row.reply) && 'rotate-90'" />
              <LoaderCircle v-if="row.reply.live" aria-hidden="true" class="size-3.5 shrink-0 motion-safe:animate-spin" />
              <span class="font-medium">{{ row.reply.live ? '正在思考' : '思考过程' }}</span>
              <span class="text-xs tabular-nums opacity-75">{{ row.reply.steps }} 步<template v-if="row.reply.durationMs"> · 用时 {{ duration(row.reply.durationMs) }}</template></span>
            </button>
            <div v-if="workOpen(row.key, row.reply)" class="flex flex-col gap-2 border-t px-3 py-2">
              <template v-for="(part, partIndex) in row.reply.work" :key="partIndex">
                <div v-if="part.type === 'reasoning' && !longReasoning(part.text)" class="rounded-lg bg-muted px-3 py-2 text-sm italic text-muted-foreground whitespace-pre-wrap">{{ part.text }}</div>
                <div v-else-if="part.type === 'reasoning'" class="overflow-hidden rounded-lg bg-muted text-sm text-muted-foreground">
                  <button
                    type="button"
                    class="flex w-full items-center gap-1.5 px-3 py-2 text-left hover:text-foreground focus-visible:text-foreground focus-visible:outline-none"
                    :aria-expanded="reasoningOpen(`${row.key}:${partIndex}`, part.text)"
                    @click="toggleReasoning(`${row.key}:${partIndex}`)"
                  >
                    <ChevronRight aria-hidden="true" class="size-3.5 shrink-0 transition-transform motion-reduce:transition-none" :class="reasoningOpen(`${row.key}:${partIndex}`, part.text) && 'rotate-90'" />
                    <span class="font-medium">思考</span>
                    <span class="min-w-0 flex-1 truncate italic opacity-75">{{ reasoningOpen(`${row.key}:${partIndex}`, part.text) ? '' : part.text.replace(/\s+/g, ' ').trim() }}</span>
                    <span class="shrink-0 text-xs tabular-nums opacity-75">{{ part.text.length }} 字</span>
                  </button>
                  <p v-if="reasoningOpen(`${row.key}:${partIndex}`, part.text)" class="px-3 pb-2 italic whitespace-pre-wrap">{{ part.text }}</p>
                </div>
                <ToolOutput v-else-if="part.type === 'tool'" :tool="part" />
                <Markdown v-else-if="part.text" :text="part.text" />
              </template>
            </div>
          </div>
          <Markdown v-for="(text, answerIndex) in row.reply.answer" :key="answerIndex" :text="text" />
        </div>
      </article>
      <p v-if="activity" role="status" class="flex items-center gap-2 text-sm text-muted-foreground">
        <LoaderCircle aria-hidden="true" class="size-4 motion-safe:animate-spin" />
        {{ activity }}
      </p>
    </div>
  </ScrollArea>
  <div class="pointer-events-none absolute inset-x-0 bottom-3 z-10 flex justify-center">
    <Transition enter-from-class="translate-y-2 opacity-0" leave-to-class="translate-y-2 opacity-0" enter-active-class="transition duration-150 motion-reduce:transition-none" leave-active-class="transition duration-150 motion-reduce:transition-none">
      <Button v-if="awayFromBottom" type="button" size="icon" variant="outline" class="pointer-events-auto size-8 rounded-full bg-background shadow-md" aria-label="回到底部" title="回到底部" @click="toBottom()">
        <ArrowDown />
      </Button>
    </Transition>
  </div>
  </div>
</template>
