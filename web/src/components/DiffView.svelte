<script lang="ts">
  import { MergeView } from '@codemirror/merge'
  import { EditorView, lineNumbers } from '@codemirror/view'
  import { EditorState } from '@codemirror/state'
  import type { Extension } from '@codemirror/state'
  import type { FileContent } from '../lib/types'
  import { themeExt } from '../lib/themes'
  import { resolveLanguage } from '../lib/highlight'

  interface Props {
    content: FileContent | null
    loading: boolean
    themeId: string
  }
  let { content, loading, themeId }: Props = $props()

  let host = $state<HTMLDivElement>()
  let view: { destroy(): void } | null = null
  let seq = 0

  function exts(lang: Extension | null): Extension[] {
    const e: Extension[] = [
      lineNumbers(),
      EditorView.editable.of(false), // Fase 1: solo lectura. La edición llega en Fase 4.
      EditorState.readOnly.of(true),
      EditorView.lineWrapping,
      themeExt(themeId),
    ]
    if (lang) e.push(lang)
    return e
  }

  // Clasifica el cambio para no partir la pantalla cuando no hay nada que comparar.
  function classify(c: FileContent | null): 'none' | 'created' | 'deleted' | 'diff' {
    if (!c) return 'none'
    const b = c.before ?? ''
    const a = c.after ?? ''
    if (a === '' && b !== '') return 'deleted'
    if (b === '') return 'created'
    return 'diff'
  }

  let mode = $derived(classify(content))

  function dispose() {
    if (view) {
      view.destroy()
      view = null
    }
  }

  async function build(c: FileContent) {
    const my = ++seq
    const lang = await resolveLanguage(c.file) // carga perezosa del parser
    if (my !== seq || !host) return // descartar si llegó otro cambio mientras cargaba
    dispose()
    const m = classify(c)
    if (m === 'diff') {
      view = new MergeView({
        a: { doc: c.before, extensions: exts(lang) },
        b: { doc: c.after, extensions: exts(lang) },
        parent: host,
        collapseUnchanged: { margin: 3, minSize: 4 },
      })
    } else {
      view = new EditorView({
        doc: m === 'deleted' ? c.before : c.after,
        extensions: exts(lang),
        parent: host,
      })
    }
  }

  $effect(() => {
    const c = content
    themeId // dependencia: recrear al cambiar de tema
    if (host && c) {
      build(c)
    } else {
      dispose()
    }
    return dispose
  })
</script>

<div class="diff-wrap">
  {#if mode === 'created'}
    <div class="banner created">＋ new file</div>
  {:else if mode === 'deleted'}
    <div class="banner deleted">－ deleted file</div>
  {/if}

  <div class="diff-host" class:single={mode === 'created' || mode === 'deleted'} bind:this={host}></div>

  {#if loading}
    <div class="status">Loading diff…</div>
  {:else if !content}
    <div class="status">Select a file in the sidebar to view its diff.</div>
  {:else if mode === 'diff' && !content.beforeAvailable}
    <div class="status">No previous state recorded for this file in this session.</div>
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
  /* Cadena de alturas completa: el wrapper flex intermedio (.cm-mergeViewEditors)
     también necesita height:100% o los editores se quedan al alto del contenido y
     dejan un hueco vacío bajo los diffs cortos. */
  .diff-host :global(.cm-mergeView),
  .diff-host :global(.cm-mergeViewEditors),
  .diff-host :global(.cm-editor) {
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
