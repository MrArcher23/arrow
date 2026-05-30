<script lang="ts">
  import { MergeView } from '@codemirror/merge'
  import { EditorView, lineNumbers } from '@codemirror/view'
  import { EditorState } from '@codemirror/state'
  import type { FileContent } from '../lib/types'

  interface Props {
    content: FileContent | null
    loading: boolean
  }
  let { content, loading }: Props = $props()

  let host = $state<HTMLDivElement>()
  let view: { destroy(): void } | null = null

  const theme = EditorView.theme(
    {
      '&': { backgroundColor: 'transparent', color: 'var(--fg)' },
      '.cm-scroller': { fontFamily: 'var(--mono)', fontSize: '12.5px', lineHeight: '1.5' },
      '.cm-gutters': { backgroundColor: 'transparent', border: 'none', color: 'var(--dim)' },
      '.cm-activeLineGutter': { backgroundColor: 'transparent' },
    },
    { dark: true }
  )

  function exts() {
    return [
      lineNumbers(),
      EditorView.editable.of(false), // Fase 1: solo lectura. La edición llega en Fase 4.
      EditorState.readOnly.of(true),
      EditorView.lineWrapping,
      theme,
    ]
  }

  // Clasifica el cambio para no partir la pantalla cuando no hay nada que comparar.
  function classify(c: FileContent | null): 'none' | 'created' | 'deleted' | 'diff' {
    if (!c) return 'none'
    const b = c.before ?? ''
    const a = c.after ?? ''
    if (a === '' && b !== '') return 'deleted'
    if (b === '') return 'created' // archivo nuevo (sin "antes" que comparar)
    return 'diff'
  }

  let mode = $derived(classify(content))

  function dispose() {
    if (view) {
      view.destroy()
      view = null
    }
  }

  $effect(() => {
    const c = content
    dispose()
    if (host && c) {
      const m = classify(c)
      if (m === 'diff') {
        view = new MergeView({
          a: { doc: c.before, extensions: exts() },
          b: { doc: c.after, extensions: exts() },
          parent: host,
          collapseUnchanged: { margin: 3, minSize: 4 },
        })
      } else {
        // Un solo panel: nuevo (after) o eliminado (before).
        view = new EditorView({
          doc: m === 'deleted' ? c.before : c.after,
          extensions: exts(),
          parent: host,
        })
      }
    }
    return dispose
  })
</script>

<div class="diff-wrap">
  {#if mode === 'created'}
    <div class="banner created">＋ nuevo archivo</div>
  {:else if mode === 'deleted'}
    <div class="banner deleted">－ archivo eliminado</div>
  {/if}

  <div class="diff-host" class:single={mode === 'created' || mode === 'deleted'} bind:this={host}></div>

  {#if loading}
    <div class="status">Cargando diff…</div>
  {:else if !content}
    <div class="status">Selecciona un archivo en la barra lateral para ver su diff.</div>
  {:else if mode === 'diff' && !content.beforeAvailable}
    <div class="status">Sin estado previo registrado para este archivo en esta sesión.</div>
  {/if}
</div>

<style>
  .diff-wrap {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }
  .banner {
    flex: none;
    font-size: 11px;
    padding: 4px 14px;
    border-bottom: 1px solid var(--border);
  }
  .banner.created {
    color: var(--green);
    background: #3fb9500f;
  }
  .banner.deleted {
    color: var(--red);
    background: #f851490f;
  }
  .diff-host {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .diff-host :global(.cm-mergeView),
  .diff-host :global(.cm-editor) {
    height: 100%;
  }
  .diff-host.single :global(.cm-gutters) {
    border-right: 2px solid #3fb95044;
  }
  .status {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--dim);
    font-size: 13px;
    pointer-events: none;
  }
</style>
