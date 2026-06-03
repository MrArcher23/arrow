// Contrato producido por `arrow --json` (modo reporte) y `arrow --content`.

export interface Report {
  projectsDir: string
  repoCount: number
  repos: Repo[]
}

export interface Repo {
  cwd: string
  gitBranch: string | null
  sessions: Session[] // ordenadas por última actividad (más reciente primero)
}

export interface Session {
  sessionId: string
  title: string | null // ai-title generado por Claude
  lastPrompt: string | null
  firstActivity: string | null // ISO 8601
  lastActivity: string | null // ISO 8601
  fileCount: number
  files: FileChange[]
}

export interface FileChange {
  path: string
  writeType: string | null // "create" | "update" | null
  userModified: boolean
  ops: number
  added: number
  removed: number
  lastTouched: string | null // ISO 8601 — última edición de este archivo; los archivos vienen ordenados por esto (más reciente primero)
}

export interface FileContent {
  file: string
  session: string | null
  before: string
  after: string
  beforeAvailable: boolean
  afterAvailable: boolean
  userModified: boolean
  ops: number
  firstChangedLine: number | null // 1-based earliest changed line (after side); null if none
}

// An editor detected on the user's machine (for "Open in editor"). Tauri-only.
export interface Editor {
  id: string
  name: string
}
