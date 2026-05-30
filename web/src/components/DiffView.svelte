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
  let mv: MergeView | null = null

  const theme = EditorView.theme(
    {
      '&': { backgroundColor: 'transparent', color: 'var(--fg)' },
      '.cm-scroller': {
        fontFamily: 'var(--mono)',
        fontSize: '12.5px',
        lineHeight: '1.5',
      },
      '.cm-gutters': {
        backgroundColor: 'transparent',
        border: 'none',
        color: 'var(--dim)',
      },
      '.cm-activeLineGutter': { backgroundColor: 'transparent' },
    },
    { dark: true }
  )

  function side(doc: string) {
    return {
      doc,
      extensions: [
        lineNumbers(),
        EditorView.editable.of(false), // Fase 1: solo lectura. La edición llega en Fase 4.
        EditorState.readOnly.of(true),
        EditorView.lineWrapping,
        theme,
      ],
    }
  }

  function dispose() {
    if (mv) {
      mv.destroy()
      mv = null
    }
  }

  $effect(() => {
    // Se re-ejecuta al cambiar `content` o al montarse `host`.
    const c = content
    dispose()
    if (host && c) {
      mv = new MergeView({
        a: side(c.before),
        b: side(c.after),
        parent: host,
        collapseUnchanged: { margin: 3, minSize: 4 },
      })
    }
    return dispose
  })
</script>

<div class="diff-wrap">
  <div class="diff-host" bind:this={host}></div>
  {#if loading}
    <div class="status">Cargando diff…</div>
  {:else if !content}
    <div class="status">Selecciona un archivo en la barra lateral para ver su diff.</div>
  {:else if !content.beforeAvailable}
    <div class="status">Sin estado previo registrado para este archivo en esta sesión.</div>
  {/if}
</div>

<style>
  .diff-wrap {
    position: relative;
    height: 100%;
    overflow: auto;
  }
  .diff-host {
    height: 100%;
  }
  .diff-host :global(.cm-mergeView) {
    height: 100%;
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
