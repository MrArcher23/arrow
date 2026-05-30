// Contrato producido por `arrow --json` (modo reporte) y `arrow --content`.

export interface Report {
  projectsDir: string
  repoCount: number
  repos: Repo[]
}

export interface Repo {
  cwd: string
  gitBranch: string | null
  sessions: Session[]
}

export interface Session {
  sessionId: string
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
  hunks: Hunk[]
}

export interface Hunk {
  oldStart: number
  oldLines: number
  newStart: number
  newLines: number
  lines: string[]
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
}
