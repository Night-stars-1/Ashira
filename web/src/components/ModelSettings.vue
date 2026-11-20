<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Empty } from '@/components/ui/empty'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Checkbox } from '@/components/ui/checkbox'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import ListSkeleton from '@/components/ListSkeleton.vue'
import ModelCombobox from '@/components/ModelCombobox.vue'
import { api, type ModelConfig, type ModelItem, type ProviderConfig, type ProviderForm } from '@/lib/api'

const props = defineProps<{ active: boolean }>()

const config = ref<ModelConfig | null>(null)
const models = ref<ModelItem[]>([])
const levels = ref<string[]>([])
const loadError = ref('')
const notice = ref('')

type Role = 'default' | 'task'
/** Each role as model selector + thinking level ('' = model default). */
const roles = ref<Record<Role, { model: string; level: string }>>({ default: { model: '', level: '' }, task: { model: '', level: '' } })
const rolesSaving = ref(false)
const joined = (role: Role) => roles.value[role].model ? roles.value[role].model + (roles.value[role].level ? `:${roles.value[role].level}` : '') : ''
const rolesDirty = computed(() => config.value !== null && (joined('default') !== config.value.default || joined('task') !== config.value.task))

const API_LABELS: Record<string, string> = {
  'openai-responses': 'OpenAI Responses',
  'openai-completions': 'OpenAI Chat Completions',
  'anthropic-messages': 'Anthropic Messages',
}
const KEY_KINDS = [
  { id: 'literal', label: '直接填写' },
  { id: 'env', label: '环境变量' },
  { id: 'command', label: '命令' },
  { id: 'none', label: '不需要' },
] as const

type Draft = Omit<ProviderForm, 'models'> & { models: string }
const emptyDraft = (): Draft => ({ id: '', api: 'openai-completions', baseUrl: '', key: { kind: 'literal', value: '' }, discovery: true, models: '' })
const draft = ref<Draft>(emptyDraft())
/** Provider being edited; empty while adding. */
const editing = ref<ProviderConfig | null>(null)
const formOpen = ref(false)
const formError = ref('')
const saving = ref(false)
/** Ids the endpoint listed for the form's connection; null before fetching. */
const fetched = ref<string[] | null>(null)
const fetching = ref(false)
const fetchError = ref('')
const fetchQuery = ref('')
const shownFetched = computed(() => {
  const terms = fetchQuery.value.trim().toLowerCase().split(/\s+/).filter(Boolean)
  return (fetched.value ?? []).filter((id) => terms.every((term) => id.toLowerCase().includes(term)))
})
const removing = ref<string | null>(null)
const removingLabel = ref('')
watch(removing, (id) => { if (id) removingLabel.value = id })

/** True when the stored key can be kept by leaving the field empty. */
const keepsKey = computed(() => {
  const stored = editing.value?.key.kind
  return stored !== undefined && stored === draft.value.key.kind && stored !== 'none'
})
const keyPlaceholder = computed(() => {
  if (keepsKey.value && draft.value.key.kind === 'literal') return '已保存，留空则不修改'
  if (draft.value.key.kind === 'env') return 'OPENAI_API_KEY'
  if (draft.value.key.kind === 'command') return 'op read op://vault/openai/key'
  return 'sk-...'
})

function message(reason: unknown, fallback: string): string {
  return reason instanceof Error ? reason.message : fallback
}

async function load() {
  loadError.value = ''
  try {
    const [next, catalog] = await Promise.all([api.modelConfig(), api.models().catch(() => null)])
    if (catalog) {
      models.value = catalog.models
      levels.value = catalog.levels.filter((level) => level !== '默认')
    }
    config.value = next
    roles.value = { default: split(next.default), task: split(next.task) }
  } catch (reason) {
    loadError.value = message(reason, '读取模型配置失败')
  }
}

watch(() => props.active, (value) => { if (value) void load() }, { immediate: true })

/** `provider/model:level` → parts; a trailing segment that isn't a level is part of the id. */
function split(selector: string): { model: string; level: string } {
  const at = selector.lastIndexOf(':')
  const tail = at > 0 ? selector.slice(at + 1) : ''
  return levels.value.includes(tail) ? { model: selector.slice(0, at), level: tail } : { model: selector, level: '' }
}

/** Levels the role's model accepts; unknown support offers all, like the model picker. */
function roleLevels(selector: string): string[] {
  const efforts = models.value.find((model) => model.selector === selector)?.efforts ?? []
  return levels.value.filter((level) => !efforts.length || efforts.includes(level) || (level === 'off' && efforts.includes('none')))
}

async function saveRoles() {
  rolesSaving.value = true
  notice.value = ''
  try {
    await api.saveRoles({ default: joined('default'), task: joined('task') })
    await load()
  } catch (reason) {
    notice.value = message(reason, '保存失败')
  } finally {
    rolesSaving.value = false
  }
}

function resetFetch() {
  fetched.value = null
  fetchError.value = ''
  fetchQuery.value = ''
}

function startAdd() {
  editing.value = null
  draft.value = emptyDraft()
  formError.value = ''
  resetFetch()
  formOpen.value = true
}

function listed(): string[] {
  return draft.value.models.split(/[\n,]/).map((model) => model.trim()).filter(Boolean)
}

/** Asks the endpoint for its models with the form's current connection (nothing is saved). */
async function fetchModels() {
  fetching.value = true
  fetchError.value = ''
  try {
    fetched.value = await api.probeModels(editing.value?.id ?? '', form())
    if (fetched.value.length === 0) fetchError.value = '接口没有返回任何模型'
  } catch (reason) {
    fetchError.value = message(reason, '获取失败')
  } finally {
    fetching.value = false
  }
}

function toggleModel(id: string, on: boolean) {
  const current = listed()
  draft.value.models = (on ? [...current.filter((model) => model !== id), id] : current.filter((model) => model !== id)).join('\n')
}

function startEdit(provider: ProviderConfig) {
  editing.value = provider
  const key = provider.key
  draft.value = {
    id: provider.id,
    api: provider.api || 'openai-completions',
    baseUrl: provider.baseUrl,
    // A missing key is entered as a literal; env/command values are shown, a literal never is.
    key: key.kind === 'env' || key.kind === 'command' ? { kind: key.kind, value: key.value } : { kind: key.kind === 'missing' ? 'literal' : key.kind, value: '' },
    discovery: provider.discovery,
    models: provider.models.join('\n'),
  }
  formError.value = ''
  resetFetch()
  formOpen.value = true
}

function form(): ProviderForm {
  return {
    ...draft.value,
    id: draft.value.id.trim(),
    baseUrl: draft.value.baseUrl.trim(),
    key: { kind: draft.value.key.kind, value: draft.value.key.value.trim() },
    models: listed(),
  }
}

async function saveProvider() {
  saving.value = true
  formError.value = ''
  try {
    await api.saveProvider(editing.value?.id ?? '', form())
    formOpen.value = false
    await load()
  } catch (reason) {
    formError.value = message(reason, '保存失败')
  } finally {
    saving.value = false
  }
}

async function removeProvider() {
  const id = removing.value
  if (!id) return
  notice.value = ''
  try {
    await api.removeProvider(id)
    removing.value = null
    await load()
  } catch (reason) {
    removing.value = null
    notice.value = message(reason, '删除失败')
  }
}

function keyLabel(provider: ProviderConfig): string {
  const key = provider.key
  if (key.kind === 'env') return key.unset ? `环境变量 ${key.value}（未设置）` : `环境变量 ${key.value}`
  if (key.kind === 'command') return '命令获取'
  if (key.kind === 'literal') return '已填写 Key'
  if (key.kind === 'none') return '无需 Key'
  return '未填写 Key'
}

function statusLabel(provider: ProviderConfig): string {
  const status = provider.status
  if (!status) return '未加载'
  if (status.state === 'pending') return '获取中'
  if (status.state === 'failed') return '失败'
  return `${status.models} 个模型`
}
</script>

<template>
  <p v-if="loadError" role="alert" class="text-sm text-destructive">{{ loadError }}</p>
  <ListSkeleton v-else-if="!config" />
  <template v-else>
    <p v-if="config.loadError" role="alert" class="rounded-lg border border-destructive/50 px-3 py-2 text-sm whitespace-pre-wrap text-destructive">{{ config.loadError }}</p>
    <p v-if="notice" role="alert" class="text-sm text-destructive">{{ notice }}</p>

    <section aria-labelledby="model-roles" class="flex flex-col gap-2 rounded-lg border p-3">
      <h3 id="model-roles" class="text-sm font-medium">默认模型</h3>
      <div v-for="role in ([['default', '新会话', '未设置（用第一个可用模型）'], ['task', '子代理', '跟随当前会话的模型']] as const)" :key="role[0]" class="flex flex-col gap-1">
        <span class="text-xs text-muted-foreground">{{ role[1] }}</span>
        <div class="flex gap-2">
          <div class="min-w-0 flex-1">
            <ModelCombobox v-model="roles[role[0]].model" :models="models" :placeholder="role[2]" :label="`${role[1]}模型`" />
          </div>
          <!-- reka forbids '' as an item value; 'default' stands for no level suffix. -->
          <Select :model-value="roles[role[0]].level || 'default'" :disabled="!roles[role[0]].model" @update:model-value="(level) => roles[role[0]].level = level === 'default' ? '' : String(level)">
            <SelectTrigger :aria-label="`${role[1]}思考强度`" class="h-9 w-28 shrink-0">
              <SelectValue />
            </SelectTrigger>
            <SelectContent position="popper">
              <SelectItem value="default">默认强度</SelectItem>
              <SelectItem v-for="level in roleLevels(roles[role[0]].model)" :key="level" :value="level">{{ level }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>
      <div class="flex justify-end">
        <Button type="button" size="sm" :disabled="!rolesDirty || rolesSaving" @click="saveRoles">{{ rolesSaving ? '保存中...' : '保存' }}</Button>
      </div>
    </section>

    <div class="flex items-center gap-2">
      <h3 class="min-w-0 flex-1 text-sm font-medium">服务商</h3>
      <Button type="button" size="sm" @click="startAdd">添加</Button>
    </div>
    <Empty v-if="config.providers.length === 0" title="没有服务商" description="添加一个服务商，填写接口地址和 API Key 后就能选择它的模型。" />
    <div v-for="provider in config.providers" :key="provider.id" class="flex items-center gap-2 rounded-lg border px-3 py-2">
      <div class="min-w-0 flex-1">
        <p class="truncate text-sm">{{ provider.id }}</p>
        <p class="truncate text-xs text-muted-foreground" :title="provider.baseUrl">{{ API_LABELS[provider.api] || provider.api || '未设置接口' }} · {{ keyLabel(provider) }}</p>
        <p v-if="provider.status?.detail" class="mt-0.5 line-clamp-2 text-xs text-destructive" :title="provider.status.detail">{{ provider.status.detail }}</p>
      </div>
      <Badge :variant="provider.status?.state === 'failed' ? 'destructive' : 'secondary'" class="shrink-0">{{ statusLabel(provider) }}</Badge>
      <Button type="button" size="sm" variant="ghost" @click="startEdit(provider)">编辑</Button>
      <Button type="button" size="sm" variant="ghost" class="text-destructive hover:text-destructive" @click="removing = provider.id">删除</Button>
    </div>
    <p v-if="config.warnings.length" class="text-xs whitespace-pre-wrap text-muted-foreground">{{ config.warnings.join('\n') }}</p>
  </template>

  <Dialog v-model:open="formOpen">
    <DialogContent class="max-h-[90dvh] overflow-y-auto sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{{ editing ? `编辑 ${editing.id}` : '添加服务商' }}</DialogTitle>
        <DialogDescription>保存后会立即生效，并重新获取模型列表。</DialogDescription>
      </DialogHeader>
      <form class="flex flex-col gap-3" @submit.prevent="saveProvider">
        <label class="flex flex-col gap-1 text-xs text-muted-foreground">
          服务商 ID
          <Input v-model="draft.id" placeholder="openai" autocomplete="off" spellcheck="false" />
        </label>
        <div class="flex flex-col gap-1">
          <span class="text-xs text-muted-foreground">接口类型</span>
          <Select v-model="draft.api">
            <SelectTrigger aria-label="接口类型" class="h-9 w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent position="popper">
              <SelectItem v-for="api in config?.apis ?? []" :key="api" :value="api">{{ API_LABELS[api] || api }}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <label class="flex flex-col gap-1 text-xs text-muted-foreground">
          Base URL
          <Input v-model="draft.baseUrl" placeholder="https://api.openai.com/v1" autocomplete="off" spellcheck="false" />
        </label>
        <fieldset class="flex flex-col gap-1">
          <legend class="mb-1 text-xs text-muted-foreground">API Key</legend>
          <div class="flex flex-wrap gap-1">
            <Button
              v-for="kind in KEY_KINDS"
              :key="kind.id"
              type="button"
              size="sm"
              :variant="draft.key.kind === kind.id ? 'secondary' : 'ghost'"
              :aria-pressed="draft.key.kind === kind.id"
              @click="draft.key = { kind: kind.id, value: editing && (editing.key.kind === 'env' || editing.key.kind === 'command') && editing.key.kind === kind.id ? editing.key.value : '' }"
            >
              {{ kind.label }}
            </Button>
          </div>
          <Input
            v-if="draft.key.kind !== 'none'"
            v-model="draft.key.value"
            :type="draft.key.kind === 'literal' ? 'password' : 'text'"
            :placeholder="keyPlaceholder"
            :aria-label="draft.key.kind === 'env' ? '环境变量名' : draft.key.kind === 'command' ? '获取 Key 的命令' : 'API Key'"
            autocomplete="off"
            spellcheck="false"
          />
        </fieldset>
        <label class="flex items-center justify-between gap-2 text-sm">
          自动获取模型列表
          <Switch v-model="draft.discovery" />
        </label>
        <div class="flex flex-col gap-1">
          <div class="flex items-center gap-2">
            <span class="min-w-0 flex-1 text-xs text-muted-foreground">模型 ID{{ draft.discovery ? '（可选，补充自动获取不到的）' : '' }}</span>
            <Button type="button" size="xs" variant="outline" :disabled="fetching || !draft.baseUrl.trim()" @click="fetchModels">{{ fetching ? '获取中...' : '获取模型' }}</Button>
          </div>
          <p v-if="fetchError" role="alert" class="text-xs text-destructive">{{ fetchError }}</p>
          <div v-if="fetched?.length" class="flex flex-col overflow-hidden rounded-lg border">
            <Input v-model="fetchQuery" class="h-8 rounded-none border-0 border-b text-xs shadow-none focus-visible:ring-0" :placeholder="`搜索 ${fetched.length} 个模型`" aria-label="搜索获取到的模型" />
            <div class="max-h-48 overflow-y-auto p-1">
              <label v-for="id in shownFetched" :key="id" class="flex cursor-pointer items-center gap-2 rounded-md px-1.5 py-1 font-mono text-xs hover:bg-muted">
                <Checkbox :model-value="listed().includes(id)" @update:model-value="(on) => toggleModel(id, on === true)" />
                <span class="truncate">{{ id }}</span>
              </label>
              <p v-if="shownFetched.length === 0" class="px-1.5 py-1 text-xs text-muted-foreground">没有匹配的模型</p>
            </div>
          </div>
          <Textarea v-model="draft.models" class="min-h-20 font-mono text-xs" aria-label="模型 ID" placeholder="一行一个&#10;gpt-4.1&#10;gpt-4.1-mini" spellcheck="false" />
        </div>
        <p v-if="formError" role="alert" class="text-sm text-destructive">{{ formError }}</p>
        <DialogFooter>
          <Button type="button" variant="ghost" @click="formOpen = false">取消</Button>
          <Button type="submit" :disabled="saving">{{ saving ? '保存中...' : '保存' }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
  <Dialog :open="removing !== null" @update:open="(value) => { if (!value) removing = null }">
    <DialogContent class="sm:max-w-sm">
      <DialogHeader>
        <DialogTitle>删除 {{ removingLabel }}？</DialogTitle>
        <DialogDescription>会从模型配置文件里删除这个服务商及其高级设置。用它作默认模型的地方需要重新选择。</DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <Button type="button" variant="ghost" @click="removing = null">取消</Button>
        <Button type="button" variant="destructive" @click="removeProvider">删除</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
