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

  <div class="layout">
    <aside class="sidebar">
      {#if error}
        <div class="error">{error}</div>
      {/if}
      {#if report}
        <Sidebar {report} {selected} onSelect={select} />
      {:else if !error}
        <div class="loading">Loading sessions…</div>
      {/if}
    </aside>

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
