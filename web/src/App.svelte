<script lang="ts">
  import { onMount } from 'svelte'
  import Sidebar from './components/Sidebar.svelte'
  import DiffView from './components/DiffView.svelte'
  import ThemeMenu from './components/ThemeMenu.svelte'
  import WindowControls from './components/WindowControls.svelte'
  import OpenInEditor from './components/OpenInEditor.svelte'
  import WorktreesModal from './components/WorktreesModal.svelte'
  import VersionBadge from './components/VersionBadge.svelte'
  import SessionsView from './components/SessionsView.svelte'
  import { listen } from '@tauri-apps/api/event'
  import { loadReport, loadContent, clearContentCache, inTauri } from './lib/api'
  import { loadZoom, applyZoom, clampZoom, ZOOM_STEP } from './lib/zoom'
  import { winToggleMaximize } from './lib/window'
  import { isLive, focusRepos as focusReposOf } from './lib/time'
  import { auditRepos, sessionRepos, uniqueSessions } from './lib/audit'
  import { DEFAULT_THEME } from './lib/themes'
  import type { Report, FileContent } from './lib/types'

  let report = $state<Report | null>(null)
  let error = $state<string | null>(null)
  let content = $state<FileContent | null>(null)
  let loadingContent = $state(false)
  // Bound to the DiffView instance so the global Ctrl+F can open its find panel.
  let diffView = $state<{ openSearch: () => void }>()
  let selected = $state<{ session: string; path: string } | null>(null)
  let theme = $state(localStorage.getItem('arrow.theme') ?? DEFAULT_THEME)
  let showWorktrees = $state(false)

  // Active tab [Audit | Sessions], persisted. Audit = the original view
  // (sidebar + diff); Sessions = resume/clean sessions (SessionsView).
  let view = $state<'audit' | 'sessions'>(
    localStorage.getItem('arrow.view') === 'sessions' ? 'sessions' : 'audit'
  )
  $effect(() => {
    localStorage.setItem('arrow.view', view)
  })

  // Audit-view repos: only sessions WITH edits (the parser now also reports
  // chat-only sessions, which belong to the Sessions tab).
  let auditList = $derived(report ? auditRepos(report.repos) : [])

  // Sessions-tab topbar counters, deduped: the raw report repeats a session
  // under every git root it edited from, but it is still ONE session.
  let dedupedSessions = $derived(report ? uniqueSessions(report.repos) : [])
  let sessionCount = $derived(dedupedSessions.length)
  let liveSessionCount = $derived(dedupedSessions.filter((s) => s.live).length)
  let sessionRepoCount = $derived(report ? sessionRepos(report.repos).length : 0)

  // Main repo roots the worktree inventory queries (one `git worktree list` each).
  // Deduped; with the git_root re-anchor these are already main roots, not worktrees.
  let mainRepoRoots = $derived([...new Set(auditList.map((r) => r.cwd))])

  // Reloj independiente: un tick periódico (ver onMount) reasigna `now` para que el
  // tiempo relativo ("Nm ago") y el punto verde envejezcan/caduquen aunque el report no
  // cambie. Display-only: NO refetcha ni reordena (el orden se ancla al timestamp del dato).
  let now = $state(Date.now())

  // Repos con punto verde = los del foco (sesión activa + ráfaga) con actividad reciente.
  let liveCount = $derived(
    focusReposOf(auditList, now).filter((r) => isLive(r.sessions[0]?.lastActivity, now)).length
  )

  // Zoom de la UI (estilo VSCode/terminal): Ctrl +/−/0. El estado vive aquí (reactivo);
  // applyZoom aísla el entorno (setZoom nativo en Tauri, CSS `zoom` en navegador).
  let zoomFactor = $state(loadZoom())
  let zoomPct = $derived(Math.round(zoomFactor * 100))

  function setZoom(n: number) {
    zoomFactor = clampZoom(n)
    applyZoom(zoomFactor)
  }

  function onKey(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey)) return
    if (e.key === '+' || e.key === '=') {
      e.preventDefault()
      setZoom(zoomFactor + ZOOM_STEP)
    } else if (e.key === '-' || e.key === '_') {
      e.preventDefault()
      setZoom(zoomFactor - ZOOM_STEP)
    } else if (e.key === '0') {
      e.preventDefault()
      setZoom(1)
    } else if (e.key === 'f' || e.key === 'F') {
      // Ctrl/Cmd+F: open the in-diff find panel. Routed through DiffView so it works even
      // when no editor column is focused yet. No-op (and lets the default through) if there
      // is no diff shown (or the Sessions tab is active — the diff is hidden there).
      if (view !== 'audit' || !content) return
      e.preventDefault()
      diffView?.openSearch()
    }
  }

  // Doble-click en la zona de arrastre = maximizar/restaurar. No está garantizado por
  // data-tauri-drag-region en WebKitGTK, así que lo cableamos explícitamente; solo
  // dispara sobre la superficie de arrastre (no sobre los botones de la barra).
  function onTitlebarDblClick(e: MouseEvent) {
    if (!inTauri) return
    if ((e.target as HTMLElement)?.hasAttribute('data-tauri-drag-region')) winToggleMaximize()
  }

  // Sidebar redimensionable + colapsable (estilo VSCode): arrastrar el divisor cambia el
  // ancho; doble-click o el botón de la topbar lo colapsa. Ancho y estado persistidos.
  const SIDEBAR_MIN = 180
  const SIDEBAR_MAX = 640
  function loadSidebarWidth(): number {
    const n = Number(localStorage.getItem('arrow.sidebarWidth'))
    return Number.isFinite(n) && n >= SIDEBAR_MIN && n <= SIDEBAR_MAX ? n : 340
  }
  let sidebarWidth = $state(loadSidebarWidth())
  let sidebarCollapsed = $state(localStorage.getItem('arrow.sidebarCollapsed') === 'true')
  let resizing = $state(false)
  let layoutEl: HTMLDivElement | undefined
  // Estado del arrastre. Umbral de 4px para distinguir un drag real de un click/doble-click
  // (un click nunca inicia un resize), y pointer capture para que el pointerup SIEMPRE llegue
  // aunque sueltes fuera del divisor → el arrastre no se "queda pegado".
  let armed = false
  let startX = 0
  let startWidth = 0

  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed
    localStorage.setItem('arrow.sidebarCollapsed', String(sidebarCollapsed))
  }
  function onResizeDown(e: PointerEvent) {
    if (e.button !== 0) return
    armed = true
    startX = e.clientX
    startWidth = sidebarCollapsed ? SIDEBAR_MIN : sidebarWidth
    // El pointer capture se libera solo en pointerup (no rompe el doble-click).
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  }
  function onResizeMove(e: PointerEvent) {
    if (!armed || !layoutEl) return
    const dx = e.clientX - startX
    if (!resizing) {
      if (Math.abs(dx) < 4) return // aún no es un drag: deja pasar el click/doble-click
      resizing = true
      sidebarCollapsed = false
    }
    sidebarWidth = Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, startWidth + dx))
  }
  function onResizeUp() {
    if (!armed) return
    armed = false
    if (resizing) {
      resizing = false
      localStorage.setItem('arrow.sidebarWidth', String(Math.round(sidebarWidth)))
    }
  }

  $effect(() => {
    localStorage.setItem('arrow.theme', theme)
  })

  // Whether the (session, path) still exists in a freshly fetched report. Used to decide if the
  // open diff can be refreshed in place — a session can age out or a file stop being touched.
  function fileStillInReport(r: Report, sel: { session: string; path: string }): boolean {
    return r.repos.some((repo) =>
      repo.sessions.some(
        (s) => s.sessionId === sel.session && s.files.some((f) => f.path === sel.path)
      )
    )
  }

  let lastJson = ''
  async function refresh(initial: boolean) {
    try {
      const r = await loadReport()
      const txt = JSON.stringify(r)
      if (txt === lastJson) return // nada cambió: no re-renderizar
      lastJson = txt
      // El report cambió (hubo ediciones nuevas): purgamos el cache de contenidos
      // para que los diffs no queden obsoletos. Las revisitas dentro del mismo
      // estado del report siguen siendo instantáneas.
      clearContentCache()
      report = r
      if (initial) {
        // Auto-open the top file ONLY when there is ACTIVE work (a recent edit). On an idle
        // launch — e.g. opening arrow the next day, not working on anything yet — we show
        // nothing as current (honest): the sidebar shows collapsed history instead. Anchored
        // to the data timestamp (isLive), not "the last thing arrow ever saw". Scoped to
        // the Audit repos (with edits): a chat-only session never auto-opens anything.
        const s = auditRepos(r.repos)[0]?.sessions[0]
        const f = s?.files[0]
        if (s && f && isLive(s.lastActivity)) select(s.sessionId, f.path)
      } else if (selected && fileStillInReport(r, selected)) {
        // Live refresh closes the loop: re-fetch the OPEN file's diff in place. Same selection
        // (no focus steal), and no blank flash — unlike select(), we keep the current diff shown
        // and swap only when the fresh content actually differs. clearContentCache() above makes
        // this fetch fresh; the equality guard means editing some OTHER file won't reset the
        // CodeMirror diff of the file we're looking at. Best-effort: a failed fetch leaves the
        // prior diff in place and the next refresh retries.
        try {
          const fresh = await loadContent(selected.path, selected.session)
          if (JSON.stringify(fresh) !== JSON.stringify(content)) content = fresh
        } catch {
          /* keep showing the prior diff; the next refresh will retry */
        }
      } else if (selected) {
        // The open file aged out of the report (its session/file is gone) → don't keep
        // showing a stale diff; drop the selection so the panel returns to a clean state.
        selected = null
        content = null
      }
    } catch (e) {
      error = String(e)
    }
  }

  onMount(() => {
    applyZoom(zoomFactor) // reaplica el zoom persistido (setZoom no sobrevive al reinicio)
    refresh(true)
    // Navegador: polling cada 5s. Tauri: el watcher nativo emite `report-changed`
    // y refrescamos al instante; dejamos un polling lento como backstop (honestidad:
    // si el watcher fallara, la UI no se queda congelada).
    const id = setInterval(() => refresh(false), inTauri ? 15000 : 5000)
    let unlisten: (() => void) | undefined
    if (inTauri) {
      listen('report-changed', () => refresh(false)).then((u) => (unlisten = u))
    }

    // Tick de reloj (30 s) independiente del report: hace avanzar el tiempo relativo y
    // caducar el punto verde sin esperar a una edición nueva. Es puramente de display
    // (reasigna `now`); no refetcha ni reordena. Se pausa con la ventana oculta para no
    // trabajar en segundo plano, y se pone al día al volver al foco.
    let clockId: ReturnType<typeof setInterval> | undefined
    const startClock = () => {
      if (clockId == null) clockId = setInterval(() => (now = Date.now()), 30000)
    }
    const stopClock = () => {
      if (clockId != null) {
        clearInterval(clockId)
        clockId = undefined
      }
    }
    const onVisibility = () => {
      if (document.hidden) stopClock()
      else {
        now = Date.now() // catch-up inmediato al volver
        startClock()
      }
    }
    startClock()
    document.addEventListener('visibilitychange', onVisibility)

    return () => {
      clearInterval(id)
      unlisten?.()
      stopClock()
      document.removeEventListener('visibilitychange', onVisibility)
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

<svelte:window onkeydown={onKey} />

<div class="app">
  <!-- Titlebar custom: el doble-click maximiza (convención de ventana, no control de teclado). -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="topbar" data-tauri-drag-region ondblclick={onTitlebarDblClick}>
    {#if view === 'audit'}
      <button class="icon-btn" onclick={toggleSidebar} title="Toggle sidebar" aria-label="Toggle sidebar">
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" stroke="currentColor" />
          <line x1="6" y1="2.5" x2="6" y2="13.5" stroke="currentColor" />
        </svg>
      </button>
    {/if}
    <span class="brand" data-tauri-drag-region>arrow</span>
    <span class="subtitle" data-tauri-drag-region>
      {view === 'audit' ? 'audit of Claude Code changes' : 'resume any session, anywhere'}
    </span>
    <nav class="tabs" aria-label="View">
      <button class="tab" class:on={view === 'audit'} onclick={() => (view = 'audit')}>Audit</button>
      <button class="tab" class:on={view === 'sessions'} onclick={() => (view = 'sessions')}>
        Sessions
      </button>
    </nav>
    <div class="actions">
      {#if view === 'sessions'}
        {#if liveSessionCount > 0}<span class="live">● {liveSessionCount}</span>{/if}
        {#if report}<span class="repos">{sessionRepoCount} repos · {sessionCount} sessions</span>{/if}
      {:else}
        {#if liveCount > 0}<span class="live">● {liveCount}</span>{/if}
        {#if report}<span class="repos">{auditList.length} repos</span>{/if}
      {/if}
      {#if inTauri && report}
        <button class="wtbtn" onclick={() => (showWorktrees = true)} title="Worktree inventory">
          <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <circle cx="4" cy="3.5" r="2" stroke="currentColor" stroke-width="1.4" />
            <circle cx="4" cy="12.5" r="2" stroke="currentColor" stroke-width="1.4" />
            <circle cx="12" cy="3.5" r="2" stroke="currentColor" stroke-width="1.4" />
            <path d="M4 5.5v5M4 9.5c0-4 8-2 8-4" stroke="currentColor" stroke-width="1.4" />
          </svg>
          Worktrees
        </button>
      {/if}
      <div class="zoom">
        <button class="zbtn" onclick={() => setZoom(zoomFactor - ZOOM_STEP)} title="Zoom out (Ctrl −)" aria-label="Zoom out">−</button>
        <button class="zpct" onclick={() => setZoom(1)} title="Reset zoom (Ctrl 0)" aria-label="Reset zoom">{zoomPct}%</button>
        <button class="zbtn" onclick={() => setZoom(zoomFactor + ZOOM_STEP)} title="Zoom in (Ctrl +)" aria-label="Zoom in">+</button>
      </div>
      <ThemeMenu current={theme} onSelect={(id) => (theme = id)} />
      <VersionBadge />
      <WindowControls />
    </div>
  </header>

  <!-- Both views stay mounted (hidden via CSS, like the mockup): the open diff,
       scroll position and toggles survive switching tabs. -->
  <div class="layout" class:resizing class:hide={view !== 'audit'} bind:this={layoutEl}>
    <aside
      class="sidebar"
      class:collapsed={sidebarCollapsed}
      style:width={sidebarCollapsed ? '0px' : sidebarWidth + 'px'}
    >
      {#if error}
        <div class="error">{error}</div>
      {/if}
      {#if report}
        <Sidebar {report} {selected} {now} onSelect={select} />
      {:else if !error}
        <div class="loading">Loading sessions…</div>
      {/if}
    </aside>

    <!-- Divisor arrastrable: ancho con el mouse; doble-click colapsa/expande. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="resizer"
      title="Drag to resize · double-click to collapse"
      onpointerdown={onResizeDown}
      onpointermove={onResizeMove}
      onpointerup={onResizeUp}
      ondblclick={toggleSidebar}
    ></div>

    <main class="main">
      {#if selected}
        <div class="filebar">
          <span class="filepath">{selected.path}</span>
          {#if content?.userModified}
            <span class="warn">⚠ modified outside Claude</span>
          {/if}
          <OpenInEditor
            path={selected.path}
            line={content?.firstChangedLine ?? null}
            afterAvailable={content?.afterAvailable === true}
          />
        </div>
      {/if}
      <div class="diff-area">
        <DiffView bind:this={diffView} {content} loading={loadingContent} themeId={theme} />
      </div>
    </main>
  </div>

  <!-- Sessions tab: repos → sessions to resume or clean up (SessionsView). -->
  <div class="viewwrap" class:hide={view !== 'sessions'}>
    {#if error}
      <div class="error">{error}</div>
    {/if}
    {#if report}
      <SessionsView {report} {now} onRefresh={() => refresh(false)} />
    {:else if !error}
      <div class="loading">Loading sessions…</div>
    {/if}
  </div>

  {#if showWorktrees && report}
    <WorktreesModal
      {report}
      repoRoots={mainRepoRoots}
      {now}
      onClose={() => (showWorktrees = false)}
    />
  {/if}
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    /* Sin decoraciones nativas (decorations:false) GTK no garantiza sombra/borde:
       un borde sutil separa la ventana del escritorio. */
    border: 1px solid var(--border);
  }
  .topbar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
    flex: none;
    /* La barra completa es la zona de arrastre de la ventana sin titlebar nativa. */
    cursor: default;
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .brand {
    font-weight: 700;
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }
  .subtitle {
    color: var(--dim);
    font-size: 12px;
  }
  /* Switcher segmentado [Audit | Sessions] (mockup) */
  .tabs {
    display: inline-flex;
    margin-left: 4px;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    background: var(--chip);
  }
  .tab {
    border: none;
    background: transparent;
    color: var(--dim);
    font: inherit;
    font-size: 12px;
    padding: 3px 12px;
    cursor: pointer;
  }
  .tab:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .tab.on {
    background: var(--active);
    color: var(--fg);
  }
  .tab:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: 1px;
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
  .wtbtn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: var(--chip);
    color: var(--fg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 9px;
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .wtbtn:hover {
    background: var(--hover);
  }
  .wtbtn svg {
    color: var(--dim);
  }
  .zoom {
    display: inline-flex;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    background: var(--chip);
  }
  .zbtn,
  .zpct {
    border: none;
    background: transparent;
    color: var(--fg);
    font: inherit;
    cursor: pointer;
  }
  .zbtn {
    width: 22px;
    padding: 1px 0;
    font-size: 14px;
    line-height: 1;
    color: var(--dim);
  }
  .zbtn:hover,
  .zpct:hover {
    background: var(--hover);
  }
  .zpct {
    min-width: 40px;
    padding: 2px 4px;
    font-size: 11px;
    color: var(--dim);
    text-align: center;
    border-left: 1px solid var(--border);
    border-right: 1px solid var(--border);
  }
  .layout {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  /* La pestaña inactiva se oculta sin desmontarse (conserva su estado). */
  .layout.hide,
  .viewwrap.hide {
    display: none;
  }
  /* Contenedor scrolleable de la pestaña Sessions (mockup .viewwrap). */
  .viewwrap {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 12px 16px 0;
  }
  .sidebar {
    flex: none;
    background: var(--panel);
    overflow: auto;
    padding: 8px;
    transition: width 0.12s ease;
  }
  .sidebar.collapsed {
    padding: 0;
    overflow: hidden;
  }
  /* El divisor arrastrable hace de borde entre sidebar y diff. */
  .resizer {
    flex: none;
    width: 6px;
    cursor: col-resize;
    border-left: 1px solid var(--border);
    background: transparent;
  }
  .resizer:hover,
  .layout.resizing .resizer {
    border-left-color: var(--accent);
    background: var(--active);
  }
  /* Durante el arrastre: sin transición (sigue al cursor) y sin interacción con el diff. */
  .layout.resizing {
    cursor: col-resize;
    user-select: none;
  }
  .layout.resizing .sidebar {
    transition: none;
  }
  .layout.resizing .main {
    pointer-events: none;
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
