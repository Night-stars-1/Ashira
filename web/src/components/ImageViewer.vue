<script setup lang="ts">
import { X } from '@lucide/vue'
import { DialogClose, DialogContent, DialogOverlay, DialogPortal, DialogRoot, DialogTitle } from 'reka-ui'
import { reactive, ref, watch } from 'vue'

/** A picture over the whole window: wheel or pinch zooms around the pointer, drag pans,
 *  double click toggles 2.5×, Esc / ✕ / a click on the backdrop at fit size closes. */
const props = defineProps<{ src: string; alt: string }>()
const open = defineModel<boolean>('open', { required: true })

const MAX = 8
const view = reactive({ scale: 1, x: 0, y: 0 })
const stage = ref<HTMLElement | null>(null)
watch(open, (shown) => { if (shown) Object.assign(view, { scale: 1, x: 0, y: 0 }) })

/** Scales to `next` keeping the stage point under (`px`, `py`) still. */
function zoomAt(next: number, px: number, py: number) {
  const rect = stage.value?.getBoundingClientRect()
  if (!rect) return
  const scale = Math.min(MAX, Math.max(1, next))
  // Offsets are from the stage centre, where the picture sits unscaled.
  const cx = px - rect.left - rect.width / 2
  const cy = py - rect.top - rect.height / 2
  const ratio = scale / view.scale
  view.x = scale === 1 ? 0 : cx - (cx - view.x) * ratio
  view.y = scale === 1 ? 0 : cy - (cy - view.y) * ratio
  view.scale = scale
}

function onWheel(event: WheelEvent) {
  zoomAt(view.scale * Math.exp(-event.deltaY * 0.0015), event.clientX, event.clientY)
}

// Pointers on the stage: one drags, two pinch. Taps are told apart here rather than with
// click/dblclick, which pointer capture retargets and touch screens do not double reliably.
const pointers = new Map<number, { x: number; y: number }>()
let pinch = 0
let moved = false
/** Where the press began: on the picture, or the backdrop around it. */
let onPicture = false
let lastTap = { at: 0, x: 0, y: 0 }

function onPointerDown(event: PointerEvent) {
  stage.value?.setPointerCapture(event.pointerId)
  pointers.set(event.pointerId, { x: event.clientX, y: event.clientY })
  if (pointers.size === 1) {
    moved = false
    onPicture = event.target !== stage.value
  } else {
    moved = true
    const [a, b] = [...pointers.values()]
    pinch = Math.hypot(a.x - b.x, a.y - b.y)
  }
}

function onPointerMove(event: PointerEvent) {
  const last = pointers.get(event.pointerId)
  if (!last) return
  const point = { x: event.clientX, y: event.clientY }
  if (Math.abs(point.x - last.x) + Math.abs(point.y - last.y) > 2) moved = true
  pointers.set(event.pointerId, point)
  if (pointers.size === 2) {
    const [a, b] = [...pointers.values()]
    const distance = Math.hypot(a.x - b.x, a.y - b.y)
    if (pinch > 0) zoomAt(view.scale * (distance / pinch), (a.x + b.x) / 2, (a.y + b.y) / 2)
    pinch = distance
  } else if (view.scale > 1) {
    view.x += point.x - last.x
    view.y += point.y - last.y
  }
}

/** A second tap within 300 ms toggles 2.5× there; a single tap on the backdrop at fit size closes. */
function onPointerUp(event: PointerEvent) {
  pointers.delete(event.pointerId)
  if (pointers.size < 2) pinch = 0
  if (pointers.size > 0 || moved) return
  const now = performance.now()
  const double = now - lastTap.at < 300 && Math.hypot(event.clientX - lastTap.x, event.clientY - lastTap.y) < 24
  lastTap = double ? { at: 0, x: 0, y: 0 } : { at: now, x: event.clientX, y: event.clientY }
  if (double) zoomAt(view.scale > 1.01 ? 1 : 2.5, event.clientX, event.clientY)
  else if (!onPicture && view.scale <= 1.01) open.value = false
}
</script>

<template>
  <DialogRoot v-model:open="open">
    <DialogPortal>
      <DialogOverlay class="fixed inset-0 z-50 bg-black/90 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0" />
      <DialogContent class="fixed inset-0 z-50 outline-none" :aria-describedby="undefined">
        <DialogTitle class="sr-only">{{ props.alt }}</DialogTitle>
        <div
          ref="stage"
          class="flex size-full touch-none items-center justify-center overflow-hidden select-none"
          :class="view.scale > 1 ? 'cursor-grab active:cursor-grabbing' : 'cursor-zoom-in'"
          @wheel.prevent="onWheel"
          @pointerdown="onPointerDown"
          @pointermove="onPointerMove"
          @pointerup="onPointerUp"
          @pointercancel="pointers.delete($event.pointerId)"
        >
          <img
            :src="props.src"
            :alt="props.alt"
            draggable="false"
            class="max-h-full max-w-full object-contain"
            :style="{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }"
          />
        </div>
        <DialogClose class="absolute top-[max(0.75rem,env(safe-area-inset-top))] right-3 inline-flex size-9 items-center justify-center rounded-full bg-black/50 text-white hover:bg-black/70 focus-visible:ring-2 focus-visible:ring-white focus-visible:outline-none" aria-label="关闭">
          <X class="size-5" aria-hidden="true" />
        </DialogClose>
        <p v-if="view.scale > 1.01" class="pointer-events-none absolute bottom-[max(0.75rem,env(safe-area-inset-bottom))] left-1/2 -translate-x-1/2 rounded-full bg-black/50 px-2.5 py-1 text-xs text-white tabular-nums">{{ Math.round(view.scale * 100) }}%</p>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
