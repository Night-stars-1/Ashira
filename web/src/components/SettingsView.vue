<script setup lang="ts">
import { TabsList, TabsRoot, TabsTrigger } from 'reka-ui'
import { nextTick, ref, watch } from 'vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Empty } from '@/components/ui/empty'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import ModelSettings from '@/components/ModelSettings.vue'
import UsageSettings from '@/components/UsageSettings.vue'
import ConnectionForm from '@/components/ConnectionForm.vue'
import AboutSettings from '@/components/AboutSettings.vue'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Switch } from '@/components/ui/switch'
import { wrapToolOutput } from '@/lib/display'
import { api, type Archive, type McpForm, type McpServer, type MemoryEntry } from '@/lib/api'

const open = defineModel<boolean>('open', { required: true })
const props = defineProps<{ archive: Archive; serverVersion: string; serverUpdate: string | null; hosted: boolean }>()
const emit = defineEmits<{ restore: [kind: 'session' | 'folder', key: string] }>()

export type SettingsTab = 'models' | 'usage' | 'display' | 'plugins' | 'mcp' | 'memory' | 'archive' | 'connection' | 'about'
type Tab = SettingsTab
const tab = defineModel<Tab>('tab', { default: 'models' })
const tabs: { id: Tab; label: string }[] = [
  { id: 'models', label: '模型' },
  { id: 'usage', label: '用量' },
  { id: 'display', label: '显示' },
  { id: 'plugins', label: '插件' },
  { id: 'mcp', label: 'MCP' },
  { id: 'memory', label: '记忆' },
  { id: 'archive', label: '归档' },
  { id: 'connection', label: '连接' },
  { id: 'about', label: '关于' },
]
/** Where the sliding highlight sits: the chosen row's offset and height within the nav. */
const nav = ref<HTMLElement | null>(null)
const pill = ref({ top: 0, height: 0 })
function placePill() {
  const row = nav.value?.querySelector<HTMLElement>(`[data-tab="${tab.value}"]`)
  if (row) pill.value = { top: row.offsetTop, height: row.offsetHeight }
}
watch([tab, open], () => { void nextTick(placePill) }, { immediate: true })
const archivedDate = new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium' })
// Sessions grouped under their folder name, so two "hi" threads stay distinguishable.
const folderOf = (cwd: string) => cwd.split(/[\\/]/).filter(Boolean).pop() || cwd

const plugins = ref<string[]>([])
const pluginNote = ref('')
const servers = ref<McpServer[]>([])
const mcpNote = ref('')
const memories = ref<MemoryEntry[]>([])
const memoryNote = ref('')
const detail = ref<McpServer | null>(null)

function failure(status: string): string {
  return status.startsWith('failed: ') ? status.slice('failed: '.length) : ''
}

function statusLabel(status: string): string {
  if (status === 'connected') return '已连接'
  if (status === 'connecting') return '连接中'
  if (status === 'disabled') return '已关闭'
  if (status === 'failed' || status === 'error' || status.startsWith('failed')) return '失败'
  return status || '未知'
}

const emptyForm = (): McpForm => ({ name: '', type: 'stdio', command: '', args: '', url: '', headers: '' })
const form = ref<McpForm>(emptyForm())
/** Name being edited; empty while adding. */
const editing = ref('')
const mcpError = ref('')
const saving = ref(false)
const mcpOpen = ref(false)
const removing = ref<string | null>(null)
/** Survives the close animation after `removing` is cleared. */
const removingLabel = ref('')
watch(removing, (name) => { if (name) removingLabel.value = name })

function startAdd() {
  editing.value = ''
  form.value = emptyForm()
  mcpError.value = ''
  mcpOpen.value = true
}

async function startEdit(name: string) {
  mcpError.value = ''
  try {
    const { source: _source, ...current } = await api.mcpConfig(name)
    editing.value = name
    form.value = current
    mcpOpen.value = true
  } catch (reason) {
    mcpError.value = reason instanceof Error ? reason.message : '读取配置失败'
  }
}

async function saveMcp() {
  saving.value = true
  mcpError.value = ''
  const trimmed = Object.fromEntries(Object.entries(form.value).map(([key, value]) => [key, value.trim()])) as McpForm
  try {
    if (editing.value) await api.mcpUpdate(editing.value, trimmed)
    else await api.mcpAdd(trimmed)
    mcpOpen.value = false
    await load()
  } catch (reason) {
    mcpError.value = reason instanceof Error ? reason.message : '保存失败'
  } finally {
    saving.value = false
  }
}

async function removeMcp() {
  const name = removing.value
  if (!name) return
  mcpError.value = ''
  try {
    await api.mcpRemove(name)
    removing.value = null
    // The server has already written the file; drop the row before the refetch.
    servers.value = servers.value.filter((server) => server.name !== name)
    if (detail.value?.name === name) detail.value = null
    await load()
  } catch (reason) {
    removing.value = null
    mcpError.value = reason instanceof Error ? reason.message : '删除失败'
  }
}

async function load() {
  const [pluginText, mcpReport, memoryReport] = await Promise.all([
    api.plugins().catch((reason: unknown) => reason instanceof Error ? reason.message : '读取失败'),
    api.mcp().then((report) => report).catch((reason: unknown) => reason instanceof Error ? reason.message : '读取失败'),
    api.memory().then((report) => report).catch((reason: unknown) => reason instanceof Error ? reason.message : '读取失败'),
  ])
  if (pluginText === '读取失败') {
    plugins.value = []
    pluginNote.value = pluginText
  } else if (pluginText.startsWith('没有')) {
    plugins.value = []
    pluginNote.value = ''
  } else {
    plugins.value = pluginText.split('\n').map((line) => line.trim()).filter(Boolean)
    pluginNote.value = ''
  }
  if (typeof mcpReport === 'string') {
    servers.value = []
    mcpNote.value = mcpReport
  } else {
    servers.value = mcpReport.servers
    mcpNote.value = mcpReport.warnings.join('\n')
  }
  if (typeof memoryReport === 'string') {
    memories.value = []
    memoryNote.value = memoryReport
  } else {
    memories.value = memoryReport.entries
    memoryNote.value = ''
  }
}

watch(open, (value) => { if (value) void load() })
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="flex h-[80dvh] max-w-[calc(100%-2rem)] flex-col gap-0 overflow-hidden p-0 sm:h-[70dvh] sm:max-w-4xl" :aria-describedby="undefined">
      <DialogHeader class="border-b px-4 py-3">
        <DialogTitle>设置</DialogTitle>
      </DialogHeader>
      <div class="flex min-h-0 flex-1 flex-col sm:flex-row">
        <!-- Phones: a scrolling row of underlined tabs. -->
        <TabsRoot v-model="tab" activation-mode="manual" class="shrink-0 border-b sm:hidden">
          <TabsList aria-label="设置分类" class="flex overflow-x-auto px-2 [scrollbar-width:none]">
            <TabsTrigger
              v-for="item in tabs"
              :key="item.id"
              :value="item.id"
              class="relative h-10 shrink-0 px-3 text-sm font-medium text-muted-foreground outline-none transition-colors after:absolute after:inset-x-3 after:bottom-0 after:h-0.5 after:rounded-full after:bg-foreground after:opacity-0 after:transition-opacity hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 data-[state=active]:text-foreground data-[state=active]:after:opacity-100"
            >
              {{ item.label }}
            </TabsTrigger>
          </TabsList>
        </TabsRoot>
        <!-- Wider: the side list. -->
        <!-- One highlight slides to the chosen row, measured from the row itself. -->
        <nav ref="nav" class="relative hidden w-36 shrink-0 flex-col gap-1 border-r p-2 sm:flex">
          <span
            aria-hidden="true"
            class="pointer-events-none absolute inset-x-2 top-0 rounded-md bg-secondary transition-[transform,height] duration-200 ease-out motion-reduce:transition-none"
            :style="{ transform: `translateY(${pill.top}px)`, height: `${pill.height}px` }"
          />
          <Button
            v-for="item in tabs"
            :key="item.id"
            type="button"
            size="sm"
            variant="ghost"
            class="relative justify-start hover:bg-muted/50"
            :data-tab="item.id"
            :aria-current="tab === item.id ? 'page' : undefined"
            @click="tab = item.id"
          >
            {{ item.label }}
          </Button>
        </nav>
        <ScrollArea class="min-h-0 min-w-0 flex-1">
          <div class="flex flex-col gap-3 p-3 [overflow-wrap:anywhere] sm:p-4">
            <ModelSettings v-if="tab === 'models'" :active="open && tab === 'models'" />
            <UsageSettings v-else-if="tab === 'usage'" :active="open && tab === 'usage'" />
            <template v-else-if="tab === 'plugins'">
              <p v-if="pluginNote" class="text-sm text-destructive">{{ pluginNote }}</p>
              <Empty v-else-if="plugins.length === 0" title="没有插件" description="把插件放到 ~/.ashira/plugins 或项目的 .ashira/plugins。" />
              <div v-for="line in plugins" :key="line" class="rounded-lg border px-3 py-2 text-sm">{{ line }}</div>
            </template>
            <template v-else-if="tab === 'mcp'">
              <div class="flex justify-end">
                <Button type="button" size="sm" @click="startAdd">添加</Button>
              </div>
              <p v-if="mcpError && !mcpOpen" class="text-sm text-destructive">{{ mcpError }}</p>
              <p v-if="mcpNote && mcpNote !== '读取失败'" class="text-sm whitespace-pre-wrap text-muted-foreground">{{ mcpNote }}</p>
              <p v-if="mcpNote === '读取失败'" class="text-sm text-destructive">{{ mcpNote }}</p>
              <Empty v-else-if="servers.length === 0" title="没有 MCP" description="添加一个本地命令或远程地址。写入后会重新连接。" />
              <div v-for="server in servers" :key="server.name" class="flex items-center gap-2 rounded-lg border px-3 py-2">
                <button type="button" class="min-w-0 flex-1 text-left outline-none focus-visible:underline" :aria-label="`查看 ${server.name} 详情`" @click="detail = server">
                  <p class="truncate text-sm">{{ server.name }}</p>
                  <p class="truncate text-xs text-muted-foreground">{{ server.kind || '已关闭' }}<template v-if="server.tools.length"> · {{ server.tools.length }} 个工具</template></p>
                </button>
                <Badge :variant="server.status.startsWith('failed') || server.status === 'error' ? 'destructive' : 'secondary'">{{ statusLabel(server.status) }}</Badge>
                <Button type="button" size="sm" variant="ghost" @click="startEdit(server.name)">编辑</Button>
                <Button type="button" size="sm" variant="ghost" class="text-destructive hover:text-destructive" @click="removing = server.name">删除</Button>
              </div>
            </template>
            <ConnectionForm v-else-if="tab === 'connection'" />
            <div v-else-if="tab === 'display'" class="rounded-lg border">
              <label class="flex items-center gap-3 px-3 py-3">
                <span class="min-w-0 flex-1">
                  <span class="block text-sm">工具输出自动换行</span>
                  <span class="block text-xs text-muted-foreground">关闭后长行保持完整，左右滚动查看</span>
                </span>
                <Switch v-model="wrapToolOutput" />
              </label>
            </div>
            <AboutSettings v-else-if="tab === 'about'" :server-version="props.serverVersion" :server-update="props.serverUpdate" :hosted="props.hosted" />
            <template v-else-if="tab === 'archive'">
              <Empty v-if="props.archive.folders.length === 0 && props.archive.sessions.length === 0" title="没有归档" description="在会话列表里用「…」或右键归档会话和文件夹，归档后可在这里还原。" />
              <template v-if="props.archive.folders.length">
                <h3 class="text-xs font-medium text-muted-foreground">文件夹</h3>
                <div v-for="folder in props.archive.folders" :key="folder.path" class="flex items-center gap-2 rounded-lg border px-3 py-2">
                  <div class="min-w-0 flex-1">
                    <p class="truncate text-sm">{{ folder.name }}</p>
                    <p class="truncate text-xs text-muted-foreground" :title="folder.path">{{ folder.path }}</p>
                  </div>
                  <Button type="button" size="sm" variant="outline" @click="emit('restore', 'folder', folder.path)">还原</Button>
                </div>
              </template>
              <template v-if="props.archive.sessions.length">
                <h3 class="text-xs font-medium text-muted-foreground" :class="{ 'mt-2': props.archive.folders.length > 0 }">会话</h3>
                <div v-for="item in props.archive.sessions" :key="item.id" class="flex items-center gap-2 rounded-lg border px-3 py-2">
                  <div class="min-w-0 flex-1">
                    <p class="truncate text-sm" :class="item.prompt ? '' : 'text-muted-foreground'">{{ item.prompt || '空会话' }}</p>
                    <p class="truncate text-xs text-muted-foreground">{{ folderOf(item.cwd) }}<template v-if="item.modified"> · {{ archivedDate.format(item.modified) }}</template></p>
                  </div>
                  <Button type="button" size="sm" variant="outline" @click="emit('restore', 'session', item.id)">还原</Button>
                </div>
              </template>
            </template>
            <template v-else>
              <p v-if="memoryNote" class="text-sm text-destructive">{{ memoryNote }}</p>
              <Empty v-else-if="memories.length === 0" title="没有记忆" description="对话里让 Ashira 记住的内容会出现在这里。" />
              <div v-for="entry in memories" :key="entry.key" class="rounded-lg border px-3 py-2">
                <p class="text-sm font-medium">{{ entry.key }}</p>
                <p class="mt-1 whitespace-pre-wrap text-sm text-muted-foreground">{{ entry.value }}</p>
              </div>
            </template>
          </div>
        </ScrollArea>
      </div>
    </DialogContent>
  </Dialog>
  <Dialog v-model:open="mcpOpen">
    <DialogContent class="max-h-[90dvh] overflow-y-auto sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{{ editing ? `编辑 ${editing}` : '添加 MCP' }}</DialogTitle>
        <DialogDescription>本地命令用 stdio，远程服务用 http 或 sse。保存后会重新连接。</DialogDescription>
      </DialogHeader>
      <div class="flex flex-col gap-2">
        <Input v-model="form.name" placeholder="名称" />
        <div class="flex flex-wrap gap-1">
          <Button v-for="item in (['stdio', 'http', 'sse'] as const)" :key="item" type="button" size="sm" :variant="form.type === item ? 'secondary' : 'ghost'" @click="form.type = item">{{ item }}</Button>
        </div>
        <template v-if="form.type === 'stdio'">
          <Input v-model="form.command" placeholder="命令，例如 npx" />
          <Input v-model="form.args" placeholder="参数，用空格分开" />
        </template>
        <Input v-else v-model="form.url" placeholder="https://example.com/mcp" />
        <Textarea v-if="form.type !== 'stdio'" v-model="form.headers" class="min-h-20 font-mono text-xs" placeholder="请求头，可选。一行一个&#10;Authorization: Bearer token&#10;X-Api-Key: ${API_KEY}" />
        <p v-if="mcpError" class="text-sm text-destructive">{{ mcpError }}</p>
      </div>
      <DialogFooter>
        <Button type="button" variant="outline" @click="mcpOpen = false">取消</Button>
        <Button type="button" :disabled="saving || !form.name.trim()" @click="saveMcp">{{ editing ? '保存' : '添加' }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
  <Dialog :open="removing !== null" @update:open="(value) => { if (!value) removing = null }">
    <DialogContent class="sm:max-w-sm">
      <DialogHeader>
        <DialogTitle>删除 {{ removingLabel }}？</DialogTitle>
        <DialogDescription>会从 Ashira 的 MCP 配置文件中移除这个服务器并断开连接，无法撤销。</DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <Button type="button" variant="outline" @click="removing = null">取消</Button>
        <Button type="button" variant="destructive" @click="removeMcp">删除</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
  <Dialog :open="detail !== null" @update:open="(value) => { if (!value) detail = null }">
    <DialogContent class="flex max-h-[min(85dvh,36rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-xl">
      <DialogHeader class="shrink-0 border-b px-5 pt-5 pb-4 pr-12">
        <DialogTitle class="flex items-center gap-2">
          <span class="truncate">{{ detail?.name }}</span>
          <Badge v-if="detail" :variant="detail.status.startsWith('failed') ? 'destructive' : 'secondary'">{{ statusLabel(detail.status) }}</Badge>
        </DialogTitle>
        <DialogDescription class="sr-only">MCP 服务器详情</DialogDescription>
      </DialogHeader>
      <ScrollArea v-if="detail" type="auto" class="grid min-h-0 flex-1 grid-rows-[minmax(0,1fr)]" viewport-class="h-auto min-h-0">
        <div class="flex flex-col gap-4 p-5 [overflow-wrap:anywhere]">
          <p v-if="failure(detail.status)" class="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive whitespace-pre-wrap">{{ failure(detail.status) }}</p>
          <dl class="divide-y rounded-lg border px-3 text-sm">
            <div v-if="detail.kind" class="flex gap-3 py-2"><dt class="w-16 shrink-0 text-muted-foreground">传输</dt><dd>{{ detail.kind }}</dd></div>
            <div v-if="detail.target" class="flex gap-3 py-2"><dt class="w-16 shrink-0 text-muted-foreground">{{ detail.kind === 'stdio' ? '命令' : '地址' }}</dt><dd class="min-w-0 font-mono text-xs leading-5">{{ detail.target }}</dd></div>
            <div class="flex gap-3 py-2"><dt class="w-16 shrink-0 text-muted-foreground">配置</dt><dd class="min-w-0 font-mono text-xs leading-5">{{ detail.source }}</dd></div>
          </dl>
          <section>
            <h3 class="mb-2 text-sm font-medium">工具（{{ detail.tools.length }}）</h3>
            <p v-if="detail.tools.length === 0" class="text-sm text-muted-foreground">没有可用工具。</p>
            <ul v-else class="divide-y rounded-lg border">
              <li v-for="tool in detail.tools" :key="tool.name" class="px-3 py-2">
                <p class="font-mono text-xs">{{ tool.name }}</p>
                <p v-if="tool.description" class="mt-0.5 text-xs text-muted-foreground">{{ tool.description }}</p>
              </li>
            </ul>
          </section>
        </div>
      </ScrollArea>
    </DialogContent>
  </Dialog>
</template>
