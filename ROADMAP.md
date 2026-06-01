# ROADMAP / workfile de arrow

Archivo de seguimiento entre sesiones. El **roadmap canónico de fases** vive en
[README.md](README.md#roadmap); aquí va el detalle operativo: estado fino, deuda técnica y un
backlog de ideas que aún no están comprometidas a ninguna fase. Las **convenciones** del proyecto
están en [CLAUDE.md](CLAUDE.md); el contrato de datos en [SPEC.md](SPEC.md).

> Última actualización: 2026-05-30 (Fase 2 + pulido + mejoras visuales: zoom, titlebar custom, y foco por sesión activa).

## Estado actual

| Fase | Estado | Notas |
|---|---|---|
| 0 — parser/CLI | ✅ | `src/lib.rs` (lib) + `src/main.rs` (CLI). |
| 1 — UI web (Svelte + CodeMirror) | ✅ | `web/`, dev-server ejecuta el binario. |
| 2 — empaquetado Tauri 2.x | ✅ | `.deb` + AppImage; backend nativo `src-tauri/`; frontend dual-mode. |
| 3 — honestidad + git | ⏳ pendiente | Ver README. Tiene nota de diseño del badge `live` en SPEC.md. |
| 4 — edición + GitHub | ⏳ postergado | Ver README. |

### Pulido aplicado (post-Fase 2)
- **Tests del parser**: 9 tests unitarios en `src/lib.rs` (`#[cfg(test)] mod tests`) con
  transcripts-fixture en tempdir. Cubren lo NO obvio: parsing defensivo, solo-top-level, agrupación
  por raíz git, conteo +/-, filtro de `~/.claude/`, orden por recencia, metadatos, `file_content`.
  Correr con `cargo test --release`. Complementan a `/verify-parser` (datos reales).
- **Watcher resiliente** (`src-tauri/src/lib.rs`): reintenta establecer el watch si
  `~/.claude/projects` no existe aún o se borra y recrea (antes se rendía para siempre). El polling
  del frontend sigue siendo el respaldo último.
- **Binario optimizado**: perfil release en `src-tauri/Cargo.toml` (`strip` + `lto` +
  `codegen-units=1` + `opt-level="s"` + `panic="abort"`) para reducir el tamaño del bundle.
- **Skill `/rust-review`**: clippy + rustfmt + checklist de best-practices (ver
  `.claude/skills/rust-review/`).

### Mejoras visuales / UX (post-Fase 2)
- **Zoom de la UI** (`Ctrl +` / `Ctrl −` / `Ctrl 0`, estilo VSCode): zoom **nativo** del webview en la
  app Tauri (`getCurrentWebview().setZoom`; no toca el layout → no descoloca a CodeMirror ni depende
  de la versión de WebKitGTK) y CSS `zoom` como fallback en el navegador (dev). Persiste en
  `localStorage` (`arrow.zoom`) y se reaplica al arrancar; rango 60–200%, widget `− % +` en la topbar.
  Lógica aislada en `web/src/lib/zoom.ts` (misma convención que `api.ts`). Requirió el permiso ACL
  `core:webview:allow-set-webview-zoom` en `src-tauri/capabilities/default.json`.
- **Titlebar custom** (`decorations:false` + `resizable:true`): botones min/max/close propios
  (`web/src/components/WindowControls.svelte` → `getCurrentWindow().minimize()/toggleMaximize()/close()`,
  aislados en `web/src/lib/window.ts`), barra como zona de arrastre (`data-tauri-drag-region`),
  doble-click = maximizar, y borde CSS sutil (GTK pierde la sombra sin decoraciones). Garantiza los
  botones **cross-distro**: en Pop!_OS/GNOME el WM no los pintaba de forma fiable bajo Pop Shell, y el
  `gsettings button-layout` ya estaba correcto pero no ayudaba (ni viaja con la app, ni aplica en COSMIC).
  Permisos ACL: `core:window:allow-{minimize,toggle-maximize,close,start-dragging}`.
- **Fix de arranque Tauri**: `beforeDevCommand`/`beforeBuildCommand` en `tauri.conf.json` eran
  `npm --prefix web run …`, pero el CLI 2.11 los ejecuta desde `web/` (dir del frontend), NO desde la
  raíz → buscaba `web/web/package.json` y `cargo tauri dev` fallaba. Corregidos a `npm run dev` /
  `npm run build` (sin `--prefix`); nota de CLAUDE.md actualizada. ✅ Verificado: `cargo tauri dev`
  levanta la ventana y `cargo tauri build` genera `.deb` (2.0M) + AppImage (77M).
- **Foco = sesión activa** (rediseño de la semántica `live`, resuelve la deuda de abajo): el sidebar
  muestra arriba los repos de la **sesión activa** (actividad más reciente) + ráfaga de ~10 min
  (`BURST_WINDOW`), en vez de "todos los repos con actividad <20 min" — así un repo de una sesión
  anterior deja de quedar "pegado" arriba. **Punto verde** como único indicador (solo en repos del
  foco con actividad reciente); se quitó el badge de texto `● live`. Lógica única en
  `web/src/lib/time.ts` (`focusRepos()`), reusada por `Sidebar.svelte` y la topbar de `App.svelte`
  (antes el cálculo `isLive` estaba duplicado en 4 sitios). Verificado contra datos reales. Detalle de
  honestidad: "sesión activa" = actividad más reciente en disco, NO proceso en ejecución; el foco se
  ancla al timestamp del dato (no al reloj). Nota del modelo: una sesión (mismo `sessionId`) vive en
  UN repo (raíz git del cwd), así que el foco son ≥1 repo según cuántas sesiones recientes haya en la
  ráfaga, no "una sesión tocando varios repos".

## Backlog de ideas (sin comprometer fase)

Mejoras propuestas que NO están en el roadmap de fases. A discutir/priorizar antes de implementar;
todas deben respetar la honestidad del producto (no afirmar más de lo que el dato sabe).

- **Búsqueda / filtro en el sidebar** — filtrar el árbol por nombre de repo, de archivo o por
  contenido del diff. Mejora de navegación cuando hay muchos repos/sesiones. (Solo frontend:
  `web/src/components/Sidebar.svelte`; no toca el parser.)
- **Stats de cambios** — totales de líneas `+/-` por repo / sesión / día; quizá un mini-resumen en
  la topbar o un panel. Refuerza el ángulo de "auditoría". (El parser ya expone `added`/`removed`
  por archivo; sería agregación, posible en frontend o como campo nuevo en el contrato.)
- **Export** — copiar el diff de un archivo al portapapeles o exportar un resumen de sesión a
  Markdown (lista de archivos + `+/-` + título). Útil para PRs o reportes.
- **Atajos de teclado** — navegar el árbol y abrir diffs sin ratón (j/k, enter, etc.). (Los atajos de
  **zoom** `Ctrl +/−/0` ya están; faltan los de navegación. Reusarían el mismo `keydown` global de
  `App.svelte` (`onKey`).)

## Deuda técnica / notas

- **Semántica del badge `live`** — ✅ resuelto (post-Fase 2; ver "Mejoras visuales / UX"). El foco del
  sidebar pasó de "todos los repos con actividad <20 min" a "los de la **sesión activa** (actividad más
  reciente) + ráfaga ~10 min"; el **punto verde** quedó como único indicador (se quitó el badge de
  texto `live`). Sigue honesto: "activa" = actividad más reciente en disco, no proceso vivo. Bonus: el
  foco se ancla al **dato** (no al reloj), así que no sufre el congelamiento de tiempo relativo (abajo).
- **Tiempo relativo congelado**: `relative()`/`isLive()` solo se recalculan al re-render; si el
  report no cambia, el "Nm ago" no avanza. Un tick de reloj independiente lo resolvería (también
  anotado en SPEC.md).
- **Sin CI**: los tests y `/rust-review` se corren a mano. Si el proyecto crece, valdría un workflow
  de GitHub Actions (`cargo test` + `cargo clippy` + `cargo fmt --check`). También habilitaría el
  **build de macOS** en un runner `macos` (`tauri-action`) sin necesitar una Mac física.
- **macOS sin verificar**: el código ya está adaptado a Mac (titlebar nativa + font-weight por OS;
  ver [MACOS.md](MACOS.md)), pero el bloque `cfg(target_os = "macos")` solo se activa al compilar en
  una Mac → falta el primer `cargo tauri build --bundles app dmg` real para confirmar el look de la
  titlebar y el `.dmg`. La firma/notarización (Gatekeeper) queda fuera del MVP.
