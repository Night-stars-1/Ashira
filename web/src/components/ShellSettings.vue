<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { api, type ShellStatus } from '@/lib/api'

/** The shell the bash tool runs commands with, saved in the server's config.yml. */
const props = defineProps<{ active: boolean; restarting: boolean }>()
const emit = defineEmits<{ restart: [] }>()

const status = ref<ShellStatus | null>(null)
const draft = ref('')
const error = ref('')
const saving = ref(false)
const saved = ref(false)

async function load() {
  error.value = ''
  try {
    status.value = await api.shell()
    draft.value = status.value.configured ?? ''
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : '读取失败'
  }
}
watch(() => props.active, (active) => { if (active) void load() }, { immediate: true })

const source = computed(() => ({ env: '环境变量 ASHIRA_SHELL', config: 'config.yml', auto: '自动查找' })[status.value?.source ?? 'auto'])
const changed = computed(() => draft.value.trim() !== (status.value?.configured ?? ''))

/** The server runs `printf` through the shell before saving, so a bad path never lands. */
async function save(shell: string) {
  saving.value = true
  error.value = ''
  saved.value = false
  try {
    status.value = await api.setShell(shell)
    draft.value = status.value.configured ?? ''
    saved.value = true
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : '保存失败'
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <section class="flex flex-col gap-3 rounded-lg border px-3 py-3">
    <div class="flex items-start gap-3">
      <div class="min-w-0 flex-1">
        <p class="text-sm">Shell</p>
        <p class="text-xs text-muted-foreground">AI 执行命令（bash 工具）用的 shell，需要是 bash、zsh、sh 这类 POSIX shell</p>
      </div>
    </div>
    <p v-if="status" class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
      <span>正在使用</span>
      <code class="rounded bg-muted px-1.5 py-0.5 font-mono break-all text-foreground">{{ status.current }}</code>
      <Badge variant="secondary">{{ source }}</Badge>
    </p>
    <p v-if="status?.envOverride" class="text-xs text-muted-foreground">服务端设置了环境变量 ASHIRA_SHELL，它优先于这里保存的路径。</p>
    <form class="flex flex-col gap-2 sm:flex-row" @submit.prevent="save(draft)">
      <Input v-model="draft" class="font-mono" placeholder="留空则自动查找，例如 C:\Program Files\Git\bin\bash.exe" autocomplete="off" spellcheck="false" aria-label="Shell 路径" />
      <div class="flex shrink-0 gap-2">
        <Button type="submit" :disabled="saving || !changed">{{ saving ? '测试中…' : '测试并保存' }}</Button>
        <Button v-if="status?.configured" type="button" variant="outline" :disabled="saving" @click="save('')">恢复自动</Button>
      </div>
    </form>
    <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>
    <p v-else-if="status?.problem" role="alert" class="text-sm text-destructive">{{ status.problem }}</p>
    <div v-if="status?.restartNeeded" class="flex items-center gap-2 text-xs text-muted-foreground">
      <span class="min-w-0 flex-1">{{ saved ? '已保存，' : '' }}重启服务端后生效</span>
      <Button type="button" size="sm" :disabled="restarting" @click="emit('restart')">{{ restarting ? '重启中…' : '重启' }}</Button>
    </div>
    <p v-else-if="saved" role="status" class="text-xs text-muted-foreground">已保存</p>
  </section>
</template>
