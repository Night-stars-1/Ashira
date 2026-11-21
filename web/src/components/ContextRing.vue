<script setup lang="ts">
import { computed } from 'vue'

/** Context used by the current session, as a ring; the exact numbers are in the tooltip. */
const props = defineProps<{ context: number; window: number | null }>()

const tokens = new Intl.NumberFormat('zh-CN')
const share = computed(() => props.window ? Math.min(1, props.context / props.window) : 0)
const radius = 7
const length = 2 * Math.PI * radius
const label = computed(() => props.window
  ? `上下文 ${Math.round(share.value * 100)}%：${tokens.format(props.context)} / ${tokens.format(props.window)} tokens`
  : `上下文 ${tokens.format(props.context)} tokens`)
</script>

<template>
  <span role="img" class="inline-flex size-8 shrink-0 items-center justify-center" :aria-label="label" :title="label">
    <svg viewBox="0 0 18 18" class="size-[18px] -rotate-90" aria-hidden="true">
      <circle cx="9" cy="9" :r="radius" fill="none" stroke-width="2.5" class="stroke-muted-foreground/30" />
      <circle
        cx="9" cy="9" :r="radius" fill="none" stroke-width="2.5" stroke-linecap="round"
        :stroke-dasharray="length" :stroke-dashoffset="length * (1 - share)"
        :class="share >= 0.9 ? 'stroke-destructive' : share >= 0.7 ? 'stroke-amber-500' : 'stroke-muted-foreground'"
        class="transition-[stroke-dashoffset] duration-300"
      />
    </svg>
  </span>
</template>
