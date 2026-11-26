<script setup lang="ts">
import { ChevronRight, File, Folder } from '@lucide/vue'
import { onMounted, ref, watch } from 'vue'
import { Button } from '@/components/ui/button'
import FileTreeSkeleton from '@/components/FileTreeSkeleton.vue'
import { api, type FileEntry } from '@/lib/api'

/** `refresh`: reload the folder named by `parent`, or with no `parent` every subfolder (the root gets fresh `initialEntries`). */
const props = defineProps<{ sessionId: string; path: string; name?: string; depth: number; active?: string; initialEntries?: FileEntry[]; refresh?: { parent?: string } }>()
const emit = defineEmits<{ open: [path: string] }>()
const open = ref(false)
const loaded = ref(props.initialEntries !== undefined)
const entries = ref<FileEntry[]>(props.initialEntries ?? [])
const failed = ref('')
let loadId = 0

async function load() {
  const id = ++loadId
  failed.value = ''
  try {
    const listing = await api.files(props.sessionId, props.path)
    if (id !== loadId) return
    entries.value = listing.entries
    loaded.value = true
  } catch (reason) {
    if (id !== loadId) return
    failed.value = reason instanceof Error ? reason.message : '无法读取目录'
  }
}

async function toggle() {
  open.value = !open.value
  if (open.value && !loaded.value) await load()
}

onMounted(() => {
  if (!props.name && !loaded.value) void load()
})

watch(() => props.initialEntries, (listing) => {
  if (!listing) return
  loadId++
  entries.value = listing
  loaded.value = true
  failed.value = ''
})

watch(() => props.refresh, (change) => {
  if (!change) return
  if (change.parent === undefined) {
    // An expanded folder reloads now; a collapsed one read earlier reloads when opened.
    if (props.name && open.value) void load()
    else if (props.name) loaded.value = false
  } else if (change.parent === props.path && (!props.name || loaded.value || open.value)) void load()
})

function childPath(name: string) {
  return props.path ? `${props.path}/${name}` : name
}

function inset(extra = 0) {
  return `${8 + (props.name ? props.depth + 1 : props.depth) * 12 + extra}px`
}
</script>

<template>
  <Button
    v-if="name"
    type="button"
    variant="ghost"
    size="sm"
    class="h-8 w-full justify-start gap-1.5 px-2 font-normal"
    :style="{ paddingLeft: `${8 + depth * 12}px` }"
    @click="toggle"
  >
    <ChevronRight class="size-3.5 shrink-0 transition-transform" :class="open ? 'rotate-90' : ''" />
    <Folder class="size-3.5 shrink-0 text-muted-foreground" />
    <span class="truncate">{{ name }}</span>
  </Button>
  <p v-if="failed" class="px-2 py-1 text-xs text-destructive" :style="{ paddingLeft: inset() }">{{ failed }}</p>
  <FileTreeSkeleton v-if="!name && !loaded && !failed" />
  <div v-show="!name || open">
    <template v-for="entry in entries" :key="entry.name">
      <FileTree v-if="entry.dir" :session-id="sessionId" :path="childPath(entry.name)" :name="entry.name" :depth="name ? depth + 1 : depth" :active="active" :refresh="refresh" @open="emit('open', $event)" />
      <button
        v-else
        type="button"
        class="flex h-8 w-full items-center gap-1.5 rounded-lg pr-2 text-left text-sm outline-none transition-colors hover:bg-muted/50 focus-visible:ring-3 focus-visible:ring-ring/50 aria-[current=true]:bg-muted aria-[current=true]:font-medium"
        :aria-current="childPath(entry.name) === active"
        :style="{ paddingLeft: inset(18) }"
        @click="emit('open', childPath(entry.name))"
      >
        <File class="size-3.5 shrink-0 text-muted-foreground" />
        <span class="truncate">{{ entry.name }}</span>
      </button>
    </template>
  </div>
</template>
