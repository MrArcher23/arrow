# SPEC — Fase 2: empaquetar arrow en Tauri 2.x

> Spec autocontenido para ejecutar en una **sesión nueva y limpia**. Contexto base: @CLAUDE.md
> y @README.md. Estado al escribir esto: Fase 0 (parser Rust) y Fase 1 (UI web Svelte) completas
> y funcionando; todo en `main` y en GitHub (`MrArcher23/arrow`).

## Objetivo

Convertir arrow en una **app de escritorio instalable en Linux** (`.deb` / AppImage) donde el
parser de Rust es el **backend nativo** (sin sidecar, sin servidor HTTP). La UI Svelte actual se
reutiliza tal cual; solo cambia su capa de transporte: de `fetch('/api/...')` a Tauri `invoke()`.

**Criterio de éxito:** `cargo tauri build` produce un instalable que, al abrirse, muestra los
mismos datos en vivo que la versión web (sidebar repo→sesión→archivos, diff CodeMirror, selector
de temas), leyendo `~/.claude/projects` directamente desde Rust.

## Arquitectura objetivo

Compartir el código del parser entre la CLI existente y el backend de Tauri (NO duplicarlo):

1. **Extraer el parser a una librería.** Mover la lógica de `src/main.rs` a `src/lib.rs` como
   funciones públicas puras + los structs (`ReportOut`, `ContentOut`, `RepoOut`, `SessionOut`,
   `FileOut`) que ya derivan `Serialize`:
   - `pub fn build_report(projects_dir: &str) -> ReportOut`
   - `pub fn file_content(projects_dir: &str, file: &str, session: Option<&str>) -> ContentOut`
   - Mantener `git_root`, el filtro de `~/.claude/`, el parsing defensivo, la agrupación por raíz
     git y los metadatos de sesión EXACTAMENTE como están (no cambiar el comportamiento del parser).
   - `src/main.rs` queda como CLI delgada que llama a la lib (preservar flags `--list/--json/--repo/--session/--content/--file`).
   - Verificar que la CLI sigue idéntica con la skill `/verify-parser` antes de seguir.

2. **App Tauri en `src-tauri/`** (crate nuevo) que depende de la lib por path: `arrow = { path = ".." }`.
   Dos comandos que envuelven la lib y devuelven el MISMO JSON que hoy consume la UI:
   ```rust
   #[tauri::command]
   fn report() -> ReportOut { arrow::build_report(&projects_dir()) }
   #[tauri::command]
   fn content(file: String, session: Option<String>) -> ContentOut {
       arrow::file_content(&projects_dir(), &file, session.as_deref())
   }
   ```
   Registrar con `tauri::generate_handler![report, content]`. `projects_dir()` = `$HOME/.claude/projects`.

3. **Frontend dual-mode** en `web/src/lib/api.ts` (única capa que cambia; los componentes Svelte NO
   se tocan). Detectar entorno y usar `invoke` dentro de Tauri o `fetch` en el navegador (para que
   `npm run dev` siga sirviendo en localhost:5173 sin Tauri):
   ```ts
   import { invoke } from '@tauri-apps/api/core'
   const inTauri = '__TAURI_INTERNALS__' in window
   export const loadReport = () =>
     inTauri ? invoke<Report>('report') : fetch('/api/report').then(r => r.json())
   export const loadContent = (file: string, session?: string | null) =>
     inTauri ? invoke<FileContent>('content', { file, session: session ?? null })
             : fetch('/api/content?' + new URLSearchParams({ file, ...(session ? { session } : {}) })).then(r => r.json())
   ```
   Instalar `@tauri-apps/api` en `web/`.

4. **`tauri.conf.json`** apunta al frontend Vite existente:
   ```json
   {
     "build": {
       "beforeDevCommand": "npm --prefix ../web run dev",
       "devUrl": "http://localhost:5173",
       "beforeBuildCommand": "npm --prefix ../web run build",
       "frontendDist": "../web/dist"
     },
     "app": { "windows": [{ "title": "arrow", "width": 1280, "height": 800 }] },
     "bundle": { "active": true, "targets": ["deb", "appimage"] }
   }
   ```

5. **Mitigaciones Linux/WebKitGTK** (de la investigación): en `src-tauri` `main()`, ANTES de
   construir la app:
   ```rust
   std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
   // y si hay pantalla en blanco en Wayland: WEBKIT_DISABLE_COMPOSITING_MODE=1
   ```
   El bug de font-weight (+100) ya está compensado en `web/src/app.css` (`font-weight: 350`).

6. **Refresco en vivo nativo** (sustituye el polling de 5 s del web): watcher con el crate `notify`
   sobre `~/.claude/projects`; con debounce, emitir un evento Tauri `app.emit("report-changed", ())`.
   El frontend escucha con `@tauri-apps/api/event` `listen('report-changed', refresh)` en `App.svelte`
   (solo si `inTauri`; mantener el `setInterval` como fallback del modo navegador).
   *Aceptable para un primer corte:* dejar el polling también en Tauri y añadir el watcher después.
   - **Semántica del badge `live` (revisar al cablear el watcher).** Hoy `isLive()`
     (`web/src/lib/time.ts`) marca una sesión como `live` si su `lastActivity` cae en los últimos
     20 min (`LIVE_WINDOW`), evaluado sobre `sessions[0]` (la sesión más reciente del repo). Es
     **"actividad reciente", no "sesión en ejecución ahora"**: una sesión ya terminada conserva el
     badge hasta ~20 min tras su última edición. Dos límites honestos del ciclo de vida a tener
     presentes al rehacer el refresco:
     - Una sesión **sin** ediciones (sin `toolUseResult.filePath`) **no aparece**: arrow es un visor
       de archivos tocados, no un monitor de procesos; solo se lista al primer `Edit`/`Write`/`MultiEdit`
       (`src/main.rs` solo crea la sesión en la rama de cambio de archivo, no con metadatos sueltos).
     - El refresco web actual (polling 5 s en `App.svelte`) **cortocircuita el re-render** cuando el
       report no cambia (`if (txt === lastJson) return`); como `relative()`/`isLive()` solo se
       recalculan al re-renderizar, el `"Nm ago"` y el badge `live` se **congelan** hasta que una
       edición mueva el report (el badge no caduca a los 20 min por sí solo).
     Al introducir el watcher + evento `report-changed`, decidir: (a) ¿`live` sigue siendo
     "actividad reciente" o pasa a "proceso de Claude activo" (necesitaría una señal del sistema)?;
     (b) añadir un tick de reloj independiente para que el tiempo relativo y la caducidad de `live`
     avancen aunque el report no cambie. El badge no debe sugerir más de lo que sabe (honestidad).

## Prerrequisitos (Linux)

- Dependencias de sistema (apt; requieren sudo — pídeselas al usuario con `!sudo apt install ...`):
  `libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`
- Tauri CLI: `cargo install tauri-cli --version "^2"` (o `npm --prefix web i -D @tauri-apps/cli@^2`).
- Rust ya instalado (`~/.cargo`). Node v22 + npm ya disponibles.

## Plan paso a paso

1. Refactor: `src/main.rs` → `src/lib.rs` (lib) + CLI delgada. Verificar con `/verify-parser` (la
   salida de `--json`/`--list` debe ser idéntica a la de antes del refactor).
2. `cargo tauri init` dentro del repo (o crear `src-tauri/` a mano) con la config de arriba.
3. Implementar los comandos `report`/`content` usando la lib.
4. Instalar `@tauri-apps/api` y reescribir `web/src/lib/api.ts` a modo dual.
5. Mitigaciones WebKitGTK en `main()`.
6. `cargo tauri dev` → validar en ventana nativa.
7. (Opcional en este corte) watcher `notify` + evento + `listen`.
8. `cargo tauri build` → instalable; probar el AppImage de forma standalone.
9. Actualizar README (marcar Fase 2 ✅, instrucciones de build de la app) y CLAUDE.md si cambian comandos.

## Fuera de alcance (NO hacer en Fase 2)

- **Edición de archivos** desde la app (guardar a disco) → Fase 4.
- **Integración GitHub** (PRs/commits) → Fase 4.
- **Toggle "git diff working tree"** y timeline point-in-time (`file-history`) → Fase 3.
- Auto-update de Tauri, firma de binarios, build para macOS/Windows → más adelante.
- No cambiar la lógica del parser ni el modelo de datos; solo moverla a una lib.

## Verificación end-to-end

1. `/verify-parser` pasa igual que antes del refactor (parser intacto).
2. `cargo tauri dev` abre una **ventana nativa** (no navegador) que muestra: repos en vivo arriba,
   diff con CodeMirror, selector de temas funcionando — todo vía `invoke`, sin servidor HTTP.
3. `npm --prefix web run dev` + abrir localhost:5173 sigue funcionando (modo navegador, vía `fetch`).
4. `cargo tauri build` genera `.deb` y `.AppImage` en `src-tauri/target/release/bundle/`.
5. Ejecutar el `.AppImage` **standalone** (sin dev-server corriendo): abre y muestra los datos reales.
6. Editar un archivo con Claude en otro repo y confirmar que la app refleja el cambio (vía watcher
   o polling) sin reiniciar.

## Riesgos / gotchas

- WebKitGTK en Linux: pantalla en blanco (DMABUF/NVIDIA/Wayland) → las env vars del paso 5 lo mitigan;
  probar en el hardware real.
- El refactor a lib es mecánico pero **verificar que la CLI no cambia** es obligatorio antes de seguir.
- `cargo tauri init` puede generar un `src-tauri/Cargo.toml` con `name = "app"`; renómbralo a `arrow-app`
  o similar para claridad (no colisionar con el crate `arrow` lib).
- Mantener el modo dual en `api.ts`: si rompes el `fetch` del navegador, pierdes el flujo de dev rápido.
