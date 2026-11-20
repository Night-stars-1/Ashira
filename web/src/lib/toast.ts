import { ref } from 'vue'

export type Toast = { id: number; text: string }

/** Transient top-right notices for request and connection failures. */
export const toasts = ref<Toast[]>([])
let next = 0

export function toast(text: string) {
  // The same failure repeating (reconnect loops) refreshes one notice instead of stacking.
  const existing = toasts.value.find((item) => item.text === text)
  if (existing) dismiss(existing.id)
  const id = next++
  toasts.value.push({ id, text })
  if (toasts.value.length > 3) toasts.value.shift()
  setTimeout(() => dismiss(id), 5000)
}

export function dismiss(id: number) {
  toasts.value = toasts.value.filter((item) => item.id !== id)
}
