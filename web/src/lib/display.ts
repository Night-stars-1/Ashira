import { useLocalStorage } from '@vueuse/core'

/** 设置 → 通用 → 显示: long tool-output lines wrap; off keeps them whole and scrolls sideways. */
export const wrapToolOutput = useLocalStorage('ashira.wrapToolOutput', true)

/** 设置 → 通用 → 显示: tool cards show only their header until opened. The running reply's
 *  newest tool, a failed one and the todo board stay open. */
export const foldToolOutput = useLocalStorage('ashira.foldToolOutput', false)

/** 设置 → 通用 → 终端: the terminal tab wraps at its width; off gives it `NO_WRAP_COLS` columns and a sideways scroll. */
export const wrapTerminal = useLocalStorage('ashira.wrapTerminal', true)
export const NO_WRAP_COLS = 240
