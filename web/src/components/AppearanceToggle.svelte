<script lang="ts">
  // Chrome appearance control, deliberately separate from ThemeMenu: that one picks the
  // editor's syntax theme, this one paints the app itself. Two states only, so a
  // segmented toggle beats a dropdown — it shows the current state and costs one click,
  // and it visually rhymes with the zoom widget already sitting next to it in the topbar.
  import type { Appearance } from '../lib/appearance'

  interface Props {
    current: Appearance
    onSelect: (a: Appearance) => void
  }
  let { current, onSelect }: Props = $props()
</script>

<div class="appearance" role="group" aria-label="Appearance">
  <button
    class="abtn"
    class:active={current === 'light'}
    onclick={() => onSelect('light')}
    title="Light appearance"
    aria-label="Light appearance"
    aria-pressed={current === 'light'}
  >
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r="3.1" stroke="currentColor" stroke-width="1.4" />
      <path
        d="M8 1.2v1.6M8 13.2v1.6M1.2 8h1.6M13.2 8h1.6M3.2 3.2l1.1 1.1M11.7 11.7l1.1 1.1M12.8 3.2l-1.1 1.1M4.3 11.7l-1.1 1.1"
        stroke="currentColor"
        stroke-width="1.4"
        stroke-linecap="round"
      />
    </svg>
  </button>
  <button
    class="abtn"
    class:active={current === 'dark'}
    onclick={() => onSelect('dark')}
    title="Dark appearance"
    aria-label="Dark appearance"
    aria-pressed={current === 'dark'}
  >
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M13.4 9.6A5.8 5.8 0 0 1 6.4 2.6a5.8 5.8 0 1 0 7 7Z"
        stroke="currentColor"
        stroke-width="1.4"
        stroke-linejoin="round"
      />
    </svg>
  </button>
</div>

<style>
  .appearance {
    display: inline-flex;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    background: var(--chip);
  }
  .abtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    padding: 3px 0;
    border: none;
    background: transparent;
    color: var(--dim);
    font: inherit;
    line-height: 1;
    cursor: pointer;
  }
  .abtn:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .abtn.active {
    background: var(--active);
    color: var(--fg);
  }
</style>
