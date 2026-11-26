<script setup lang="ts">
import { ArrowRight, ArrowUp, FilePlus, FolderPlus, RefreshCw } from '@lucide/vue'
import { nextTick, ref, watch } from 'vue'
import FileTree from '@/components/FileTree.vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { api, type FileListing } from '@/lib/api'

const props = defineProps<{ sessionId: string; cwd: string; active: string }>()
const emit = defineEmits<{ open: [path: string] }>()
const directory = ref('')
const location = ref(props.cwd)
const initialEntries = ref<FileListing['entries']>()
const navigating = ref(false)
const navigationError = ref('')
const windowsPath = /^[a-z]:|^\\\\/i.test(props.cwd)
const separator = windowsPath ? '\\' : '/'
const locationInput = ref<{ $el: HTMLInputElement } | null>(null)

// The leaf folder is the useful part of a long path; keep it in view when not editing.
function showPathEnd() {
  const field = locationInput.value?.$el
  if (field && document.activeElement !== field) field.scrollLeft = field.scrollWidth
}
watch([location, locationInput], () => { void nextTick(showPathEnd) })

function address(relative: string) {
  if (!relative) return props.cwd
  return props.cwd.replace(/[/\\]+$/, '') + separator + relative.replace(/\//g, separator)
}

function parentOf(relative: string) {
  return relative.slice(0, Math.max(0, relative.lastIndexOf('/')))
}

function resolveLocation(value: string) {
  const entered = value.trim().replace(/\\/g, '/')
  const root = props.cwd.replace(/\\/g, '/').replace(/\/+$/, '')
  const absolute = entered.startsWith('/') || /^[a-z]:/i.test(entered)
  let relative = entered
  if (absolute) {
    const comparable = (path: string) => windowsPath ? path.toLowerCase() : path
    const requested = comparable(entered)
    const base = comparable(root)
    if (requested === base) return ''
    if (!requested.startsWith(`${base}/`)) throw new Error('只能打开当前工作区内的路径')
    relative = entered.slice(root.length + 1)
  }
  const parts = absolute || !entered || !directory.value ? [] : directory.value.split('/')
  for (const part of relative.split('/')) {
    if (!part || part === '.') continue
    if (part === '..') {
      if (!parts.length) throw new Error('只能打开当前工作区内的路径')
      parts.pop()
    } else if (windowsPath && part.includes(':')) {
      throw new Error('请输入有效的工作区路径')
    } else {
      parts.push(part)
    }
  }
  return parts.join('/')
}

function showDirectory(listing: FileListing) {
  initialEntries.value = listing.entries
  directory.value = listing.path
  location.value = address(listing.path)
}

async function navigate(value: string) {
  if (pending.value || navigating.value) return
  navigationError.value = ''
  navigating.value = true
  try {
    const relative = resolveLocation(value)
    let listing: FileListing
    try {
      listing = await api.files(props.sessionId, relative)
    } catch (reason) {
      if (!relative) throw reason
      const parent = parentOf(relative)
      const name = relative.slice(parent.length ? parent.length + 1 : 0)
      listing = await api.files(props.sessionId, parent)
      const entry = listing.entries.find((item) => item.name === name)
        ?? (windowsPath ? listing.entries.find((item) => item.name.toLowerCase() === name.toLowerCase()) : undefined)
      if (!entry || entry.dir) throw reason
      showDirectory(listing)
      kind.value = null
      emit('open', listing.path ? `${listing.path}/${entry.name}` : entry.name)
      return
    }
    showDirectory(listing)
    kind.value = null
  } catch (reason) {
    navigationError.value = reason instanceof Error ? reason.message : '无法打开路径'
  } finally {
    navigating.value = false
  }
}
type EntryKind = 'file' | 'directory'
const kind = ref<EntryKind | null>(null)
const input = ref<{ $el: HTMLInputElement } | null>(null)
const path = ref('')
const pending = ref(false)
const error = ref('')
const refresh = ref<{ parent?: string }>()
const reloading = ref(false)

/** Rereads the shown folder and every expanded one; the tree keeps what is open. */
async function reload() {
  if (reloading.value || navigating.value) return
  reloading.value = true
  navigationError.value = ''
  try {
    const listing = await api.files(props.sessionId, directory.value)
    initialEntries.value = listing.entries
    refresh.value = {}
  } catch (reason) {
    navigationError.value = reason instanceof Error ? reason.message : '无法读取目录'
  } finally {
    reloading.value = false
  }
}

async function start(next: EntryKind) {
  kind.value = next
  path.value = ''
  error.value = ''
  await nextTick()
  input.value?.$el.focus()
}

function cancel() {
  if (!pending.value) kind.value = null
}

async function create() {
  const entered = path.value.trim().replace(/\\/g, '/')
  if (!kind.value || !entered || pending.value || navigating.value) return
  const relative = directory.value ? `${directory.value}/${entered}` : entered
  pending.value = true
  error.value = ''
  try {
    await api.createEntry(props.sessionId, relative, kind.value)
    refresh.value = { parent: parentOf(relative) }
    kind.value = null
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : '新建失败'
  } finally {
    pending.value = false
  }
}
</script>

<template>
  <aside class="flex w-64 shrink-0 flex-col border-l border-sidebar-border bg-sidebar text-sidebar-foreground">
    <form class="flex h-12 shrink-0 items-center gap-1 px-2 max-lg:pr-12" @submit.prevent="navigate(location)">
      <Button type="button" size="icon-xs" variant="ghost" class="max-lg:size-8" aria-label="返回上级目录" title="返回上级目录" :disabled="!directory || pending || navigating" @click="navigate(address(parentOf(directory)))">
        <ArrowUp aria-hidden="true" />
      </Button>
      <div class="flex min-w-0 flex-1 items-center rounded-md border bg-background/60 focus-within:border-ring">
        <Input ref="locationInput" v-model="location" class="h-7 flex-1 rounded-none border-0 px-2 font-mono text-base focus-visible:ring-0 dark:bg-transparent lg:text-xs" aria-label="文件路径" :title="location" placeholder="输入路径" :disabled="pending || navigating" spellcheck="false" @blur="showPathEnd" />
        <Button v-if="location !== address(directory)" type="submit" size="icon-xs" variant="ghost" class="mr-0.5" aria-label="前往路径" title="前往路径" :disabled="pending || navigating">
          <ArrowRight aria-hidden="true" />
        </Button>
      </div>
      <div role="group" aria-label="目录操作" class="flex shrink-0 items-center">
        <Button type="button" size="icon-xs" variant="ghost" class="max-lg:size-8" aria-label="刷新" title="刷新" :disabled="reloading || navigating" @click="reload">
          <RefreshCw aria-hidden="true" :class="reloading && 'motion-safe:animate-spin'" />
        </Button>
        <Button type="button" size="icon-xs" variant="ghost" class="max-lg:size-8" aria-label="新建文件" title="新建文件" :disabled="pending" @click="start('file')">
          <FilePlus aria-hidden="true" />
        </Button>
        <Button type="button" size="icon-xs" variant="ghost" class="max-lg:size-8" aria-label="新建文件夹" title="新建文件夹" :disabled="pending" @click="start('directory')">
          <FolderPlus aria-hidden="true" />
        </Button>
      </div>
    </form>
    <p v-if="navigationError" role="alert" class="shrink-0 px-3 pb-2 text-xs break-all text-destructive">{{ navigationError }}</p>
    <form v-if="kind" class="shrink-0 space-y-2 border-t px-3 py-2" @submit.prevent="create" @keydown.esc.stop="cancel">
      <label class="block text-xs text-muted-foreground">
        {{ kind === 'file' ? '新建文件' : '新建文件夹' }} · 相对当前目录的路径
        <Input ref="input" v-model="path" class="mt-1" :placeholder="kind === 'file' ? '例如 notes.md' : '例如 docs'" :disabled="pending" />
      </label>
      <p v-if="error" role="alert" class="text-xs break-all text-destructive">{{ error }}</p>
      <div class="flex justify-end gap-2">
        <Button type="button" size="sm" variant="ghost" :disabled="pending" @click="cancel">取消</Button>
        <Button type="submit" size="sm" :disabled="pending || navigating || !path.trim()">{{ pending ? '创建中…' : '创建' }}</Button>
      </div>
    </form>
    <ScrollArea class="min-h-0 flex-1 px-2 pb-2">
      <FileTree :key="directory" :session-id="sessionId" :path="directory" :initial-entries="initialEntries" :depth="0" :active="active" :refresh="refresh" @open="emit('open', $event)" />
    </ScrollArea>
  </aside>
</template>
