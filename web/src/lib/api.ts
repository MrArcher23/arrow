import type { Report, FileContent } from './types'

// Única capa que conoce el transporte. En Fase 2 (Tauri) se reemplaza por invoke().

export async function loadReport(): Promise<Report> {
  const res = await fetch('/api/report')
  const data = await res.json()
  if (!res.ok || data.error) throw new Error(data.error ?? `report HTTP ${res.status}`)
  return data as Report
}

export async function loadContent(file: string, session?: string | null): Promise<FileContent> {
  const params = new URLSearchParams({ file })
  if (session) params.set('session', session)
  const res = await fetch('/api/content?' + params.toString())
  const data = await res.json()
  if (!res.ok || data.error) throw new Error(data.error ?? `content HTTP ${res.status}`)
  return data as FileContent
}
