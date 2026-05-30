// Utilidades de tiempo: "en vivo", tiempo relativo y buckets de fecha para el historial.
// (Los strings devueltos son texto de UI -> en inglés.)

const MIN = 60_000
const HOUR = 3_600_000
const DAY = 86_400_000
const LIVE_WINDOW = 20 * MIN

export function isLive(iso?: string | null): boolean {
  if (!iso) return false
  const t = Date.parse(iso)
  if (Number.isNaN(t)) return false
  return Date.now() - t < LIVE_WINDOW
}

export function relative(iso?: string | null): string {
  if (!iso) return ''
  const t = Date.parse(iso)
  if (Number.isNaN(t)) return ''
  const d = Date.now() - t
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
