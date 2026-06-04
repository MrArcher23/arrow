<script lang="ts">
  import { appVersion, RELEASES_URL } from '../lib/version'

  let open = $state(false)
  let root = $state<HTMLDivElement>()
  let copied = $state(false)

  function onWindowClick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') open = false
  }
  async function copyLink() {
    try {
      await navigator.clipboard.writeText(RELEASES_URL)
      copied = true
      setTimeout(() => (copied = false), 1200)
    } catch {
      /* clipboard unavailable: ignore */
    }
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onKey} />

<div class="ver" bind:this={root}>
  <button class="badge" onclick={() => (open = !open)} aria-haspopup="dialog" aria-expanded={open} title="About arrow">
    v{appVersion}
  </button>

  {#if open}
    <div class="popover" role="dialog" aria-label="About arrow">
      <div class="title">arrow <span class="v">v{appVersion}</span></div>
      <div class="sub">Audit viewer for Claude Code</div>
      <div class="link">
        <span class="url" title={RELEASES_URL}>{RELEASES_URL}</span>
        <button class="copy" onclick={copyLink}>{copied ? 'copied!' : 'copy'}</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .ver {
    position: relative;
  }
  .badge {
    font: inherit;
    font-size: 11px;
    color: var(--dim);
    background: var(--chip);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px 9px;
    cursor: pointer;
  }
  .badge:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .popover {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 50;
    min-width: 260px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
  }
  .title {
    font-weight: 700;
    letter-spacing: 0.5px;
    font-size: 14px;
  }
  .title .v {
    color: var(--dim);
    font-weight: 400;
  }
  .sub {
    color: var(--dim);
    font-size: 12px;
    margin-top: 2px;
  }
  .link {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
  }
  .url {
    font-family: var(--mono);
    font-size: 10px;
    color: var(--dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
  }
  .copy {
    font: inherit;
    font-size: 11px;
    color: var(--dim);
    background: var(--chip);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 2px 8px;
    cursor: pointer;
    flex: none;
  }
  .copy:hover {
    background: var(--hover);
    color: var(--fg);
  }
</style>
