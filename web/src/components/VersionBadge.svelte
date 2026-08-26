<script lang="ts">
  import { onMount } from 'svelte'
  import { appVersion, RELEASES_URL } from '../lib/version'
  import { checkUpdate, openUrl } from '../lib/api'
  import type { UpdateStatus } from '../lib/types'

  let open = $state(false)
  let root = $state<HTMLDivElement>()
  let copied = $state(false)
  // Release-update check (Tauri-only): runs once on mount, off the render path.
  // `null` until it resolves or in the browser dev build (no app to update).
  let upd = $state<UpdateStatus | null>(null)
  let hasUpdate = $derived(upd?.updateAvailable === true)
  let releaseUrl = $derived(upd?.url ?? RELEASES_URL)

  onMount(async () => {
    try {
      upd = await checkUpdate()
    } catch {
      /* offline / rate-limited: stay silent, the badge just shows the version */
    }
  })

  function onWindowClick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') open = false
  }
  async function copyLink() {
    try {
      await navigator.clipboard.writeText(releaseUrl)
      copied = true
      setTimeout(() => (copied = false), 1200)
    } catch {
      /* clipboard unavailable: ignore */
    }
  }
  async function openRelease() {
    try {
      await openUrl(releaseUrl)
    } catch {
      /* opener unavailable: the copy button is the fallback */
    }
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onKey} />

<div class="ver" bind:this={root}>
  <button
    class="badge"
    class:has-update={hasUpdate}
    onclick={() => (open = !open)}
    aria-haspopup="dialog"
    aria-expanded={open}
    title={hasUpdate ? `Update available: v${upd?.latest}` : 'About arrow'}
  >
    v{appVersion}{#if hasUpdate}<span class="udot" aria-hidden="true"></span>{/if}
  </button>

  {#if open}
    <div class="popover" role="dialog" aria-label="About arrow">
      <div class="title">arrow <span class="v">v{appVersion}</span></div>
      <div class="sub">Audit viewer for Claude Code</div>
      {#if hasUpdate}
        <div class="update">
          <span class="up-text">Update available → <strong>v{upd?.latest}</strong></span>
          <button class="open" onclick={openRelease}>Open release</button>
        </div>
        <div class="hint">macOS is unsigned — re-run the installer to update.</div>
      {/if}
      <div class="link">
        <span class="url" title={releaseUrl}>{releaseUrl}</span>
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
  .badge.has-update {
    color: var(--fg);
    border-color: var(--green);
  }
  .udot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 999px;
    background: var(--green);
    margin-left: 5px;
    vertical-align: middle;
  }
  .update {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding: 6px 8px;
    background: var(--chip);
    border: 1px solid var(--green);
    border-radius: 6px;
    font-size: 12px;
  }
  .up-text {
    color: var(--fg);
    flex: 1;
    min-width: 0;
  }
  .open {
    font: inherit;
    font-size: 11px;
    color: var(--on-accent);
    background: var(--green);
    border: 1px solid var(--green);
    border-radius: 6px;
    padding: 2px 9px;
    cursor: pointer;
    flex: none;
  }
  /* Moves the fill AWAY from the panel instead of fading it toward it: on light, an
     opacity wash lightened the green under a white label down to 4.25:1. */
  .open:hover {
    background: var(--green-hover);
    border-color: var(--green-hover);
  }
  .hint {
    color: var(--dim);
    font-size: 11px;
    margin-top: 6px;
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
    box-shadow: var(--shadow-lg);
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
