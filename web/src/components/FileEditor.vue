<script setup lang="ts">
import { EditorState, Prec, type Extension } from '@codemirror/state'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView, keymap } from '@codemirror/view'
import { basicSetup } from 'codemirror'
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { FolderTree } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import FileTree from '@/components/FileTree.vue'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { ScrollArea } from '@/components/ui/scroll-area'
import { api } from '@/lib/api'

const props = defineProps<{ sessionId: string; path: string }>()
const emit = defineEmits<{ close: []; open: [path: string]; files: [] }>()
const host = ref<HTMLElement | null>(null)
const view = shallowRef<EditorView | null>(null)
const saved = ref('')
const current = ref('')
const error = ref('')
const loading = ref(true)
const writing = ref(false)
const kind = ref<'text' | 'large' | 'binary' | null>(null)
const offset = ref(0)
const previousOffsets = ref<number[]>([])
const nextOffset = ref<number | null>(null)
const dirty = computed(() => kind.value === 'text' && current.value !== saved.value)
const notice = computed(() => error.value || (kind.value === 'binary'
  ? '此文件不是文本，无法预览。'
  : loading.value && kind.value === null ? '正在加载文件…' : ''))
let request = 0

async function language(path: string): Promise<Extension> {
  const ext = path.split(/[/\\]/).pop()?.split('.').pop()?.toLowerCase() ?? ''
  if (ext === 'ts' || ext === 'tsx' || ext === 'mts' || ext === 'cts') return (await import('@codemirror/lang-javascript')).javascript({ typescript: true, jsx: ext === 'tsx' })
  if (ext === 'js' || ext === 'jsx' || ext === 'mjs' || ext === 'cjs') return (await import('@codemirror/lang-javascript')).javascript({ jsx: ext === 'jsx' })
  if (ext === 'json') return (await import('@codemirror/lang-json')).json()
  if (ext === 'css' || ext === 'scss') return (await import('@codemirror/lang-css')).css()
  if (ext === 'html' || ext === 'vue' || ext === 'svelte') return (await import('@codemirror/lang-html')).html()
  if (ext === 'md' || ext === 'mdx') return (await import('@codemirror/lang-markdown')).markdown()
  if (ext === 'py') return (await import('@codemirror/lang-python')).python()
  if (ext === 'rs') return (await import('@codemirror/lang-rust')).rust()
  if (ext === 'yml' || ext === 'yaml') return (await import('@codemirror/lang-yaml')).yaml()
  return []
}

function clearView() {
  view.value?.destroy()
  view.value = null
}

function mount(text: string, syntax: Extension, editable: boolean) {
  clearView()
  if (!host.value) return
  const dark = document.documentElement.classList.contains('dark')
  view.value = new EditorView({
    parent: host.value,
    state: EditorState.create({
      doc: text,
      extensions: [
        basicSetup,
        syntax,
        EditorView.lineWrapping,
        EditorView.theme({
          '&': { position: 'absolute', inset: '0', height: '100%' },
          '.cm-scroller': {
            overflow: 'auto',
            fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
          },
          '&.cm-focused': { outline: 'none' },
        }),
        dark ? oneDark : [],
        editable ? [
          Prec.highest(keymap.of([{ key: 'Mod-s', run: () => { void save(); return true } }])),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) current.value = update.state.doc.toString()
          }),
        ] : [
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          EditorView.contentAttributes.of({ tabindex: '0' }),
        ],
      ],
    }),
  })
}

async function load(target = 0, direction?: 'previous' | 'next') {
  const { sessionId, path } = props
  const token = ++request
  if (!direction) {
    clearView()
    kind.value = null
    offset.value = 0
    previousOffsets.value = []
    nextOffset.value = null
    saved.value = ''
    current.value = ''
    writing.value = false
  }
  loading.value = true
  error.value = ''
  try {
    const file = await api.readFile(sessionId, path, target)
    if (token !== request) return
    if (file.kind !== 'binary') {
      const syntax = await language(path)
      if (token !== request) return
      mount(file.text, syntax, file.kind === 'text')
    } else {
      clearView()
    }
    kind.value = file.kind
    if (file.kind === 'text') saved.value = current.value = file.text
    if (file.kind === 'large') {
      if (direction === 'previous') previousOffsets.value.pop()
      if (direction === 'next') previousOffsets.value.push(offset.value)
      offset.value = target
      nextOffset.value = file.nextOffset
    } else {
      offset.value = 0
      previousOffsets.value = []
      nextOffset.value = null
    }
  } catch (reason) {
    if (token === request) error.value = reason instanceof Error && reason.message ? reason.message : '无法读取文件'
  } finally {
    if (token === request) loading.value = false
  }
}

function movePage(direction: 'previous' | 'next') {
  if (loading.value || kind.value !== 'large') return
  const target = direction === 'previous'
    ? previousOffsets.value[previousOffsets.value.length - 1]
    : nextOffset.value
  if (target != null) void load(target, direction)
}

async function save() {
  if (!dirty.value || loading.value || writing.value || !view.value) return
  const token = request
  writing.value = true
  error.value = ''
  const text = view.value.state.doc.toString()
  try {
    await api.writeFile(props.sessionId, props.path, text)
    if (token === request) saved.value = text
  } catch (reason) {
    if (token === request) error.value = reason instanceof Error && reason.message ? reason.message : '保存失败'
  } finally {
    if (token === request) writing.value = false
  }
}

function choose(path: string) {
  if (path === props.path) return
  if (dirty.value) {
    error.value = '先保存当前文件'
    return
  }
  emit('open', path)
}

function close() {
  if (dirty.value && !window.confirm('当前文件有未保存的修改，确定关闭？')) return
  emit('close')
}

/** Mobile: leave the editor for the full files drawer. */
function showFiles() {
  if (dirty.value && !window.confirm('当前文件有未保存的修改，确定关闭？')) return
  emit('files')
}

onMounted(() => { void load() })
watch([() => props.sessionId, () => props.path], () => { void load() })
onBeforeUnmount(() => {
  request++
  clearView()
})
</script>

<template>
  <Dialog :open="true" @update:open="(open) => { if (!open) close() }">
    <DialogContent class="flex h-[90dvh] w-[min(1360px,calc(100%-2rem))] max-w-none flex-col gap-0 overflow-hidden p-0 sm:h-[85dvh] sm:max-w-none">
      <DialogHeader class="shrink-0 px-3 py-3 pr-20 sm:px-6 sm:py-4 sm:pr-12">
        <DialogTitle class="truncate">{{ path }}</DialogTitle>
        <DialogDescription class="sr-only">查看或编辑当前文件</DialogDescription>
      </DialogHeader>
      <!-- Sits left of the dialog's close button. -->
      <Button type="button" size="icon-sm" variant="ghost" class="absolute top-2 right-10 sm:hidden" aria-label="关闭编辑并打开文件列表" title="文件列表" @click="showFiles">
        <FolderTree />
      </Button>
      <div class="flex min-h-0 flex-1 flex-col border-y sm:flex-row">
        <aside class="hidden w-72 shrink-0 flex-col border-r bg-sidebar sm:flex">
          <ScrollArea class="min-h-0 flex-1">
            <FileTree :session-id="sessionId" path="" :depth="0" :active="path" @open="choose" />
          </ScrollArea>
        </aside>
        <div class="flex min-h-0 min-w-0 flex-1 flex-col">
          <p v-if="notice" role="status" class="px-4 py-3 text-sm break-words text-muted-foreground">{{ notice }}</p>
          <div ref="host" class="relative min-h-0 flex-1 overflow-hidden" />
        </div>
      </div>
      <DialogFooter v-if="kind === 'text' || kind === 'large'" class="mx-0 mb-0 shrink-0 flex-row items-center justify-end rounded-none p-3 sm:p-4">
        <template v-if="kind === 'large'">
          <span class="mr-auto text-sm text-muted-foreground">{{ loading ? '正在加载…' : `只读预览 · 第 ${previousOffsets.length + 1} 段` }}</span>
          <Button type="button" variant="outline" size="sm" :disabled="loading || !previousOffsets.length" @click="movePage('previous')">上一段</Button>
          <Button type="button" variant="outline" size="sm" :disabled="loading || nextOffset === null" @click="movePage('next')">下一段</Button>
        </template>
        <template v-else>
          <span v-if="dirty" class="mr-auto text-sm text-muted-foreground">未保存</span>
          <Button type="button" :disabled="!dirty || loading || writing" @click="save">{{ writing ? '保存中…' : '保存' }}</Button>
        </template>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
