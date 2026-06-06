# arrow — visor de auditoría de Claude Code

App de escritorio que audita **qué archivos tocó Claude Code**:
`repo → sesión → archivos → diff`, **sin chat con IA**. Ver @README.md para overview y roadmap,
y @ROADMAP.md para seguimiento operativo, deuda técnica y backlog de ideas.

Stack: parser en **Rust** como **librería** (`src/lib.rs`) consumida por la CLI (`src/main.rs`) y
por la **app de escritorio nativa egui/eframe** (`gui/`). La UI es **egui** (immediate-mode,
Rust puro) — **sin webview, sin Node/Vite, un solo binario**. La app consume los structs del parser
**directo, sin IPC ni JSON** (no hay capa de transporte). Antes era Tauri 2.x + UI web Svelte 5 +
CodeMirror 6 (`web/` + `src-tauri/`); se **migró a egui** y esos dos directorios se eliminaron.

## Build / run / verificar
- Compilar parser/CLI: `cargo build --release` → binario en `target/release/arrow`.
  Si `cargo` no está en PATH, usar `~/.cargo/bin/cargo` (toolchain instalada con rustup).
  Toda la lógica vive en `src/lib.rs` (`build_report`, `file_content`); `src/main.rs` es la CLI.
- App de escritorio (egui): `cargo run` **dentro de `gui/`** (o `cargo run --manifest-path
  gui/Cargo.toml`). `gui/` es su **propia raíz de workspace** (igual que lo era `src-tauri/`):
  así `cargo build` en la raíz del repo compila SOLO el parser/CLI y **no arrastra el árbol pesado de
  egui**. El parser se reutiliza vía `arrow = { path = ".." }`, sin duplicar lógica. La app llama a
  `arrow::build_report`/`arrow::file_content` **directo** (no hay dev-server, ni HTTP, ni `invoke`).
  En Linux, build deps de egui/eframe (apt): `libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev
  libxcb-xfixes0-dev libxkbcommon-dev libssl-dev` (ya NO se necesita WebKitGTK/Node).
- Empaquetado: `cargo deb` (`.deb`) y AppImage vía `linuxdeploy` en Linux; `cargo bundle` (`.app`/`.dmg`)
  en macOS. Lo orquesta `.github/workflows/release.yml` en un tag `v*` (pendiente de validar con un tag real).
- **Tests**: 24 tests en el crate del parser (`src/lib.rs` + `src/update.rs`, `cargo test --release` en la
  raíz) **+ 9 tests en `gui/`** (`focus.rs` y `worktrees.rs`, `cargo test --release` dentro de
  `gui/`). Complementan —no reemplazan— la verificación contra datos reales del skill `/verify-parser`.
- **Update check** (`src/update.rs`, capa de red opt-in y secundaria — el parser nunca la llama): consulta
  el último Release de GitHub vía `curl` (sin dep HTTP) y **solo avisa** si hay versión nueva, nunca instala.
  Expuesto en CLI (`--check-update [--json]`), Tauri (`check_update`/`open_url`) y la VersionBadge.

## Modelo de datos (lo NO obvio — léelo antes de tocar el parser)
- Fuente de verdad: transcripts NATIVOS `~/.claude/projects/<dir>/<sessionId>.jsonl`. **No se usa ningún hook.**
- Archivos tocados = records con `toolUseResult.filePath` (solo `Edit`/`Write`/`MultiEdit`; `Bash` NO).
  Re-editar el mismo archivo NO lo duplica: se acumula en una entrada (`ops`/`+/-`/`hunks`). En el
  contrato (`--json`/UI) los archivos salen **ordenados por recencia de edición** (`lastTouched` desc,
  sin fecha al final, desempate por path; ver `build_report_from`) — mismo split que repos/sesiones: el
  `--list` (terminal) sigue alfabético. El `--list` itera el `BTreeMap` interno (por path); la recencia
  vive solo en `build_report_from`.
- **"Open in editor"** (botón en la barra de archivo, **GUI-only**): tabla de editores = **dato** en
  `gui/src/editor.rs` (movido verbatim desde el viejo `src-tauri/`, cero deps de Tauri);
  apertura por familia de sintaxis (VS Code `-g {file}:{line}:{col}`,
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
- **"Worktrees inventory + Clean"** (botón `Worktrees` en la topbar, **Tauri-only**): lista/clasifica
  los worktrees de git por repo (active / stale ≥10 min / "merged → safe to remove") con tamaños bajo
  demanda y `copy cmd`. El botón **`Clean`** (`remove_worktree`/`prune_worktrees` en `worktrees.rs`) es la
  **ÚNICA acción que muta disco** en arrow: corre `git worktree remove` **sin `--force`** (git rehúsa un
  worktree locked/sucio) tras **dry-run + confirmación**, solo en filas que git limpiaría (merged+clean o
  prunable), y tras un borrado real **re-encola un `LoadReport`** para re-escanear. Todo el shelling a
  git vive en `gui/src/worktrees.rs` (GUI-only, movido verbatim desde `src-tauri/`): el parser
  `src/lib.rs` **sigue sin invocar git**. argv directo sin shell, lectura read-only con timeout.
  El modal (`gui/src/worktrees_modal.rs`) corre list/clean síncronos (rápidos) y el cálculo de
  tamaños off-thread. Honestidad: rama por defecto
  **resuelta dinámicamente** (nunca hardcodeada); "merged" en verde **solo** si el tip es ancestro
  (squash/rebase ⇒ "can't tell", nunca verde falso); active/stale = edición reciente de archivos, NO
  proceso vivo; tamaño aproximado. active/stale se deriva en el **modal** desde `lastTouched` (ventana
  de 10 min, `STALE_AFTER` en `gui/src/focus.rs`); `worktrees.rs` solo reporta hechos de git.
- Solo cuentan transcripts de **primer nivel**; los `.jsonl` anidados (subagentes) se ignoran.

## Reglas del proyecto (IMPORTANT)
- **Filtrar `~/.claude/`** (HOME global de Claude): es bookkeeping interno, NO código del usuario.
  Un `.claude/` DENTRO de un repo (settings/skills del proyecto) SÍ se muestra. Filtro por **prefijo del HOME**, no por la subcadena `/.claude/`.
- **Honestidad**: etiquetar "ediciones vía herramientas de Claude", NUNCA "todo lo que hizo Claude"
  (los cambios vía `Bash` no se capturan). Marcar `userModified` con ⚠.
- Formato JSONL **interno, no documentado, volátil, se auto-borra a ~30 días** → **parsing defensivo**:
  una línea inválida se ignora, nunca rompe (parse a `serde_json::Value`, no a structs rígidos).
- `git diff` es solo **vista secundaria** opcional (muchos repos no son git; no atribuye por sesión).
- **No hay capa de transporte**: la app egui llama a `arrow::build_report`/`arrow::file_content`
  **directo** (structs Rust nativos). El parsing y la lectura de disco corren en un **worker thread**
  (`gui/src/worker.rs`, cola mpsc `Cmd`→`Msg`) para no bloquear el frame; el watcher `notify`
  encola `LoadReport`. Los resultados de contenido obsoletos se descartan comparando el path con la
  selección actual (no hay cache explícito como tenía el viejo `api.ts`).

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
  UI viven en los módulos de `gui/src/` (sidebar/diff/topbar/worktrees_modal) y el tiempo
  relativo + buckets de fecha en `gui/src/focus.rs`.
- **Verifica cada cambio del parser contra datos reales** (`/verify-parser`) antes de decir que funciona; reporta la salida, no una afirmación.
- Commits: mensaje en imperativo. Rama `main`. Push solo cuando se pida.

## Skills del proyecto
- `/run-arrow` — compila y levanta la app de escritorio egui (`cargo run` en `gui/`).
- `/verify-parser` — recompila y corre el parser contra `~/.claude` (verificación pass/fail).
- `/inspect-transcript` — recetas `jq` para explorar el formato JSONL al extender el parser.
- `/rust-review` — revisa la calidad del Rust (clippy + rustfmt + checklist de best-practices afinado a arrow). Complementa a `/verify-parser` (datos): este mira el código.
