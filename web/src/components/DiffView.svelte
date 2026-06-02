<script lang="ts">
  import { tick } from 'svelte'
  import { MergeView } from '@codemirror/merge'
  import { EditorView, lineNumbers, Decoration } from '@codemirror/view'
  import type { DecorationSet } from '@codemirror/view'
  import { search, setSearchQuery, findNext, findPrevious, SearchQuery } from '@codemirror/search'
  import { EditorState, StateField, StateEffect } from '@codemirror/state'
  import type { Extension, Range } from '@codemirror/state'
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

  // Floating find bar. We deliberately do NOT use CodeMirror's docked search panel: in the
  // merge view the panel anchors to the bottom of the full-document-height editor, so opening
  // it jumped to the end of the file and the close button ended up under the scrollbar. Instead
  // we render our own overlay and drive @codemirror/search through its API.
  let searchOpen = $state(false)
  let queryText = $state('')
  let caseSensitive = $state(false)
  let useRegexp = $state(false)
  let wholeWord = $state(false)
  let matchCount = $state(0)
  let queryValid = $state(true)
  let findInput = $state<HTMLInputElement>()

  // Our own match highlighter. CodeMirror's built-in search only paints matches while its
  // docked panel is open (which we don't use), so we decorate matches ourselves and style
  // them with .cm-arrow-match. The decorations are pushed per editor by applyQuery().
  const setMatchDeco = StateEffect.define<DecorationSet>()
  const matchField = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(value, tr) {
      value = value.map(tr.changes)
      for (const e of tr.effects) if (e.is(setMatchDeco)) value = e.value
      return value
    },
    provide: (f) => EditorView.decorations.from(f),
  })
  const matchMark = Decoration.mark({ class: 'cm-arrow-match' })

  function exts(lang: Extension | null): Extension[] {
    const e: Extension[] = [
      lineNumbers(),
      EditorView.editable.of(false), // Fase 1: solo lectura. La edición llega en Fase 4.
      EditorState.readOnly.of(true),
      EditorView.lineWrapping,
      themeExt(themeId),
      // Search state (drives findNext/findPrevious) + our own match-highlight field. We never
      // open CodeMirror's panel; we drive it via setSearchQuery / findNext and paint matches
      // ourselves (matchField), because CM only highlights while its panel is open.
      search(),
      matchField,
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

  // The merge view exposes its two editors as `.a`/`.b`; single-file mode is the view itself.
  function editorViews(): EditorView[] {
    if (!view) return []
    const mv = view as unknown as { a?: EditorView; b?: EditorView }
    return mv.a && mv.b ? [mv.a, mv.b] : [view as unknown as EditorView]
  }

  // Editor that next/prev act on: the focused column, otherwise the right one ("after").
  function activeEditor(): EditorView | undefined {
    const eds = editorViews()
    return eds.find((ed) => ed.hasFocus) ?? eds[eds.length - 1]
  }

  // Push the current query to BOTH columns (so both highlight their matches) and recompute
  // the match count. Setting the query does not move the selection, so opening the bar never
  // scrolls the document — navigation happens only on next/prev.
  function applyQuery() {
    const eds = editorViews()
    if (!eds.length) return
    const q = new SearchQuery({ search: queryText, caseSensitive, regexp: useRegexp, wholeWord })
    queryValid = queryText === '' || q.valid
    let count = 0
    eds.forEach((ed, i) => {
      // Walk the query cursor over THIS column's doc: collect highlight ranges and, for the
      // right ("after") column, the match count shown in the bar.
      const ranges: Range<Decoration>[] = []
      if (q.valid) {
        const cur = q.getCursor(ed.state)
        for (let r = cur.next(); !r.done; r = cur.next()) {
          if (r.value.from < r.value.to) ranges.push(matchMark.range(r.value.from, r.value.to))
        }
      }
      ed.dispatch({ effects: [setSearchQuery.of(q), setMatchDeco.of(Decoration.set(ranges, true))] })
      if (i === eds.length - 1) count = ranges.length
    })
    matchCount = count
  }

  // Opens the floating find bar (the global Ctrl+F in App.svelte calls this, so it works even
  // before a column is clicked). Focuses the input and preselects any existing query text.
  export async function openSearch() {
    if (!view) return
    searchOpen = true
    await tick()
    findInput?.focus()
    findInput?.select()
    if (queryText) applyQuery()
  }

  function closeSearch() {
    searchOpen = false
    const empty = new SearchQuery({ search: '' })
    for (const ed of editorViews())
      ed.dispatch({ effects: [setSearchQuery.of(empty), setMatchDeco.of(Decoration.none)] })
    matchCount = 0
    activeEditor()?.focus()
  }

  // findNext/findPrevious move the selection and scroll the match into view; they don't need
  // the editor focused, so the find input keeps focus while you cycle matches.
  function findNextMatch() {
    const ed = activeEditor()
    if (ed && queryText) findNext(ed)
  }
  function findPrevMatch() {
    const ed = activeEditor()
    if (ed && queryText) findPrevious(ed)
  }

  function onFindKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault()
      if (e.shiftKey) findPrevMatch()
      else findNextMatch()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      closeSearch()
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

  // Re-run the query whenever the text or any toggle changes (only while the bar is open).
  $effect(() => {
    queryText
    caseSensitive
    useRegexp
    wholeWord
    if (searchOpen) applyQuery()
  })

  // Switching file or theme rebuilds the editors, so close the find bar to avoid acting on a
  // stale view; the user reopens with Ctrl+F on the new diff.
  $effect(() => {
    content
    themeId
    searchOpen = false
  })
</script>

<div class="diff-wrap">
  {#if mode === 'created'}
    <div class="banner created">＋ new file</div>
  {:else if mode === 'deleted'}
    <div class="banner deleted">－ deleted file</div>
  {/if}

  {#if searchOpen}
    <!-- Floating find bar overlaid on the diff (Ctrl+F). Anchored to the panel, not the
         document, so it stays put while scrolling and never hides under the scrollbar. -->
    <div class="find-bar" role="search">
      <input
        class="find-input"
        class:invalid={!queryValid}
        type="text"
        placeholder="Find in diff"
        spellcheck="false"
        autocomplete="off"
        bind:value={queryText}
        bind:this={findInput}
        onkeydown={onFindKey}
      />
      <span class="find-count">
        {#if queryText === ''}{:else if !queryValid}bad pattern{:else if matchCount === 0}no results{:else}{matchCount} match{matchCount === 1 ? '' : 'es'}{/if}
      </span>
      <button class="find-btn" onclick={findPrevMatch} disabled={!matchCount} title="Previous match (Shift+Enter)" aria-label="Previous match">↑</button>
      <button class="find-btn" onclick={findNextMatch} disabled={!matchCount} title="Next match (Enter)" aria-label="Next match">↓</button>
      <button class="find-btn" class:on={caseSensitive} onclick={() => (caseSensitive = !caseSensitive)} title="Match case" aria-label="Match case">Aa</button>
      <button class="find-btn" class:on={useRegexp} onclick={() => (useRegexp = !useRegexp)} title="Regular expression" aria-label="Use regular expression">.*</button>
      <button class="find-btn" class:on={wholeWord} onclick={() => (wholeWord = !wholeWord)} title="Whole word" aria-label="Whole word">W</button>
      <button class="find-btn close" onclick={closeSearch} title="Close (Esc)" aria-label="Close search">✕</button>
    </div>
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
  /* Height chain for the diff. @codemirror/merge makes .cm-mergeView the scroll container
     (its baseTheme sets `.cm-mergeView { overflow-y: auto }` and forces the inner editors to
     auto height with `overflow-y: visible !important`). So we cap .cm-mergeView to the panel
     and let the editors row grow past it. Using `min-height` (not `height`) on
     .cm-mergeViewEditors is the key: short diffs still fill the panel (no empty gap below),
     while long diffs grow taller than the panel and .cm-mergeView scrolls them. A hard
     `height: 100%` here capped the row, and the per-column `.cm-mergeViewEditor { overflow:
     hidden }` clipped anything past the fold — which is why scrolling was dead. */
  .diff-host :global(.cm-mergeView),
  .diff-host :global(.cm-editor) {
    height: 100%;
  }
  .diff-host :global(.cm-mergeViewEditors) {
    min-height: 100%;
  }
  /* Single-editor mode (new/deleted file) is a plain EditorView, not a merge view, so the
     merge baseTheme above doesn't apply. CodeMirror's baseTheme only gives .cm-scroller
     `overflow-x`, so enable vertical scroll explicitly for that case. */
  .diff-host :global(.cm-scroller) {
    overflow: auto;
  }
  /* Search match highlight. We paint these ourselves (CodeMirror only highlights while its
     own panel is open, which we don't use). Semi-transparent amber so the syntax colors and
     the current-match selection stay readable on both dark and light themes. */
  .diff-host :global(.cm-arrow-match) {
    background: #d2992255;
    border-radius: 2px;
    box-shadow: 0 0 0 1px #d2992288;
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

  /* Floating find bar, anchored to the panel's top-right (clear of the vertical scrollbar). */
  .find-bar {
    position: absolute;
    top: 8px;
    right: 16px;
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 4px 5px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.4);
    font-size: 12px;
  }
  .find-input {
    width: 190px;
    padding: 3px 7px;
    background: var(--bg);
    color: var(--fg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font: inherit;
    outline: none;
  }
  .find-input:focus {
    border-color: var(--accent);
  }
  .find-input.invalid {
    border-color: var(--red);
  }
  .find-count {
    min-width: 68px;
    padding: 0 4px;
    color: var(--dim);
    font-size: 11px;
    white-space: nowrap;
  }
  .find-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 22px;
    height: 22px;
    padding: 0 4px;
    background: transparent;
    color: var(--dim);
    border: 1px solid transparent;
    border-radius: 4px;
    font-size: 12px;
    line-height: 1;
    cursor: pointer;
  }
  .find-btn:hover:not(:disabled) {
    background: var(--hover);
    color: var(--fg);
  }
  .find-btn.on {
    color: var(--accent);
    border-color: var(--accent);
  }
  .find-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .find-btn.close {
    font-size: 13px;
  }
</style>
