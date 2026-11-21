import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  // Release builds get the tag (CI's VERSION, e.g. `v0.2.4`); local builds are `dev` and never see an update.
  define: {
    __ASHIRA_WEB_VERSION__: JSON.stringify(process.env.VERSION?.replace(/^v/, '') || 'dev'),
  },
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    proxy: {
      '/v1': { target: 'http://127.0.0.1:8787', ws: true },
    },
  },
})
