<script setup lang="ts">
import { FileText, X } from '@lucide/vue'
import { computed, ref } from 'vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import type { TextFile } from '@/components/Transcript.vue'

const props = defineProps<{ file: TextFile; removable?: boolean }>()
const emit = defineEmits<{ remove: [] }>()
const open = ref(false)

// Compact counts so each fits the image card's width; exact numbers are in the tooltip.
const count = (n: number) => n >= 10_000 ? `${Number((n / 10_000).toFixed(1))}万` : String(n)
const size = computed(() => `${props.file.lines} 行 · ${props.file.text.length} 字`)
</script>

<template>
  <div class="relative flex w-24 max-w-full shrink-0 overflow-hidden rounded-md border bg-muted text-foreground">
    <button type="button" class="flex min-w-0 flex-1 flex-col justify-center gap-1 p-2 text-left hover:bg-accent focus-visible:bg-accent focus-visible:outline-none" :aria-label="`查看文件 ${file.name}`" :title="`${file.name}\n${size}`" @click="open = true">
      <FileText aria-hidden="true" class="size-5 shrink-0 text-muted-foreground" />
      <span class="truncate text-xs font-medium">{{ file.name }}</span>
      <span class="flex flex-col text-[0.7rem] leading-tight text-muted-foreground">
        <span class="truncate">{{ count(file.lines) }} 行</span>
        <span class="truncate">{{ count(file.text.length) }} 字</span>
      </span>
    </button>
    <Button v-if="removable" type="button" size="icon-xs" variant="secondary" class="absolute top-1 right-1" :aria-label="`移除文件 ${file.name}`" @click="emit('remove')">
      <X />
    </Button>
  </div>
  <Dialog v-model:open="open">
    <DialogContent class="flex max-h-[85dvh] flex-col sm:max-w-3xl">
      <DialogHeader>
        <DialogTitle class="truncate pr-6">{{ file.name }}</DialogTitle>
        <DialogDescription>{{ size }}</DialogDescription>
      </DialogHeader>
      <pre class="min-h-0 flex-1 overflow-auto rounded-md bg-muted p-3 font-mono text-xs leading-relaxed whitespace-pre">{{ file.text }}</pre>
    </DialogContent>
  </Dialog>
</template>
