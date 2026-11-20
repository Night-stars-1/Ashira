<script setup lang="ts">
import FileTreeSkeleton from '@/components/FileTreeSkeleton.vue'
</script>

<template>
  <!-- Same box as FilesSidebar, so switching sessions never resizes the chat column. -->
  <aside aria-hidden="true" class="flex w-64 shrink-0 flex-col border-l border-sidebar-border bg-sidebar">
    <div class="flex h-12 shrink-0 items-center gap-1 px-2">
      <span class="size-6 shrink-0 rounded-md bg-muted motion-safe:animate-pulse" />
      <span class="h-7 flex-1 rounded-md bg-muted motion-safe:animate-pulse" />
      <span class="size-6 shrink-0 rounded-md bg-muted motion-safe:animate-pulse" />
      <span class="size-6 shrink-0 rounded-md bg-muted motion-safe:animate-pulse" />
    </div>
    <div class="px-2 pb-2"><FileTreeSkeleton /></div>
  </aside>
</template>
