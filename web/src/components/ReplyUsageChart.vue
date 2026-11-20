<script setup lang="ts">
import { computed, ref } from 'vue'
import { CurveType } from '@unovis/ts'
import { VisArea, VisAxis, VisLine, VisScatter, VisXYContainer } from '@unovis/vue'
import { Button } from '@/components/ui/button'
import { type ChartConfig, ChartContainer, ChartCrosshair, ChartTooltip, ChartTooltipContent, componentToString } from '@/components/ui/chart'
import type { Block } from '@/components/Transcript.vue'
import type { ReplyMetrics } from '@/lib/api'

const props = defineProps<{ blocks: Block[] }>()
const choices = [
  { key: 'total', label: '总 Token' }, { key: 'output', label: '输出' },
  { key: 'hit', label: '缓存命中' }, { key: 'rate', label: 'TPS' }, { key: 'cost', label: '费用' },
] as const
type Metric = typeof choices[number]['key']
const metric = ref<Metric>('total')
const pageSize = 30
const pageOffset = ref(0)
const replies = computed(() => props.blocks
  .filter((block): block is Extract<Block, { kind: 'assistant' }> => block.kind === 'assistant' && !block.live)
  .map((block, index) => ({ number: index + 1, metrics: block.metrics, text: block.parts.flatMap((part) => part.type === 'text' ? [part.text] : []).join('\n') })))
const end = computed(() => Math.max(0, replies.value.length - pageOffset.value * pageSize))
const page = computed(() => replies.value.slice(Math.max(0, end.value - pageSize), end.value))
const tokens = new Intl.NumberFormat('zh-CN')
const short = new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 1 })
const dollars = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumSignificantDigits: 5 })
const percent = new Intl.NumberFormat('zh-CN', { style: 'percent', maximumFractionDigits: 1 })

function value(reply: ReplyMetrics | null | undefined, key: Metric): number | null {
  if (!reply) return null
  if (key === 'rate' || key === 'cost') return reply[key]
  if (!reply.usage) return null
  const { input, output, cacheRead, cacheWrite } = reply.usage
  const prompt = input + cacheRead + cacheWrite
  // Share of this reply's prompt read from cache.
  if (key === 'hit') return prompt ? cacheRead / prompt : null
  return key === 'output' ? output : prompt + output
}

function format(amount: number | null, key: Metric, compact = false) {
  if (amount === null) return '—'
  if (key === 'cost') return dollars.format(amount)
  if (key === 'hit') return percent.format(amount)
  if (key === 'rate') return `${amount.toFixed(1)}${compact ? '' : ' tok/s'}`
  return (compact ? short : tokens).format(amount)
}

type Point = { number: number; value: number | null }
const data = computed<Point[]>(() => page.value.map((row) => ({ number: row.number, value: value(row.metrics, metric.value) })))
const hasData = computed(() => data.value.some((point) => point.value !== null))
const label = computed(() => choices.find((choice) => choice.key === metric.value)!.label)
const config = computed<ChartConfig>(() => ({ value: { label: label.value, color: 'var(--color-sky-500)' } }))

const x = (point: Point) => point.number
const y = (point: Point) => point.value ?? undefined
/** Whole reply numbers only; a short page would otherwise get fractional ticks. */
const xTicks = computed(() => {
  const numbers = data.value.map((point) => point.number)
  if (numbers.length <= 6) return numbers
  const step = Math.ceil(numbers.length / 6)
  return numbers.filter((_, index) => index % step === 0 || index === numbers.length - 1)
})
const tooltip = computed(() => componentToString(config.value, ChartTooltipContent, {
  labelFormatter: (number: number | Date) => `回复 #${number}`,
}))
// The tooltip shows the formatted value, not the raw number.
const tooltipTemplate = computed(() => {
  const render = tooltip.value
  return render ? (point: Point, at: number | Date) => render({ value: format(point.value, metric.value) }, at) : undefined
})
</script>

<template>
  <section aria-label="逐回复统计" class="min-w-0 space-y-3">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h3 class="font-medium">逐回复趋势</h3>
      <div v-if="replies.length" class="flex flex-wrap gap-1" role="group" aria-label="趋势指标">
        <Button v-for="choice in choices" :key="choice.key" size="sm" class="h-7 px-2 text-xs" :variant="metric === choice.key ? 'secondary' : 'ghost'" :aria-pressed="metric === choice.key" @click="metric = choice.key">{{ choice.label }}</Button>
      </div>
    </div>
    <p v-if="!replies.length" class="text-xs text-muted-foreground">暂无已完成的回复。</p>
    <template v-else>
      <div class="rounded-lg border p-3">
        <ChartContainer v-if="hasData" :config="config" class="aspect-auto h-44 w-full" cursor role="img" :aria-label="`逐回复${label}折线图`">
          <VisXYContainer :data="data" :margin="{ left: 4, right: 12, top: 8 }" :y-domain="metric === 'hit' ? [0, 1] : [0, undefined]">
            <VisArea :x="x" :y="y" color="var(--color-value)" :opacity="0.15" :curve-type="CurveType.MonotoneX" />
            <VisLine :x="x" :y="y" color="var(--color-value)" :line-width="2" :curve-type="CurveType.MonotoneX" />
            <!-- A lone reply has no line; the dot keeps it visible. -->
            <VisScatter v-if="data.length === 1" :x="x" :y="y" color="var(--color-value)" :size="8" />
            <VisAxis type="x" :x="x" :tick-values="xTicks" :tick-format="(number: number) => `#${number}`" :grid-line="false" :domain-line="false" :tick-line="false" />
            <VisAxis type="y" :num-ticks="3" :tick-format="(amount: number) => format(amount, metric, true)" :domain-line="false" :tick-line="false" />
            <ChartTooltip />
            <ChartCrosshair :template="tooltipTemplate" color="var(--color-value)" />
          </VisXYContainer>
        </ChartContainer>
        <p v-else class="px-2 py-6 text-center text-xs text-muted-foreground">这组回复没有记录此项数据。</p>
      </div>
      <div v-if="replies.length > pageSize" class="flex flex-wrap items-center justify-end gap-1">
        <Button size="sm" variant="ghost" :disabled="end <= pageSize" @click="pageOffset++">较早</Button>
        <Button size="sm" variant="ghost" :disabled="pageOffset === 0" @click="pageOffset--">较新</Button>
      </div>
      <details class="rounded-lg border">
        <summary class="cursor-pointer px-3 py-2 text-xs font-medium">逐回复明细</summary>
        <div class="overflow-x-auto">
          <table aria-label="逐回复用量明细" class="w-full text-right text-xs whitespace-nowrap tabular-nums">
            <thead class="border-t border-b text-muted-foreground">
              <tr><th scope="col" class="px-3 py-2 text-left">回复</th><th scope="col" class="px-2">输入</th><th scope="col" class="px-2">输出</th><th scope="col" class="px-2">缓存读 / 写</th><th scope="col" class="px-2">缓存命中</th><th scope="col" class="px-2">TPS</th><th scope="col" class="px-3">估算费用</th></tr>
            </thead>
            <tbody class="divide-y">
              <tr v-for="row in page" :key="row.number">
                <th scope="row" class="px-3 py-2 text-left font-medium" :title="row.text.slice(0, 120)">#{{ row.number }}<span v-if="row.metrics?.model" class="ml-2 text-muted-foreground">{{ row.metrics.model }}</span></th>
                <td class="px-2">{{ row.metrics?.usage ? tokens.format(row.metrics.usage.input + row.metrics.usage.cacheRead + row.metrics.usage.cacheWrite) : '—' }}</td>
                <td class="px-2">{{ format(value(row.metrics, 'output'), 'output') }}</td>
                <td class="px-2">{{ row.metrics?.usage ? `${tokens.format(row.metrics.usage.cacheRead)} / ${tokens.format(row.metrics.usage.cacheWrite)}` : '—' }}</td>
                <td class="px-2">{{ format(value(row.metrics, 'hit'), 'hit') }}</td>
                <td class="px-2">{{ format(value(row.metrics, 'rate'), 'rate', true) }}</td>
                <td class="px-3">{{ format(value(row.metrics, 'cost'), 'cost') }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </details>
    </template>
  </section>
</template>
