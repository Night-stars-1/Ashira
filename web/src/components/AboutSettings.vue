<script setup lang="ts">
import { ChevronDown, ExternalLink, RefreshCw } from '@lucide/vue'
import { computed, ref } from 'vue'
import Markdown from '@/components/Markdown.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { UpdateProgress } from '@/lib/api'
import { checkWebUpdate, serverBehind as behind, webUpdate, webVersion } from '@/lib/update'

const props = defineProps<{ serverVersion: string; serverUpdate: string | null; hosted: boolean; restartPending: boolean; container: boolean; restarting: boolean; updating: boolean; updateProgress: UpdateProgress | null }>()
const emit = defineEmits<{ restart: []; update: [] }>()

const stageLabels = { checking: '正在查询版本', downloading: '正在下载', installing: '正在校验并替换', web: '正在更新网页' } as const

/** The line under the server row while it updates: the step, and a share while a file downloads. */
const progress = computed(() => {
  if (!props.updating) return null
  const update = props.updateProgress
  const downloading = update?.stage === 'downloading' || update?.stage === 'web'
  const percent = downloading && update?.total ? Math.min(100, Math.round((update.received / update.total) * 100)) : null
  return { label: update?.stage ? stageLabels[update.stage] : '正在开始', percent }
})

const releases = 'https://github.com/Night-stars-1/Ashira/releases'
const shown = (version: string) => (version ? `v${version.replace(/^v/, '')}` : '未知')

const serverBehind = computed(() => behind(props.serverUpdate, props.serverVersion))

/** A page the core serves (`--web`) is the core's own: it updates and restarts with it, so it has no row. */
const webState = computed(() => {
  if (webVersion === 'dev') return { label: '开发版', variant: 'outline' as const }
  if (webUpdate.available) return { label: `可更新到 ${webUpdate.available.tag}`, variant: 'default' as const }
  if (webUpdate.latest) return { label: '已是最新', variant: 'secondary' as const }
  return { label: '未检查', variant: 'outline' as const }
})

/** Release notes start folded; the header row toggles them. */
const notesOpen = ref(false)
</script>

<template>
  <div class="flex flex-col gap-4">
    <header class="flex items-center gap-3">
      <img src="/favicon.svg" alt="" class="size-12 rounded-xl" />
      <div class="min-w-0 flex-1">
        <p class="text-base font-medium">Ashira</p>
        <p class="text-xs text-muted-foreground">
          最新发布 {{ webUpdate.checking ? '检查中...' : webUpdate.latest?.tag || '未知' }}
        </p>
      </div>
      <Button type="button" size="sm" variant="outline" :disabled="webUpdate.checking" @click="checkWebUpdate(true)">
        <RefreshCw :class="webUpdate.checking && 'motion-safe:animate-spin'" />
        {{ webUpdate.checking ? '检查中' : '检查更新' }}
      </Button>
    </header>

    <p v-if="webUpdate.error" role="alert" class="text-sm text-destructive">{{ webUpdate.error }}</p>

    <ul class="flex flex-col divide-y rounded-lg border">
      <li v-if="!hosted" class="flex items-center gap-3 px-3 py-3">
        <div class="min-w-0 flex-1">
          <p class="text-sm">网页</p>
          <p class="text-xs text-muted-foreground tabular-nums">{{ webVersion === 'dev' ? '本地构建' : shown(webVersion) }}</p>
        </div>
        <Badge :variant="webState.variant">{{ webState.label }}</Badge>
        <Button v-if="webUpdate.available" as="a" size="sm" :href="webUpdate.available.url" target="_blank" rel="noreferrer noopener">
          下载 <ExternalLink />
        </Button>
      </li>
      <li class="flex flex-col gap-2 px-3 py-3">
        <div class="flex items-center gap-3">
          <div class="min-w-0 flex-1">
            <p class="text-sm">服务端</p>
            <p class="text-xs text-muted-foreground tabular-nums">{{ shown(serverVersion) }}</p>
          </div>
          <template v-if="restartPending">
            <Badge variant="default">已更新，待重启</Badge>
            <Button type="button" size="sm" :disabled="restarting" @click="emit('restart')">{{ restarting ? '重启中…' : '重启' }}</Button>
          </template>
          <!-- A container is updated by pulling its image; nothing to press here. -->
          <Badge v-else-if="serverBehind && container" variant="default" title="docker compose pull && docker compose up -d">可更新到 {{ serverBehind }}，请拉取新镜像</Badge>
          <template v-else-if="serverBehind">
            <Badge variant="default">可更新到 {{ serverBehind }}</Badge>
            <Button type="button" size="sm" :disabled="updating" @click="emit('update')">{{ updating ? '更新中…' : '更新' }}</Button>
          </template>
          <Badge v-else-if="serverVersion" variant="secondary">已是最新</Badge>
        </div>
        <div v-if="progress" class="flex items-center gap-3">
          <div role="progressbar" :aria-label="`服务端更新：${progress.label}`" aria-valuemin="0" aria-valuemax="100" :aria-valuenow="progress.percent ?? undefined" class="relative h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-muted">
            <div v-if="progress.percent !== null" class="h-full rounded-full bg-primary transition-[width] duration-300 ease-out motion-reduce:transition-none" :style="{ width: `${progress.percent}%` }" />
            <!-- No share to show: a segment slides across so it does not read as half done. -->
            <div v-else class="progress-sweep absolute inset-y-0 w-1/3 rounded-full bg-primary" />
          </div>
          <span class="shrink-0 text-xs text-muted-foreground tabular-nums">{{ progress.percent !== null ? `${progress.label} ${progress.percent}%` : progress.label }}</span>
        </div>
      </li>
    </ul>

    <section v-if="webUpdate.latest?.notes" class="rounded-lg bg-muted">
      <button type="button" class="flex w-full items-center gap-1.5 px-4 py-2.5 text-left text-sm" :aria-expanded="notesOpen" @click="notesOpen = !notesOpen">
        <span class="min-w-0 flex-1">{{ webUpdate.latest.tag }} 更新说明</span>
        <ChevronDown aria-hidden="true" class="size-4 shrink-0 text-muted-foreground transition-transform motion-reduce:transition-none" :class="notesOpen && 'rotate-180'" />
      </button>
      <!-- grid-rows 0fr→1fr animates to the notes' natural height; inert keeps folded links out of tab order. -->
      <div class="grid transition-[grid-template-rows] duration-200 ease-out motion-reduce:transition-none" :class="notesOpen ? 'grid-rows-[1fr]' : 'grid-rows-[0fr]'" :inert="!notesOpen">
        <div class="min-h-0 overflow-hidden">
          <div class="px-4 pb-3">
            <Markdown :text="webUpdate.latest.notes" />
          </div>
        </div>
      </div>
    </section>

    <a :href="releases" target="_blank" rel="noreferrer noopener" class="inline-flex w-fit items-center gap-1 text-xs text-muted-foreground underline-offset-4 hover:text-foreground hover:underline">
      所有版本与更新说明 <ExternalLink class="size-3" />
    </a>
  </div>
</template>

<style scoped>
.progress-sweep {
  animation: progress-sweep 1.2s ease-in-out infinite;
}
@keyframes progress-sweep {
  from { left: -33%; }
  to { left: 100%; }
}
@media (prefers-reduced-motion: reduce) {
  .progress-sweep { animation: none; left: 0; width: 100%; opacity: 0.4; }
}
</style>
