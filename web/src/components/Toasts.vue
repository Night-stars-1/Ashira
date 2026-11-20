<script setup lang="ts">
import { CircleAlert, X } from '@lucide/vue'
import { dismiss, toasts } from '@/lib/toast'
</script>

<template>
  <ol aria-live="assertive" class="pointer-events-none fixed top-3 right-3 z-[100] flex w-[min(22rem,calc(100%-1.5rem))] flex-col gap-2">
    <TransitionGroup
      enter-from-class="translate-x-4 opacity-0"
      leave-to-class="translate-x-4 opacity-0"
      enter-active-class="transition duration-200 motion-reduce:transition-none"
      leave-active-class="transition duration-200 motion-reduce:transition-none"
    >
      <li v-for="item in toasts" :key="item.id" role="alert" class="pointer-events-auto flex items-start gap-2 rounded-lg border border-destructive/40 bg-popover px-3 py-2.5 text-sm text-popover-foreground shadow-lg">
        <CircleAlert aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-destructive" />
        <span class="min-w-0 flex-1 [overflow-wrap:anywhere]">{{ item.text }}</span>
        <button type="button" class="-m-1 shrink-0 rounded p-1 text-muted-foreground hover:text-foreground focus-visible:outline-2" aria-label="关闭提示" @click="dismiss(item.id)">
          <X class="size-3.5" />
        </button>
      </li>
    </TransitionGroup>
  </ol>
</template>
