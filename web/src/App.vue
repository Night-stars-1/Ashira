<script setup lang="ts">
import { useLocalStorage, useMediaQuery } from '@vueuse/core'
import { defineAsyncComponent, ref, watch } from 'vue'
import ChatView from '@/components/ChatView.vue'
import FilesSidebar from '@/components/FilesSidebar.vue'
import ProjectPicker from '@/components/ProjectPicker.vue'
import ModelPicker from '@/components/ModelPicker.vue'
import SessionSidebar from '@/components/SessionSidebar.vue'
import SettingsView, { type SettingsTab } from '@/components/SettingsView.vue'
import UsageDialog from '@/components/UsageDialog.vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { api } from '@/lib/api'
import ConnectionForm from '@/components/ConnectionForm.vue'
import { useAshira } from '@/lib/session'
import { checkWebUpdate } from '@/lib/update'
import Toasts from '@/components/Toasts.vue'
import FilesSidebarSkeleton from '@/components/FilesSidebarSkeleton.vue'

const FileEditor = defineAsyncComponent(() => import('@/components/FileEditor.vue'))
const modelsOpen = ref(false)
const pickerOpen = ref(false)
const settingsOpen = ref(false)
const settingsTab = ref<SettingsTab>('models')
const usageOpen = ref(false)
const filesOpen = useLocalStorage('ashira.filesOpen', true)
const sessionsOpen = useLocalStorage('ashira.sessionsOpen', true)
const desktop = useMediaQuery('(min-width: 64rem)')
const drawer = ref<'sessions' | 'files' | null>(null)
const filePath = ref('')
const aliasPath = ref('')
const aliasName = ref('')
const aliasError = ref('')
const {
  status, server, refreshServer, restarting, restartServer, updating, updateProgress, updateServer, tree, archive, blocks, draft, model, modelTitle, fast, connected, loading, naming, confirm, question,
  busy, hasReplyText, shownId, shownCwd, sessionName, needsConnection, runningIds, askingIds, send, openSession, decide, answer, interrupt, refresh, updateSettings, renameSession, setArchived, setFolded,
} = useAshira()

// Hint only: a newer web release shows a download link in the sidebar.
void checkWebUpdate()

function chooseModel(selector: string) {
  return updateSettings((id) => api.model(id, selector))
}

function showModels() {
  modelsOpen.value = true
}

function setFast(enabled: boolean) {
  void updateSettings((id) => api.fast(id, enabled))
}

async function startSession(cwd: string) {
  pickerOpen.value = false
  await openSession(true, undefined, cwd)
}

watch(shownId, () => { filePath.value = '' })
watch(desktop, () => { drawer.value = null })

function showProjectPicker() {
  drawer.value = null
  pickerOpen.value = true
}

/** Opens settings, on `tab` when given (the sidebar's update hint opens 关于). */
function showSettings(tab?: SettingsTab) {
  drawer.value = null
  if (tab) settingsTab.value = tab
  settingsOpen.value = true
  void refreshServer().catch(() => {})
}

function selectSession(id: string) {
  drawer.value = null
  if (shownId.value !== id) void openSession(false, id)
}

function toggleFiles() {
  if (desktop.value) filesOpen.value = !filesOpen.value
  else if (shownId.value) drawer.value = 'files'
}

function openFile(path: string) {
  drawer.value = null
  filePath.value = path
}

function openAlias(path: string, name: string) {
  drawer.value = null
  aliasPath.value = path
  aliasName.value = name
  aliasError.value = ''
}

async function saveAlias() {
  aliasError.value = ''
  try {
    await api.alias(aliasPath.value, aliasName.value.trim())
    aliasPath.value = ''
    await refresh()
  } catch (reason) {
    aliasError.value = reason instanceof Error ? reason.message : '别名保存失败'
  }
}

// Rename dialog: an empty title restores the first-message title.
const renaming = ref('')
const renameTitle = ref('')

function openRename(id: string, title: string) {
  drawer.value = null
  renaming.value = id
  renameTitle.value = title
}

function saveRename() {
  const id = renaming.value
  renaming.value = ''
  void renameSession(id, renameTitle.value.trim())
}
</script>

<template>
  <div class="flex h-dvh min-w-0 overflow-hidden bg-background text-foreground">
    <!-- Folds away to the left; the chat header brings it back. -->
    <div v-if="desktop" class="flex shrink-0 overflow-hidden border-sidebar-border transition-[width] duration-200 ease-out motion-reduce:transition-none" :class="sessionsOpen ? 'w-64 border-r' : 'w-0'">
      <div class="flex w-64 shrink-0">
        <SessionSidebar :tree="tree" :active-id="shownId" :running="runningIds" :asking="askingIds" :connected="connected" :update="server.update" :hosted="server.web !== null" :restart-pending="server.restartPending" :restarting="restarting" collapsible
          @resume="selectSession" @alias="openAlias" @rename="openRename" @archive="(kind, key) => setArchived(kind, key, true)" @fold="setFolded" @create="showProjectPicker" @settings="showSettings()" @about="showSettings('about')" @collapse="sessionsOpen = false" @restart="restartServer" />
      </div>
    </div>

    <Dialog :open="drawer !== null" @update:open="(open) => { if (!open) drawer = null }">
      <DialogContent
        class="top-0 flex h-dvh w-[min(20rem,calc(100%-2rem))] max-w-none translate-x-0 translate-y-0 flex-col gap-0 overflow-hidden rounded-none bg-sidebar p-0 text-sidebar-foreground sm:max-w-none"
        :class="drawer === 'sessions' ? 'left-0 border-r' : 'right-0 left-auto border-l'"
        :aria-describedby="undefined"
      >
        <DialogTitle class="sr-only">{{ drawer === 'sessions' ? '会话列表' : '项目文件' }}</DialogTitle>
        <SessionSidebar v-if="drawer === 'sessions'" :tree="tree" :active-id="shownId" :running="runningIds" :asking="askingIds" :connected="connected" :update="server.update" :hosted="server.web !== null" :restart-pending="server.restartPending" :restarting="restarting"
          @resume="selectSession" @alias="openAlias" @rename="openRename" @archive="(kind, key) => setArchived(kind, key, true)" @fold="setFolded" @create="showProjectPicker" @settings="showSettings()" @about="showSettings('about')" @restart="restartServer" />
        <FilesSidebar v-else-if="drawer === 'files' && shownId" :key="shownId" class="w-full flex-1 border-0"
          :session-id="shownId" :cwd="shownCwd" :active="filePath" @open="openFile" />
      </DialogContent>
    </Dialog>

    <ChatView
      :key="shownId"
      :session-id="shownId"
      :cwd="shownCwd"
      :session-name="sessionName"
      :naming="naming"
      :blocks="blocks"
      :draft="draft"
      :model="modelTitle || model"
      :busy="busy"
      :has-reply-text="hasReplyText"
      :send="send"
      :connected="connected"
      :loading="loading"
      :files-open="desktop && filesOpen"
      :sessions-collapsed="desktop && !sessionsOpen"
      :context-tokens="status?.context ?? 0"
      :context-window="status?.window ?? null"
      :question="question"
      @update:draft="draft = $event"
      @interrupt="interrupt"
      @answer="answer"
      @open-models="showModels"
      @open-usage="usageOpen = true"
      @open-sessions="drawer = 'sessions'"
      @expand-sessions="sessionsOpen = true"
      @toggle-files="toggleFiles"
    />

    <FileEditor v-if="filePath && shownId" :key="shownId" :session-id="shownId" :path="filePath" @close="filePath = ''" @open="filePath = $event" @files="filePath = ''; drawer = 'files'" />
    <FilesSidebar v-if="desktop && filesOpen && shownId" :key="shownId" :session-id="shownId" :cwd="shownCwd" :active="filePath" @open="openFile" />
    <!-- A session is opening: hold the column instead of collapsing it. -->
    <FilesSidebarSkeleton v-else-if="desktop && filesOpen && loading" />

    <SettingsView
      v-model:open="settingsOpen"
      v-model:tab="settingsTab"
      :archive="archive"
      :server-version="server.version"
      :server-update="server.update"
      :hosted="server.web !== null"
      :restart-pending="server.restartPending"
      :restarting="restarting"
      :updating="updating"
      :update-progress="updateProgress"
      @restart="restartServer"
      @update="updateServer"
      @restore="(kind, key) => setArchived(kind, key, false)"
    />
    <UsageDialog v-model:open="usageOpen" :status="status" :blocks="blocks" />

    <ProjectPicker v-model:open="pickerOpen" :start-path="shownCwd" @start="startSession" />
    <ModelPicker v-model:open="modelsOpen" :current="model" :fast="fast" @select="chooseModel" @fast="setFast" />

    <Dialog :open="confirm !== null" @update:open="(open) => { if (!open) decide('no') }">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>确认 {{ confirm?.name }}</DialogTitle>
          <DialogDescription>{{ confirm?.reason }}</DialogDescription>
        </DialogHeader>
        <pre class="max-h-48 overflow-auto rounded-md bg-muted p-3 text-xs whitespace-pre-wrap">{{ confirm?.command }}</pre>
        <DialogFooter>
          <Button variant="outline" @click="decide('no')">拒绝</Button>
          <Button variant="secondary" @click="decide('always')">{{ confirm?.always || '始终允许' }}</Button>
          <Button @click="decide('yes')">允许</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="aliasPath !== ''" @update:open="(open) => { if (!open) aliasPath = '' }">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>设置别名</DialogTitle>
          <DialogDescription class="truncate">{{ aliasPath }}</DialogDescription>
        </DialogHeader>
        <Input v-model="aliasName" placeholder="留空则恢复目录名" @keydown.enter.prevent="saveAlias" />
        <p v-if="aliasError" class="text-sm text-destructive">{{ aliasError }}</p>
        <DialogFooter>
          <Button variant="outline" @click="aliasPath = ''">取消</Button>
          <Button @click="saveAlias">保存</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="renaming !== ''" @update:open="(open) => { if (!open) renaming = '' }">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>重命名会话</DialogTitle>
          <DialogDescription>留空则恢复为 AI 生成的标题。</DialogDescription>
        </DialogHeader>
        <Input v-model="renameTitle" maxlength="120" aria-label="会话标题" placeholder="会话标题" @keydown.enter.prevent="saveRename" />
        <DialogFooter>
          <Button variant="outline" @click="renaming = ''">取消</Button>
          <Button @click="saveRename">保存</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="needsConnection">
      <DialogContent :show-close-button="false" :aria-describedby="undefined" @interact-outside.prevent @escape-key-down.prevent>
        <DialogHeader>
          <DialogTitle>连接服务</DialogTitle>
        </DialogHeader>
        <ConnectionForm />
      </DialogContent>
    </Dialog>
    <Toasts />
  </div>
</template>
