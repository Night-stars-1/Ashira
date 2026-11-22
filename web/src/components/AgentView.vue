<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import AgentTranscript, { type Block } from '@/components/Transcript.vue'
import { ScrollArea } from '@/components/ui/scroll-area'
import TranscriptSkeleton from '@/components/TranscriptSkeleton.vue'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { api } from '@/lib/api'
import { historyBlocks } from '@/lib/session'

const props = defineProps<{ id: string }>()
const emit = defineEmits<{ close: [] }>()

const blocks = ref<Block[]>([])
/** A background command's output: shown as the terminal printed it, not as a reply. */
const output = ref<string | null>(null)
const error = ref('')
const loading = ref(false)

const visible = computed(() => blocks.value.map((block) => block.kind === 'user' ? { ...block, text: taskText(block.text) } : block))

watch(() => props.id, (id) => {
  blocks.value = []
  output.value = null
  error.value = ''
  if (!id) return
  loading.value = true
  api.subagent(id).then((agent) => {
    if (props.id !== id) return
    if (agent.shell) {
      output.value = agent.report
      return
    }
    const parsed = historyBlocks({ messages: agent.messages })
    blocks.value = parsed.length > 0 ? parsed : agent.report ? [{ kind: 'assistant', parts: [{ type: 'text', text: agent.report }], live: false }] : []
  }).catch((reason: unknown) => {
    if (props.id === id) error.value = reason instanceof Error ? reason.message : '无法读取子任务'
  }).finally(() => {
    if (props.id === id) loading.value = false
  })
}, { immediate: true })

function taskText(text: string): string {
  const mark = '\nTask:\n'
  const at = text.lastIndexOf(mark)
  return at >= 0 ? text.slice(at + mark.length).trim() : text
}
</script>

<template>
  <Dialog :open="id !== ''" @update:open="(open) => { if (!open) emit('close') }">
    <DialogContent class="flex h-[85vh] w-[min(1480px,calc(100%-2rem))] max-w-none flex-col gap-0 overflow-hidden p-0 sm:max-w-none">
      <DialogHeader class="border-b px-6 py-4">
        <DialogTitle>{{ output !== null ? '后台命令' : '子任务' }}</DialogTitle>
      </DialogHeader>
      <p v-if="error" class="px-6 py-4 text-sm text-destructive">{{ error }}</p>
      <TranscriptSkeleton v-else-if="loading" />
      <ScrollArea v-else-if="output !== null" class="min-h-0 flex-1">
        <pre v-if="output" class="px-6 py-4 font-mono text-xs leading-relaxed whitespace-pre-wrap [overflow-wrap:anywhere]">{{ output }}</pre>
        <p v-else class="px-6 py-4 text-sm text-muted-foreground">没有输出</p>
      </ScrollArea>
      <div v-else class="min-h-0 flex-1">
        <AgentTranscript v-if="visible.length > 0" :blocks="visible" />
        <p v-else class="px-6 py-4 text-sm text-muted-foreground">没有对话</p>
      </div>
    </DialogContent>
  </Dialog>
</template>
