<script lang="ts">
  import { onMount } from 'svelte'
  import { detectEditors, openInEditor } from '../lib/api'
  import type { Editor } from '../lib/types'

  interface Props {
    path: string
    line: number | null // 1-based first changed line; null → open at the top
    afterAvailable: boolean // false when the file is gone on disk → disable opening (honest)
  }
  let { path, line, afterAvailable }: Props = $props()

  // Editors are detected once (they don't change per file). Renders nothing when
  // none are found or in the browser (detectEditors() returns [] outside Tauri).
  let editors = $state<Editor[]>([])
  let chosen = $state('')
  let err = $state<string | null>(null)

  // Clear a stale error when the file changes: a previous file's failure ⚠ must
  // never carry over to (and lie about) the newly selected file.
  $effect(() => {
    path
    err = null
  })

  onMount(async () => {
    try {
      editors = await detectEditors()
      const saved = localStorage.getItem('arrow.editor')
      chosen = editors.some((e) => e.id === saved) ? saved! : (editors[0]?.id ?? '')
    } catch {
      editors = []
    }
  })

  async function open() {
    if (!chosen || !afterAvailable) return
    err = null
    try {
      await openInEditor(chosen, path, line ?? 1)
    } catch (e) {
      err = String(e)
    }
  }
</script>

{#if editors.length}
  <div class="oie">
    {#if editors.length > 1}
      <select
        class="pick"
        bind:value={chosen}
        onchange={() => localStorage.setItem('arrow.editor', chosen)}
        title="Choose editor"
      >
        {#each editors as ed}
          <option value={ed.id}>{ed.name}</option>
        {/each}
      </select>
    {/if}
    <button
      class="open"
      onclick={open}
      disabled={!afterAvailable}
      title={afterAvailable
        ? `Open the current file on disk near the first change (line ${line ?? 1})`
        : 'File no longer on disk'}
    >
      Open in editor ↗
    </button>
    {#if err}<span class="err" role="img" aria-label={`Open failed: ${err}`} aria-live="polite" title={err}>⚠</span>{/if}
  </div>
{/if}

<style>
  .oie {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
  }
  .pick {
    font: inherit;
    font-size: 11px;
    color: var(--fg);
    background: var(--chip);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 2px 4px;
    max-width: 140px;
  }
  .open {
    font: inherit;
    font-size: 11px;
    color: var(--dim);
    background: var(--chip);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 8px;
    cursor: pointer;
    white-space: nowrap;
  }
  .open:hover:not(:disabled) {
    background: var(--hover);
    color: var(--fg);
    border-color: var(--accent);
  }
  .open:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .err {
    color: var(--warn);
    cursor: help;
  }
</style>
