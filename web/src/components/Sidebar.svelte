<script lang="ts">
  import type { Report, Session } from '../lib/types'
  import { isLive, relative, dateBucket, BUCKET_ORDER } from '../lib/time'

  interface Props {
    report: Report
    selected: { session: string; path: string } | null
    onSelect: (session: string, path: string) => void
  }
  let { report, selected, onSelect }: Props = $props()

  // Repos en vivo se auto-expanden; el resto arrancan colapsados.
  function initRepos() {
    const o: Record<string, boolean> = {}
    for (const r of report.repos) o[r.cwd] = isLive(r.sessions[0]?.lastActivity)
    return o
  }
  let openRepos = $state<Record<string, boolean>>(initRepos())
  let openHist = $state<Record<string, boolean>>({})
  let openBucket = $state<Record<string, boolean>>({})
  let openSess = $state<Record<string, boolean>>({})

  function basename(p: string): string {
    const parts = p.split('/').filter(Boolean)
    return parts[parts.length - 1] || p
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

<nav class="tree">
  {#each report.repos as repo}
    {@const current = repo.sessions[0]}
    {@const rest = repo.sessions.slice(1)}
    {@const live = isLive(current?.lastActivity)}

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
            {#if live}<span class="badge">● en vivo</span>{/if}
            <span class="stitle" title={titleOf(current)}>{titleOf(current)}</span>
            <span class="time">{relative(current.lastActivity)}</span>
          </div>
          {#each current.files as f}
            <button
              class="row file"
              class:active={selected?.session === current.sessionId && selected?.path === f.path}
              onclick={() => onSelect(current.sessionId, f.path)}
              title={f.path}
            >
              <span class="fname">{basename(f.path)}</span>
              <span class="stats"><span class="add">+{f.added}</span><span class="del">-{f.removed}</span></span>
              {#if f.userModified}<span class="flag" title="Modificado fuera de Claude">⚠</span>{/if}
            </button>
          {/each}
        {/if}

        {#if rest.length}
          <button class="row hist-head" onclick={() => (openHist = toggle(openHist, repo.cwd))}>
            <span class="chev">{openHist[repo.cwd] ? '▾' : '▸'}</span>
            <span class="hist-label">Historial</span>
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
                    <span class="time">{relative(s.lastActivity)}</span>
                  </button>
                  {#if openSess[skey]}
                    {#each s.files as f}
                      <button
                        class="row file deep"
                        class:active={selected?.session === s.sessionId && selected?.path === f.path}
                        onclick={() => onSelect(s.sessionId, f.path)}
                        title={f.path}
                      >
                        <span class="fname">{basename(f.path)}</span>
                        <span class="stats"><span class="add">+{f.added}</span><span class="del">-{f.removed}</span></span>
                        {#if f.userModified}<span class="flag">⚠</span>{/if}
                      </button>
                    {/each}
                  {/if}
                {/each}
              {/if}
            {/each}
          {/if}
        {/if}
      {/if}
    </div>
  {/each}
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
  .badge {
    font-size: 9px;
    color: var(--green);
    border: 1px solid var(--green);
    border-radius: 999px;
    padding: 0 5px;
    flex: none;
    letter-spacing: 0.3px;
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
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stats {
    margin-left: auto;
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
</style>
