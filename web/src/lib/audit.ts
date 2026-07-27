// Report-shaping helpers. The parser reports EVERY session (including chat-only
// ones with no edits) and, for a session that edited files from more than one
// git root, one copy under each root. The two tabs want different shapes:
//   - Audit keeps only sessions WITH edits (and every per-root copy), so the
//     view renders exactly as it did before chat-only sessions existed.
//   - Sessions lists each session exactly ONCE, under its primary repo.
import type { Repo, Session } from './types'

function ts(iso?: string | null): number {
  if (!iso) return 0
  const t = Date.parse(iso)
  return Number.isNaN(t) ? 0 : t
}

export function auditRepos(repos: Repo[]): Repo[] {
  return repos
    .map((r) => ({ ...r, sessions: r.sessions.filter((s) => s.fileCount > 0) }))
    .filter((r) => r.sessions.length > 0)
    .sort((a, b) => ts(b.sessions[0]?.lastActivity) - ts(a.sessions[0]?.lastActivity))
}

export interface RepoSessions {
  repo: Repo
  sessions: Session[]
}

// Path-boundary prefix check: '/a/bc' is NOT within '/a/b'.
function within(cwd: string | null | undefined, root: string): boolean {
  return !!cwd && (cwd === root || cwd.startsWith(root.endsWith('/') ? root : root + '/'))
}

// One entry per repo, each session listed exactly once. A session the raw
// report duplicates across git roots is owned by the repo its resumeCwd lives
// in (most specific root wins); with no resumeCwd match it stays with its
// first (most recent) occurrence. Repo order is preserved from the report.
export function sessionRepos(repos: Repo[]): RepoSessions[] {
  const owner = new Map<string, Repo>()
  for (const r of repos) {
    for (const s of r.sessions) {
      const prev = owner.get(s.sessionId)
      if (!prev) {
        owner.set(s.sessionId, r)
        continue
      }
      const inPrev = within(s.resumeCwd, prev.cwd)
      const inThis = within(s.resumeCwd, r.cwd)
      if (inThis && (!inPrev || r.cwd.length > prev.cwd.length)) owner.set(s.sessionId, r)
    }
  }
  const seen = new Set<string>()
  return repos
    .map((repo) => ({
      repo,
      sessions: repo.sessions.filter((s) => {
        if (owner.get(s.sessionId) !== repo || seen.has(s.sessionId)) return false
        seen.add(s.sessionId)
        return true
      }),
    }))
    .filter((x) => x.sessions.length > 0)
}

export function uniqueSessions(repos: Repo[]): Session[] {
  return sessionRepos(repos).flatMap((x) => x.sessions)
}
