<script setup lang="ts">
import { Check, Copy } from '@lucide/vue'
import { ref } from 'vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { Separator } from '@/components/ui/separator'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

export type MdNode = {
  type: string
  text?: string
  depth?: number
  lang?: string
  href?: string
  ordered?: boolean
  align?: 'left' | 'center' | 'right' | null
  tokens?: MdNode[]
  items?: MdNode[]
  header?: MdNode[]
  rows?: MdNode[][]
}

defineProps<{ tokens: MdNode[] }>()
const copied = ref<number | null>(null)

async function copy(index: number, text: string) {
  try {
    await navigator.clipboard.writeText(text)
    copied.value = index
    setTimeout(() => { if (copied.value === index) copied.value = null }, 1500)
  } catch { /* clipboard needs a secure context or permission */ }
}

function alignClass(align: MdNode['align']): string {
  if (align === 'center') return 'text-center'
  if (align === 'right') return 'text-right'
  return 'text-left'
}

function headingClass(depth: number): string {
  if (depth <= 1) return 'text-xl'
  if (depth === 2) return 'text-lg'
  if (depth === 3) return 'text-base'
  return 'text-sm'
}

function safeHref(href: string | undefined): string {
  const value = href?.trim() ?? ''
  return /^(https?:|mailto:)/i.test(value) ? value : ''
}
</script>

<template>
  <template v-for="(token, index) in tokens" :key="index">
    <component
      :is="`h${Math.min(token.depth || 1, 6)}`"
      v-if="token.type === 'heading'"
      class="font-semibold leading-snug"
      :class="headingClass(token.depth || 1)"
    >
      <MarkdownNodes :tokens="token.tokens || []" />
    </component>

    <p v-else-if="token.type === 'paragraph'">
      <MarkdownNodes :tokens="token.tokens || []" />
    </p>

    <component
      :is="token.ordered ? 'ol' : 'ul'"
      v-else-if="token.type === 'list'"
      class="flex flex-col gap-1 pl-5"
      :class="token.ordered ? 'list-decimal' : 'list-disc'"
    >
      <li v-for="(item, itemIndex) in token.items || []" :key="itemIndex">
        <MarkdownNodes :tokens="item.tokens || []" />
      </li>
    </component>

    <Card v-else-if="token.type === 'blockquote'" size="sm" class="border-l-2 py-2">
      <CardContent class="text-muted-foreground">
        <MarkdownNodes :tokens="token.tokens || []" />
      </CardContent>
    </Card>

    <Card v-else-if="token.type === 'code'" size="sm" class="gap-2 py-3">
      <CardHeader class="flex items-center justify-between py-0">
        <Badge v-if="token.lang" variant="secondary">{{ token.lang }}</Badge>
        <span v-else />
        <Button type="button" size="icon-xs" variant="ghost" :aria-label="copied === index ? '已复制' : '复制代码'" :title="copied === index ? '已复制' : '复制代码'" @click="copy(index, token.text || '')">
          <Check v-if="copied === index" aria-hidden="true" />
          <Copy v-else aria-hidden="true" />
        </Button>
      </CardHeader>
      <CardContent>
        <pre class="overflow-x-auto font-mono text-xs leading-relaxed whitespace-pre [overflow-wrap:normal]">{{ token.text }}</pre>
      </CardContent>
    </Card>

    <Table v-else-if="token.type === 'table'">
      <TableHeader>
        <TableRow>
          <TableHead v-for="(cell, cellIndex) in token.header || []" :key="cellIndex" class="font-semibold" :class="alignClass(cell.align)">
            <MarkdownNodes :tokens="cell.tokens || []" />
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableRow v-for="(row, rowIndex) in token.rows || []" :key="rowIndex">
          <TableCell v-for="(cell, cellIndex) in row" :key="cellIndex" :class="alignClass(cell.align)">
            <MarkdownNodes :tokens="cell.tokens || []" />
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>

    <Separator v-else-if="token.type === 'hr'" />

    <a
      v-else-if="token.type === 'link' && safeHref(token.href)"
      class="text-sky-600 underline underline-offset-2 hover:text-sky-500 dark:text-sky-400 dark:hover:text-sky-300"
      :href="safeHref(token.href)"
      target="_blank"
      rel="noreferrer noopener"
    >
      <MarkdownNodes :tokens="token.tokens || []" />
    </a>
    <MarkdownNodes v-else-if="token.type === 'link'" :tokens="token.tokens || []" />

    <strong v-else-if="token.type === 'strong'" class="font-semibold">
      <MarkdownNodes :tokens="token.tokens || []" />
    </strong>
    <em v-else-if="token.type === 'em'">
      <MarkdownNodes :tokens="token.tokens || []" />
    </em>
    <del v-else-if="token.type === 'del'">
      <MarkdownNodes :tokens="token.tokens || []" />
    </del>
    <code v-else-if="token.type === 'codespan'" class="rounded-md bg-muted px-1 py-0.5 font-mono text-[0.85em]">{{ token.text }}</code>
    <br v-else-if="token.type === 'br'">
    <MarkdownNodes v-else-if="token.tokens?.length" :tokens="token.tokens" />
    <template v-else-if="token.text">{{ token.text }}</template>
  </template>
</template>
