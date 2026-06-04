<script lang="ts">
  import { onMount } from 'svelte'
  import { loadWorktrees, calcWorktreeSizes } from '../lib/api'
  import { isActive, relative } from '../lib/time'
  import type { Report, RepoWorktrees, Worktree } from '../lib/types'

  interface Props {
    report: Report
    repoRoots: string[]
    now: number
    onClose: () => void
  }
  let { report, repoRoots, now, onClose }: Props = $props()

  let groups = $state<RepoWorktrees[]>([])
  let loading = $state(true)
  let error = $state<string | null>(null)
  // Disk sizes (bytes) keyed by worktree path — only filled when "Calculate
  // sizes" is pressed (walking a full checkout is the one slow part).
  let sizes = $state<Record<string, number>>({})
  let sizing = $state(false)
  let copied = $state<string | null>(null) // path of the row whose cmd was just copied

  // Flatten the report's files once: classifying active/stale only needs the most
  // recent edit under each worktree path, not the repo→session→file tree.
  let allFiles = $derived(
    report.repos.flatMap((r) => r.sessions.flatMap((s) => s.files))
  )

  async function load() {
    loading = true
    error = null
    try {
      groups = await loadWorktrees(repoRoots)
    } catch (e) {
      error = String(e)
    } finally {
      loading = false
    }
  }

  onMount(load)

  // Most recent edit (ISO) of any tracked file under `path`, or null if none.
  // A linked worktree never nests another, so a prefix match is unambiguous.
  function latestEditUnder(path: string): string | null {
    const prefix = path.endsWith('/') ? path : path + '/'
    let bestMs: number | null = null
    let bestIso: string | null = null
    for (const f of allFiles) {
      if (f.lastTouched && f.path.startsWith(prefix)) {
        const t = Date.parse(f.lastTouched)
        if (!Number.isNaN(t) && (bestMs === null || t > bestMs)) {
          bestMs = t
          bestIso = f.lastTouched
        }
      }
    }
    return bestIso
  }

  type Klass = 'safe' | 'stale' | 'active'

  // Precedence: an actively-edited worktree is "active" regardless of merge state
  // (you're working in it); a proven-merged + clean one is "safe to remove";
  // everything else old is "stale".
  function classify(w: Worktree): Klass {
    if (isActive(latestEditUnder(w.path), now)) return 'active'
    if (w.isMerged && !w.dirty && !w.locked) return 'safe'
    return 'stale'
  }

  function mergeText(g: RepoWorktrees, w: Worktree): string {
    if (w.branch === null) return 'detached HEAD'
    if (g.defaultBranch === null) return "can't tell (no default branch)"
    if (w.isMerged) return `merged into ${g.defaultBranch}`
    const ahead = w.ahead && w.ahead > 0 ? `${w.ahead} ahead` : 'diverged'
    return `${ahead} of ${g.defaultBranch} · can't confirm merged`
  }

  function linked(g: RepoWorktrees): Worktree[] {
    return g.worktrees.filter((w) => !w.isMain)
  }

  function repoName(root: string): string {
    return root.replace(/\/+$/, '').split('/').pop() || root
  }

  // Single-quote a path for a shell command the USER will paste (arrow never runs it).
  function q(s: string): string {
    return "'" + s.replace(/'/g, "'\\''") + "'"
  }

  function cmdFor(g: RepoWorktrees, w: Worktree): string {
    if (w.prunable) return `git -C ${q(g.repoRoot)} worktree prune`
    return `git -C ${q(g.repoRoot)} worktree remove ${q(w.path)}`
  }

  async function copyCmd(g: RepoWorktrees, w: Worktree) {
    try {
      await navigator.clipboard.writeText(cmdFor(g, w))
      copied = w.path
      setTimeout(() => {
        if (copied === w.path) copied = null
      }, 1200)
    } catch {
      /* clipboard unavailable: ignore (read-only feature, no harm) */
    }
  }

  async function calcSizes() {
    sizing = true
    try {
      const paths = groups.flatMap((g) => linked(g).map((w) => w.path))
      sizes = await calcWorktreeSizes(paths)
    } catch (e) {
      error = String(e)
    } finally {
      sizing = false
    }
  }

  function hsize(bytes: number | undefined): string {
    if (bytes == null) return ''
    const u = ['B', 'KB', 'MB', 'GB', 'TB']
    let n = bytes
    let i = 0
    while (n >= 1024 && i < u.length - 1) {
      n /= 1024
      i++
    }
    return `${n.toFixed(n < 10 && i > 0 ? 1 : 0)} ${u[i]}`
  }

  function lastEditLabel(w: Worktree): string {
    const iso = latestEditUnder(w.path)
    if (!iso) return 'no recent edits'
    const rel = relative(iso, now)
    return rel ? `edited ${rel}` : 'edited'
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') onClose()
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onClose}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="panel" role="dialog" aria-modal="true" aria-label="Worktree cleanup" onclick={(e) => e.stopPropagation()}>
    <header class="head">
      <h2>Worktree cleanup</h2>
      <div class="head-actions">
        <button class="icon" onclick={load} title="Refresh" aria-label="Refresh">↻</button>
        <button class="icon" onclick={onClose} title="Close" aria-label="Close">✕</button>
      </div>
    </header>

    <div class="legend">
      <span class="lg"><span class="dot safe"></span> safe to remove</span>
      <span class="lg"><span class="dot stale"></span> stale (old, unmerged)</span>
      <span class="lg"><span class="dot active"></span> active</span>
      <button class="sizes" onclick={calcSizes} disabled={sizing || loading || !groups.length}>
        {sizing ? 'Calculating…' : 'Calculate sizes'}
      </button>
    </div>

    <div class="body">
      {#if loading}
        <div class="state">Loading worktrees…</div>
      {:else if error}
        <div class="state err">{error}</div>
      {:else if !groups.length}
        <div class="state">No linked worktrees found.</div>
      {:else}
        {#each groups as g (g.repoRoot)}
          <section class="repo">
            <div class="repo-head">
              <span class="repo-name">{repoName(g.repoRoot)}</span>
              {#if g.defaultBranch}<span class="branch-chip">{g.defaultBranch}</span>{/if}
              <span class="count">{linked(g).length} wt</span>
            </div>
            {#each linked(g) as w (w.path)}
              {@const k = classify(w)}
              <div class="row">
                <span class="dot {k}" title={k}></span>
                <span class="wt-branch">{w.branch ?? '(detached)'}</span>
                <span class="wt-status">
                  {mergeText(g, w)}
                  {#if w.dirty}<span class="flag warn">· uncommitted changes</span>{/if}
                  {#if w.locked}<span class="flag warn">· locked</span>{/if}
                  {#if w.prunable}<span class="flag warn">· missing (prunable)</span>{/if}
                </span>
                <span class="wt-age">{lastEditLabel(w)}</span>
                {#if sizes[w.path] != null}<span class="wt-size">{hsize(sizes[w.path])}</span>{/if}
                <button class="copy" onclick={() => copyCmd(g, w)} title={cmdFor(g, w)}>
                  {copied === w.path ? 'copied!' : 'copy cmd'}
                </button>
              </div>
            {/each}
          </section>
        {/each}
        <p class="foot">
          Read-only: arrow lists worktrees and copies the <code>git worktree remove</code> command for
          you to run — it never deletes anything. “merged” is shown only when the branch is provably an
          ancestor of the default branch; squash/rebase merges read as “can't confirm”. Sizes are
          approximate.
        </p>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 64px 16px 16px;
  }
  .panel {
    width: min(760px, 100%);
    max-height: calc(100vh - 96px);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }
  .head h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .head-actions {
    display: flex;
    gap: 6px;
  }
  .icon {
    width: 26px;
    height: 26px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--dim);
    font-size: 14px;
    cursor: pointer;
  }
  .icon:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .legend {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    color: var(--dim);
  }
  .lg {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    flex: none;
    background: var(--dim);
  }
  .dot.safe {
    background: var(--green);
  }
  .dot.stale {
    background: var(--warn);
  }
  .dot.active {
    background: var(--dim);
  }
  .sizes {
    margin-left: auto;
    font: inherit;
    font-size: 12px;
    color: var(--fg);
    background: var(--chip);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 9px;
    cursor: pointer;
  }
  .sizes:hover:not(:disabled) {
    background: var(--hover);
  }
  .sizes:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .body {
    overflow: auto;
    padding: 8px 8px 12px;
  }
  .state {
    padding: 24px;
    text-align: center;
    color: var(--dim);
    font-size: 13px;
  }
  .state.err {
    color: var(--red);
    white-space: pre-wrap;
  }
  .repo {
    padding: 6px 8px;
  }
  .repo-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 6px 4px;
  }
  .repo-name {
    font-weight: 600;
    font-size: 13px;
  }
  .branch-chip {
    font-size: 11px;
    color: var(--dim);
    background: var(--chip);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 7px;
  }
  .count {
    margin-left: auto;
    font-size: 11px;
    color: var(--dim);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px;
    border-radius: 6px;
    font-size: 12px;
  }
  .row:hover {
    background: var(--hover);
  }
  .wt-branch {
    font-family: var(--mono);
    color: var(--fg);
    white-space: nowrap;
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: none;
  }
  .wt-status {
    color: var(--dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
  }
  .flag.warn {
    color: var(--warn);
  }
  .wt-age {
    color: var(--dim);
    white-space: nowrap;
    flex: none;
  }
  .wt-size {
    font-variant-numeric: tabular-nums;
    color: var(--fg);
    white-space: nowrap;
    flex: none;
    min-width: 56px;
    text-align: right;
  }
  .copy {
    font: inherit;
    font-size: 11px;
    color: var(--dim);
    background: var(--chip);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 2px 8px;
    cursor: pointer;
    white-space: nowrap;
    flex: none;
  }
  .copy:hover {
    background: var(--active);
    color: var(--fg);
    border-color: var(--accent);
  }
  .foot {
    margin: 10px 8px 2px;
    font-size: 11px;
    line-height: 1.5;
    color: var(--dim);
  }
  .foot code {
    font-family: var(--mono);
    font-size: 10px;
  }
</style>
