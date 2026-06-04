# arrow — visor de auditoría de Claude Code

App de escritorio que audita **qué archivos tocó Claude Code**:
`repo → sesión → archivos → diff`, **sin chat con IA**. Ver @README.md para overview y roadmap,
y @ROADMAP.md para seguimiento operativo, deuda técnica y backlog de ideas.

Stack: parser en **Rust** como **librería** (`src/lib.rs`) consumida por la CLI (`src/main.rs`) y
por el backend de Tauri (`src-tauri/`); UI web **Svelte 5 + Vite + CodeMirror 6** (`web/`),
empaquetada en **Tauri 2.x**. Estado: Fases 0, 1 y 2 completas (parser + UI web + app de escritorio).

## Build / run / verificar
- Compilar parser/CLI: `cargo build --release` → binario en `target/release/arrow`.
  Si `cargo` no está en PATH, usar `~/.cargo/bin/cargo` (toolchain instalada con rustup).
  Toda la lógica vive en `src/lib.rs` (`build_report`, `file_content`); `src/main.rs` es la CLI.
- Levantar la UI web (dev): `cd web && npm install && npm run dev` → http://localhost:5173
  El dev-server (`web/vite.config.ts`) **ejecuta el binario** `target/release/arrow`; tras tocar
  `src/lib.rs` hay que **recompilar** para que la UI vea los cambios.
- App de escritorio (Tauri): `cargo tauri dev` (ventana nativa) / `cargo tauri build`
  (`.deb` + AppImage en `src-tauri/target/release/bundle/`). Requiere `cargo install tauri-cli --version "^2"`
  y, en Linux, `libwebkit2gtk-4.1-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev` (apt, sudo).
  `cargo tauri` (CLI 2.x) ejecuta `beforeDevCommand`/`beforeBuildCommand` desde el **dir del frontend**
  (`web/`, derivado de `frontendDist: ../web/dist`), NO desde la raíz del repo → por eso son `npm run dev`
  / `npm run build` **sin** `--prefix web` (un `--prefix web` buscaría `web/web/package.json` y falla).
  `src-tauri/` es su **propia raíz de workspace**: `cargo build` en la raíz NO arrastra el backend Tauri.
- Build del frontend: `npm --prefix web run build`.
- **Tests**: 20 tests unitarios del parser en `src/lib.rs` (`cargo test --release`). Complementan
  —no reemplazan— la verificación contra datos reales del skill `/verify-parser`.

## Modelo de datos (lo NO obvio — léelo antes de tocar el parser)
- Fuente de verdad: transcripts NATIVOS `~/.claude/projects/<dir>/<sessionId>.jsonl`. **No se usa ningún hook.**
- Archivos tocados = records con `toolUseResult.filePath` (solo `Edit`/`Write`/`MultiEdit`; `Bash` NO).
  Re-editar el mismo archivo NO lo duplica: se acumula en una entrada (`ops`/`+/-`/`hunks`). En el
  contrato (`--json`/UI) los archivos salen **ordenados por recencia de edición** (`lastTouched` desc,
  sin fecha al final, desempate por path; ver `build_report_from`) — mismo split que repos/sesiones: el
  `--list` (terminal) sigue alfabético. El `--list` itera el `BTreeMap` interno (por path); la recencia
  vive solo en `build_report_from`.
- **"Open in editor"** (botón en la barra de archivo, **Tauri-only**): tabla de editores = **dato** en
  `src-tauri/src/editor.rs`; apertura por familia de sintaxis (VS Code `-g {file}:{line}:{col}`,
  Zed/Sublime posicional, JetBrains `--line/--column`), **argv directo sin shell** (bug de espacios
  vscode#39891). Salta a `ContentOut.firstChangedLine` (menor `newStart`, lado after). **Honesto**: abre
  el archivo **actual en disco** (after), no el snapshot reconstruido. Editores de terminal NO soportados
  (sin TTY desde una app GUI). Ver ROADMAP para los diferidos.
- Diff = `toolUseResult.structuredPatch` (hunks). `--content` reconstruye before / after (disco).
  El **before** = estado previo a la 1ª edición de la sesión, resuelto en cascada (`resolve_before`):
  (0) si el 1er Edit es un `create` (Write de archivo nuevo) ⇒ before vacío ("new file"); (1) `originalFile`
  inline del 1er Edit; (2) si Claude lo emite como `null` —pasa en parte de los Edit aunque haya
  `structuredPatch`—, el `originalFile` exacto más temprano de una edición posterior reaplicando en
  reversa las previas; (3) el último `Read` **completo** (los parciales/offset se descartan), solo si
  cuadra con el 1er Edit; (4) reaplicar en reversa TODAS las ediciones sobre el `after` de disco.
  Cada reconstrucción se **verifica por hunk** (si no cuadra → drift → se aborta); la verificación es
  local a las regiones tocadas, así que el caso (4) sobre un disco desfasado es best-effort. Sin fuente
  válida ⇒ `beforeAvailable:false` (el frontend muestra el archivo actual con banner honesto, NUNCA "new
  file"). `--content` sin `--session` elige la sesión más reciente que tocó el archivo (no mezcla sesiones).
- Metadatos de sesión: `ai-title`→título, `last-prompt`, `timestamp`→actividad.
- Repo = **raíz git del `cwd`** de la sesión (`git_root`): fusiona subdirs como `web/` con su repo. Un
  **worktree enlazado** tiene un `.git` que es un **FILE** (`gitdir: …/.git/worktrees/<name>`); `git_root`
  lo detecta y **re-ancla al repo principal** (lee el puntero, sin invocar git), así un cwd dentro de
  `<repo>/.claude/worktrees/<name>/` NO aparece como repo fantasma. Un submódulo (`.git/modules/…`) NO
  se re-ancla (es su propio repo).
- **"Worktrees inventory"** (botón `Worktrees` en la topbar, **Tauri-only**, read-only): lista/clasifica
  los worktrees de git por repo (active / stale ≥10 min / "merged → safe to remove") con tamaños bajo
  demanda y `copy cmd`. **NO borra nada** (el botón `Clean` se difirió — ver ROADMAP). Todo el shelling a
  git vive en `src-tauri/src/worktrees.rs` (Tauri-only, clona `editor.rs`): el parser `src/lib.rs` **sigue
  sin invocar git**. argv directo sin shell, comandos read-only con timeout. Honestidad: rama por defecto
  **resuelta dinámicamente** (nunca hardcodeada); "merged" en verde **solo** si el tip es ancestro
  (squash/rebase ⇒ "can't tell", nunca verde falso); active/stale = edición reciente de archivos, NO
  proceso vivo; tamaño aproximado. active/stale se deriva en el **frontend** desde `lastTouched` (ventana
  de 10 min, `STALE_AFTER` en `time.ts`); el backend solo reporta hechos de git.
- Solo cuentan transcripts de **primer nivel**; los `.jsonl` anidados (subagentes) se ignoran.

## Reglas del proyecto (IMPORTANT)
- **Filtrar `~/.claude/`** (HOME global de Claude): es bookkeeping interno, NO código del usuario.
  Un `.claude/` DENTRO de un repo (settings/skills del proyecto) SÍ se muestra. Filtro por **prefijo del HOME**, no por la subcadena `/.claude/`.
- **Honestidad**: etiquetar "ediciones vía herramientas de Claude", NUNCA "todo lo que hizo Claude"
  (los cambios vía `Bash` no se capturan). Marcar `userModified` con ⚠.
- Formato JSONL **interno, no documentado, volátil, se auto-borra a ~30 días** → **parsing defensivo**:
  una línea inválida se ignora, nunca rompe (parse a `serde_json::Value`, no a structs rígidos).
- `git diff` es solo **vista secundaria** opcional (muchos repos no son git; no atribuye por sesión).
- La capa de fetch del frontend vive aislada en `web/src/lib/api.ts` (ya dual-mode: Tauri `invoke()`
  dentro de la app, `fetch` en el navegador, detectado por `__TAURI_INTERNALS__`). No acoplar
  componentes Svelte al transporte: cualquier cambio de transporte se hace solo en `api.ts`.
  `api.ts` cachea contenidos ya cargados (revisitas instantáneas) y purga el cache al cambiar el report.

## Convenciones
- Comunícate **conmigo (el usuario) en español**. En cambio, **el repo es público**: todo el texto
  que vive en el código y de cara a colaboradores va en **inglés**.
- **Comentarios de código en inglés** (de aquí en adelante; los comentarios legacy en español migran
  gradualmente, sin reescribirlo todo de golpe), identificadores en inglés.
- **Docs públicas y de contribución en inglés**: `README.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`,
  `SECURITY.md` y las plantillas de `.github/`. Este `CLAUDE.md`, `ROADMAP.md`, `SPEC.md` y `MACOS.md`
  siguen en español (son docs de trabajo del mantenedor).
- **Texto visible en la UI en INGLÉS** (labels, estados, títulos, tooltips, banners). arrow es una
  app de marca inglesa. Ej: `live`, `History`, `Other repos`, `new file`, `5m ago`. Los strings de
  UI viven en los componentes Svelte y en `web/src/lib/time.ts` (tiempo relativo y buckets de fecha).
- **Verifica cada cambio del parser contra datos reales** (`/verify-parser`) antes de decir que funciona; reporta la salida, no una afirmación.
- Commits: mensaje en imperativo. Rama `main`. Push solo cuando se pida.

## Skills del proyecto
- `/run-arrow` — compila el parser y levanta la UI.
- `/verify-parser` — recompila y corre el parser contra `~/.claude` (verificación pass/fail).
- `/inspect-transcript` — recetas `jq` para explorar el formato JSONL al extender el parser.
- `/rust-review` — revisa la calidad del Rust (clippy + rustfmt + checklist de best-practices afinado a arrow). Complementa a `/verify-parser` (datos): este mira el código.
