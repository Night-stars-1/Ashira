<script setup lang="ts">
import { ref, watch } from 'vue'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { api, type ApprovalMode, type ApprovalSettings } from '@/lib/api'
import { approvalOptions } from '@/lib/approval'

/** config.yml `approval:` and `proactiveQuestions` on the server: the mode sessions start in, whether
 *  the chat hides its switch, and whether the model asks about open choices before it works. */
const props = defineProps<{ active: boolean }>()
const emit = defineEmits<{ saved: [value: ApprovalSettings] }>()

const settings = ref<ApprovalSettings | null>(null)
const error = ref('')
const saving = ref(false)

watch(() => props.active, async (active) => {
  if (!active) return
  error.value = ''
  try { settings.value = await api.approval() } catch (reason) { error.value = reason instanceof Error ? reason.message : '读取失败' }
}, { immediate: true })

async function save(change: { default?: ApprovalMode; hidden?: boolean; questions?: boolean }) {
  const current = settings.value
  if (!current || saving.value) return
  saving.value = true
  error.value = ''
  try {
    settings.value = await api.setApproval({ default: change.default ?? current.default, hidden: change.hidden ?? current.hidden, questions: change.questions ?? current.questions })
    emit('saved', settings.value)
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : '保存失败'
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="flex items-center gap-3 px-4 py-3.5">
    <span class="min-w-0 flex-1">
      <span class="block text-sm">默认审批模式</span>
      <span class="block text-xs text-muted-foreground">新打开的会话使用此模式</span>
    </span>
    <Select :model-value="settings?.default ?? ''" :disabled="!settings || saving" @update:model-value="(mode) => save({ default: mode as ApprovalMode })">
      <SelectTrigger aria-label="默认审批模式" class="shrink-0">
        <SelectValue placeholder="读取中">{{ approvalOptions.find((option) => option.value === settings?.default)?.label }}</SelectValue>
      </SelectTrigger>
      <SelectContent position="popper" align="end" class="w-60">
        <SelectItem v-for="option in approvalOptions" :key="option.value" :value="option.value">
          <span class="flex flex-col items-start gap-0.5 whitespace-normal">
            <span>{{ option.label }}</span>
            <span class="text-xs text-muted-foreground">{{ option.detail }}</span>
          </span>
        </SelectItem>
      </SelectContent>
    </Select>
  </div>
  <label class="flex items-center gap-3 px-4 py-3.5">
    <span class="min-w-0 flex-1">
      <span class="block text-sm">在输入框显示审批按钮</span>
      <span class="block text-xs text-muted-foreground">关闭后所有会话都按默认审批模式运行</span>
    </span>
    <Switch :model-value="settings ? !settings.hidden : true" :disabled="!settings || saving" @update:model-value="(shown) => save({ hidden: !shown })" />
  </label>
  <p v-if="settings?.forced" class="px-4 pb-3.5 text-xs text-muted-foreground">服务端以 --yes 启动，所有会话固定为完整权限，默认模式在去掉 --yes 后生效</p>
  <label class="flex items-center gap-3 px-4 py-3.5">
    <span class="min-w-0 flex-1">
      <span class="block text-sm">主动提问</span>
      <span class="block text-xs text-muted-foreground">开始前遇到格式、方案、范围等会影响结果的选择先问你</span>
    </span>
    <Switch :model-value="settings ? settings.questions : true" :disabled="!settings || saving" @update:model-value="(on) => save({ questions: on })" />
  </label>
  <p v-if="error" role="alert" class="px-4 pb-3.5 text-sm text-destructive">{{ error }}</p>
</template>
