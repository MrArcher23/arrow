<script lang="ts">
  import { onMount } from 'svelte'
  import Sidebar from './components/Sidebar.svelte'
  import DiffView from './components/DiffView.svelte'
  import ThemeMenu from './components/ThemeMenu.svelte'
  import { loadReport, loadContent } from './lib/api'
  import { isLive } from './lib/time'
  import { DEFAULT_THEME } from './lib/themes'
  import type { Report, FileContent } from './lib/types'

  let report = $state<Report | null>(null)
  let error = $state<string | null>(null)
  let content = $state<FileContent | null>(null)
  let loadingContent = $state(false)
  let selected = $state<{ session: string; path: string } | null>(null)
  let theme = $state(localStorage.getItem('arrow.theme') ?? DEFAULT_THEME)

  let liveCount = $derived(
    report ? report.repos.filter((r) => isLive(r.sessions[0]?.lastActivity)).length : 0
  )

  $effect(() => {
    localStorage.setItem('arrow.theme', theme)
  })

  onMount(async () => {
    try {
      report = await loadReport()
      const s = report.repos[0]?.sessions[0]
      const f = s?.files[0]
      if (s && f) select(s.sessionId, f.path)
    } catch (e) {
      error = String(e)
    }
  })

  async function select(session: string, path: string) {
    selected = { session, path }
    content = null
    loadingContent = true
    try {
      content = await loadContent(path, session)
    } catch (e) {
      error = String(e)
    } finally {
      loadingContent = false
    }
  }
</script>

<div class="app">
  <header class="topbar">
    <span class="brand">arrow</span>
    <span class="subtitle">auditoría de cambios de Claude Code</span>
    <div class="actions">
      {#if liveCount > 0}<span class="live">● {liveCount} en vivo</span>{/if}
      {#if report}<span class="repos">{report.repoCount} repos</span>{/if}
      <ThemeMenu current={theme} onSelect={(id) => (theme = id)} />
    </div>
  </header>

  <div class="layout">
    <aside class="sidebar">
      {#if error}
        <div class="error">{error}</div>
      {/if}
      {#if report}
        <Sidebar {report} {selected} onSelect={select} />
      {:else if !error}
        <div class="loading">Cargando sesiones…</div>
      {/if}
    </aside>

    <main class="main">
      {#if selected}
        <div class="filebar">
          <span class="filepath">{selected.path}</span>
          {#if content?.userModified}
            <span class="warn">⚠ modificado fuera de Claude</span>
          {/if}
        </div>
      {/if}
      <div class="diff-area">
        <DiffView {content} loading={loadingContent} themeId={theme} />
      </div>
    </main>
  </div>
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .topbar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
    flex: none;
  }
  .brand {
    font-weight: 700;
    letter-spacing: 0.5px;
  }
  .subtitle {
    color: var(--dim);
    font-size: 12px;
  }
  .actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 12px;
  }
  .live {
    color: var(--green);
  }
  .repos {
    color: var(--dim);
  }
  .layout {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .sidebar {
    width: 340px;
    flex: none;
    border-right: 1px solid var(--border);
    background: var(--panel);
    overflow: auto;
    padding: 8px;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .filebar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .filepath {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
  }
  .warn {
    margin-left: auto;
    color: var(--warn);
    font-size: 12px;
    white-space: nowrap;
  }
  .diff-area {
    flex: 1;
    min-height: 0;
  }
  .error {
    color: var(--red);
    font-size: 12px;
    padding: 8px;
    white-space: pre-wrap;
  }
  .loading {
    color: var(--dim);
    font-size: 13px;
    padding: 8px;
  }
</style>
