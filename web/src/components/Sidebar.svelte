<script lang="ts">
  import type { Report, Repo, Session } from '../lib/types'
  import { isLive, focusRepos as focusReposOf, relative, dateBucket, BUCKET_ORDER } from '../lib/time'

  interface Props {
    report: Report
    selected: { session: string; path: string } | null
    now: number // reloj reactivo (tick en App.svelte): envejece los "Nm ago" y el punto verde
    onSelect: (session: string, path: string) => void
  }
  let { report, selected, now, onSelect }: Props = $props()

  // Foco: los repos de la sesión activa (la de actividad más reciente) + cualquiera tocado
  // en la misma ráfaga (~10 min). El resto va a "Other repos". Lógica única en time.ts.
  let focusRepos = $derived(focusReposOf(report.repos))
  let otherRepos = $derived(report.repos.filter((r) => !focusRepos.includes(r)))

  function focusDefaults() {
    const o: Record<string, boolean> = {}
    for (const r of focusReposOf(report.repos)) o[r.cwd] = true
    return o
  }
  let openRepos = $state<Record<string, boolean>>(focusDefaults())

  // Tras un refresco, auto-expandir los repos del foco nuevos sin pisar los toggles del usuario.
  $effect(() => {
    const fset = new Set(focusReposOf(report.repos).map((r) => r.cwd))
    let changed = false
    const next = { ...openRepos }
    for (const r of report.repos) {
      if (fset.has(r.cwd) && !(r.cwd in next)) {
        next[r.cwd] = true
        changed = true
      }
    }
    if (changed) openRepos = next
  })
  let openOthers = $state(false)
  let openHist = $state<Record<string, boolean>>({})
  let openBucket = $state<Record<string, boolean>>({})
  let openSess = $state<Record<string, boolean>>({})

  function basename(p: string): string {
    const parts = p.split('/').filter(Boolean)
    return parts[parts.length - 1] || p
  }
  // Carpeta del archivo relativa a la raíz del repo (sin el nombre del archivo).
  function relDir(path: string, root: string): string {
    let p = path
    if (root && p.startsWith(root)) p = p.slice(root.length).replace(/^\/+/, '')
    const i = p.lastIndexOf('/')
    return i >= 0 ? p.slice(0, i) : ''
  }
  // Compacta a las últimas 2 carpetas: web/src/components -> …/src/components
  function shortDir(dir: string): string {
    if (!dir) return ''
    const parts = dir.split('/').filter(Boolean)
    return parts.length <= 2 ? parts.join('/') : '…/' + parts.slice(-2).join('/')
  }
  function titleOf(s: Session): string {
    return s.title || (s.lastPrompt ? s.lastPrompt.slice(0, 60) : '') || s.sessionId.slice(0, 8)
  }
  function buckets(sessions: Session[]) {
    const map = new Map<string, Session[]>()
    for (const s of sessions) {
      const b = dateBucket(s.lastActivity)
      if (!map.has(b)) map.set(b, [])
      map.get(b)!.push(s)
    }
    return BUCKET_ORDER.filter((b) => map.has(b)).map((b) => ({ bucket: b, sessions: map.get(b)! }))
  }
  function toggle(o: Record<string, boolean>, k: string): Record<string, boolean> {
    return { ...o, [k]: !o[k] }
  }
</script>

{#snippet fileRow(sessionId: string, f: any, deep: boolean, root: string)}
  {@const dir = shortDir(relDir(f.path, root))}
  {@const ft = relative(f.lastTouched, now)}
  <button
    class="row file"
    class:deep
    class:active={selected?.session === sessionId && selected?.path === f.path}
    onclick={() => onSelect(sessionId, f.path)}
    title={f.path}
  >
    <span class="fname">{basename(f.path)}</span>
    {#if dir}<span class="fdir">{dir}</span>{/if}
    <span class="meta">
      {#if ft}<span class="ftime">{ft}</span>{/if}
      <span class="stats"><span class="add">+{f.added}</span><span class="del">-{f.removed}</span></span>
      {#if f.userModified}<span class="flag" title="Modified outside Claude">⚠</span>{/if}
    </span>
  </button>
{/snippet}

{#snippet repoRow(repo: Repo, inFocus: boolean)}
  {@const current = repo.sessions[0]}
  {@const rest = repo.sessions.slice(1)}
  <!-- Punto verde: solo en repos del foco que además tienen actividad reciente (no glow falso si es viejo). -->
  {@const live = inFocus && isLive(current?.lastActivity, now)}
  <div class="repo">
    <button class="row repo-head" onclick={() => (openRepos = toggle(openRepos, repo.cwd))} title={repo.cwd}>
      <span class="chev">{openRepos[repo.cwd] ? '▾' : '▸'}</span>
      {#if live}<span class="dot"></span>{/if}
      <span class="repo-name">{basename(repo.cwd)}</span>
      {#if repo.gitBranch}<span class="branch">{repo.gitBranch}</span>{/if}
    </button>

    {#if openRepos[repo.cwd]}
      {#if current}
        <div class="current-head">
          <span class="stitle" title={titleOf(current)}>{titleOf(current)}</span>
          <span class="time">{relative(current.lastActivity, now)}</span>
        </div>
        {#each current.files as f}
          {@render fileRow(current.sessionId, f, false, repo.cwd)}
        {/each}
      {/if}

      {#if rest.length}
        <button class="row hist-head" onclick={() => (openHist = toggle(openHist, repo.cwd))}>
          <span class="chev">{openHist[repo.cwd] ? '▾' : '▸'}</span>
          <span class="hist-label">History</span>
          <span class="count">{rest.length}</span>
        </button>
        {#if openHist[repo.cwd]}
          {#each buckets(rest) as grp}
            {@const bkey = repo.cwd + '::' + grp.bucket}
            <button class="row bucket-head" onclick={() => (openBucket = toggle(openBucket, bkey))}>
              <span class="chev">{openBucket[bkey] ? '▾' : '▸'}</span>
              <span class="bucket-label">{grp.bucket}</span>
              <span class="count">{grp.sessions.length}</span>
            </button>
            {#if openBucket[bkey]}
              {#each grp.sessions as s}
                {@const skey = repo.cwd + '::' + s.sessionId}
                <button class="row hist-session" onclick={() => (openSess = toggle(openSess, skey))} title={titleOf(s)}>
                  <span class="chev">{openSess[skey] ? '▾' : '▸'}</span>
                  <span class="stitle small">{titleOf(s)}</span>
                  <span class="time">{relative(s.lastActivity, now)}</span>
                </button>
                {#if openSess[skey]}
                  {#each s.files as f}
                    {@render fileRow(s.sessionId, f, true, repo.cwd)}
                  {/each}
                {/if}
              {/each}
            {/if}
          {/each}
        {/if}
      {/if}
    {/if}
  </div>
{/snippet}

<nav class="tree">
  {#each focusRepos as repo}
    {@render repoRow(repo, true)}
  {/each}

  {#if otherRepos.length}
    <button class="row others-head" onclick={() => (openOthers = !openOthers)}>
      <span class="chev">{openOthers ? '▾' : '▸'}</span>
      <span class="others-label">Other repos</span>
      <span class="count">{otherRepos.length}</span>
    </button>
    {#if openOthers}
      <div class="others">
        {#each otherRepos as repo}
          {@render repoRow(repo, false)}
        {/each}
      </div>
    {/if}
  {/if}
</nav>

<style>
  .tree {
    font-size: 13px;
    user-select: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    border: none;
    background: transparent;
    color: var(--fg);
    text-align: left;
    padding: 4px 8px;
    cursor: pointer;
    border-radius: 5px;
    font: inherit;
    overflow: hidden;
  }
  .row:hover {
    background: var(--hover);
  }
  .chev {
    color: var(--dim);
    width: 10px;
    flex: none;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--green);
    flex: none;
    box-shadow: 0 0 6px var(--green);
  }
  .repo-name {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .branch {
    margin-left: auto;
    font-size: 10px;
    color: var(--dim);
    background: var(--chip);
    padding: 1px 6px;
    border-radius: 999px;
    white-space: nowrap;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: none;
  }
  .current-head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px 2px 22px;
    overflow: hidden;
  }
  .stitle {
    font-size: 12px;
    color: var(--fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stitle.small {
    color: var(--dim);
  }
  .time {
    margin-left: auto;
    font-size: 10px;
    color: var(--dim);
    white-space: nowrap;
    flex: none;
  }
  .file {
    padding-left: 28px;
  }
  .file.deep {
    padding-left: 48px;
  }
  .file.active {
    background: var(--active);
  }
  .fname {
    flex: 0 1 auto;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fdir {
    flex: 1 1 auto;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 10px;
    color: var(--dim);
    opacity: 0.6;
  }
  /* Cluster de metadata a la derecha de la fila de archivo: "Nm ago" + +/− + ⚠.
     margin-left:auto lo ancla a la derecha tanto si hay carpeta (fdir crece) como si no. */
  .meta {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .ftime {
    font-size: 10px;
    color: var(--dim);
    white-space: nowrap;
  }
  .stats {
    font-family: var(--mono);
    font-size: 11px;
    display: flex;
    gap: 6px;
    flex: none;
  }
  .add {
    color: var(--green);
  }
  .del {
    color: var(--red);
  }
  .flag {
    flex: none;
    color: var(--warn);
  }
  .hist-head {
    padding-left: 22px;
    color: var(--dim);
  }
  .hist-label {
    font-size: 12px;
  }
  .bucket-head {
    padding-left: 36px;
    color: var(--dim);
    font-size: 12px;
  }
  .hist-session {
    padding-left: 50px;
  }
  .count {
    margin-left: auto;
    color: var(--dim);
    font-size: 11px;
    flex: none;
  }
  .others-head {
    margin-top: 8px;
    border-top: 1px solid var(--border);
    border-radius: 0;
    padding-top: 8px;
    color: var(--dim);
    font-size: 12px;
  }
  .others-label {
    text-transform: uppercase;
    letter-spacing: 0.5px;
    font-size: 11px;
  }
  .others {
    opacity: 0.85;
  }
</style>
