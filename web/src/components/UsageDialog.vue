<script setup lang="ts">
import { computed } from 'vue'
import ReplyUsageChart from '@/components/ReplyUsageChart.vue'
import type { Block } from '@/components/Transcript.vue'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { ScrollArea } from '@/components/ui/scroll-area'
import type { Status } from '@/lib/api'

const props = defineProps<{ status: Status | null; blocks: Block[] }>()
const open = defineModel<boolean>('open', { required: true })

const tokens = new Intl.NumberFormat('zh-CN')
const compact = new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 1 })
const dollars = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 4 })
const percent = new Intl.NumberFormat('zh-CN', { style: 'percent', maximumFractionDigits: 1 })

/** This session only: its own replies plus the subagents it started. */
const summary = computed(() => {
  const status = props.status
  if (!status) return []
  const { input, output, cacheRead, cacheWrite } = status.usage
  const prompt = input + cacheRead + cacheWrite
  const total = prompt + output
  const cost = status.cost.main + status.cost.child
  const unknown = status.cost.mainUnknown || status.cost.childUnknown
  return [
    { label: 'Token', value: compact.format(total), title: `${tokens.format(total)} tokens（输入 ${tokens.format(prompt)} · 输出 ${tokens.format(output)}）` },
    { label: '缓存命中', value: prompt ? percent.format(cacheRead / prompt) : '—', title: `输入中 ${tokens.format(cacheRead)} / ${tokens.format(prompt)} tokens 来自缓存` },
    { label: '费用', value: unknown ? (cost > 0 ? `≥ ${dollars.format(cost)}` : '未知') : dollars.format(cost), title: unknown ? '部分请求没有单价，未计入' : '' },
    { label: '请求', value: tokens.format(status.requests.main + status.requests.child), title: `主代理 ${status.requests.main} · 子代理 ${status.requests.child}` },
    { label: '上下文', value: status.window ? `${Math.round(status.context / status.window * 100)}%` : compact.format(status.context), title: `${tokens.format(status.context)} / ${status.window ? tokens.format(status.window) : '—'} tokens` },
  ]
})
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="flex max-h-[85dvh] flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl">
      <DialogHeader class="shrink-0 px-5 pt-5 pb-4 pr-12">
        <DialogTitle>本会话用量</DialogTitle>
        <DialogDescription>只统计这个会话（含它启动的子代理）。所有会话的用量在 设置 → 用量。</DialogDescription>
      </DialogHeader>
      <ScrollArea v-if="status" type="auto" class="grid min-h-0 min-w-0 grid-rows-[minmax(0,1fr)]" viewport-class="h-auto min-h-0 px-5 pb-5">
        <div class="grid min-w-0 gap-4">
          <dl class="grid grid-cols-2 gap-3 rounded-lg border bg-muted/30 p-3 tabular-nums sm:grid-cols-5">
            <div v-for="item in summary" :key="item.label" :title="item.title">
              <dt class="text-xs text-muted-foreground">{{ item.label }}</dt>
              <dd class="mt-1 text-lg font-semibold">{{ item.value }}</dd>
            </div>
          </dl>
          <ReplyUsageChart :key="status.sessionId" :blocks="blocks" />
        </div>
      </ScrollArea>
      <p v-else class="px-5 pb-5 text-muted-foreground">请选择会话。</p>
    </DialogContent>
  </Dialog>
</template>
