<script setup lang="ts">
withDefaults(defineProps<{ rows?: number; twoLine?: boolean }>(), { rows: 6, twoLine: false })
// Varied but fixed widths read as a real list without jumping between renders.
const widths = ['w-2/3', 'w-1/2', 'w-3/4', 'w-2/5', 'w-3/5', 'w-1/2', 'w-2/3', 'w-1/3']
</script>

<template>
  <div role="status" aria-label="正在加载" class="flex flex-col">
    <div v-for="index in rows" :key="index" class="flex flex-col justify-center gap-1.5 border-b px-3 last:border-b-0" :class="twoLine ? 'h-14' : 'h-10'">
      <div class="h-3.5 rounded bg-muted motion-safe:animate-pulse" :class="widths[(index - 1) % widths.length]" />
      <div v-if="twoLine" class="h-2.5 w-1/3 rounded bg-muted motion-safe:animate-pulse" />
    </div>
  </div>
</template>
