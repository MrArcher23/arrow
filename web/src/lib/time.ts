// Utilidades de tiempo: "en vivo", foco de la sesión activa, tiempo relativo y buckets
// de fecha para el historial. (Los strings devueltos son texto de UI -> en inglés.)
import type { Repo } from './types'

const MIN = 60_000
const HOUR = 3_600_000
const DAY = 86_400_000
const LIVE_WINDOW = 20 * MIN
// Ventana de "ráfaga": un repo entra al foco si su última actividad cae dentro de estos
// minutos respecto a la de la sesión activa (la más reciente). Anclada al DATO, no al
// reloj, así el foco no envejece solo entre refrescos.
const BURST_WINDOW = 10 * MIN

// `now` is a parameter (default = wall clock) so callers can pass a reactive
// clock tick: that way the "Nm ago" labels and the green dot keep aging and
// expire after LIVE_WINDOW even when the report itself hasn't changed (relative
// times otherwise freeze, since they only recompute on re-render). See the
// 30s tick in App.svelte.
export function isLive(iso?: string | null, now: number = Date.now()): boolean {
  if (!iso) return false
  const t = Date.parse(iso)
  if (Number.isNaN(t)) return false
  return now - t < LIVE_WINDOW
}

// Repos en foco: el de la sesión activa (actividad globalmente más reciente, = repos[0]
// por el orden que ya garantiza el parser) + cualquier repo tocado dentro de BURST_WINDOW
// de esa actividad (trabajo en la misma ráfaga). Siempre devuelve ≥1 repo (el más reciente)
// como fallback de navegación. Devuelve REFERENCIAS del array original (no copias) para que
// el complemento `otherRepos` por identidad de objeto siga funcionando.
export function focusRepos(repos: Repo[]): Repo[] {
  const top = repos[0]?.sessions[0]?.lastActivity
  const t0 = top ? Date.parse(top) : NaN
  if (Number.isNaN(t0)) return repos.slice(0, 1)
  return repos.filter((r) => {
    const la = r.sessions[0]?.lastActivity
    const t = la ? Date.parse(la) : NaN
    return !Number.isNaN(t) && t0 - t <= BURST_WINDOW
  })
}

export function relative(iso?: string | null, now: number = Date.now()): string {
  if (!iso) return ''
  const t = Date.parse(iso)
  if (Number.isNaN(t)) return ''
  const d = now - t
  if (d < MIN) return 'now'
  if (d < HOUR) return `${Math.floor(d / MIN)}m ago`
  if (d < DAY) return `${Math.floor(d / HOUR)}h ago`
  const days = Math.floor(d / DAY)
  if (days < 30) return `${days}d ago`
  return new Date(t).toLocaleDateString('en', { day: 'numeric', month: 'short' })
}

export const BUCKET_ORDER = ['Today', 'Yesterday', 'This week', 'Older', 'No date']

export function dateBucket(iso?: string | null): string {
  if (!iso) return 'No date'
  const t = Date.parse(iso)
  if (Number.isNaN(t)) return 'No date'
  const now = new Date()
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  if (t >= startToday) return 'Today'
  if (t >= startToday - DAY) return 'Yesterday'
  if (t >= startToday - 6 * DAY) return 'This week'
  return 'Older'
}
