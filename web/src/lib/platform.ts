// Detección de plataforma para ajustes específicos del OS (distinto del transporte de
// api.ts). Se usa para: gatear el font-weight de WebKitGTK (solo Linux) y ocultar los
// botones de ventana custom en macOS (allí mandan los semáforos nativos).
const ua = navigator.userAgent
export const isMac = /Macintosh|Mac OS X/.test(ua)
export const isWindows = /Windows/.test(ua)
export const isLinux = !isMac && !isWindows

// Marca <html> con la clase del OS para poder gatear estilos por plataforma desde CSS.
export function applyOsClass(): void {
  document.documentElement.classList.add(isMac ? 'is-mac' : isWindows ? 'is-windows' : 'is-linux')
}
