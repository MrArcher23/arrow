// Aplicación del zoom de la UI, aislando el entorno (misma convención que api.ts):
//   - App Tauri: zoom NATIVO del webview (getCurrentWebview().setZoom). No toca el
//     árbol de layout, así que no confunde el click-to-position de CodeMirror ni
//     depende de la versión de WebKitGTK del sistema.
//   - Navegador (npm run dev): fallback a la propiedad CSS `zoom` sobre <html>.
// El zoom del webview NO persiste entre reinicios -> guardamos el factor en
// localStorage y lo reaplicamos al arrancar (ver applyZoom + onMount en App.svelte).
import { inTauri } from './api'

export const ZOOM_MIN = 0.6
export const ZOOM_MAX = 2.0
export const ZOOM_STEP = 0.1
const KEY = 'arrow.zoom'

/** Acota el factor al rango sano y lo redondea a 2 decimales. */
export function clampZoom(n: number): number {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(n * 100) / 100))
}

/** Lee el factor guardado (1.0 por defecto si no hay nada válido). */
export function loadZoom(): number {
  const raw = parseFloat(localStorage.getItem(KEY) ?? '')
  return Number.isFinite(raw) ? clampZoom(raw) : 1
}

/** Persiste y aplica el factor de zoom según el entorno. */
export async function applyZoom(factor: number): Promise<void> {
  localStorage.setItem(KEY, String(factor))
  if (inTauri) {
    try {
      const { getCurrentWebview } = await import('@tauri-apps/api/webview')
      await getCurrentWebview().setZoom(factor)
    } catch {
      // Sin el permiso ACL set-webview-zoom (o entorno raro): degradar sin romper.
    }
  } else {
    document.documentElement.style.zoom = String(factor)
  }
}
