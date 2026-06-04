import { invoke } from '@tauri-apps/api/core'
import type { Report, FileContent, Editor, RepoWorktrees, UpdateStatus } from './types'

// Única capa que conoce el transporte. Dos modos, MISMO contrato:
//   - App de escritorio (Tauri): llama al backend Rust vía invoke() — sin HTTP.
//   - Navegador (npm run dev): pega al dev-server que ejecuta el binario `arrow`.
// Los componentes Svelte no saben en cuál están: solo consumen loadReport/loadContent.
export const inTauri = '__TAURI_INTERNALS__' in window

export async function loadReport(): Promise<Report> {
  if (inTauri) return invoke<Report>('report')

  const res = await fetch('/api/report')
  const data = await res.json()
  if (!res.ok || data.error) throw new Error(data.error ?? `report HTTP ${res.status}`)
  return data as Report
}

// Cache de contenidos ya cargados: revisitar un archivo (al navegar el árbol) es
// instantáneo, sin volver a pegar al backend. El `before` de un (archivo, sesión)
// es estable, pero `after`/`ops` cambian con ediciones nuevas; por eso el cache se
// invalida (clearContentCache) cada vez que el report cambia. Ver App.svelte.
const contentCache = new Map<string, FileContent>()

export function clearContentCache(): void {
  contentCache.clear()
}

export async function loadContent(file: string, session?: string | null): Promise<FileContent> {
  const key = (session ?? '') + ' ' + file
  const hit = contentCache.get(key)
  if (hit) return hit

  let data: FileContent
  if (inTauri) {
    data = await invoke<FileContent>('content', { file, session: session ?? null })
  } else {
    const params = new URLSearchParams({ file })
    if (session) params.set('session', session)
    const res = await fetch('/api/content?' + params.toString())
    const json = await res.json()
    if (!res.ok || json.error) throw new Error(json.error ?? `content HTTP ${res.status}`)
    data = json as FileContent
  }
  contentCache.set(key, data)
  return data
}

// "Open in editor" — Tauri-only: opening a local editor only makes sense from
// the native app, so these no-op / return empty in the browser dev mode.
export async function detectEditors(): Promise<Editor[]> {
  if (!inTauri) return []
  return invoke<Editor[]>('editors')
}

export async function openInEditor(
  editorId: string,
  file: string,
  line: number,
  col = 1
): Promise<void> {
  if (!inTauri) throw new Error('Open in editor is only available in the desktop app')
  await invoke('open_in_editor', { editorId, file, line, col })
}

// "Worktrees" inventory — Tauri-only (inspecting/cleaning git worktrees only
// makes sense from the native app). Returns [] in the browser dev mode.
export async function loadWorktrees(repoRoots: string[]): Promise<RepoWorktrees[]> {
  if (!inTauri) return []
  return invoke<RepoWorktrees[]>('worktrees', { repoRoots })
}

// On-demand disk sizes (approx apparent bytes) per worktree path. Tauri-only.
export async function calcWorktreeSizes(paths: string[]): Promise<Record<string, number>> {
  if (!inTauri) return {}
  return invoke<Record<string, number>>('worktree_sizes', { paths })
}

// Check GitHub for a newer release (the version badge's "update available" hint).
// Tauri-only — in the browser dev build there's no app to update, so it returns a
// neutral "no check" result instead of hitting the network.
export async function checkUpdate(): Promise<UpdateStatus | null> {
  if (!inTauri) return null
  return invoke<UpdateStatus>('check_update')
}

// Open an https URL in the default browser (the "Open release" action). Tauri-only.
export async function openUrl(url: string): Promise<void> {
  if (!inTauri) {
    window.open(url, '_blank', 'noopener')
    return
  }
  await invoke('open_url', { url })
}
