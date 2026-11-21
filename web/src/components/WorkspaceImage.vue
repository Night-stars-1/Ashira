<script setup lang="ts">
import { computed, inject, ref, watch } from 'vue'
import ImageViewer from '@/components/ImageViewer.vue'
import { rawUrl, workspaceSessionKey } from '@/lib/api'

/** An image file in the current session's workspace; click opens it full screen. */
const props = defineProps<{ path: string; alt?: string; size?: 'thumb' | 'inline' }>()
const session = inject(workspaceSessionKey, ref(''))
const src = computed(() => session.value ? rawUrl(session.value, props.path) : '')
const failed = ref(false)
const zoomed = ref(false)
watch(src, () => { failed.value = false })
</script>

<template>
  <p v-if="failed || !src" class="rounded-md border border-dashed px-2 py-1 text-xs text-muted-foreground">图片无法加载：{{ path }}</p>
  <template v-else>
    <button type="button" class="inline-block max-w-full cursor-zoom-in" :title="path" @click="zoomed = true">
      <img
        :src="src"
        :alt="alt || path"
        loading="lazy"
        class="max-w-full rounded-lg border object-contain"
        :class="size === 'thumb' ? 'max-h-64' : 'max-h-96'"
        @error="failed = true"
      />
    </button>
    <ImageViewer v-model:open="zoomed" :src="src" :alt="alt || path" />
  </template>
</template>
