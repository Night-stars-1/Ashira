<script setup lang="ts">
import { useElementSize } from '@vueuse/core'
import { computed, ref } from 'vue'

/** Every reply in miniature, with a box over the ones shown above: drag the box or its top
 *  bar to move it, a handle to resize it, or click elsewhere to centre it there. */
const props = defineProps<{ values: (number | null)[]; from: number; to: number }>()
const emit = defineEmits<{ change: [range: [number, number]] }>()

const BAR = 6
const TRACK = 24
const HEIGHT = BAR + TRACK
const HANDLE_W = 8
const HANDLE_H = 16

const box = ref<HTMLDivElement | null>(null)
const { width } = useElementSize(box)
const count = computed(() => props.values.length)
const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high)
/** Half a handle of room at each end, so a handle on the first or last reply stays whole. */
const PAD = HANDLE_W / 2 + 1
const inner = computed(() => Math.max(1, width.value - PAD * 2))
const xOf = (reply: number) => PAD + (count.value > 1 ? ((reply - 1) / (count.value - 1)) * inner.value : 0)
const replyAt = (x: number) => 1 + ((x - PAD) / inner.value) * (count.value - 1)
const left = computed(() => xOf(props.from))
const right = computed(() => xOf(props.to))

/** The data as a line and the area under it, scaled into the track. */
const shape = computed(() => {
  const top = Math.max(0, ...props.values.map((value) => value ?? 0))
  if (count.value < 2 || top === 0 || width.value === 0) return { line: '', area: '' }
  const points = props.values.flatMap((value, index) => value === null ? [] : [[xOf(index + 1), HEIGHT - 2 - (value / top) * (TRACK - 6)] as const])
  if (!points.length) return { line: '', area: '' }
  const line = points.map(([x, y], index) => `${index ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`).join('')
  return { line, area: `M${points[0]![0].toFixed(1)},${HEIGHT}${line.replace(/^M/, 'L')}L${points.at(-1)![0].toFixed(1)},${HEIGHT}Z` }
})

/** Reply numbers beside the handles, moved inside the box where the edge has no room. */
const labelWidth = (text: string) => text.length * 6.5 + 2
const labels = computed(() => {
  const start = `#${props.from}`
  const end = `#${props.to}`
  const gap = HANDLE_W / 2 + 4
  const startOutside = left.value - gap - labelWidth(start) >= 0
  const endOutside = right.value + gap + labelWidth(end) <= width.value
  return [
    { text: start, x: startOutside ? left.value - gap : left.value + gap, anchor: startOutside ? 'end' : 'start' },
    { text: end, x: endOutside ? right.value + gap : right.value - gap, anchor: endOutside ? 'start' : 'end' },
  ]
})

type Grip = 'start' | 'end' | 'move'
const dragging = ref<Grip | null>(null)
const hover = ref<Grip | 'outside'>('outside')
/** The reply numbers beside the handles show only while the pointer is over the bar, a
 *  drag runs, or the keyboard has it, so they never cover the overview at rest. */
const pointerIn = ref(false)
const focused = ref(false)
const showLabels = computed(() => pointerIn.value || focused.value || dragging.value !== null)
let startAt = 0
let startRange: [number, number] = [1, 1]

function local(event: PointerEvent) {
  return event.clientX - box.value!.getBoundingClientRect().left
}

function gripAt(x: number): Grip | null {
  const near = HANDLE_W
  const toStart = Math.abs(x - left.value)
  const toEnd = Math.abs(x - right.value)
  if (toStart <= near || toEnd <= near) return toStart < toEnd || (toStart === toEnd && x < left.value) ? 'start' : 'end'
  return x > left.value && x < right.value ? 'move' : null
}

function down(event: PointerEvent) {
  if (count.value < 2 || event.button !== 0) return
  const x = local(event)
  let grip = gripAt(x)
  if (grip) startRange = [props.from, props.to]
  else {
    // Outside the box: centre it there, keeping its length, and keep dragging it.
    const span = props.to - props.from
    const from = clamp(Math.round(replyAt(x)) - Math.floor(span / 2), 1, count.value - span)
    startRange = [from, from + span]
    emit('change', startRange)
    grip = 'move'
  }
  startAt = replyAt(x)
  dragging.value = grip
  box.value!.setPointerCapture(event.pointerId)
  event.preventDefault()
}

function move(event: PointerEvent) {
  const x = local(event)
  const grip = dragging.value
  if (!grip) {
    hover.value = gripAt(x) ?? 'outside'
    return
  }
  const [from, to] = startRange
  const reply = Math.round(replyAt(clamp(x, 0, width.value)))
  if (grip === 'start') emit('change', [clamp(reply, 1, to), to])
  else if (grip === 'end') emit('change', [from, clamp(reply, from, count.value)])
  else {
    const next = clamp(from + Math.round(replyAt(x) - startAt), 1, count.value - (to - from))
    emit('change', [next, next + to - from])
  }
}

function up(event: PointerEvent) {
  dragging.value = null
  if (box.value?.hasPointerCapture(event.pointerId)) box.value.releasePointerCapture(event.pointerId)
}

/** Arrows move the box a reply at a time; Home and End jump to either end. */
function key(event: KeyboardEvent) {
  const span = props.to - props.from
  const step = { ArrowLeft: -1, ArrowRight: 1, Home: -count.value, End: count.value }[event.key]
  if (step === undefined) return
  const from = clamp(props.from + step, 1, count.value - span)
  emit('change', [from, from + span])
  event.preventDefault()
}

const cursor = computed(() => {
  const state = dragging.value ?? hover.value
  return state === 'move' ? (dragging.value ? 'grabbing' : 'grab') : state === 'outside' ? 'pointer' : 'ew-resize'
})
</script>

<template>
  <div
    ref="box"
    class="relative w-full touch-none select-none rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
    :style="{ height: `${HEIGHT}px`, cursor }"
    role="slider"
    tabindex="0"
    aria-label="显示的回复区间"
    :aria-valuemin="1"
    :aria-valuemax="count"
    :aria-valuenow="from"
    :aria-valuetext="`#${from} 到 #${to}`"
    @pointerdown="down"
    @pointermove="move"
    @pointerup="up"
    @pointercancel="up"
    @pointerenter="pointerIn = true"
    @pointerleave="pointerIn = false; hover = 'outside'"
    @focus="focused = true"
    @blur="focused = false"
    @keydown="key"
  >
    <svg v-if="width > 0" :width="width" :height="HEIGHT" class="block overflow-visible" aria-hidden="true">
      <rect x="0" :y="BAR" :width="width" :height="TRACK" rx="2" class="fill-muted/60" />
      <path v-if="shape.area" :d="shape.area" class="fill-sky-500/10" />
      <path v-if="shape.line" :d="shape.line" class="fill-none stroke-sky-500/50" stroke-width="1" />
      <rect :x="left" :y="BAR" :width="Math.max(0, right - left)" :height="TRACK" class="fill-sky-500/15 stroke-sky-500/40" stroke-width="1" />
      <rect :x="left" y="0" :width="Math.max(0, right - left)" :height="BAR" rx="2" class="fill-sky-500/45" />
      <rect v-for="offset in [-3, 0, 3]" :key="offset" :x="(left + right) / 2 + offset - 0.5" y="1.5" width="1" height="3" class="fill-background" />
      <rect
        v-for="(x, index) in [left, right]"
        :key="index"
        :x="x - HANDLE_W / 2"
        :y="BAR + (TRACK - HANDLE_H) / 2"
        :width="HANDLE_W"
        :height="HANDLE_H"
        rx="3"
        class="fill-background stroke-sky-500/70"
        stroke-width="1"
      />
      <text
        v-for="label in showLabels ? labels : []"
        :key="label.text + label.anchor"
        :x="label.x"
        :y="BAR + TRACK / 2 + 4"
        :text-anchor="label.anchor"
        class="fill-muted-foreground text-[11px] tabular-nums"
      >{{ label.text }}</text>
    </svg>
  </div>
</template>
