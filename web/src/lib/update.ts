import { reactive } from 'vue'

/** This build's version, injected by vite.config.ts from the release tag; `dev` otherwise. */
export const webVersion = __ASHIRA_WEB_VERSION__

const latestUrl = 'https://api.github.com/repos/Night-stars-1/Ashira/releases/latest'
const cacheKey = 'ashira.webRelease'
const day = 24 * 60 * 60 * 1000

/** `notes` is the release body (Markdown), shown under 设置 → 关于. */
type Release = { tag: string; url: string; notes: string }

/** A newer web release (hint only: the web is replaced by hand), and the last check's state. */
export const webUpdate = reactive({
  latest: null as Release | null,
  checking: false,
  error: '',
  get available(): Release | null {
    return this.latest && newer(this.latest.tag, webVersion) ? this.latest : null
  },
})

/** `force` skips the one-day cache (the settings button); start-up checks stay quiet. */
export async function checkWebUpdate(force = false): Promise<void> {
  if (webUpdate.checking) return
  if (!force) {
    const cached = readCache()
    if (cached) {
      webUpdate.latest = cached
      return
    }
  }
  webUpdate.checking = true
  webUpdate.error = ''
  try {
    const response = await fetch(latestUrl, { headers: { accept: 'application/vnd.github+json' } })
    if (!response.ok) throw new Error(`GitHub 返回 ${response.status}`)
    const body = await response.json() as { tag_name?: unknown; html_url?: unknown; body?: unknown }
    if (typeof body.tag_name !== 'string' || typeof body.html_url !== 'string') throw new Error('Release 信息不完整')
    webUpdate.latest = { tag: body.tag_name, url: body.html_url, notes: typeof body.body === 'string' ? body.body : '' }
    localStorage.setItem(cacheKey, JSON.stringify({ at: Date.now(), ...webUpdate.latest }))
  } catch (reason) {
    if (force) webUpdate.error = `检查更新失败：${reason instanceof Error ? reason.message : String(reason)}`
  } finally {
    webUpdate.checking = false
  }
}

function readCache(): Release | null {
  try {
    const value = JSON.parse(localStorage.getItem(cacheKey) || 'null') as { at?: unknown; tag?: unknown; url?: unknown; notes?: unknown } | null
    if (!value || typeof value.at !== 'number' || Date.now() - value.at > day) return null
    // Caches written before notes existed miss them; refetch instead of showing none.
    if (typeof value.tag !== 'string' || typeof value.url !== 'string' || typeof value.notes !== 'string') return null
    return { tag: value.tag, url: value.url, notes: value.notes }
  } catch {
    return null
  }
}

/** `1.2.3` from `v1.2.3`; pre-release suffixes are ignored, and `dev` builds never compare. */
function parse(version: string): number[] | null {
  const parts = version.trim().replace(/^v/, '').split(/[-+]/)[0].split('.').map(Number)
  if (!Number.isInteger(parts[0])) return null
  return [0, 1, 2].map((index) => (Number.isInteger(parts[index]) ? parts[index] : 0))
}

export function newer(candidate: string, current: string): boolean {
  const a = parse(candidate)
  const b = parse(current)
  if (!a || !b) return false
  for (let index = 0; index < 3; index++) {
    if (a[index] !== b[index]) return a[index] > b[index]
  }
  return false
}

/** The release the server is behind: its own check (cached for a day on the server), else the
 *  newer release this page found. Right after a release the server's check still says none. */
export function serverBehind(serverUpdate: string | null, serverVersion: string): string | null {
  if (serverUpdate) return serverUpdate
  const latest = webUpdate.latest
  return latest && serverVersion && newer(latest.tag, serverVersion) ? latest.tag : null
}
