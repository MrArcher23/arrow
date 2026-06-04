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

// A git worktree of a repo, for the read-only "Worktrees" inventory. Tauri-only.
export interface Worktree {
  path: string
  branch: string | null // null = detached HEAD
  head: string // abbreviated HEAD commit
  isMain: boolean // the repo's primary worktree (never removable)
  isMerged: boolean // proven merged: branch tip is an ancestor of the default branch
  ahead: number | null // commits on the branch not in the default branch; null if unknown
  dirty: boolean // uncommitted/untracked changes in the working tree
  locked: boolean
  prunable: boolean
}

export interface RepoWorktrees {
  repoRoot: string
  defaultBranch: string | null // null = couldn't resolve → merge classification suppressed
  worktrees: Worktree[]
}

// Result of the release-update check (the version badge's "update available" hint).
export interface UpdateStatus {
  current: string // running version, no leading "v"
  latest: string | null // latest published tag, no "v"; null if the check failed
  updateAvailable: boolean // a strictly-newer latest than current
  url: string | null // release page to open/share
  error: string | null // why the check failed (network, no release); null on success
}
