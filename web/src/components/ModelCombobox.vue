<script setup lang="ts">
import { computed, ref } from 'vue'
import { CheckIcon, ChevronsUpDownIcon } from '@lucide/vue'
import { ComboboxAnchor, ComboboxTrigger } from 'reka-ui'
import { Combobox, ComboboxEmpty, ComboboxGroup, ComboboxInput, ComboboxItem, ComboboxItemIndicator, ComboboxList, ComboboxViewport } from '@/components/ui/combobox'
import type { ModelItem } from '@/lib/api'

/** Searchable model selector; '' is the "none" choice described by `placeholder`. */
const value = defineModel<string>({ required: true })
const props = defineProps<{ models: ModelItem[]; placeholder: string; label: string }>()

/** reka forbids '' as an item value, so the "none" row uses a sentinel. */
const NONE = '\u0000none'
const open = ref(false)

const groups = computed(() => {
  const byProvider = new Map<string, ModelItem[]>()
  for (const model of props.models) byProvider.set(model.provider, [...(byProvider.get(model.provider) ?? []), model])
  return [...byProvider.entries()]
})
/** A configured model the registry doesn't list (provider down, typo) stays visible and flagged. */
const unlisted = computed(() => value.value !== '' && !props.models.some((model) => model.selector === value.value))
const selected = computed(() => props.models.find((model) => model.selector === value.value))

const inner = computed({
  get: () => value.value || NONE,
  set: (next: string) => { value.value = next === NONE ? '' : next },
})
</script>

<template>
  <Combobox v-model="inner" v-model:open="open" :reset-search-term-on-blur="true" :reset-search-term-on-select="true">
    <ComboboxAnchor as-child>
      <ComboboxTrigger
        :aria-label="label"
        class="flex h-9 w-full min-w-0 items-center gap-2 rounded-lg border border-input bg-transparent px-2.5 text-left text-sm outline-none transition-colors hover:bg-input/50 focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30"
      >
        <span v-if="!value" class="min-w-0 flex-1 truncate text-muted-foreground">{{ placeholder }}</span>
        <span v-else class="flex min-w-0 flex-1 items-baseline gap-1.5">
          <span class="truncate">{{ value }}</span>
          <span v-if="selected?.name" class="truncate text-xs text-muted-foreground">{{ selected.name }}</span>
          <span v-if="unlisted" class="shrink-0 text-xs text-destructive">不在模型列表里</span>
        </span>
        <ChevronsUpDownIcon aria-hidden="true" class="size-4 shrink-0 text-muted-foreground" />
      </ComboboxTrigger>
    </ComboboxAnchor>
    <ComboboxList class="w-(--reka-combobox-trigger-width) min-w-72">
      <!-- The trigger shows the selection; the search box always starts empty. -->
      <ComboboxInput placeholder="搜索模型" aria-label="搜索模型" :display-value="() => ''" />
      <ComboboxEmpty>没有匹配的模型</ComboboxEmpty>
      <ComboboxViewport>
        <ComboboxItem :value="NONE" :text-value="placeholder">
          <span class="truncate text-muted-foreground">{{ placeholder }}</span>
          <ComboboxItemIndicator><CheckIcon /></ComboboxItemIndicator>
        </ComboboxItem>
        <ComboboxItem v-if="unlisted" :value="value" :text-value="value">
          <span class="truncate">{{ value }}</span>
          <span class="ml-auto shrink-0 text-xs text-destructive">不在模型列表里</span>
          <ComboboxItemIndicator><CheckIcon /></ComboboxItemIndicator>
        </ComboboxItem>
        <ComboboxGroup v-for="[provider, items] in groups" :key="provider" :heading="provider">
          <ComboboxItem v-for="model in items" :key="model.selector" :value="model.selector" :text-value="`${model.selector} ${model.name ?? ''}`">
            <span class="truncate">{{ model.id }}</span>
            <span v-if="model.name && model.name !== model.id" class="truncate text-xs text-muted-foreground">{{ model.name }}</span>
            <ComboboxItemIndicator><CheckIcon /></ComboboxItemIndicator>
          </ComboboxItem>
        </ComboboxGroup>
      </ComboboxViewport>
    </ComboboxList>
  </Combobox>
</template>
