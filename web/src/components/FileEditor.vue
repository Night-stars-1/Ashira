<script setup lang="ts">
import { EditorState, Prec, type Extension } from '@codemirror/state'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView, keymap } from '@codemirror/view'
import { basicSetup } from 'codemirror'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
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
const loading = ref(false)
const writing = ref(false)
const dirty = computed(() => current.value !== saved.value)

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

function mount(text: string, syntax: Extension) {
  view.value?.destroy()
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
        Prec.highest(keymap.of([{ key: 'Mod-s', run: () => { void save(); return true } }])),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) current.value = update.state.doc.toString()
        }),
      ],
    }),
  })
}

async function load(path: string) {
  loading.value = true
  error.value = ''
  await nextTick()
  try {
    const [file, syntax] = await Promise.all([api.readFile(props.sessionId, path), language(path)])
    if (path !== props.path) return
    saved.value = file.text
    current.value = file.text
    mount(file.text, syntax)
  } catch (reason) {
    if (path !== props.path) return
    saved.value = ''
    current.value = ''
    view.value?.destroy()
    view.value = null
    error.value = reason instanceof Error ? reason.message : '无法读取文件'
  } finally {
    loading.value = false
  }
}

async function save() {
  if (!dirty.value || writing.value || !view.value) return
  writing.value = true
  error.value = ''
  const text = view.value.state.doc.toString()
  try {
    await api.writeFile(props.sessionId, props.path, text)
    saved.value = text
    current.value = text
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : '保存失败'
  } finally {
    writing.value = false
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

onMounted(() => { void load(props.path) })
watch(() => props.path, (path) => { void load(path) })
onBeforeUnmount(() => view.value?.destroy())
</script>

<template>
  <Dialog :open="true" @update:open="(open) => { if (!open) close() }">
    <DialogContent class="flex h-[90dvh] w-[min(1360px,calc(100%-2rem))] max-w-none flex-col gap-0 overflow-hidden p-0 sm:h-[85dvh] sm:max-w-none">
      <DialogHeader class="shrink-0 px-3 py-3 pr-20 sm:px-6 sm:py-4 sm:pr-12">
        <DialogTitle class="truncate">{{ path }}</DialogTitle>
        <DialogDescription :class="error ? 'text-destructive' : 'sr-only'">{{ error || '编辑当前文件' }}</DialogDescription>
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
        <div ref="host" class="relative min-h-0 min-w-0 flex-1 overflow-hidden" />
      </div>
      <DialogFooter class="mx-0 mb-0 shrink-0 flex-row items-center justify-end rounded-none p-3 sm:p-4">
        <span v-if="dirty" class="mr-auto text-sm text-muted-foreground">未保存</span>
        <Button type="button" :disabled="!dirty || loading || writing" @click="save">保存</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
