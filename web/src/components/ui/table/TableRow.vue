<script setup lang="ts">
import type { HTMLAttributes } from 'vue'
import { cn } from 'cn'

const props = defineProps<{ class?: HTMLAttributes['class'] }>()
</script>

<template>
  <tr data-slot="table-row" :class="cn('hover:bg-muted/50 border-b transition-colors', props.class)">
    <slot />
  </tr>
</template>
