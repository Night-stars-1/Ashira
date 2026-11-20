<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { api, type ModelItem } from '@/lib/api'
import ListSkeleton from '@/components/ListSkeleton.vue'

const open = defineModel<boolean>('open', { required: true })
const props = defineProps<{ current: string; fast: boolean }>()
const emit = defineEmits<{ select: [selector: string]; fast: [enabled: boolean] }>()

const query = ref('')
const level = ref('默认')
const group = ref('')
const models = ref<ModelItem[]>([])
const levels = ref<string[]>([])
const loading = ref(false)
const failed = ref('')
// The model the level row describes: the current one, then whichever was clicked.
const picked = ref('')

const matched = computed(() => {
  const terms = query.value.trim().toLowerCase().split(/\s+/).filter(Boolean)
  return models.value.filter((model) => {
    const hay = `${model.selector} ${model.name ?? ''} ${model.provider}`.toLowerCase()
    return terms.every((term) => hay.includes(term))
  })
})

const groups = computed(() => {
  const counts = new Map<string, number>()
  for (const model of matched.value) counts.set(model.provider, (counts.get(model.provider) ?? 0) + 1)
  return [...counts.entries()].sort((left, right) => left[0].localeCompare(right[0]))
})

const shown = computed(() => group.value ? matched.value.filter((model) => model.provider === group.value) : matched.value)
const currentMissing = computed(() => Boolean(props.current) && !models.value.some((model) => model.selector === currentBase(props.current)))

// Only levels the model accepts, like the TUI; unknown support (no models.dev data) offers all.
const pickedModel = computed(() => models.value.find((model) => model.selector === picked.value))
const available = computed(() => {
  const efforts = pickedModel.value?.efforts ?? []
  return levels.value.filter((item) => item === '默认' || !efforts.length || efforts.includes(item) || (item === 'off' && efforts.includes('none')))
})

function currentLevel(selector: string) {
  const split = selector.lastIndexOf(':')
  if (split <= 0) return '默认'
  const tail = selector.slice(split + 1)
  return levels.value.includes(tail) ? tail : '默认'
}

function currentBase(selector: string) {
  const split = selector.lastIndexOf(':')
  if (split <= 0) return selector
  const tail = selector.slice(split + 1)
  return levels.value.includes(tail) ? selector.slice(0, split) : selector
}

watch(open, async (visible) => {
  if (!visible) return
  loading.value = true
  failed.value = ''
  try {
    const catalog = await api.models()
    models.value = catalog.models
    levels.value = catalog.levels.length ? catalog.levels : ['默认']
    level.value = currentLevel(props.current)
    picked.value = currentBase(props.current)
    const provider = props.current.split('/')[0]
    group.value = catalog.models.some((model) => model.provider === provider) ? provider : ''
  } catch (reason) {
    failed.value = reason instanceof Error ? reason.message : '无法加载模型'
  } finally {
    loading.value = false
  }
})

// Picking applies immediately and keeps the dialog open so the level can follow.
function apply() {
  if (!picked.value) return
  emit('select', level.value === '默认' ? picked.value : `${picked.value}:${level.value}`)
}

function choose(model: ModelItem) {
  picked.value = model.selector
  // A level the new model doesn't accept falls back to 默认 instead of being sent.
  if (!available.value.includes(level.value)) level.value = '默认'
  apply()
}

function chooseLevel(item: string) {
  level.value = item
  apply()
}

function contextLabel(value: number) {
  if (value >= 1_000_000) return `${trim(value / 1_000_000)}m`
  if (value >= 1_000) return `${trim(value / 1_000)}k`
  return String(value)
}

function priceLabel(model: ModelItem) {
  if (model.input === null || model.output === null) return ''
  return `$${rate(model.input)} / $${rate(model.output)}`
}

function trim(value: number) {
  return value.toFixed(1).replace(/\.0$/, '')
}

function rate(value: number) {
  if (Number.isInteger(value)) return String(value)
  return String(Number(value.toFixed(4)))
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="flex max-h-[90dvh] flex-col gap-3 overflow-hidden sm:max-w-2xl" :aria-describedby="undefined">
      <DialogHeader>
        <DialogTitle>切换模型</DialogTitle>
      </DialogHeader>
      <Input v-model="query" class="shrink-0" placeholder="搜索服务商或模型" />
      <div class="flex shrink-0 items-start gap-2 sm:items-center">
        <div class="flex min-w-0 flex-1 flex-wrap items-center gap-1">
          <span class="mr-1 text-xs text-muted-foreground">思考级别</span>
          <Button v-for="item in available" :key="item" type="button" size="sm" :variant="item === level ? 'default' : 'outline'" :aria-pressed="item === level" :disabled="!picked" @click="chooseLevel(item)">{{ item }}</Button>
        </div>
        <label v-if="pickedModel?.fast" class="flex shrink-0 items-center gap-2 text-sm" title="OpenAI fast：发送 service_tier=fast，费用更高">
          Fast
          <Switch :model-value="fast" @update:model-value="emit('fast', Boolean($event))" />
        </label>
      </div>
      <div v-if="loading" class="flex h-80 min-h-0 overflow-hidden rounded-md border">
        <div class="hidden w-36 shrink-0 border-r sm:block"><ListSkeleton :rows="6" /></div>
        <div class="min-w-0 flex-1"><ListSkeleton :rows="6" two-line /></div>
      </div>
      <p v-else-if="failed" class="text-sm text-destructive">{{ failed }}</p>
      <div v-else class="flex h-80 min-h-0 flex-col overflow-hidden rounded-md border sm:flex-row">
        <!-- reka forbids '' as an item value; 'all' stands for no provider filter. -->
        <Select :model-value="group || 'all'" @update:model-value="(value) => group = value === 'all' ? '' : String(value)">
          <SelectTrigger aria-label="服务商" class="h-10 w-full shrink-0 rounded-none border-0 border-b sm:hidden">
            <SelectValue />
          </SelectTrigger>
          <SelectContent position="popper">
            <SelectItem value="all">全部服务商（{{ matched.length }}）</SelectItem>
            <SelectItem v-for="[name, count] in groups" :key="name" :value="name">{{ name }}（{{ count }}）</SelectItem>
          </SelectContent>
        </Select>
        <aside class="hidden w-36 shrink-0 flex-col border-r sm:flex">
          <ScrollArea class="min-h-0 flex-1">
            <Button type="button" variant="ghost" size="sm" class="h-8 w-full justify-between rounded-none px-3 font-normal" :class="group === '' ? 'bg-muted' : ''" @click="group = ''">
              <span>全部</span>
              <span class="text-xs text-muted-foreground">{{ matched.length }}</span>
            </Button>
            <Button v-for="[name, count] in groups" :key="name" type="button" variant="ghost" size="sm" class="h-8 w-full justify-between rounded-none px-3 font-normal" :class="group === name ? 'bg-muted' : ''" @click="group = name">
              <span class="truncate">{{ name }}</span>
              <span class="text-xs text-muted-foreground">{{ count }}</span>
            </Button>
          </ScrollArea>
        </aside>
        <ScrollArea class="min-h-0 min-w-0 flex-1">
          <div v-if="currentMissing" class="flex flex-col gap-0.5 border-b bg-muted px-3 py-2 text-sm">
            <span class="font-medium [overflow-wrap:anywhere]">当前：{{ current }}</span>
            <span class="text-xs text-muted-foreground">不在模型目录中，可能已从配置移除；选择其他模型后会替换它。</span>
          </div>
          <Button
            v-for="model in shown"
            :key="model.selector"
            type="button"
            variant="ghost"
            class="h-auto w-full flex-wrap justify-start gap-x-3 gap-y-1 rounded-none border-b px-3 py-2 text-left font-normal last:border-b-0 sm:flex-nowrap"
            :class="picked === model.selector ? 'bg-muted' : ''"
            :aria-pressed="picked === model.selector"
            @click="choose(model)"
          >
            <span class="min-w-0 basis-full sm:flex-1">
              <span class="block truncate font-medium">{{ model.name || model.id }}</span>
              <span class="block truncate text-xs text-muted-foreground">{{ model.selector }}</span>
            </span>
            <span class="flex flex-wrap gap-x-2 text-xs text-muted-foreground sm:shrink-0 sm:flex-col sm:text-right">
              <span class="block">{{ contextLabel(model.context) }} 上下文</span>
              <span v-if="priceLabel(model)" class="block">{{ priceLabel(model) }}</span>
            </span>
          </Button>
          <p v-if="shown.length === 0" class="px-3 py-6 text-center text-sm text-muted-foreground">没有匹配的模型</p>
        </ScrollArea>
      </div>
    </DialogContent>
  </Dialog>
</template>
