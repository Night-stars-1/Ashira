<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { VisAxis, VisStackedBar, VisXYContainer } from '@unovis/vue'
import { Button } from '@/components/ui/button'
import { type ChartConfig, ChartContainer, ChartCrosshair, ChartTooltip, ChartTooltipContent, componentToString } from '@/components/ui/chart'
import { Empty } from '@/components/ui/empty'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import ListSkeleton from '@/components/ListSkeleton.vue'
import { api, type UsageBucket, type UsageReport } from '@/lib/api'

const props = defineProps<{ active: boolean }>()

const report = ref<UsageReport | null>(null)
const failed = ref('')
const ranges = [
  { id: '24h', label: '24 小时' }, { id: 'today', label: '今天' },
  { id: '7d', label: '7 天' }, { id: '30d', label: '30 天' }, { id: 'all', label: '全部' },
] as const
type Range = typeof ranges[number]['id']
const range = ref<Range>('30d')
/** Short ranges chart by hour, the rest by day. */
const hourly = computed(() => range.value === '24h' || range.value === 'today')

watch(() => props.active, async (value) => {
  if (!value) return
  failed.value = ''
  try {
    report.value = await api.usage()
  } catch (reason) {
    failed.value = reason instanceof Error ? reason.message : '读取用量失败'
  }
}, { immediate: true })

const tokens = new Intl.NumberFormat('zh-CN')
const compact = new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 1 })
const dollars = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 4 })
const percent = new Intl.NumberFormat('zh-CN', { style: 'percent', maximumFractionDigits: 1 })

const pad = (value: number) => String(value).padStart(2, '0')
/** Local calendar day of an hour bucket, `YYYY-MM-DD`. */
function dayOf(hour: number): string {
  const date = new Date(hour * 3_600_000)
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

const nowHour = () => Math.floor(Date.now() / 3_600_000)
/** First hour (unix hours, may be fractional at local midnight) inside the range. */
const since = computed(() => {
  if (range.value === 'all') return 0
  if (range.value === '24h') return nowHour() - 23
  const start = new Date()
  start.setHours(0, 0, 0, 0)
  start.setDate(start.getDate() - { today: 0, '7d': 6, '30d': 29 }[range.value])
  return start.getTime() / 3_600_000
})
const rows = computed(() => (report.value?.buckets ?? []).filter((bucket) => bucket.hour + 1 > since.value))

type Sums = { tokens: number; input: number; output: number; cacheRead: number; prompt: number; cost: number; unknown: number; requests: number; timedOutput: number; durationMs: number; firstOutputTotalMs: number; firstOutputSamples: number; firstTextTotalMs: number; firstTextSamples: number }
const empty = (): Sums => ({ tokens: 0, input: 0, output: 0, cacheRead: 0, prompt: 0, cost: 0, unknown: 0, requests: 0, timedOutput: 0, durationMs: 0, firstOutputTotalMs: 0, firstOutputSamples: 0, firstTextTotalMs: 0, firstTextSamples: 0 })
function add(sums: Sums, bucket: UsageBucket) {
  const prompt = bucket.input + bucket.cacheRead + bucket.cacheWrite
  sums.tokens += prompt + bucket.output
  sums.input += prompt
  sums.output += bucket.output
  sums.cacheRead += bucket.cacheRead
  sums.prompt += prompt
  sums.cost += bucket.cost
  sums.unknown += bucket.unknown
  sums.requests += bucket.requests
  sums.timedOutput += bucket.timedOutput
  sums.durationMs += bucket.durationMs
  sums.firstOutputTotalMs += bucket.firstOutputTotalMs
  sums.firstOutputSamples += bucket.firstOutputSamples
  sums.firstTextTotalMs += bucket.firstTextTotalMs
  sums.firstTextSamples += bucket.firstTextSamples
}
/** Share of prompt tokens served from cache. */
const cacheRate = (sums: Sums) => sums.prompt ? percent.format(sums.cacheRead / sums.prompt) : '—'
const costLabel = (sums: Sums) => sums.unknown ? (sums.cost > 0 ? `≥ ${dollars.format(sums.cost)}` : '未知') : dollars.format(sums.cost)
/** Average output speed over the requests that logged their generation time. */
const tps = (sums: Sums) => sums.durationMs ? (sums.timedOutput * 1000 / sums.durationMs).toFixed(1) : '—'
const averageLatency = (totalMs: number, samples: number) => samples ? `${(totalMs / samples / 1000).toFixed(3)} 秒` : '—'

const totals = computed(() => {
  const all = empty()
  for (const bucket of rows.value) add(all, bucket)
  const items = [
    { label: 'Token', value: compact.format(all.tokens), title: `${tokens.format(all.tokens)} tokens（输入 ${tokens.format(all.input)} · 输出 ${tokens.format(all.output)}）` },
    { label: '缓存命中', value: cacheRate(all), title: `输入中 ${tokens.format(all.cacheRead)} / ${tokens.format(all.prompt)} tokens 来自缓存` },
    { label: '费用', value: costLabel(all), title: all.unknown ? `${all.unknown} 个请求没有单价，未计入` : '' },
    { label: '请求', value: tokens.format(all.requests), title: '' },
  ]
  if (!hourly.value) items.push({ label: '活跃天数', value: String(new Set(rows.value.map((bucket) => dayOf(bucket.hour))).size), title: '' })
  return items
})

/** Per model, most tokens first. */
const models = computed(() => {
  const byModel = new Map<string, Sums>()
  for (const bucket of rows.value) {
    const sums = byModel.get(bucket.model) ?? empty()
    add(sums, bucket)
    byModel.set(bucket.model, sums)
  }
  return [...byModel.entries()].map(([model, sums]) => ({ model, sums })).sort((left, right) => right.sums.tokens - left.sums.tokens)
})

type Slot = { index: number; tick: string; title: string; input: number; output: number }
/** One bar per hour or local day in the range; empty slots stay so gaps read as gaps. */
const slots = computed<Slot[]>(() => {
  const keyOf = hourly.value ? (hour: number) => String(hour) : dayOf
  const sums = new Map<string, { input: number; output: number }>()
  for (const bucket of rows.value) {
    const slot = sums.get(keyOf(bucket.hour)) ?? { input: 0, output: 0 }
    slot.input += bucket.input + bucket.cacheRead + bucket.cacheWrite
    slot.output += bucket.output
    sums.set(keyOf(bucket.hour), slot)
  }
  if (sums.size === 0) return []
  const result: Slot[] = []
  const push = (key: string, tick: string, title: string) => result.push({ index: result.length, tick, title, ...(sums.get(key) ?? { input: 0, output: 0 }) })
  if (hourly.value) {
    for (let hour = Math.ceil(since.value); hour <= nowHour(); hour++) {
      const clock = `${pad(new Date(hour * 3_600_000).getHours())}:00`
      push(String(hour), clock, `${dayOf(hour)} ${clock}`)
    }
  } else {
    const first = range.value === 'all' ? new Date(`${[...sums.keys()].sort()[0]}T00:00:00`) : new Date(since.value * 3_600_000)
    for (const date = first; date <= new Date(); date.setDate(date.getDate() + 1)) {
      const day = dayOf(date.getTime() / 3_600_000)
      push(day, day.slice(5).replace('-', '/'), day)
    }
  }
  return result
})

const config = {
  input: { label: '输入', color: 'var(--color-sky-500)' },
  output: { label: '输出', color: 'var(--color-amber-500)' },
} satisfies ChartConfig
const x = (slot: Slot) => slot.index
const ys = [(slot: Slot) => slot.input, (slot: Slot) => slot.output]
const tickLabel = (index: number) => slots.value[index]?.tick ?? ''
const xTicks = computed(() => {
  const count = slots.value.length
  const step = Math.max(1, Math.ceil(count / 7))
  return slots.value.filter((slot) => slot.index % step === 0 || slot.index === count - 1).map((slot) => slot.index)
})
const render = componentToString(config, ChartTooltipContent, { labelFormatter: (index: number | Date) => slots.value[Number(index)]?.title ?? '' })
const tooltip = (slot: Slot, at: number | Date) => render?.({ input: compact.format(slot.input), output: compact.format(slot.output) }, at)
</script>

<template>
  <p v-if="failed" role="alert" class="text-sm text-destructive">{{ failed }}</p>
  <ListSkeleton v-else-if="!report" />
  <template v-else>
    <div class="flex flex-wrap items-center gap-2">
      <p class="min-w-0 flex-1 text-xs text-muted-foreground">所有会话（含已归档、子代理）· 共 {{ report.sessions }} 个会话</p>
      <div class="flex flex-wrap gap-1" role="group" aria-label="统计范围">
        <Button v-for="item in ranges" :key="item.id" type="button" size="sm" class="h-7 px-2 text-xs" :variant="range === item.id ? 'secondary' : 'ghost'" :aria-pressed="range === item.id" @click="range = item.id">{{ item.label }}</Button>
      </div>
    </div>
    <Empty v-if="rows.length === 0" title="这段时间没有用量" description="发起对话后，这里会按时间和模型汇总所有会话的 Token 与费用。" />
    <template v-else>
      <dl class="grid grid-cols-2 gap-3 rounded-lg border bg-muted/30 p-3 tabular-nums" :class="totals.length === 5 ? 'sm:grid-cols-5' : 'sm:grid-cols-4'">
        <div v-for="item in totals" :key="item.label" :title="item.title">
          <dt class="text-xs text-muted-foreground">{{ item.label }}</dt>
          <dd class="mt-1 text-lg font-semibold">{{ item.value }}</dd>
        </div>
      </dl>
      <section :aria-label="hourly ? '每小时 Token' : '每日 Token'" class="rounded-lg border p-3">
        <h3 class="mb-2 text-sm font-medium">{{ hourly ? '每小时 Token' : '每日 Token' }}</h3>
        <ChartContainer :config="config" class="aspect-auto h-44 w-full" cursor>
          <VisXYContainer :data="slots" :margin="{ left: 4, right: 4, top: 8 }">
            <VisStackedBar :x="x" :y="ys" :color="[config.input.color, config.output.color]" :rounded-corners="2" :bar-padding="0.2" />
            <VisAxis type="x" :x="x" :tick-values="xTicks" :tick-format="tickLabel" :grid-line="false" :domain-line="false" :tick-line="false" />
            <VisAxis type="y" :num-ticks="3" :tick-format="(amount: number) => compact.format(amount)" :domain-line="false" :tick-line="false" />
            <ChartTooltip />
            <ChartCrosshair :template="tooltip" :color="[config.input.color, config.output.color]" />
          </VisXYContainer>
        </ChartContainer>
      </section>
      <section aria-label="按模型" class="overflow-hidden rounded-lg border">
        <Table class="tabular-nums">
          <TableHeader>
            <TableRow>
              <TableHead>模型</TableHead>
              <TableHead class="text-right">Token</TableHead>
              <TableHead class="text-right">缓存命中</TableHead>
              <TableHead class="text-right">请求</TableHead>
              <TableHead class="text-right" title="平均输出速度（tokens/秒）">TPS</TableHead>
              <TableHead class="text-right" title="从模型请求开始到收到首个有内容的输出块，包括思考和工具调用">平均首输出块</TableHead>
              <TableHead class="text-right" title="从模型请求开始到收到首段非空白正文；思考内容和工具调用不算正文">平均首段正文</TableHead>
              <TableHead class="text-right">费用</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow v-for="{ model, sums } in models" :key="model">
              <TableCell class="max-w-56 truncate" :title="model">{{ model }}</TableCell>
              <TableCell class="text-right" :title="`输入 ${tokens.format(sums.input)} · 输出 ${tokens.format(sums.output)}`">{{ compact.format(sums.tokens) }}</TableCell>
              <TableCell class="text-right">{{ cacheRate(sums) }}</TableCell>
              <TableCell class="text-right">{{ tokens.format(sums.requests) }}</TableCell>
              <TableCell class="text-right tabular-nums" :title="sums.durationMs ? `${tokens.format(sums.timedOutput)} tokens / ${(sums.durationMs / 1000).toFixed(1)} 秒` : '没有带耗时的请求'">{{ tps(sums) }}</TableCell>
              <TableCell class="text-right" :title="`有记录的请求：${sums.firstOutputSamples}`">{{ averageLatency(sums.firstOutputTotalMs, sums.firstOutputSamples) }}</TableCell>
              <TableCell class="text-right" :title="`有记录的请求：${sums.firstTextSamples}`">{{ averageLatency(sums.firstTextTotalMs, sums.firstTextSamples) }}</TableCell>
              <TableCell class="text-right" :class="sums.unknown && !sums.cost ? 'text-muted-foreground' : ''">{{ costLabel(sums) }}</TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </section>
    </template>
  </template>
</template>
