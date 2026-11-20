<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ChevronRight, Folder } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
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

const crumbs = computed(() => {
  const value = path.value
  if (!value) return []
  const sep = value.includes('\\') ? '\\' : '/'
  const parts = value.split(/[\\/]/).filter(Boolean)
  const items: { name: string; path: string }[] = []
  let acc = ''
  for (const part of parts) {
    acc = !acc ? (part.endsWith(':') ? `${part}${sep}` : part) : `${acc.replace(/[\\/]$/, '')}${sep}${part}`
    items.push({ name: part, path: acc.replace(/[\\/]$/, '') })
  }
  return items
})

const folderName = computed(() => crumbs.value.at(-1)?.name || path.value || '目录')

async function load(next: string) {
  loading.value = true
  error.value = ''
  try {
    const listing = await api.browse(next)
    path.value = listing.path
    parent.value = listing.parent
    entries.value = listing.entries
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : '无法打开目录'
  } finally {
    loading.value = false
  }
}

watch(() => props.open, (open) => {
  if (open) void load(props.startPath || '')
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
      <div class="flex min-w-0 shrink-0 items-center gap-1 overflow-x-auto overflow-y-hidden py-0.5 text-sm sm:flex-wrap sm:overflow-visible">
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
      <p v-if="error" class="text-sm text-destructive">{{ error }}</p>
      <ScrollArea class="h-80 min-h-20 rounded-lg border">
        <ListSkeleton v-if="loading" :rows="8" />
        <Button
          v-for="entry in entries"
          v-else
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
        <p v-if="!loading && entries.length === 0" class="px-3 py-8 text-center text-sm text-muted-foreground">没有子目录，可以直接使用当前目录</p>
      </ScrollArea>
      <DialogFooter class="min-w-0 shrink-0 items-stretch sm:items-center sm:justify-between">
        <p class="min-w-0 truncate text-xs text-muted-foreground" :title="path">{{ path }}</p>
        <Button type="button" class="min-w-0 shrink-0" :disabled="!path || loading" @click="start"><span class="truncate">在 {{ folderName }} 开始</span></Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
