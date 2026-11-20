<script setup lang="ts">
import { computed } from 'vue'
import { lexer } from 'marked'
import MarkdownNodes, { type MdNode } from '@/components/MarkdownNodes.vue'

const props = defineProps<{ text: string }>()
const tokens = computed(() => lexer(props.text) as unknown as MdNode[])
</script>

<template>
  <div class="flex flex-col gap-3 text-sm">
    <MarkdownNodes :tokens="tokens" />
  </div>
</template>
