<script setup lang="ts">
import { ref } from 'vue'
import { Plus, X } from '@lucide/vue'
import TerminalPane from '@/components/TerminalPane.vue'

/** The session's terminals: a tab strip over one shell per tab (each its own server PTY). */
const props = defineProps<{ sessionId: string; active: boolean }>()

/** The server allows 8 terminals at once across every session. */
const max = 8
let next = 1
const tabs = ref([{ id: next++ }])
const current = ref(tabs.value[0]!.id)

function add() {
  if (tabs.value.length >= max) return
  const tab = { id: next++ }
  tabs.value.push(tab)
  current.value = tab.id
}

function close(id: number) {
  const index = tabs.value.findIndex((tab) => tab.id === id)
  tabs.value.splice(index, 1)
  // The last tab closing opens a fresh shell instead of leaving the view empty.
  if (!tabs.value.length) tabs.value.push({ id: next++ })
  if (current.value === id) current.value = tabs.value[Math.min(index, tabs.value.length - 1)]!.id
}

const label = (index: number) => `终端 ${index + 1}`
</script>

<template>
  <section aria-label="终端" class="flex min-h-0 min-w-0 flex-1 flex-col bg-[#101012]">
    <div role="tablist" aria-label="终端" class="flex h-9 shrink-0 items-stretch overflow-x-auto border-b [scrollbar-width:none]">
      <div
        v-for="(tab, index) in tabs"
        :key="tab.id"
        class="group flex shrink-0 items-center border-r text-xs"
        :class="tab.id === current ? 'bg-background/40 text-foreground' : 'text-muted-foreground hover:text-foreground'"
      >
        <button type="button" role="tab" class="h-full pr-1 pl-3" :aria-selected="tab.id === current" @click="current = tab.id">{{ label(index) }}</button>
        <button
          type="button"
          class="mr-1.5 rounded p-0.5 opacity-60 hover:bg-muted hover:opacity-100"
          :aria-label="`关闭${label(index)}`"
          :title="`关闭${label(index)}`"
          @click="close(tab.id)"
        >
          <X class="size-3.5" />
        </button>
      </div>
      <button
        type="button"
        class="flex shrink-0 items-center px-2.5 text-muted-foreground hover:text-foreground disabled:opacity-40"
        :disabled="tabs.length >= max"
        aria-label="新建终端"
        :title="tabs.length >= max ? `最多 ${max} 个终端` : '新建终端'"
        @click="add"
      >
        <Plus class="size-4" />
      </button>
    </div>
    <TerminalPane
      v-for="tab in tabs"
      v-show="tab.id === current"
      :key="tab.id"
      :session-id="props.sessionId"
      :active="props.active && tab.id === current"
    />
  </section>
</template>
