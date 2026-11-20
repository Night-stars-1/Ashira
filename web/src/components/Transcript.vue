<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
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

defineProps<{ blocks: Block[]; activity?: string }>()
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
      <article v-for="(block, index) in blocks" :key="index" class="flex flex-col gap-2">
        <div v-if="block.kind === 'user'" class="ml-auto flex max-w-[85%] flex-col items-end gap-2">
          <figure v-for="(src, imageIndex) in block.images || []" :key="imageIndex" class="relative max-w-full">
            <img :src="src" :alt="`图片 ${imageIndex + 1}`" class="max-h-64 rounded-xl" />
            <figcaption class="absolute top-1 left-1 rounded bg-background px-1.5 py-0.5 text-xs font-medium text-foreground">图片 {{ imageIndex + 1 }}</figcaption>
          </figure>
          <ul v-if="block.files?.length" aria-label="附带的文本文件" class="flex max-w-full flex-wrap justify-end gap-2">
            <li v-for="(file, fileIndex) in block.files" :key="fileIndex" class="max-w-full"><TextFileCard :file="file" /></li>
          </ul>
          <div v-if="block.text" class="flex max-w-full flex-col items-start rounded-2xl bg-primary px-4 py-2 text-sm text-primary-foreground">
            <span class="whitespace-pre-wrap">{{ foldable(block.text) && !expanded.has(index) ? preview(block.text) : block.text }}</span>
            <button
              v-if="foldable(block.text)"
              type="button"
              class="mt-1 inline-flex items-center gap-1 self-end text-xs opacity-75 hover:opacity-100 focus-visible:underline focus-visible:outline-none"
              :aria-expanded="expanded.has(index)"
              @click="expanded.has(index) ? expanded.delete(index) : expanded.add(index)"
            >
              <template v-if="expanded.has(index)"><ChevronUp class="size-3.5" aria-hidden="true" />收起</template>
              <template v-else><ChevronDown class="size-3.5" aria-hidden="true" />展开全文（{{ block.text.split('\n').length }} 行，{{ block.text.length }} 字）</template>
            </button>
          </div>
        </div>
        <Button
          v-else-if="block.kind === 'note' && block.agent"
          type="button"
          variant="outline"
          class="h-auto w-full justify-start gap-2 whitespace-normal px-3 py-2 text-left font-normal"
          :class="block.error ? 'text-destructive' : 'text-muted-foreground'"
          @click="emit('openAgent', block.agent)"
        >
          <Bot class="mt-0.5 size-4 shrink-0" />
          <span class="min-w-0 flex-1 whitespace-pre-wrap">{{ block.text }}</span>
          <span class="shrink-0 underline">查看</span>
        </Button>
        <div v-else-if="block.kind === 'note'" class="rounded-lg border px-3 py-2 text-sm" :class="block.error ? 'border-destructive/40 text-destructive' : 'text-muted-foreground'">
          {{ block.text }}
        </div>
        <div v-else class="flex flex-col gap-2">
          <template v-for="(part, partIndex) in block.parts" :key="partIndex">
            <div v-if="part.type === 'reasoning' && !longReasoning(part.text)" class="rounded-lg bg-muted px-3 py-2 text-sm italic text-muted-foreground whitespace-pre-wrap">{{ part.text }}</div>
            <div v-else-if="part.type === 'reasoning'" class="overflow-hidden rounded-lg bg-muted text-sm text-muted-foreground">
              <button
                type="button"
                class="flex w-full items-center gap-1.5 px-3 py-2 text-left hover:text-foreground focus-visible:text-foreground focus-visible:outline-none"
                :aria-expanded="reasoningOpen(`${index}:${partIndex}`, part.text)"
                @click="toggleReasoning(`${index}:${partIndex}`)"
              >
                <ChevronRight aria-hidden="true" class="size-3.5 shrink-0 transition-transform motion-reduce:transition-none" :class="reasoningOpen(`${index}:${partIndex}`, part.text) && 'rotate-90'" />
                <span class="font-medium">思考</span>
                <span class="min-w-0 flex-1 truncate italic opacity-75">{{ reasoningOpen(`${index}:${partIndex}`, part.text) ? '' : part.text.replace(/\s+/g, ' ').trim() }}</span>
                <span class="shrink-0 text-xs tabular-nums opacity-75">{{ part.text.length }} 字</span>
              </button>
              <p v-if="reasoningOpen(`${index}:${partIndex}`, part.text)" class="px-3 pb-2 italic whitespace-pre-wrap">{{ part.text }}</p>
            </div>
            <ToolOutput v-else-if="part.type === 'tool'" :tool="part" />
            <Markdown v-else-if="part.text" :text="part.text" />
          </template>
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
