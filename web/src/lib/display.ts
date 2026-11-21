import { useLocalStorage } from '@vueuse/core'

/** 设置 → 显示: long tool-output lines wrap; off keeps them whole and scrolls sideways. */
export const wrapToolOutput = useLocalStorage('ashira.wrapToolOutput', true)
