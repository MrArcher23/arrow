# arrow — visor de auditoría de Claude Code

App de escritorio que audita **qué archivos tocó Claude Code**:
`repo → sesión → archivos → diff`, **sin chat con IA**. Ver @README.md para overview y roadmap.

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
  `cargo tauri` ejecuta `beforeDevCommand` desde la **raíz del repo** (de ahí `npm --prefix web`).
  `src-tauri/` es su **propia raíz de workspace**: `cargo build` en la raíz NO arrastra el backend Tauri.
- Build del frontend: `npm --prefix web run build`.
- **No hay tests**: la verificación es ejecutar el parser contra datos reales → skill `/verify-parser`.

## Modelo de datos (lo NO obvio — léelo antes de tocar el parser)
- Fuente de verdad: transcripts NATIVOS `~/.claude/projects/<dir>/<sessionId>.jsonl`. **No se usa ningún hook.**
- Archivos tocados = records con `toolUseResult.filePath` (solo `Edit`/`Write`/`MultiEdit`; `Bash` NO).
- Diff = `toolUseResult.structuredPatch` (hunks). `--content` reconstruye before (primer `originalFile`) / after (disco).
- Metadatos de sesión: `ai-title`→título, `last-prompt`, `timestamp`→actividad.
- Repo = **raíz git del `cwd`** de la sesión (`git_root`): fusiona subdirs como `web/` con su repo.
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
- Comunícate en **español**; comentarios de código en español, identificadores en inglés.
- **Texto visible en la UI en INGLÉS** (labels, estados, títulos, tooltips, banners). arrow es una
  app de marca inglesa. Ej: `live`, `History`, `Other repos`, `new file`, `5m ago`. Los strings de
  UI viven en los componentes Svelte y en `web/src/lib/time.ts` (tiempo relativo y buckets de fecha).
- **Verifica cada cambio del parser contra datos reales** (`/verify-parser`) antes de decir que funciona; reporta la salida, no una afirmación.
- Commits: mensaje en imperativo; co-autor `Claude Opus 4.8 (1M context)`. Rama `main`. Push solo cuando se pida.

## Skills del proyecto
- `/run-arrow` — compila el parser y levanta la UI.
- `/verify-parser` — recompila y corre el parser contra `~/.claude` (verificación pass/fail).
- `/inspect-transcript` — recetas `jq` para explorar el formato JSONL al extender el parser.
- `/rust-review` — revisa la calidad del Rust (clippy + rustfmt + checklist de best-practices afinado a arrow). Complementa a `/verify-parser` (datos): este mira el código.
