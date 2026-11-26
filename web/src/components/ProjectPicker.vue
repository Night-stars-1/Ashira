<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { ArrowRight, ArrowUp, ChevronRight, Folder, Search } from '@lucide/vue'
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
/** The path field: the folder shown, or a path typed to jump to (Enter). */
const typed = ref('')
/** Filters the folder's subfolders by name. */
const query = ref('')
const searchInput = ref<HTMLInputElement | null>(null)

const folderName = computed(() => path.value.split(/[\\/]/).filter(Boolean).at(-1) || path.value || '目录')
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
    typed.value = listing.path
    query.value = ''
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : '无法打开目录'
  } finally {
    loading.value = false
  }
}

/** Enter in the path field; a path that does not open leaves the folder shown as it was. */
function go() {
  const next = typed.value.trim()
  if (next && next !== path.value) void load(next)
}

/** Enter in the search field opens the only folder left. */
function openMatch() {
  if (shown.value.length === 1) void load(shown.value[0].path)
}

watch(() => props.open, async (open) => {
  if (!open) return
  query.value = ''
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
      <form class="flex min-w-0 shrink-0 items-center gap-1.5" @submit.prevent="go">
        <Button type="button" size="icon-sm" variant="ghost" class="shrink-0" aria-label="上一级目录" title="上一级目录" :disabled="loading || !parent" @click="parent && load(parent)">
          <ArrowUp />
        </Button>
        <Input v-model="typed" class="h-8 min-w-0 flex-1 font-mono text-sm md:text-xs" aria-label="目录路径" placeholder="输入路径，回车前往" spellcheck="false" autocomplete="off" @keydown.esc.stop="typed = path" />
        <Button type="submit" size="icon-sm" variant="ghost" class="shrink-0" aria-label="前往路径" title="前往路径" :disabled="loading || !typed.trim() || typed.trim() === path">
          <ArrowRight />
        </Button>
      </form>
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
