// Utilidades de tiempo: "en vivo", tiempo relativo y buckets de fecha para el historial.

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
  if (d < MIN) return 'ahora'
  if (d < HOUR) return `hace ${Math.floor(d / MIN)} min`
  if (d < DAY) return `hace ${Math.floor(d / HOUR)} h`
  const days = Math.floor(d / DAY)
  if (days < 30) return `hace ${days} d`
  return new Date(t).toLocaleDateString('es', { day: 'numeric', month: 'short' })
}

export const BUCKET_ORDER = ['Hoy', 'Ayer', 'Esta semana', 'Más antiguo', 'Sin fecha']

export function dateBucket(iso?: string | null): string {
  if (!iso) return 'Sin fecha'
  const t = Date.parse(iso)
  if (Number.isNaN(t)) return 'Sin fecha'
  const now = new Date()
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  if (t >= startToday) return 'Hoy'
  if (t >= startToday - DAY) return 'Ayer'
  if (t >= startToday - 6 * DAY) return 'Esta semana'
  return 'Más antiguo'
}
