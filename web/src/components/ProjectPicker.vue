<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { ArrowRight, ChevronRight, Folder, Pencil, Search } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { api, type BrowseEntry } from '@/lib/api'
import ListSkeleton from '@/components/ListSkeleton.vue'

const props = defineProps<{ open: boolean; startPath: string }>()
const emit = defineEmits<{ 'update:open': [open: boolean]; start: [path: string] }>()

const path = ref('')
const parent = ref<string | null>(null)
const entries = ref<BrowseEntry[]>([])
const error = ref('')
const loading = ref(false)
/** The path row: breadcrumbs, or while `editing` a field for typing a path to jump to. */
const editing = ref(false)
const typed = ref('')
const pathInput = ref<{ $el: HTMLInputElement } | null>(null)
/** Filters the folder's subfolders by name. */
const query = ref('')
const searchInput = ref<HTMLInputElement | null>(null)

const crumbs = computed(() => {
  const value = path.value
  if (!value) return []
  const sep = value.includes('\\') ? '\\' : '/'
  const parts = value.split(/[\\/]/).filter(Boolean)
  const items: { name: string; path: string }[] = []
  let acc = ''
  for (const part of parts) {
    acc = !acc ? (part.endsWith(':') ? `${part}${sep}` : value.startsWith('/') ? `/${part}` : part) : `${acc.replace(/[\\/]$/, '')}${sep}${part}`
    items.push({ name: part, path: acc })
  }
  return items
})
const folderName = computed(() => crumbs.value.at(-1)?.name || path.value || '目录')
const shown = computed(() => {
  const needle = query.value.trim().toLowerCase()
  return needle ? entries.value.filter((entry) => entry.name.toLowerCase().includes(needle)) : entries.value
})

async function load(next: string) {
  loading.value = true
  error.value = ''
  try {
    const listing = await api.browse(next)
    path.value = listing.path
    parent.value = listing.parent
    entries.value = listing.entries
    editing.value = false
    query.value = ''
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : '无法打开目录'
  } finally {
    loading.value = false
  }
}

/** Swaps the breadcrumbs for the path field, with the path selected for typing over. */
async function editPath() {
  typed.value = path.value
  editing.value = true
  await nextTick()
  pathInput.value?.$el.focus()
  pathInput.value?.$el.select()
}

/** Enter in the path field; a path that does not open keeps the field and the folder shown. */
function go() {
  const next = typed.value.trim()
  if (!next || next === path.value) editing.value = false
  else void load(next)
}

/** Enter in the search field opens the only folder left. */
function openMatch() {
  if (shown.value.length === 1) void load(shown.value[0].path)
}

watch(() => props.open, async (open) => {
  if (!open) return
  query.value = ''
  editing.value = false
  await load(props.startPath || '')
  await nextTick()
  searchInput.value?.focus()
})

function start() {
  if (!path.value) return
  emit('start', path.value)
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogContent class="flex max-h-[90dvh] flex-col gap-3 overflow-y-auto sm:max-w-xl" :aria-describedby="undefined">
      <DialogHeader>
        <DialogTitle>选择目录</DialogTitle>
      </DialogHeader>
      <!-- The breadcrumbs; clicking past the last one (or the pencil) types a path instead. -->
      <form v-if="editing" class="flex min-w-0 shrink-0 items-center gap-1.5" @submit.prevent="go">
        <Input ref="pathInput" v-model="typed" class="h-8 min-w-0 flex-1 font-mono text-sm md:text-xs" aria-label="目录路径" placeholder="输入路径，回车前往" spellcheck="false" autocomplete="off" @keydown.esc.stop.prevent="editing = false" @blur="!loading && !error && (editing = false)" />
        <Button type="submit" size="icon-sm" variant="ghost" class="shrink-0" aria-label="前往路径" title="前往路径" :disabled="loading || !typed.trim()" @mousedown.prevent>
          <ArrowRight />
        </Button>
      </form>
      <div v-else class="flex min-w-0 shrink-0 items-center gap-1">
        <div class="flex min-w-0 flex-1 cursor-text items-center gap-1 overflow-x-auto overflow-y-hidden py-0.5 text-sm sm:flex-wrap sm:overflow-visible" title="点击空白处输入路径" @click.self="editPath">
          <Button
            v-for="(crumb, index) in crumbs"
            :key="crumb.path"
            type="button"
            size="sm"
            class="h-8 max-w-40 shrink-0 truncate px-2"
            :variant="index === crumbs.length - 1 ? 'secondary' : 'ghost'"
            :disabled="loading || index === crumbs.length - 1"
            @click="load(crumb.path)"
          >
            {{ crumb.name }}
          </Button>
        </div>
        <Button type="button" size="icon-sm" variant="ghost" class="shrink-0 text-muted-foreground" aria-label="输入路径" title="输入路径" :disabled="loading" @click="editPath">
          <Pencil />
        </Button>
      </div>
      <p v-if="error" role="alert" class="text-sm break-all text-destructive">{{ error }}</p>
      <div class="flex min-h-0 flex-col overflow-hidden rounded-lg border">
        <label class="flex shrink-0 items-center gap-2 border-b px-3">
          <Search aria-hidden="true" class="size-4 shrink-0 text-muted-foreground" />
          <input ref="searchInput" v-model="query" type="search" class="h-9 min-w-0 flex-1 bg-transparent text-base outline-none placeholder:text-muted-foreground md:text-sm" placeholder="搜索当前目录" aria-label="搜索当前目录" autocomplete="off" spellcheck="false" @keydown.enter.prevent="openMatch" />
        </label>
        <ScrollArea class="h-72 min-h-20">
          <ListSkeleton v-if="loading" :rows="8" />
          <template v-else>
            <Button
              v-for="entry in shown"
              :key="entry.path"
              type="button"
              variant="ghost"
              class="h-10 w-full justify-start gap-2 rounded-none px-3 font-normal"
              @click="load(entry.path)"
            >
              <Folder class="size-4 shrink-0 text-muted-foreground" />
              <span class="min-w-0 flex-1 truncate text-left">{{ entry.name }}</span>
              <ChevronRight class="size-4 shrink-0 text-muted-foreground" />
            </Button>
            <p v-if="entries.length === 0" class="px-3 py-8 text-center text-sm text-muted-foreground">没有子目录，可以直接使用当前目录</p>
            <p v-else-if="shown.length === 0" class="px-3 py-8 text-center text-sm text-muted-foreground">没有名称包含「{{ query.trim() }}」的文件夹</p>
          </template>
        </ScrollArea>
      </div>
      <DialogFooter class="min-w-0 shrink-0">
        <Button type="button" class="min-w-0 shrink-0" :title="path" :disabled="!path || loading" @click="start"><span class="truncate">在 {{ folderName }} 开始</span></Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
