<script setup lang="ts">
// Fixed widths read as a plausible listing without random jitter between renders.
const widths = ['w-14', 'w-10', 'w-12', 'w-16', 'w-10', 'w-20', 'w-24', 'w-20', 'w-28', 'w-24']
</script>

<template>
  <div role="status" aria-label="正在加载文件" class="flex flex-col">
    <div v-for="(width, index) in widths" :key="index" class="flex h-8 items-center gap-1.5 px-2">
      <span class="size-3.5 shrink-0 rounded-sm bg-muted motion-safe:animate-pulse" />
      <span class="h-3 rounded-sm bg-muted motion-safe:animate-pulse" :class="width" />
    </div>
  </div>
</template>
