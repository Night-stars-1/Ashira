<script setup lang="ts">
import { ref } from 'vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { connection, probeConnection, setConnection } from '@/lib/api'

/** API address and token for this browser; saving checks them first, then reloads to reconnect. */
const current = connection()
const server = ref(current.server)
const token = ref(current.token)
const error = ref('')
const checking = ref(false)

async function save() {
  checking.value = true
  error.value = await probeConnection(server.value, token.value)
  checking.value = false
  if (error.value) return
  setConnection(server.value, token.value)
  location.reload()
}
</script>

<template>
  <form class="flex flex-col gap-3" @submit.prevent="save">
    <label class="flex flex-col gap-1 text-xs text-muted-foreground">
      API 地址
      <Input v-model="server" placeholder="留空则用当前网页地址" autocomplete="off" spellcheck="false" />
    </label>
    <label class="flex flex-col gap-1 text-xs text-muted-foreground">
      Token
      <Input v-model="token" type="password" required placeholder="与服务端 ASHIRA_API_TOKEN 相同" autocomplete="off" spellcheck="false" />
    </label>
    <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>
    <div class="flex justify-end">
      <Button type="submit" :disabled="checking">{{ checking ? '连接中...' : '保存并重新连接' }}</Button>
    </div>
  </form>
</template>
