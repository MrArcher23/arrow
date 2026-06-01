# arrow

**Visor de auditoría de Claude Code.** Responde a una sola pregunta, de forma fiable:
*¿qué archivos tocó Claude, en qué repo, con qué diff y en qué sesión?* — sin abrir un IDE,
sin chat con IA, y **sin depender de git ni de hooks**.

> Estado: **Fase 2** — app de escritorio (Tauri 2.x) con el parser de Rust como backend nativo.
> Fases 0 (parser/CLI), 1 (UI web) y 2 (empaquetado) completas.

## Por qué existe

El espacio de tooling para Claude Code está saturado, pero nadie cubre exactamente esto:
una **UI gráfica, sin chat**, con la jerarquía `repositorio → archivos tocados → diff/editor`
como audit trail navegable. Lo más cercano es un TUI de terminal (`claude-file-recovery`),
un cliente de chat web (`claude-code-viewer`), o GUIs grandes orientadas a *ejecutar* agentes
(`opcode`, AGPL). Anthropic cerró el feature request de "historial recuperable de ediciones"
(#36542) como *not planned* — así que el hueco es real, aunque el margen es estrecho.

## La idea clave: la fuente de datos correcta

No hace falta instalar ningún hook `session-log`. Claude Code **ya persiste** todo lo necesario,
de forma nativa y estructurada (verificado en Claude Code v2.1.x):

| Dato | Fuente nativa |
|---|---|
| **Repos** | El campo `cwd` de cada record (ruta real; no se decodifica el nombre del directorio, que es ambiguo). |
| **Sesiones** | Cada `~/.claude/projects/<cwd-codificado>/<sessionId>.jsonl`. El nombre del fichero **es** el `sessionId`. |
| **Archivos tocados** | Records `type:"user"` con `toolUseResult.filePath` (solo `Edit`/`Write`/`MultiEdit`). |
| **Diff (antes/después)** | `toolUseResult.structuredPatch` — los hunks exactos `{oldStart, oldLines, newStart, newLines, lines}`. Ya es el diff por sesión. |
| **"Antes" / point-in-time** | `~/.claude/file-history/<sessionId>/<hash>@v<n>` — snapshots del contenido previo. |

`git diff` se reserva como vista **secundaria** opcional (working tree completo), y solo cuando el
repo es git: no atribuye por autor ni por sesión, y muchos repos no son git.

## Límites honestos (el producto NO debe mentir)

- Solo se captura lo que pasa por `Edit`/`Write`/`MultiEdit`. Cambios vía comandos `Bash`
  de la sesión (sed, prettier, build, `mv`, `rm`) **no** aparecen. La etiqueta correcta es
  *"ediciones vía herramientas de Claude"*, nunca *"todo lo que hizo Claude"*.
- El flag `userModified` señala drift (el usuario editó entre el read y el write): se marca con ⚠.
- El formato JSONL es **interno, no documentado, cambia entre versiones y se auto-borra a ~30 días**
  (`cleanupPeriodDays`). De ahí el parsing defensivo: una línea inválida se ignora, no rompe.

## Uso

```bash
cargo build --release

# Resumen: todos los repos -> sesiones -> archivos (+/-)
./target/release/arrow --list

# Diff completo de un repo (filtra por substring del cwd)
./target/release/arrow --repo mi-proyecto

# Una sesión concreta (prefijo del sessionId)
./target/release/arrow --session 14385fed

# JSON normalizado (el contrato que consumirá la UI de la Fase 1)
./target/release/arrow --repo mi-proyecto --json
```

Opciones: `--projects-dir <ruta>` (por defecto `~/.claude/projects`), `--repo`, `--session`,
`--list`, `--json`, y `--content --file <ruta> [--session <id>]` (emite `{before, after}` de un
archivo, para la vista de diff de la UI).

## UI web (Fase 1)

App Vite + Svelte 5 que consume el parser vía un dev-server local que ejecuta el binario `arrow`
(en `web/vite.config.ts`). La vista de diff usa **CodeMirror 6 + `@codemirror/merge`**.

```bash
cargo build --release          # el dev-server ejecuta target/release/arrow
cd web && npm install
npm run dev                     # http://localhost:5173
```

Layout: sidebar `repo → sesión → archivos tocados` (con `+/-` y ⚠ `userModified`), panel central
con el diff before/after del archivo seleccionado. Todo el peso del frontend: ~93 KB gzip.

## App de escritorio (Fase 2)

Misma UI, empaquetada en **Tauri 2.x**: el parser de Rust es el **backend nativo** (sin sidecar
ni servidor HTTP). La UI Svelte se reutiliza tal cual; solo su capa de transporte (`web/src/lib/api.ts`)
detecta el entorno y usa Tauri `invoke()` dentro de la app o `fetch` en el navegador — así
`npm run dev` sigue funcionando para iterar rápido.

```bash
# requisitos del sistema (Linux/Debian/Ubuntu/Pop!_OS), una vez:
sudo apt install -y libwebkit2gtk-4.1-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev
cargo install tauri-cli --version "^2"     # CLI de Tauri

# desarrollo: ventana nativa con hot-reload del frontend
cargo tauri dev      # (desde la raíz del repo)

# instalable: .deb + AppImage en src-tauri/target/release/bundle/
cargo tauri build
```

> **macOS:** la app se adapta sola al OS (titlebar nativa con semáforos + peso de fuente
> correcto). Para compilar el `.app`/`.dmg` —que solo se puede en una Mac— ver
> [MACOS.md](MACOS.md).

- **Backend nativo** (`src-tauri/`): dos comandos `invoke` — `report()` y `content(file, session)` —
  que envuelven la librería del parser (`arrow = { path = ".." }`, ver Arquitectura). El AppImage
  corre standalone, leyendo `~/.claude/projects` directamente desde Rust.
- **Refresco en vivo nativo**: un watcher `notify` sobre `~/.claude/projects` emite el evento
  `report-changed` (con debounce) y la UI refresca al instante; se mantiene un polling lento como
  fallback.
- **Foco por sesión activa**: arriba se muestran el/los repo(s) de la **sesión activa** (la de
  actividad más reciente) + cualquiera tocado en la misma ráfaga (~10 min, `BURST_WINDOW` en
  `web/src/lib/time.ts`); el resto baja a *Other repos* a medida que envejece respecto a la activa.
  El **punto verde** marca esos repos del foco con actividad reciente (se quitó el badge de texto
  `live`, redundante). Honesto: "sesión activa" = *actividad más reciente en disco*, no *proceso en
  ejecución* (arrow no puede saber lo segundo).
- **Ventana y zoom**: titlebar propia (`decorations:false`) con botones minimizar/maximizar/cerrar,
  arrastre y doble-click para maximizar — garantiza los controles *cross-distro* (en GNOME/Pop!_OS el
  WM no los pinta de forma fiable). Zoom de UI estilo VSCode con `Ctrl +/−/0`: nativo del webview
  (`setZoom`, no descoloca a CodeMirror), persistente entre sesiones.
- **Linux/WebKitGTK**: la app fija `WEBKIT_DISABLE_DMABUF_RENDERER=1` en su `main()` (evita la
  pantalla en blanco por DMABUF/NVIDIA); el bug de `font-weight` (+100) ya está compensado en
  `web/src/app.css` (`font-weight: 350`).

### Arquitectura (parser compartido, sin duplicar)

El parser vive en una **librería** (`src/lib.rs`): funciones puras `build_report(projects_dir)` y
`file_content(projects_dir, file, session)` + los structs serializables. La consumen dos frontends:
`src/main.rs` (la CLI, flags intactos) y `src-tauri/` (el backend de escritorio). Cero lógica
duplicada; la misma fuente de verdad para terminal, web y app nativa.

## Roadmap

- [x] **Fase 0** — parser nativo `JSONL → repo/sesión/archivo/diff`, salida en terminal y `--json`.
- [x] **Fase 1** — UI web local (Vite + Svelte 5) consumiendo el parser: sidebar
      `repo → sesión → archivos` (estilo Antigravity) + diff por archivo con
      **CodeMirror 6 + `@codemirror/merge`**. (Solo lectura; la edición es Fase 4.)
- [x] **Fase 2** — empaquetado en **Tauri 2.x** (`.deb` + AppImage). El parser de Rust se extrajo
      a una **librería** (`src/lib.rs`) y es el backend nativo (sin sidecar); la CLI y la app lo
      comparten. Frontend dual-mode (`invoke`/`fetch`), watcher `notify` → evento `report-changed`
      para refresco en vivo, y mitigación WebKitGTK. (El foco del sidebar se refinó después a
      "sesión activa + ráfaga ~10 min" con el punto verde como único indicador; ver ROADMAP.)
- [ ] **Fase 3** — honestidad + git: toggle "git diff working tree", marcado de `userModified`,
      timeline point-in-time reusando `file-history`.
- [ ] **Fase 4** (postergado) — edición real con guardado a disco, integración GitHub (PRs/commits).

Seguimiento operativo, deuda técnica y backlog de ideas (búsqueda, stats, export, atajos):
ver [ROADMAP.md](ROADMAP.md).

## Stack

Rust (parser/backend) · CodeMirror 6 (UI, Fase 1+) · Tauri 2.x (empaquetado, Fase 2+).
Sobre Linux/WebKitGTK habrá que aplicar `WEBKIT_DISABLE_DMABUF_RENDERER=1` y compensar el
bug de `font-weight` (+100) — ambos documentados para la Fase 2.

## Licencia

MIT.
