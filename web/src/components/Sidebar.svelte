<script lang="ts">
  import type { Report } from '../lib/types'

  interface Props {
    report: Report
    selected: { session: string; path: string } | null
    onSelect: (session: string, path: string) => void
  }
  let { report, selected, onSelect }: Props = $props()

  let openRepos = $state<Record<string, boolean>>({})
  let openSessions = $state<Record<string, boolean>>({})

  function basename(p: string): string {
    const parts = p.split('/').filter(Boolean)
    return parts[parts.length - 1] || p
  }
  function toggleRepo(k: string) {
    openRepos = { ...openRepos, [k]: !openRepos[k] }
  }
  function toggleSession(k: string) {
    openSessions = { ...openSessions, [k]: !openSessions[k] }
  }
</script>

<nav class="tree">
  {#each report.repos as repo}
    <div class="repo">
      <button class="row repo-head" onclick={() => toggleRepo(repo.cwd)} title={repo.cwd}>
        <span class="chev">{openRepos[repo.cwd] ? '▾' : '▸'}</span>
        <span class="repo-name">{basename(repo.cwd)}</span>
        {#if repo.gitBranch}<span class="branch">{repo.gitBranch}</span>{/if}
      </button>

      {#if openRepos[repo.cwd]}
        {#each repo.sessions as session}
          {@const skey = repo.cwd + '::' + session.sessionId}
          <div class="session">
            <button class="row session-head" onclick={() => toggleSession(skey)}>
              <span class="chev">{openSessions[skey] ? '▾' : '▸'}</span>
              <span class="sid">{session.sessionId.slice(0, 8)}</span>
              <span class="count">{session.fileCount}</span>
            </button>

            {#if openSessions[skey]}
              {#each session.files as f}
                <button
                  class="row file"
                  class:active={selected?.session === session.sessionId && selected?.path === f.path}
                  onclick={() => onSelect(session.sessionId, f.path)}
                  title={f.path}
                >
                  <span class="fname">{basename(f.path)}</span>
                  <span class="stats">
                    <span class="add">+{f.added}</span><span class="del">-{f.removed}</span>
                  </span>
                  {#if f.userModified}<span class="flag" title="Modificado también fuera de Claude">⚠</span>{/if}
                </button>
              {/each}
            {/if}
          </div>
        {/each}
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
    max-width: 110px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .session-head {
    padding-left: 20px;
  }
  .sid {
    font-family: var(--mono);
    color: var(--accent);
  }
  .count {
    margin-left: auto;
    color: var(--dim);
    font-size: 11px;
  }
  .file {
    padding-left: 36px;
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
</style>
