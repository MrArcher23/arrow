<script lang="ts">
  import { THEMES } from '../lib/themes'

  interface Props {
    current: string
    onSelect: (id: string) => void
  }
  let { current, onSelect }: Props = $props()

  let open = $state(false)
  let root = $state<HTMLDivElement>()

  function choose(id: string) {
    onSelect(id)
    open = false
  }
  function onWindowClick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') open = false
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onKey} />

<div class="menu" bind:this={root}>
  <button class="btn" onclick={() => (open = !open)} aria-haspopup="menu" aria-expanded={open}>
    Themes <span class="caret">▾</span>
  </button>

  {#if open}
    <div class="popover" role="menu">
      {#each THEMES as t}
        <button class="item" class:active={t.id === current} role="menuitem" onclick={() => choose(t.id)}>
          <span class="check">{t.id === current ? '✓' : ''}</span>
          <span class="label">{t.label}</span>
          <span class="tag">{t.dark ? 'dark' : 'light'}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .menu {
    position: relative;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--chip);
    color: var(--fg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 9px;
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .btn:hover {
    background: var(--hover);
  }
  .caret {
    color: var(--dim);
    font-size: 10px;
  }
  .popover {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 50;
    min-width: 200px;
    max-height: 340px;
    overflow: auto;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 4px;
    box-shadow: var(--shadow-lg);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    border: none;
    background: transparent;
    color: var(--fg);
    text-align: left;
    padding: 6px 8px;
    border-radius: 5px;
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  .item:hover {
    background: var(--hover);
  }
  .item.active {
    background: var(--active);
  }
  .check {
    width: 12px;
    color: var(--green);
    flex: none;
  }
  .label {
    flex: 1;
    white-space: nowrap;
  }
  .tag {
    font-size: 10px;
    color: var(--dim);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 6px;
    flex: none;
  }
</style>
