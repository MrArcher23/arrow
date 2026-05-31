// Controles de la ventana nativa para la titlebar custom (decorations:false).
// Aísla el acceso a la API de ventana de Tauri (importada de forma perezosa) igual
// que api.ts aísla el transporte de datos: el resto de la UI no sabe del entorno.
// En el navegador (npm run dev) son no-ops, así que `npm run dev` no se rompe.
import { inTauri } from './api'

async function appWindow() {
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  return getCurrentWindow()
}

export async function winMinimize(): Promise<void> {
  if (inTauri) await (await appWindow()).minimize()
}

export async function winToggleMaximize(): Promise<void> {
  if (inTauri) await (await appWindow()).toggleMaximize()
}

export async function winClose(): Promise<void> {
  if (inTauri) await (await appWindow()).close()
}
