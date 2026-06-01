<script lang="ts">
  // Botones de ventana de la titlebar custom (decorations:false). Solo se renderizan
  // dentro de la app Tauri; en el navegador (npm run dev) no aparecen. La lógica de
  // ventana vive aislada en lib/window.ts (igual que el transporte en lib/api.ts).
  import { inTauri } from '../lib/api'
  import { isMac } from '../lib/platform'
  import { winMinimize, winToggleMaximize, winClose } from '../lib/window'
</script>

<!-- En macOS la ventana usa decoración nativa (semáforos): no pintamos botones custom. -->
{#if inTauri && !isMac}
  <div class="winctl">
    <button class="wbtn" onclick={winMinimize} title="Minimize" aria-label="Minimize">
      <svg width="10" height="10" viewBox="0 0 10 10"><line x1="1" y1="5" x2="9" y2="5" /></svg>
    </button>
    <button class="wbtn" onclick={winToggleMaximize} title="Maximize" aria-label="Maximize">
      <svg width="10" height="10" viewBox="0 0 10 10"><rect x="1.5" y="1.5" width="7" height="7" /></svg>
    </button>
    <button class="wbtn close" onclick={winClose} title="Close" aria-label="Close">
      <svg width="10" height="10" viewBox="0 0 10 10"><line x1="1.5" y1="1.5" x2="8.5" y2="8.5" /><line x1="8.5" y1="1.5" x2="1.5" y2="8.5" /></svg>
    </button>
  </div>
{/if}

<style>
  .winctl {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: 2px;
  }
  .wbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
  }
  .wbtn svg {
    stroke: currentColor;
    stroke-width: 1.2;
    fill: none;
  }
  .wbtn:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .wbtn.close:hover {
    background: var(--red);
    color: #fff;
  }
</style>
