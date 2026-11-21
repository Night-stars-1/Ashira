<script setup lang="ts">
import { ChevronDown, ExternalLink, RefreshCw } from '@lucide/vue'
import { computed, ref } from 'vue'
import Markdown from '@/components/Markdown.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { api } from '@/lib/api'
import { checkWebUpdate, newer, webUpdate, webVersion } from '@/lib/update'

const props = defineProps<{ serverVersion: string; serverUpdate: string | null; hosted: boolean; restartPending: boolean; restarting: boolean }>()
const emit = defineEmits<{ restart: [] }>()

const releases = 'https://github.com/Night-stars-1/Ashira/releases'
const shown = (version: string) => (version ? `v${version.replace(/^v/, '')}` : '未知')

/** Whether the server is behind the latest release we know of (its own check, else ours). */
const serverBehind = computed(() => props.serverUpdate
  || (webUpdate.latest && props.serverVersion && newer(webUpdate.latest.tag, props.serverVersion) ? webUpdate.latest.tag : null))

/** Served by the core (`--web`): the page follows the core's version, and the core installs it.
 *  A local `dev` build is the developer's own and is never offered a replacement. */
const behindCore = computed(() => props.hosted && webVersion !== 'dev' && !!props.serverVersion && newer(props.serverVersion, webVersion))

const webState = computed(() => {
  if (webVersion === 'dev') return { label: '开发版', variant: 'outline' as const }
  if (props.hosted) return behindCore.value ? { label: `可更新到 ${shown(props.serverVersion)}`, variant: 'default' as const } : { label: '已是最新', variant: 'secondary' as const }
  if (webUpdate.available) return { label: `可更新到 ${webUpdate.available.tag}`, variant: 'default' as const }
  if (webUpdate.latest) return { label: '已是最新', variant: 'secondary' as const }
  return { label: '未检查', variant: 'outline' as const }
})

const installing = ref(false)
const installError = ref('')
/** The core swaps its web folder; a reload then loads the new build. */
async function installWeb() {
  installing.value = true
  installError.value = ''
  try {
    await api.updateWeb()
    location.reload()
  } catch (reason) {
    installError.value = `更新网页失败：${reason instanceof Error ? reason.message : String(reason)}`
    installing.value = false
  }
}

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
    <p v-if="installError" role="alert" class="text-sm text-destructive">{{ installError }}</p>

    <ul class="flex flex-col divide-y rounded-lg border">
      <li class="flex items-center gap-3 px-3 py-3">
        <div class="min-w-0 flex-1">
          <p class="text-sm">网页</p>
          <p class="text-xs text-muted-foreground tabular-nums">{{ webVersion === 'dev' ? '本地构建' : shown(webVersion) }}</p>
        </div>
        <Badge :variant="webState.variant">{{ webState.label }}</Badge>
        <Button v-if="behindCore" type="button" size="sm" :disabled="installing" @click="installWeb">
          {{ installing ? '更新中…' : '更新网页' }}
        </Button>
        <Button v-else-if="!hosted && webUpdate.available" as="a" size="sm" :href="webUpdate.available.url" target="_blank" rel="noreferrer noopener">
          下载 <ExternalLink />
        </Button>
      </li>
      <li class="flex items-center gap-3 px-3 py-3">
        <div class="min-w-0 flex-1">
          <p class="text-sm">服务端</p>
          <p class="text-xs text-muted-foreground tabular-nums">{{ shown(serverVersion) }}</p>
        </div>
        <template v-if="restartPending">
          <Badge variant="default">已更新，待重启</Badge>
          <Button type="button" size="sm" :disabled="restarting" @click="emit('restart')">{{ restarting ? '重启中…' : '重启' }}</Button>
        </template>
        <Badge v-else-if="serverBehind" variant="default" title="在服务器上运行 ashira update 更新">可更新到 {{ serverBehind }}</Badge>
        <Badge v-else-if="serverVersion" variant="secondary">已是最新</Badge>
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
