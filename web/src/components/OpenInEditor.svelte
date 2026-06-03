<script lang="ts">
  import { onMount } from 'svelte'
  import { detectEditors, openInEditor } from '../lib/api'
  import type { Editor } from '../lib/types'

  interface Props {
    path: string
    line: number | null // 1-based first changed line; null → open at the top
  }
  let { path, line }: Props = $props()

  // Editors are detected once (they don't change per file). Renders nothing when
  // none are found or in the browser (detectEditors() returns [] outside Tauri).
  let editors = $state<Editor[]>([])
  let chosen = $state('')
  let err = $state<string | null>(null)

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
    if (!chosen) return
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
    <button class="open" onclick={open} title={`Open this file in the editor at line ${line ?? 1}`}>
      Open in editor ↗
    </button>
    {#if err}<span class="err" title={err}>⚠</span>{/if}
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
  .open:hover {
    background: var(--hover);
    color: var(--fg);
    border-color: var(--accent);
  }
  .err {
    color: var(--warn);
    cursor: help;
  }
</style>
