<script lang="ts">
  // "Sessions" tab: every repo with Claude Code history, ordered by activity,
  // to resume any session (copy cmd / resume) or clean it up (system trash).
  // Ports the approved mockup (arrow-sessions-mockup.html) minus its design notes.
  import { inTauri, trashSession, resumeInTerminal, openUrl } from '../lib/api'
  import { sessionRepos } from '../lib/audit'
  import { relative } from '../lib/time'
  import type { Report, Session } from '../lib/types'

  interface Props {
    report: Report
    now: number // reactive clock (tick in App.svelte): ages "Nd ago" and "expires in Nd"
    onRefresh: () => void | Promise<void>
  }
  let { report, now, onRefresh }: Props = $props()

  const DAY = 86_400_000
  const COLLAPSED_ROWS = 4 // rows shown per repo before the "N more sessions" expander
  const JUNK_BYTES = 20 * 1024 // untitled or smaller than this = failed start / test run

  type Filter = 'all' | 'live' | 'expiring' | 'junk'
  let filter = $state<Filter>('all')
  let q = $state('')
  let qn = $derived(q.trim().toLowerCase())

  // Pinned sessions persist and are protected from bulk cleaning (skipped by
  // "Select all" and excluded from the bulk targets even if checked earlier).
  function loadPins(): Set<string> {
    try {
      const raw = JSON.parse(localStorage.getItem('arrow.pins') ?? '[]')
      return new Set(Array.isArray(raw) ? raw.filter((x) => typeof x === 'string') : [])
    } catch {
      return new Set()
    }
  }
  let pins = $state<Set<string>>(loadPins())
  function savePins(next: Set<string>) {
    pins = next
    localStorage.setItem('arrow.pins', JSON.stringify([...next]))
  }
  function togglePin(id: string) {
    const next = new Set(pins)
    if (next.has(id)) {
      next.delete(id)
    } else {
      next.add(id)
      // A pin promises "bulk clean skips this" — an earlier checkmark must not
      // survive the promise.
      if (checked.has(id)) {
        const c = new Set(checked)
        c.delete(id)
        checked = c
      }
    }
    savePins(next)
  }

  // View state. Sets are REASSIGNED (never mutated in place) so Svelte 5 sees
  // the change. `gone` = already trashed (the row stays struck through until
  // the refreshed report drops it).
  let expanded = $state<Set<string>>(new Set()) // repo cwds with "more sessions" open
  let gone = $state<Set<string>>(new Set())
  let checked = $state<Set<string>>(new Set())
  let selectMode = $state(false)
  let confirming = $state<string | null>(null) // sessionId awaiting delete confirmation
  let deleting = $state<string | null>(null) // sessionId with a trash call in flight
  let rowMsg = $state<{ id: string; ok: boolean; text: string } | null>(null)
  let copied = $state<{ key: string; ok: boolean } | null>(null)
  let bulkConfirm = $state(false)
  let bulkBusy = $state(false)
  let bulkProgress = $state<string | null>(null)
  let bulkMsg = $state<{ ok: boolean; text: string } | null>(null)

  // ── Derived ────────────────────────────────────────────────────
  // The raw report duplicates a session under every git root it edited from
  // (the Audit tab needs that); here each session appears exactly once.
  let viewRepos = $derived(sessionRepos(report.repos))
  let allSessions = $derived(viewRepos.flatMap((x) => x.sessions))
  let cAll = $derived(allSessions.length)
  let cLive = $derived(allSessions.filter((s) => s.live).length)
  let cExp = $derived(allSessions.filter((s) => isExpiring(s)).length)
  let cJunk = $derived(allSessions.filter(isJunk).length)
  let anyShown = $derived(viewRepos.some((x) => x.sessions.some((s) => matches(s, x.repo.cwd))))
  // Bulk targets: what the bar counts, sizes, and deletes. Live and pinned are
  // excluded here — not just at checkbox time — so a session that went live or
  // got pinned AFTER being checked can never be trashed.
  let selSessions = $derived(
    allSessions.filter(
      (s) =>
        checked.has(s.sessionId) &&
        !gone.has(s.sessionId) &&
        !s.live &&
        !pins.has(s.sessionId)
    )
  )
  let selBytes = $derived(selSessions.reduce((a, s) => a + s.sizeBytes, 0))

  // Prune pins of sessions that no longer exist (trashed, or expired by Claude
  // Code's own cleanup) so localStorage doesn't accumulate them forever.
  $effect(() => {
    const ids = new Set(allSessions.map((s) => s.sessionId))
    if ([...pins].some((id) => !ids.has(id))) {
      savePins(new Set([...pins].filter((id) => ids.has(id))))
    }
  })

  // ── Helpers ────────────────────────────────────────────────────
  function repoName(cwd: string): string {
    return cwd.replace(/\/+$/, '').split('/').pop() || cwd
  }

  function fmtSize(bytes: number): string {
    const kb = bytes / 1024
    return kb >= 1024 ? (kb / 1024).toFixed(1) + ' MB' : Math.round(kb) + ' KB'
  }

  // Days before Claude Code purges the transcript (retentionDays since the last
  // activity). null = no date (no label shown).
  function daysLeft(s: Session): number | null {
    if (!s.lastActivity) return null
    const t = Date.parse(s.lastActivity)
    if (Number.isNaN(t)) return null
    return Math.ceil((t + report.retentionDays * DAY - now) / DAY)
  }
  function isExpiring(s: Session): boolean {
    const d = daysLeft(s)
    return d !== null && d <= 7
  }

  function isJunk(s: Session): boolean {
    return !s.title || s.sizeBytes < JUNK_BYTES
  }

  // Single-quote for a shell command the USER will paste (arrow never runs it).
  function shq(s: string): string {
    return "'" + s.replace(/'/g, "'\\''") + "'"
  }
  function cmdFor(s: Session): string {
    const resume = 'claude --resume ' + s.sessionId
    return s.resumeCwd ? `cd ${shq(s.resumeCwd)} && ${resume}` : resume
  }
  // Browser fallback for delete: the CLI command(s) the user runs themselves
  // (disk-mutating actions are Tauri-only, same rule as the worktree Clean).
  function trashCmd(ids: string[]): string {
    return ids.map((id) => `arrow --trash-session ${id} --yes`).join(' && ')
  }

  function matches(s: Session, repoCwd: string): boolean {
    if (filter === 'live' && !s.live) return false
    if (filter === 'expiring' && !isExpiring(s)) return false
    if (filter === 'junk' && !isJunk(s)) return false
    if (qn) {
      const hay = ((s.title ?? '') + ' ' + s.lastPrompts.join(' ') + ' ' + repoCwd).toLowerCase()
      if (!hay.includes(qn)) return false
    }
    return true
  }

  // Visible rows of a repo: with a filter/search active every match shows; with
  // neither, truncate to 4 behind the "N more sessions" expander.
  function visibleOf(repoCwd: string, sessions: Session[]): { shown: Session[]; hidden: number } {
    const vis = sessions.filter((s) => matches(s, repoCwd))
    if (filter === 'all' && !qn && !expanded.has(repoCwd) && vis.length > COLLAPSED_ROWS) {
      return { shown: vis.slice(0, COLLAPSED_ROWS), hidden: vis.length - COLLAPSED_ROWS }
    }
    return { shown: vis, hidden: 0 }
  }
  function toggleExpanded(cwd: string) {
    const next = new Set(expanded)
    if (next.has(cwd)) next.delete(cwd)
    else next.add(cwd)
    expanded = next
  }

  // ── Actions ────────────────────────────────────────────────────
  let copyTimer: ReturnType<typeof setTimeout> | undefined
  async function copy(key: string, text: string) {
    let ok = true
    try {
      await navigator.clipboard.writeText(text)
    } catch {
      ok = false // shown honestly as "copy failed", never a false "copied!"
    }
    copied = { key, ok }
    clearTimeout(copyTimer)
    copyTimer = setTimeout(() => {
      if (copied?.key === key) copied = null
    }, 1200)
  }
  function copyLabel(key: string, idle: string): string {
    if (copied?.key !== key) return idle
    return copied.ok ? 'copied!' : 'copy failed'
  }

  // Tauri-only: opens a terminal running `claude --resume`. On failure (no
  // emulator, bad cwd…) the backend error shows inline suggesting copy cmd.
  async function resume(s: Session) {
    rowMsg = null
    try {
      await resumeInTerminal(s.sessionId, s.resumeCwd)
    } catch (e) {
      rowMsg = { id: s.sessionId, ok: false, text: `${e} — use “copy cmd” instead` }
    }
  }

  function askDelete(id: string) {
    rowMsg = null
    confirming = id
  }

  async function doTrash(s: Session) {
    deleting = s.sessionId
    try {
      await trashSession(s.sessionId)
      const g = new Set(gone)
      g.add(s.sessionId)
      gone = g
      const c = new Set(checked)
      c.delete(s.sessionId)
      checked = c
      confirming = null
    } catch (e) {
      confirming = null
      rowMsg = { id: s.sessionId, ok: false, text: String(e) }
    } finally {
      deleting = null
    }
  }

  function toggleCheck(id: string, on: boolean) {
    const next = new Set(checked)
    if (on) next.add(id)
    else next.delete(id)
    checked = next
  }

  function toggleSelect() {
    selectMode = !selectMode
    if (!selectMode) {
      checked = new Set()
      bulkConfirm = false
      bulkMsg = null
    }
  }
  function exitSelect() {
    selectMode = false
    checked = new Set()
    bulkConfirm = false
    bulkMsg = null
  }

  // Check everything visible (respecting filter/search/truncation); live and
  // pinned are skipped.
  function selectAllShown() {
    const next = new Set(checked)
    for (const { repo, sessions } of viewRepos) {
      for (const s of visibleOf(repo.cwd, sessions).shown) {
        if (!s.live && !pins.has(s.sessionId) && !gone.has(s.sessionId)) next.add(s.sessionId)
      }
    }
    checked = next
  }

  // Trash the selection IN SEQUENCE (one call at a time, reporting progress);
  // individual failures don't stop the rest. Refreshes the report at the end.
  async function bulkDelete() {
    const targets = [...selSessions]
    bulkBusy = true
    bulkMsg = null
    let ok = 0
    let failed = 0
    let firstErr: string | null = null
    for (let i = 0; i < targets.length; i++) {
      bulkProgress = `moving ${i + 1}/${targets.length} to trash…`
      try {
        await trashSession(targets[i].sessionId)
        ok++
        const g = new Set(gone)
        g.add(targets[i].sessionId)
        gone = g
        const c = new Set(checked)
        c.delete(targets[i].sessionId)
        checked = c
      } catch (e) {
        failed++
        if (!firstErr) firstErr = String(e)
      }
    }
    bulkProgress = null
    bulkBusy = false
    bulkConfirm = false
    bulkMsg = failed
      ? { ok: false, text: `${ok} moved to trash · ${failed} failed — ${firstErr}` }
      : { ok: true, text: `✓ ${ok} session${ok === 1 ? '' : 's'} moved to trash` }
    await onRefresh()
  }
</script>

<div class="toolbar">
  <input class="search" type="search" placeholder="filter by title, prompt, repo…" bind:value={q} />
  <button class="fchip" class:on={filter === 'all'} onclick={() => (filter = 'all')}>
    All <span>{cAll}</span>
  </button>
  <button class="fchip" class:on={filter === 'live'} onclick={() => (filter = 'live')}>
    <span class="fdot" style="background:var(--green)"></span>Live <span>{cLive}</span>
  </button>
  <button class="fchip" class:on={filter === 'expiring'} onclick={() => (filter = 'expiring')}>
    <span class="fdot" style="background:var(--warn)"></span>Expiring ≤7d <span>{cExp}</span>
  </button>
  <button class="fchip" class:on={filter === 'junk'} onclick={() => (filter = 'junk')}>
    Junk <span>{cJunk}</span>
  </button>
  <span class="spacer"></span>
  <button class="chipbtn" onclick={toggleSelect}>{selectMode ? 'Selecting…' : 'Select'}</button>
</div>

<main>
  {#each viewRepos as { repo, sessions } (repo.cwd)}
    {@const vis = visibleOf(repo.cwd, sessions)}
    {#if vis.shown.length}
      <section class="repo">
        <div class="repo-head">
          <span class="repo-name">{repoName(repo.cwd)}</span>
          <span class="repo-path">{repo.cwd}</span>
          <span class="repo-meta">
            {sessions.length} sessions · latest {relative(sessions[0]?.lastActivity, now)}
          </span>
        </div>

        {#each vis.shown as s (s.sessionId)}
          {@const pinned = pins.has(s.sessionId)}
          {@const isGone = gone.has(s.sessionId)}
          {@const lastPr = s.prLinks[s.prLinks.length - 1]}
          {@const d = daysLeft(s)}
          <div class="srow" class:gone={isGone}>
            <div class="sline">
              {#if selectMode && !isGone}
                <input
                  type="checkbox"
                  class="scheck"
                  checked={checked.has(s.sessionId)}
                  disabled={s.live || pinned}
                  title={s.live ? 'session is running now' : pinned ? 'pinned — skipped' : undefined}
                  onchange={(e) => toggleCheck(s.sessionId, e.currentTarget.checked)}
                />
              {/if}
              {#if s.live}<span class="dot" title="claude is running this session now"></span>{/if}
              {#if pinned}<span class="star" title="pinned — protected from bulk clean">★</span>{/if}
              <span class="stitle" class:untitled={!s.title} title={s.sessionId}>
                {s.title || '(no title)'}
              </span>
              <span class="smeta">
                {#if lastPr}
                  <button class="pr" title={lastPr.url} onclick={() => openUrl(lastPr.url)}>
                    PR #{lastPr.number}
                  </button>
                {/if}
                {#if repo.gitBranch && repo.gitBranch !== 'HEAD'}
                  <span class="branch">{repo.gitBranch}</span>
                {/if}
                <span class="size">{fmtSize(s.sizeBytes)}</span>
                <span class="time">{relative(s.lastActivity, now)}</span>
                {#if d !== null}
                  <span class="exp" class:warn={d > 0 && d <= 7} class:crit={d <= 0}>
                    {d <= 0 ? 'expires today' : `expires in ${d}d`}
                  </span>
                {/if}
                <span class="sactions">
                  {#if inTauri}
                    <button class="chipbtn" onclick={() => resume(s)} title={cmdFor(s)}>resume</button>
                  {/if}
                  <button
                    class="chipbtn"
                    title={s.sessionId}
                    onclick={() => copy(s.sessionId + ':id', s.sessionId)}
                  >
                    {copyLabel(s.sessionId + ':id', 'copy id')}
                  </button>
                  <button
                    class="chipbtn"
                    title={cmdFor(s)}
                    onclick={() => copy(s.sessionId + ':cmd', cmdFor(s))}
                  >
                    {copyLabel(s.sessionId + ':cmd', 'copy cmd')}
                  </button>
                  <button class="chipbtn" class:pinned onclick={() => togglePin(s.sessionId)}>
                    {pinned ? '★ pinned' : '☆ pin'}
                  </button>
                  <button
                    class="chipbtn danger"
                    disabled={s.live || isGone}
                    title={s.live ? 'session is running now' : undefined}
                    onclick={() => askDelete(s.sessionId)}
                  >
                    delete
                  </button>
                </span>
              </span>
            </div>

            {#if s.lastPrompts.length && !isGone}
              <div class="prompts">
                {#each s.lastPrompts as p}
                  <div class="prompt"><span class="tick">└</span>“{p}”</div>
                {/each}
              </div>
            {/if}

            {#if isGone}
              <div class="msg">
                ✓ moved to trash — transcript, file-history snapshots and subagent logs (recoverable)
              </div>
            {:else if confirming === s.sessionId}
              <div class="confirm">
                {#if inTauri}
                  <code class="confirm-cmd">
                    trash ~/.claude/projects/…/{s.sessionId.slice(0, 8)}….jsonl · file-history/{s.sessionId.slice(0, 8)}… · subagents/
                  </code>
                  <span class="confirm-note">moved to system trash — recoverable</span>
                  <span class="confirm-actions">
                    <button class="go" disabled={deleting === s.sessionId} onclick={() => doTrash(s)}>
                      {deleting === s.sessionId ? 'Removing…' : 'Remove'}
                    </button>
                    <button
                      class="cancel"
                      disabled={deleting === s.sessionId}
                      onclick={() => (confirming = null)}
                    >
                      Cancel
                    </button>
                  </span>
                {:else}
                  <!-- Browser: arrow never deletes over HTTP — hand the user the command. -->
                  <code class="confirm-cmd">{trashCmd([s.sessionId])}</code>
                  <span class="confirm-note">run it in your terminal — the browser build never deletes</span>
                  <span class="confirm-actions">
                    <button class="chipbtn" onclick={() => copy(s.sessionId + ':rm', trashCmd([s.sessionId]))}>
                      {copyLabel(s.sessionId + ':rm', 'copy cmd')}
                    </button>
                    <button class="cancel" onclick={() => (confirming = null)}>Close</button>
                  </span>
                {/if}
              </div>
            {:else if rowMsg?.id === s.sessionId}
              <div class="msg" class:err={!rowMsg.ok}>{rowMsg.text}</div>
            {/if}
          </div>
        {/each}

        {#if vis.hidden > 0}
          <button class="more" onclick={() => toggleExpanded(repo.cwd)}>
            ▸ {vis.hidden} more sessions
          </button>
        {:else if expanded.has(repo.cwd) && filter === 'all' && !qn && vis.shown.length > COLLAPSED_ROWS}
          <button class="more" onclick={() => toggleExpanded(repo.cwd)}>▾ show less</button>
        {/if}
      </section>
    {/if}
  {/each}

  {#if !anyShown}
    <div class="empty">No sessions match.</div>
  {/if}
</main>

{#if selectMode}
  <footer class="bulkbar">
    {#if bulkConfirm && selSessions.length}
      {#if inTauri}
        <span>Delete <b>{selSessions.length} sessions</b> ({fmtSize(selBytes)})?</span>
        <span class="bulk-note">
          removes transcripts + file-history snapshots + subagent logs · moved to system trash ·
          pinned & live sessions are skipped
        </span>
        <button class="go" disabled={bulkBusy} onclick={bulkDelete}>
          {bulkBusy ? (bulkProgress ?? 'Removing…') : 'Remove'}
        </button>
        <button class="cancel" disabled={bulkBusy} onclick={() => (bulkConfirm = false)}>Back</button>
      {:else}
        <span>Clean <b>{selSessions.length} sessions</b> ({fmtSize(selBytes)})</span>
        <span class="bulk-note">
          run the copied command in your terminal — the browser build never deletes
        </span>
        <button
          class="chipbtn"
          onclick={() => copy('bulk:rm', trashCmd(selSessions.map((s) => s.sessionId)))}
        >
          {copyLabel('bulk:rm', 'copy clean cmd')}
        </button>
        <button class="cancel" onclick={() => (bulkConfirm = false)}>Back</button>
      {/if}
    {:else}
      <span>{selSessions.length} selected · {fmtSize(selBytes)}</span>
      {#if bulkMsg}
        <span class="bulk-msg" class:err={!bulkMsg.ok}>{bulkMsg.text}</span>
      {:else}
        <span class="bulk-note">
          tip: use the Junk or Expiring filter and “Select all shown” to clean in one pass
        </span>
      {/if}
      <button class="chipbtn" onclick={selectAllShown}>Select all shown</button>
      <button
        class="chipbtn danger"
        disabled={!selSessions.length}
        onclick={() => (bulkConfirm = true)}
      >
        {inTauri ? 'Delete selected…' : 'Clean selected…'}
      </button>
      <button class="cancel" onclick={exitSelect}>Done</button>
    {/if}
  </footer>
{/if}

<style>
  /* Styles ported from the approved mockup (same tokens as app.css). */
  button:focus-visible,
  input:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: 1px;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 10px;
  }
  .search {
    font: inherit;
    font-size: 12px;
    color: var(--fg);
    background: var(--chip);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 4px 10px;
    width: 260px;
  }
  .search::placeholder {
    color: var(--dim);
  }
  .fchip {
    font: inherit;
    font-size: 12px;
    color: var(--dim);
    background: var(--chip);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px 10px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .fchip:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .fchip.on {
    background: var(--active);
    color: var(--fg);
    border-color: var(--accent);
  }
  .fdot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex: none;
  }
  .toolbar .spacer {
    margin-left: auto;
  }

  .chipbtn {
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
  .chipbtn:hover:not(:disabled) {
    background: var(--active);
    color: var(--fg);
    border-color: var(--accent);
  }
  .chipbtn.danger:hover:not(:disabled) {
    background: var(--warn);
    color: var(--on-accent);
    border-color: var(--warn);
  }
  .chipbtn.pinned {
    color: var(--warn);
    border-color: var(--warn);
  }
  .chipbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* ── Repo blocks ─────────────────────────────────────────────── */
  .repo {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 6px 8px 8px;
    margin-bottom: 10px;
  }
  .repo-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 6px 6px;
    overflow: hidden;
  }
  .repo-name {
    font-weight: 600;
    font-size: 13px;
    white-space: nowrap;
  }
  .repo-path {
    font-family: var(--mono);
    font-size: 10px;
    color: var(--dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .repo-meta {
    margin-left: auto;
    font-size: 11px;
    color: var(--dim);
    white-space: nowrap;
    flex: none;
  }

  /* ── Session rows ────────────────────────────────────────────── */
  .srow {
    border-radius: 6px;
    padding: 5px 6px 6px;
  }
  .srow:hover {
    background: var(--hover);
  }
  .srow.gone {
    opacity: 0.4;
  }
  .srow.gone .stitle {
    text-decoration: line-through;
  }
  .sline {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 22px;
  }
  .scheck {
    accent-color: var(--accent);
    margin: 0;
    flex: none;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--green);
    flex: none;
    box-shadow: var(--dot-glow);
  }
  .star {
    color: var(--warn);
    flex: none;
    font-size: 12px;
  }
  .stitle {
    font-size: 13px;
    color: var(--fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 60px;
  }
  .stitle.untitled {
    color: var(--dim);
  }
  .smeta {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .pr {
    font-size: 10px;
    color: var(--accent);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 7px;
    white-space: nowrap;
    font-family: inherit;
    cursor: pointer;
  }
  .pr:hover {
    border-color: var(--accent);
  }
  .branch {
    font-size: 10px;
    color: var(--dim);
    background: var(--chip);
    padding: 1px 6px;
    border-radius: 999px;
    white-space: nowrap;
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .size {
    font-variant-numeric: tabular-nums;
    font-size: 11px;
    color: var(--dim);
    min-width: 52px;
    text-align: right;
  }
  .time {
    font-size: 10px;
    color: var(--dim);
    white-space: nowrap;
    min-width: 46px;
    text-align: right;
  }
  .exp {
    font-size: 10px;
    color: var(--dim);
    white-space: nowrap;
    min-width: 74px;
    text-align: right;
  }
  .exp.warn {
    color: var(--warn);
  }
  .exp.crit {
    color: var(--red);
  }
  .sactions {
    display: flex;
    gap: 6px;
    opacity: 0;
    transition: opacity 0.1s ease;
  }
  .srow:hover .sactions,
  .srow:focus-within .sactions {
    opacity: 1;
  }
  @media (hover: none) {
    .sactions {
      opacity: 1;
    }
  }
  .prompts {
    padding: 2px 0 0 15px;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .prompt {
    font-size: 11px;
    color: var(--dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .prompt .tick {
    opacity: 0.55;
    margin-right: 4px;
  }
  .more {
    display: block;
    width: 100%;
    text-align: left;
    border: none;
    background: transparent;
    color: var(--dim);
    font: inherit;
    font-size: 11px;
    padding: 4px 6px;
    border-radius: 5px;
    cursor: pointer;
  }
  .more:hover {
    background: var(--hover);
  }

  /* Delete confirmation (WorktreesModal pattern) */
  .confirm {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 4px 6px 2px 21px;
    padding: 7px 9px;
    background: var(--chip);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 11px;
  }
  .confirm-cmd {
    font-family: var(--mono);
    font-size: 10px;
    color: var(--fg);
    word-break: break-all;
  }
  .confirm-note {
    color: var(--dim);
    flex: 1;
    min-width: 140px;
  }
  .confirm-actions {
    display: inline-flex;
    gap: 6px;
    margin-left: auto;
  }
  .go {
    font: inherit;
    font-size: 11px;
    border-radius: 6px;
    padding: 2px 10px;
    cursor: pointer;
    color: var(--on-accent);
    background: var(--warn);
    border: 1px solid var(--warn);
  }
  /* Muted FILL, not blanket opacity: this state carries the in-flight label ("Running…",
     "moving 3/12 to trash…") for a destructive action, so the text must stay readable.
     `opacity` would composite the label into the fill, and since --on-accent flips per
     theme, the light side washed out to 2.4:1. */
  .go:disabled {
    background: var(--warn-dim);
    border-color: var(--warn-dim);
    cursor: default;
  }
  .cancel {
    font: inherit;
    font-size: 11px;
    border-radius: 6px;
    padding: 2px 10px;
    cursor: pointer;
    color: var(--dim);
    background: transparent;
    border: 1px solid var(--border);
  }
  .cancel:hover:not(:disabled) {
    color: var(--fg);
    background: var(--hover);
  }
  .msg {
    margin: 2px 6px 2px 21px;
    padding: 3px 9px;
    font-size: 11px;
    color: var(--green);
    white-space: pre-wrap;
  }
  .msg.err {
    color: var(--red);
  }

  .empty {
    padding: 60px 24px;
    text-align: center;
    color: var(--dim);
    font-size: 13px;
  }

  /* ── Bulk-select bar ─────────────────────────────────────────── */
  .bulkbar {
    position: sticky;
    bottom: 0;
    margin: 0 -16px;
    background: var(--panel);
    border-top: 1px solid var(--border);
    padding: 8px 16px;
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
    box-shadow: var(--shadow-up);
  }
  .bulk-note {
    color: var(--dim);
    font-size: 11px;
    flex: 1;
    min-width: 160px;
  }
  .bulk-msg {
    color: var(--green);
    font-size: 11px;
    flex: 1;
    min-width: 160px;
  }
  .bulk-msg.err {
    color: var(--red);
  }
</style>
