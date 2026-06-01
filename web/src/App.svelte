<script lang="ts">
  import { onMount } from 'svelte'
  import Sidebar from './components/Sidebar.svelte'
  import DiffView from './components/DiffView.svelte'
  import ThemeMenu from './components/ThemeMenu.svelte'
  import WindowControls from './components/WindowControls.svelte'
  import { listen } from '@tauri-apps/api/event'
  import { loadReport, loadContent, clearContentCache, inTauri } from './lib/api'
  import { loadZoom, applyZoom, clampZoom, ZOOM_STEP } from './lib/zoom'
  import { winToggleMaximize } from './lib/window'
  import { isLive, focusRepos as focusReposOf } from './lib/time'
  import { DEFAULT_THEME } from './lib/themes'
  import type { Report, FileContent } from './lib/types'

  let report = $state<Report | null>(null)
  let error = $state<string | null>(null)
  let content = $state<FileContent | null>(null)
  let loadingContent = $state(false)
  let selected = $state<{ session: string; path: string } | null>(null)
  let theme = $state(localStorage.getItem('arrow.theme') ?? DEFAULT_THEME)

  // Repos con punto verde = los del foco (sesión activa + ráfaga) con actividad reciente.
  let liveCount = $derived(
    report ? focusReposOf(report.repos).filter((r) => isLive(r.sessions[0]?.lastActivity)).length : 0
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
        // Solo en la primera carga auto-seleccionamos; los refrescos NO roban foco.
        const s = r.repos[0]?.sessions[0]
        const f = s?.files[0]
        if (s && f) select(s.sessionId, f.path)
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
    return () => {
      clearInterval(id)
      unlisten?.()
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
    <button class="icon-btn" onclick={toggleSidebar} title="Toggle sidebar" aria-label="Toggle sidebar">
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
        <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" stroke="currentColor" />
        <line x1="6" y1="2.5" x2="6" y2="13.5" stroke="currentColor" />
      </svg>
    </button>
    <span class="brand" data-tauri-drag-region>arrow</span>
    <span class="subtitle" data-tauri-drag-region>audit of Claude Code changes</span>
    <div class="actions">
      {#if liveCount > 0}<span class="live">● {liveCount}</span>{/if}
      {#if report}<span class="repos">{report.repoCount} repos</span>{/if}
      <div class="zoom">
        <button class="zbtn" onclick={() => setZoom(zoomFactor - ZOOM_STEP)} title="Zoom out (Ctrl −)" aria-label="Zoom out">−</button>
        <button class="zpct" onclick={() => setZoom(1)} title="Reset zoom (Ctrl 0)" aria-label="Reset zoom">{zoomPct}%</button>
        <button class="zbtn" onclick={() => setZoom(zoomFactor + ZOOM_STEP)} title="Zoom in (Ctrl +)" aria-label="Zoom in">+</button>
      </div>
      <ThemeMenu current={theme} onSelect={(id) => (theme = id)} />
      <WindowControls />
    </div>
  </header>

  <div class="layout" class:resizing bind:this={layoutEl}>
    <aside
      class="sidebar"
      class:collapsed={sidebarCollapsed}
      style:width={sidebarCollapsed ? '0px' : sidebarWidth + 'px'}
    >
      {#if error}
        <div class="error">{error}</div>
      {/if}
      {#if report}
        <Sidebar {report} {selected} onSelect={select} />
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
