<script setup lang="ts">
import { Check, CircleHelp, CornerDownLeft, Square } from '@lucide/vue'
import { computed, nextTick, reactive, ref, watch } from 'vue'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import type { QuestionAnswers, QuestionItem, QuestionRequest } from '@/lib/api'

const props = defineProps<{ request: QuestionRequest }>()
const emit = defineEmits<{ answer: [answers: QuestionAnswers | null]; interrupt: [] }>()

type Draft = { picked: string[]; text: string }
const drafts = reactive<Record<string, Draft>>({})
const first = ref<HTMLElement | null>(null)

// Each new question group starts from the model's defaults.
watch(() => props.request.id, async () => {
  for (const key of Object.keys(drafts)) delete drafts[key]
  for (const item of props.request.questions) {
    drafts[item.id] = item.type === 'text'
      ? { picked: [], text: item.default[0] ?? '' }
      : { picked: item.default.filter((value) => item.options.some((option) => option.label === value)), text: '' }
  }
  await nextTick()
  first.value?.querySelector<HTMLElement>('button, textarea')?.focus()
}, { immediate: true })

function toggle(item: QuestionItem, option: string) {
  const draft = drafts[item.id]
  if (!draft) return
  if (item.type === 'multi') {
    draft.picked = draft.picked.includes(option) ? draft.picked.filter((value) => value !== option) : [...draft.picked, option]
  } else {
    draft.picked = draft.picked[0] === option ? [] : [option]
    draft.text = ''
  }
}

function answerOf(item: QuestionItem): string | string[] | null {
  const draft = drafts[item.id]
  if (!draft) return null
  const typed = draft.text.trim()
  if (item.type === 'text') return typed || null
  if (item.type === 'single') return typed || draft.picked[0] || null
  const values = typed && !draft.picked.includes(typed) ? [...draft.picked, typed] : draft.picked
  return values.length ? values : null
}

const answers = computed(() => Object.fromEntries(props.request.questions.map((item) => [item.id, answerOf(item)])) as QuestionAnswers)
const answered = computed(() => Object.values(answers.value).filter((value) => value !== null).length)

function submit() {
  if (answered.value) emit('answer', answers.value)
}

function onKey(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault()
    emit('answer', null)
  } else if (event.key === 'Enter' && (event.ctrlKey || event.metaKey) && !event.isComposing) {
    event.preventDefault()
    submit()
  }
}

const kindLabel = { single: '单选', multi: '多选', text: '填空' } as const
</script>

<template>
  <section ref="first" aria-label="AI 提问" class="flex max-h-[min(28rem,60dvh)] flex-col" @keydown="onKey">
    <ol class="flex min-h-0 flex-col gap-3 overflow-y-auto px-1 pt-1 pb-2">
      <li v-for="(item, index) in request.questions" :key="item.id" class="flex flex-col gap-1.5">
        <p class="flex items-start gap-2 text-sm">
          <CircleHelp aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-primary" />
          <span class="min-w-0 font-medium whitespace-pre-wrap">
            <span v-if="request.questions.length > 1" class="text-muted-foreground tabular-nums">{{ index + 1 }}. </span>{{ item.question }}
          </span>
          <span class="ml-auto shrink-0 rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">{{ kindLabel[item.type] }}</span>
        </p>
        <div
          v-if="item.options.length"
          :role="item.type === 'multi' ? 'group' : 'radiogroup'"
          :aria-label="item.question"
          class="pl-6"
          :class="item.options.some((option) => option.description) ? 'grid gap-1.5 sm:grid-cols-2' : 'flex flex-wrap gap-1.5'"
        >
          <Button
            v-for="option in item.options"
            :key="option.label"
            type="button"
            size="sm"
            :variant="drafts[item.id]?.picked.includes(option.label) ? 'default' : 'outline'"
            :role="item.type === 'multi' ? 'checkbox' : 'radio'"
            :aria-checked="drafts[item.id]?.picked.includes(option.label) ?? false"
            :aria-description="option.description || undefined"
            class="h-auto max-w-full py-1 text-left whitespace-normal"
            :class="option.description && 'flex-col items-start gap-0.5 py-2'"
            @click="toggle(item, option.label)"
          >
            <span class="inline-flex items-center gap-1.5 font-medium">
              <Check v-if="drafts[item.id]?.picked.includes(option.label)" aria-hidden="true" />{{ option.label }}
            </span>
            <span v-if="option.description" class="text-xs font-normal opacity-75">{{ option.description }}</span>
          </Button>
        </div>
        <Textarea
          v-if="drafts[item.id]"
          v-model="drafts[item.id]!.text"
          :aria-label="item.type === 'text' ? `回答：${item.question}` : `其他回答：${item.question}`"
          :placeholder="item.type === 'text' ? '输入回答' : item.type === 'multi' ? '其他（可选，会和勾选项一起提交）' : '其他（可选，填写后替代所选项）'"
          class="ml-6 min-h-9 w-[calc(100%-1.5rem)] resize-none py-1.5 text-base sm:text-sm"
          @update:model-value="item.type === 'single' && String($event).trim() && drafts[item.id] && (drafts[item.id]!.picked = [])"
        />
      </li>
    </ol>
    <div class="flex shrink-0 items-center gap-2 border-t px-1 pt-2">
      <span class="text-xs text-muted-foreground">已答 {{ answered }}/{{ request.questions.length }} · Ctrl+Enter 提交 · Esc 跳过</span>
      <Button type="button" size="sm" variant="ghost" class="ml-auto shrink-0 max-sm:h-9" @click="emit('interrupt')">
        <Square /> 停止
      </Button>
      <Button type="button" size="sm" variant="outline" class="shrink-0 max-sm:h-9" @click="emit('answer', null)">跳过</Button>
      <Button type="button" size="sm" class="shrink-0 max-sm:h-9" :disabled="!answered" @click="submit">
        <CornerDownLeft /> 提交
      </Button>
    </div>
  </section>
</template>
