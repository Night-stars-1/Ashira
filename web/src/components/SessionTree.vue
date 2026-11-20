<script setup lang="ts">
import { Archive, ChevronRight, CircleHelp, Ellipsis, Folder, LoaderCircle, Pencil, Tag } from '@lucide/vue'
import { ContextMenuContent, ContextMenuItem, ContextMenuPortal, ContextMenuRoot, ContextMenuTrigger, DropdownMenuContent, DropdownMenuItem, DropdownMenuPortal, DropdownMenuRoot, DropdownMenuTrigger } from 'reka-ui'
import { computed, ref } from 'vue'
import { Button } from '@/components/ui/button'
import type { SessionItem, SessionNode } from '@/lib/api'

const props = defineProps<{ node: SessionNode; activeId: string; running: Set<string>; asking: Set<string>; depth: number }>()
const emit = defineEmits<{
  resume: [id: string, cwd: string]
  alias: [path: string, name: string]
  rename: [id: string, title: string]
  archive: [kind: 'session' | 'folder', key: string]
}>()
const open = ref(true)
const showEmpty = ref(false)
// Empty sessions are leftovers from "新会话" clicks; keep the active one visible.
const visible = computed(() => showEmpty.value ? props.node.sessions : props.node.sessions.filter((item) => item.prompt || item.id === props.activeId))
const hidden = computed(() => props.node.sessions.length - visible.value.length)
const relative = new Intl.RelativeTimeFormat('zh-CN', { numeric: 'auto', style: 'narrow' })
const absolute = new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'short' })

function ago(time: number): string {
  if (!time) return ''
  const minutes = Math.round((time - Date.now()) / 60_000)
  if (Math.abs(minutes) < 1) return '刚刚'
  if (Math.abs(minutes) < 60) return relative.format(minutes, 'minute')
  const hours = Math.round(minutes / 60)
  if (Math.abs(hours) < 24) return relative.format(hours, 'hour')
  const days = Math.round(hours / 24)
  if (Math.abs(days) < 30) return relative.format(days, 'day')
  return absolute.format(time).split(' ')[0]
}

/** The parent shows a dialog prefilled with the current title. */
function startRename(item: SessionItem) {
  emit('rename', item.id, item.prompt)
}

const menuItem = 'flex cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none data-highlighted:bg-accent [&_svg]:size-3.5 [&_svg]:text-muted-foreground'
const menuContent = 'z-50 min-w-32 rounded-md border bg-popover p-1 text-popover-foreground shadow-md'
</script>

<template>
  <div class="mb-1">
    <ContextMenuRoot>
      <ContextMenuTrigger as-child>
        <div class="group/folder relative">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            class="h-8 w-full justify-start gap-1.5 px-2 pr-8 font-normal text-muted-foreground"
            :style="{ paddingLeft: `${8 + depth * 12}px` }"
            @click="open = !open"
          >
            <ChevronRight class="size-3.5 shrink-0 transition-transform" :class="open ? 'rotate-90' : ''" />
            <Folder class="size-3.5 shrink-0" />
            <span class="truncate">{{ node.name }}</span>
          </Button>
          <DropdownMenuRoot>
            <DropdownMenuTrigger as-child>
              <Button type="button" size="icon-xs" variant="ghost" class="absolute top-1 right-1 opacity-0 group-hover/folder:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100 max-lg:opacity-100" :aria-label="`${node.name} 的更多操作`">
                <Ellipsis />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuPortal>
              <DropdownMenuContent :class="menuContent" align="end">
                <DropdownMenuItem :class="menuItem" @select="emit('alias', node.path, node.name)"><Tag /> 设置别名</DropdownMenuItem>
                <DropdownMenuItem :class="menuItem" @select="emit('archive', 'folder', node.path)"><Archive /> 归档文件夹</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenuPortal>
          </DropdownMenuRoot>
        </div>
      </ContextMenuTrigger>
      <ContextMenuPortal>
        <ContextMenuContent :class="menuContent">
          <ContextMenuItem :class="menuItem" @select="emit('alias', node.path, node.name)"><Tag /> 设置别名</ContextMenuItem>
          <ContextMenuItem :class="menuItem" @select="emit('archive', 'folder', node.path)"><Archive /> 归档文件夹</ContextMenuItem>
        </ContextMenuContent>
      </ContextMenuPortal>
    </ContextMenuRoot>
    <div v-show="open" class="mt-0.5 flex flex-col gap-0.5">
      <template v-for="item in visible" :key="item.id">
        <ContextMenuRoot>
          <ContextMenuTrigger as-child>
            <div class="group/row relative">
              <Button
                type="button"
                size="sm"
                class="h-8 w-full justify-start gap-2 px-2 font-normal group-hover/row:pr-8 group-focus-within/row:pr-8 max-lg:pr-8"
                :variant="item.id === activeId ? 'secondary' : 'ghost'"
                :style="{ paddingLeft: `${12 + depth * 12}px` }"
                :title="item.modified ? absolute.format(item.modified) : undefined"
                @click="emit('resume', item.id, node.path)"
                @dblclick="startRename(item)"
              >
                <!-- Fixed slot so titles stay aligned whatever the state. -->
                <span class="flex size-3.5 shrink-0 items-center justify-center" :aria-label="running.has(item.id) ? (asking.has(item.id) ? '等待回答' : '进行中') : undefined">
                  <CircleHelp v-if="asking.has(item.id)" class="size-3.5 text-amber-500" aria-hidden="true" />
                  <LoaderCircle v-else-if="running.has(item.id)" class="size-3.5 text-emerald-500 motion-safe:animate-spin" aria-hidden="true" />
                  <span v-else-if="item.id === activeId" class="size-1.5 rounded-full bg-foreground/70" />
                  <span v-else class="size-1.5 rounded-full bg-muted-foreground/30" />
                </span>
                <span class="min-w-0 flex-1 truncate text-left" :class="item.prompt ? '' : 'text-muted-foreground'">{{ item.prompt || '空会话' }}</span>
                <span class="shrink-0 group-hover/row:hidden group-focus-within/row:hidden max-lg:hidden">
                  <span v-if="asking.has(item.id)" class="text-xs text-amber-500">待回答</span>
                  <span v-else-if="running.has(item.id)" class="text-xs text-emerald-500">进行中</span>
                  <span v-else-if="item.modified" class="text-xs text-muted-foreground tabular-nums">{{ ago(item.modified) }}</span>
                </span>
              </Button>
              <DropdownMenuRoot>
                <DropdownMenuTrigger as-child>
                  <Button type="button" size="icon-xs" variant="ghost" class="absolute top-1 right-1 opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100 max-lg:opacity-100" :aria-label="`${item.prompt || '空会话'} 的更多操作`">
                    <Ellipsis />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuPortal>
                  <DropdownMenuContent :class="menuContent" align="end">
                    <DropdownMenuItem :class="menuItem" @select="startRename(item)"><Pencil /> 重命名</DropdownMenuItem>
                    <DropdownMenuItem :class="menuItem" @select="emit('archive', 'session', item.id)"><Archive /> 归档</DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenuPortal>
              </DropdownMenuRoot>
            </div>
          </ContextMenuTrigger>
          <ContextMenuPortal>
            <ContextMenuContent :class="menuContent">
              <ContextMenuItem :class="menuItem" @select="startRename(item)"><Pencil /> 重命名</ContextMenuItem>
              <ContextMenuItem :class="menuItem" @select="emit('archive', 'session', item.id)"><Archive /> 归档</ContextMenuItem>
            </ContextMenuContent>
          </ContextMenuPortal>
        </ContextMenuRoot>
      </template>
      <Button
        v-if="hidden > 0 || showEmpty"
        type="button"
        size="sm"
        variant="ghost"
        class="h-7 w-full justify-start px-2 text-xs font-normal text-muted-foreground"
        :style="{ paddingLeft: `${34 + depth * 12}px` }"
        @click="showEmpty = !showEmpty"
      >
        {{ showEmpty ? '收起空会话' : `显示 ${hidden} 个空会话` }}
      </Button>
      <SessionTree
        v-for="child in node.children"
        :key="child.path"
        :node="child"
        :active-id="activeId"
        :running="running"
        :asking="asking"
        :depth="depth + 1"
        @resume="(id, cwd) => emit('resume', id, cwd)"
        @alias="(path, name) => emit('alias', path, name)"
        @rename="(id, title) => emit('rename', id, title)"
        @archive="(kind, key) => emit('archive', kind, key)"
      />
    </div>
  </div>
</template>
