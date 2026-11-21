<script setup lang="ts">
import { ChevronRight, ChevronsLeft, Plus, RotateCw, Settings } from '@lucide/vue'
import SessionTree from '@/components/SessionTree.vue'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import type { SessionNode } from '@/lib/api'
import { webUpdate } from '@/lib/update'

defineProps<{ tree: SessionNode[]; activeId: string; running: Set<string>; asking: Set<string>; connected: boolean; update?: string | null; restartPending?: boolean; restarting?: boolean; collapsible?: boolean }>()
const emit = defineEmits<{ resume: [id: string]; alias: [path: string, name: string]; rename: [id: string, title: string]; archive: [kind: 'session' | 'folder', key: string]; create: []; settings: []; about: []; collapse: []; restart: [] }>()
</script>

<template>
  <aside aria-label="会话列表" class="flex min-h-0 w-full flex-1 flex-col bg-sidebar text-sidebar-foreground">
    <div class="flex h-12 shrink-0 items-center gap-2 px-3 max-lg:pr-12">
      <p class="truncate text-sm leading-none font-medium">Ashira</p>
      <span class="size-2 shrink-0 rounded-full" :class="connected ? 'bg-emerald-500' : 'bg-destructive'" role="status" :aria-label="connected ? '已连接' : '未连接'" :title="connected ? '已连接' : '未连接'" />
      <Button v-if="collapsible" type="button" size="icon-sm" variant="ghost" class="-mr-1 ml-auto text-muted-foreground" aria-label="收起会话列表" title="收起会话列表" @click="emit('collapse')">
        <ChevronsLeft />
      </Button>
    </div>
    <div class="px-2 pb-2">
      <Button class="w-full justify-start" size="sm" variant="outline" @click="emit('create')">
        <Plus /> 新会话
      </Button>
    </div>
    <ScrollArea class="min-h-0 flex-1 px-2">
      <SessionTree
        v-for="node in tree"
        :key="node.path"
        :node="node"
        :active-id="activeId"
        :running="running"
        :asking="asking"
        :depth="0"
        @resume="(id) => emit('resume', id)"
        @alias="(path, name) => emit('alias', path, name)"
        @rename="(id, title) => emit('rename', id, title)"
        @archive="(kind, key) => emit('archive', kind, key)"
      />
      <p v-if="tree.length === 0" class="px-2 py-3 text-sm text-muted-foreground">还没有会话</p>
    </ScrollArea>
    <div class="border-t border-sidebar-border p-2 pb-[max(0.5rem,env(safe-area-inset-bottom))]">
      <!-- `ashira update` already replaced the program: one tap restarts into it. -->
      <div v-if="restartPending" class="mb-1 flex items-center gap-2 rounded-md px-2 py-1 text-xs text-muted-foreground">
        <span class="size-1.5 shrink-0 rounded-full bg-primary" aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">服务端已更新，重启后生效</span>
        <Button type="button" size="xs" :disabled="restarting" @click="emit('restart')">
          <RotateCw :class="restarting && 'motion-safe:animate-spin'" /> {{ restarting ? '重启中' : '重启' }}
        </Button>
      </div>
      <button
        v-else-if="update || webUpdate.available"
        type="button"
        class="mb-1 flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        @click="emit('about')"
      >
        <span class="size-1.5 shrink-0 rounded-full bg-primary" aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">{{ update && webUpdate.available ? '服务端和网页有新版本' : update ? `服务端有新版本 ${update}` : `网页有新版本 ${webUpdate.available?.tag}` }}</span>
        <ChevronRight class="size-3.5 shrink-0" aria-hidden="true" />
      </button>
      <Button class="w-full justify-start" variant="ghost" @click="emit('settings')">
        <Settings /> 设置
      </Button>
    </div>
  </aside>
</template>
