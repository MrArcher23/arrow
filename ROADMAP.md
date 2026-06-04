# ROADMAP / workfile de arrow

Archivo de seguimiento entre sesiones. El **roadmap canónico de fases** vive en
[README.md](README.md#roadmap); aquí va el detalle operativo: estado fino, deuda técnica y un
backlog de ideas que aún no están comprometidas a ninguna fase. Las **convenciones** del proyecto
están en [CLAUDE.md](CLAUDE.md); el contrato de datos en [SPEC.md](SPEC.md).

> Última actualización: 2026-06-04 (**Foco "activo vs idle"**: al abrir arrow sin actividad reciente
> (<20 min) ya no auto-abre ni marca nada como activo — muestra "No active work" + historial colapsado;
> + banner honesto si se borró el worktree de un archivo; marca **ARROW** en mayúscula; titlebar nativa
> oscura en macOS. Antes: **Worktrees inventory** read-only: lista/clasifica los worktrees
> de git que Claude Code crea por sesión —activos / stale ≥10 min / "merged → safe to remove"— con
> tamaños bajo demanda y `copy cmd`, sin borrar nada en la app; + badge de versión en la topbar. Antes:
> distribución macOS (matrix de CI publica el `.dmg` arm64+x64 + `install.sh` one-liner); Fase 2 + pulido:
> zoom, titlebar custom, foco por sesión activa, orden de archivos por recencia, tick de reloj y "Open in editor").

## Estado actual

| Fase | Estado | Notas |
|---|---|---|
| 0 — parser/CLI | ✅ | `src/lib.rs` (lib) + `src/main.rs` (CLI). |
| 1 — UI web (Svelte + CodeMirror) | ✅ | `web/`, dev-server ejecuta el binario. |
| 2 — empaquetado Tauri 2.x | ✅ | `.deb` + AppImage; backend nativo `src-tauri/`; frontend dual-mode. |
| 3 — honestidad + git | ⏳ pendiente | Ver README. Tiene nota de diseño del badge `live` en SPEC.md. |
| 4 — edición + GitHub | ⏳ postergado | Ver README. |

### Pulido aplicado (post-Fase 2)
- **Tests del parser**: 20 tests unitarios en `src/lib.rs` (`#[cfg(test)] mod tests`) con
  transcripts-fixture en tempdir. Cubren lo NO obvio: parsing defensivo, solo-top-level, agrupación
  por raíz git, conteo +/-, filtro de `~/.claude/`, orden de repos y de archivos por recencia,
  metadatos, `file_content`. Correr con `cargo test --release`. Complementan a `/verify-parser` (datos reales).
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
- **Archivos ordenados por recencia de edición** (no alfabético): dentro de una sesión, el archivo
  editado más recientemente va arriba y re-tocar uno lo vuelve a subir al tope en el siguiente
  refresco. El parser (`build_report_from` en `src/lib.rs`) ahora rastrea `last_ts` por archivo
  (`FileChange`) y emite `lastTouched` en `FileOut`, ordenando los archivos desc (sin fecha al final,
  desempate por path). Mismo split que repos/sesiones: el contrato `--json`/UI va por recencia, el
  `--list` (terminal) sigue alfabético/browsable. Cada fila de archivo muestra su "Nm ago"
  (`Sidebar.svelte`). Anclado al **dato** (timestamp), no al reloj. Test: `archivos_ordenados_por_ultima_edicion`.
- **Tick de reloj para el tiempo relativo** (resuelve la deuda de abajo): un `setInterval` de 30 s en
  `App.svelte` reasigna un `now` reactivo que se pasa a `relative()`/`isLive()` (`web/src/lib/time.ts`)
  y al `Sidebar` (prop `now`), así los "Nm ago" envejecen y el **punto verde** caduca a los 20 min sin
  esperar una edición nueva. Es **display-only**: no refetcha ni reordena (la reactividad granular de
  Svelte 5 recalcula solo las etiquetas, no el árbol). Se **pausa con `visibilitychange`** cuando la
  ventana se oculta y se pone al día al volver al foco. Costo: una decena de strings recalculados cada
  30 s — más barato que el poll de respaldo de 15 s que ya existía.

- **"Open in editor"** (v0.1.4, endurecido tras auditoría adversarial pre-release; salto de la auditoría al editor real — la imagen-espejo
  de `/ide`): un botón en la barra de archivo abre el archivo seleccionado en el editor del usuario, en la
  **primera línea cambiada**, delegando en el CLI del editor. arrow NO embebe editor ni LSP → sigue ligero.
  Detección por `$PATH` (`detect_editors`) sobre una **tabla de editores = dato** (`src-tauri/src/editor.rs`);
  apertura por familia de sintaxis (`open_in_editor`): VS Code `-g {file}:{line}:{col}`, Zed/Sublime
  posicional, JetBrains `--line/--column`. **argv directo, sin shell** (evita el bug de espacios
  vscode#39891 / cursor#3796). El parser expone `firstChangedLine` (menor `newStart`, lado after) en
  `ContentOut`. Frontend: `detectEditors`/`openInEditor` en `api.ts` (Tauri-only) + `OpenInEditor.svelte`
  (picker que recuerda la elección en localStorage). **Honestidad**: abre el archivo **actual en disco**
  (after), no el snapshot histórico. Verificado en vivo con VS Code, Kiro, Antigravity, Antigravity IDE y
  Zed. Test: `file_content_expone_primera_linea_cambiada`.
  - **Default de editor**: por ahora *recordado (localStorage) || primero detectado* — decisión deliberada
    (el recordado ya basta: reabrir la app conserva tu elección). Un "default inteligente" (abrir en el IDE
    conectado a Claude vía `~/.claude/ide/*.lock`, o respetar `$EDITOR`/`$VISUAL`) queda como mejora futura.
  - **Diferido (documentado, fuera del MVP):**
    - **Editores de terminal** (nvim/helix/nano/emacs-tui): omitidos a propósito. Una app GUI no les da TTY;
      necesitarían un **emulador de terminal anfitrión** (`<term> -e nvim +line file`) cuya invocación NO está
      estandarizada (gnome-terminal `--`, konsole/alacritty/kitty `-e`…) → requeriría un setting `terminal`.
      Escapes sin-TTY si se retoma: `emacsclient -c`, `gvim`, `neovide`.
    - **JetBrains / Sublime / Kate**: ya están en la tabla (plantillas de su doc oficial) pero **sin verificar**
      en esta máquina (no instalados) → best-effort.
    - **macOS / Windows**: las plantillas por familia ya son cross-plataforma; solo cambia la **resolución del
      binario** (mac: `open -a`/bundle id; Win: `.cmd`). Pendiente para cuando esas plataformas se empaqueten.

- **"Worktrees inventory"** (nuevo, read-only, Tauri-only — botón `Worktrees` en la topbar → modal): lista
  los **git worktrees** que Claude Code va creando por sesión, agrupados por repo, y los clasifica en
  **active / stale / merged ("safe to remove")**, con tamaño en disco **bajo demanda** y un `copy cmd` por
  fila. **NO borra nada desde la app** (el botón `Clean` se difiere — ver backlog); la imagen-espejo del
  problema que "Open in editor" resolvió para la edición, pero para la **higiene de disco**.

  - **Por qué se agrega (el dolor real):** la app de escritorio de Claude Code crea un worktree por sesión
    en `<repo>/.claude/worktrees/<nombre>/` (nombre random `adjetivo-verbo-sustantivo`, ej. `wise-plotting-graham`).
    Son **git worktrees enlazados reales**: comparten el object store (`.git` es un *archivo puntero*), pero
    **cada uno materializa toda la copia de trabajo** (su `node_modules`/`target`/`dist`). Medición de uno de
    ejemplo: **9.8 GB de working tree vs 2.3 MB de objetos git**. Y **se acumulan**: si la sesión dejó cambios,
    Claude no los borra al salir. Quien *steerás* mucho a Claude Code (justo el público de arrow) termina con
    decenas de worktrees colgados comiendo disco y un `git worktree list` ilegible. Disparador adicional:
    **ya se estaban colando en arrow** — un cwd dentro de un worktree hacía que `git_root` lo tratara como
    "repo" propio (ver fix abajo), inflando el contador de repos y mezclando worktrees con repos reales.

  - **Diseño (clona el patrón de `editor.rs`):** todo el shelling a git vive en un módulo nuevo Tauri-only
    `src-tauri/src/worktrees.rs` (el parser `src/lib.rs` **sigue sin invocar git nunca** — premisa del
    producto: "without depending on git or hooks"). Dos comandos: `worktrees(repo_roots)` corre
    `git -C <root> worktree list --porcelain` por repo (argv directo, **sin shell**, como `open_in_editor`) y
    `worktree_sizes(paths)` suma bytes con `walkdir` **solo cuando se pulsa "Calculate sizes"**. Frontend:
    `loadWorktrees`/`calcWorktreeSizes` en `api.ts` (Tauri-only, `[]` en el navegador como `detectEditors`) +
    `WorktreesModal.svelte`. **Sin nuevos permisos ACL** (un `#[tauri::command]` con `std::process::Command`
    se autoriza por estar en `generate_handler!`, igual que `open_in_editor`).

  - **Rendimiento — impacto negligible (medido):** el **hot path no se toca** (`report()`/`--json`/CLI/watcher
    quedan exactamente igual; el feature NO entra en `build_report`). Abrir el modal = un `git worktree list`
    por repo (~1 ms c/u; ~33 ms para 39 repos en serie, es plumbing que solo lee metadata de
    `$GIT_DIR/worktrees/`, no escanea el working tree). La **única** operación cara es el tamaño en disco
    (walkdir sobre `node_modules` en frío puede tardar segundos), por eso es **bajo demanda**, off-thread y
    cancelable. El watcher (`~/.claude/projects`) **no** se extiende a los working trees de los repos
    (explotaría `inotify`); el modal es pull-only con refresco manual.

  - **Honestidad (el filo del feature):**
    - **Rama por defecto resuelta dinámicamente** (`symbolic-ref refs/remotes/origin/HEAD` → fallbacks
      locales → `main`/`master`), **nunca hardcodeada** — este mismo repo es `main`, así que un "master"
      fijo mal-etiquetaría todo. Si no se resuelve ⇒ se **deshabilita** la clasificación de merged.
    - **"merged → safe to remove" (verde) solo si se prueba:** `git merge-base --is-ancestor <tip> <default>`
      = 0. El **squash/rebase merge** (default de muchos equipos) deja la rama sin ser ancestro aunque el
      trabajo ya esté en `main` ⇒ se muestra **"can't tell"**, nunca un "no fusionado" tajante ni un verde
      falso. (Reproducido en este repo: `fix/live-refresh-open-diff` no es ancestro pero `git cherry` lo da
      por "landed" y el contenido aún difiere — por eso `cherry` NO es luz verde de borrado.)
    - **active / stale** = actividad de **edición de archivos** reciente bajo el path del worktree
      (de `lastTouched` del report; ventana de 10 min, la misma `BURST_WINDOW` de `time.ts`), **no** un
      proceso vivo — mismo caveat que el punto verde / "active session". La etiqueta dice "recent edits".
    - **detached / locked / prunable / dirty** se muestran honestos: detached no se clasifica como merged;
      locked no se ofrece como "safe to remove" (`worktree remove` lo rechaza); prunable sugiere
      `git worktree prune`, no `remove`; el **worktree principal** se marca y nunca se ofrece para borrar.
    - **Tamaño = aproximado** (bytes aparentes vía `walkdir`, sin seguir symlinks); se etiqueta como tal.
    - **Read-only:** `copy cmd` copia el `git worktree remove <path>` para que **el usuario** lo corra; arrow
      no muta el estado de git. (El botón `Clean` in-app se difiere a propósito — ver backlog.)

  - **Fix colateral en el parser (`git_root`, `src/lib.rs`):** un worktree enlazado tiene un `.git` que es un
    **FILE** (`gitdir: …/.git/worktrees/<name>`), y `git_root` usaba `dir.join(".git").exists()` (true también
    para un file) ⇒ paraba en el worktree y lo listaba como repo fantasma. Ahora, si `.git` es un file, **lee
    el puntero y re-ancla a la raíz del repo principal** (sigue siendo *solo filesystem*, no invoca git ⇒ no
    rompe la regla del lib). Test nuevo paralelo a `agrupa_por_raiz_git`; los 20 tests previos no se afectan
    (todos crean `.git` como **directorio**, que toma el camino sin cambios).

- **Badge de versión en la topbar** (`v0.1.x`, clicable → popover "About" con link a Releases): la versión se
  inyecta en build con un `define` de Vite leído de `src-tauri/tauri.conf.json` (la fuente que sella el bundle),
  expuesta en `web/src/lib/version.ts` (misma convención aislada que `zoom.ts`/`window.ts`). Funciona igual en
  la app y en `npm run dev` (es síncrono, sin ACL `app:default` ni `getVersion()` async). Texto de UI en inglés.

- **Foco "activo vs idle"** (v0.1.7 — resuelve que arrow presentara trabajo viejo como si fuera actual):
  arrow distingue **trabajo activo** (última edición < `LIVE_WINDOW` = 20 min) de **idle**. En idle —p.ej.
  abrir arrow al día siguiente sin estar trabajando— **no** auto-abre ningún diff, **no** muestra punto
  verde, y el sidebar pasa a un estado **minimalista**: "No active work · last activity Nh ago" + historial
  colapsado (navegable). `focusRepos(repos, now)` (`time.ts`) devuelve `[]` cuando la actividad más reciente
  supera la ventana; el auto-select inicial (`App.svelte`) se condiciona a `isLive`. Anclado al **dato**
  (timestamp), no al reloj. **Self-heal**: si el archivo abierto desaparece del report, se limpia la
  selección (no se queda clavado). Todo **frontend**, cross-plataforma (también arregla el bug reportado en
  Mac). **Diagnóstico** (auditoría de 6 agentes con reproducción real): la causa NO era el `git_root` de
  0.1.6 (verificado compilando ambas versiones) — era pre-existente: el `after` se lee de disco y al borrar
  el worktree desaparece, dejando solo el `before` (= lo ya pusheado) + la selección clavada.
- **Banner honesto de "worktree borrado"** (`DiffView.svelte`): si abres un archivo cuyo worktree fue
  eliminado, el `after` ya no existe en disco; en vez de pintarlo como "deleted file" se muestra *"worktree
  deleted — showing the last recorded state, not a live diff"*. Detectado por el path `/.claude/worktrees/`
  en `content.file` (sin tocar el contrato del parser).
- **Marca en mayúscula** (`ARROW`): `text-transform: uppercase` en `.brand` (la fuente sigue "arrow").
- **Titlebar nativa oscura en macOS** (`src-tauri`): por defecto seguía la apariencia del sistema (modo
  claro ⇒ barra **blanca**) y chocaba con el tema oscuro de arrow. Ahora `set_theme(Dark)` + `hiddenTitle`
  (oculta el título nativo duplicado). Trade-off: con un tema CLARO de arrow la barra desentona un poco.
  **Sin verificar en hardware Mac** — el `.dmg` de CI lo prueba el mantenedor/líder.

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
- **Botón `Clean` in-app para worktrees** (diferido del "Worktrees inventory"; sería la **única acción
  destructiva** de arrow, así que cruza la línea read-only y encaja en la Fase 4 de edición). Hoy el modal
  solo lista + `copy cmd`. Un `Clean` exigiría **blindaje fuerte**, según la auditoría de seguridad: correr
  `git worktree remove` (nunca `rm -rf`, luego `worktree prune`); **rechazar** worktrees con
  `git status --porcelain` no vacío (sucios/untracked — p.ej. el `.deb` sin trackear de este repo se
  perdería con `--force`); **nunca** `--force` ni `branch -d/-D` automático (una rama sin upstream es la
  única copia); tratar detached/locked como hard-stop; nunca el worktree principal; y mostrar el comando
  exacto antes de ejecutarlo. **Riesgo central** (reproducido): un detector de "merged" laxo (que confíe en
  `git cherry` en vez de ancestro estricto o el forge) + un borrado de rama destruiría trabajo no fusionado.
  Por eso el MVP se queda en read-only.
- **Detección de "merged" más fina para squash/rebase** — hoy un squash-merge se reporta honesto como
  "can't tell" (la rama no es ancestro aunque el trabajo esté en `main`). Mejora: consultar el forge
  (`gh pr view --json mergedAt`) o `range-diff` para **subir** la confianza a "likely merged", sin que deje
  de ser estricto para autorizar un borrado. Requiere red / `gh` ⇒ opt-in, time-boxed.
- **Commit asistido por IA sin abrir el IDE** (detalla la **Fase 4: edición + GitHub — commits/PRs**;
  feedback de un usuario real probando arrow). La observación: arrow ya elimina el motivo #1 para abrir
  el IDE —*revisar* lo que tocó Claude—; el motivo #2 que aún arrastra al IDE es **hacer el commit con la
  IA integrada del editor**. El pedido: integrar un flujo de Git en arrow cuyo mensaje de commit lo
  redacte *la IA que el usuario ya usa*. Cita: *"a mí me gusta entrar al IDE solo a ver esto que hiciste
  y hacer el commit con la IA integrada... ¿será que podría integrar ese flujo de Git con la IA que ya
  uno use?"*.
  - **Principio de diseño (respeta el ADN de arrow):** NO embeber un LLM ni pedir API keys — arrow es
    deliberadamente *"sin chat con IA"* y ligero. En vez de eso, **delegar en la IA que el usuario ya
    tiene**: como su público ya vive en Claude Code, arrow podría pasarle el diff a Claude (p.ej. `claude
    -p` en modo headless) para redactar el mensaje y/o hacer el commit. Mismo espíritu que el `copy cmd`
    del inventario de worktrees: **arrow prepara, la herramienta del usuario ejecuta** (arrow no se vuelve
    pesado ni "otra IA más").
  - **Honestidad / límites:** el commit es una acción de **ESCRITURA** (salto desde el read-only actual,
    por eso vive en la Fase 4). Exige: confirmación explícita, mostrar **exactamente qué se va a stagear**,
    y la salvedad de siempre — arrow ve *"ediciones vía herramientas de Claude"*, NO los cambios por `Bash`;
    un commit desde arrow no debe pretender capturar lo que no rastrea. Considerar el **worktree** correcto
    (commitear en el árbol que toca, no en el principal).
  - **Escalón mínimo (sin IA, ya alineado al `copy cmd`):** un botón que prepare/copie el `git add` +
    `git commit -m "…"` para que el usuario lo dispare; la capa de IA (mensaje autogenerado por Claude) es
    la mejora encima. Así se entrega valor incremental sin cruzar de golpe a "arrow commitea por ti".

## Deuda técnica / notas

- **Semántica del badge `live`** — ✅ resuelto (post-Fase 2; ver "Mejoras visuales / UX"). El foco del
  sidebar pasó de "todos los repos con actividad <20 min" a "los de la **sesión activa** (actividad más
  reciente) + ráfaga ~10 min"; el **punto verde** quedó como único indicador (se quitó el badge de
  texto `live`). Sigue honesto: "activa" = actividad más reciente en disco, no proceso vivo. Bonus: el
  foco se ancla al **dato** (no al reloj), así que no sufre el congelamiento de tiempo relativo (abajo).
- **Tiempo relativo congelado** — ✅ resuelto (ver "Mejoras visuales / UX"). `relative()`/`isLive()`
  solo se recalculaban al re-render; si el report no cambiaba, el "Nm ago" no avanzaba y el punto verde
  no caducaba. Ahora un tick de reloj independiente (30 s, `App.svelte`) reasigna un `now` reactivo que
  reciben `relative()`/`isLive()` y el `Sidebar`; se pausa con la ventana oculta. Display-only (no
  refetcha ni reordena). La nota de diseño en SPEC.md quedó actualizada (decisión (b) tomada).
- **Reconstrucción del `before` cuando `originalFile` es `null`** — ✅ resuelto (fix
  `before-null-originalfile`). Claude Code emite `originalFile:null` en parte de los Edit aunque
  haya `structuredPatch`; antes eso hacía que un archivo existente se pintara como "new file" (una
  sola columna, sin el diff). Ahora `resolve_before` (`src/lib.rs`) lo resuelve en cascada: (1)
  `originalFile` inline del 1er Edit; (2) el `originalFile` exacto más temprano de una edición posterior
  reaplicando en reversa las previas; (3) el último `Read` **completo** (los parciales/offset se
  descartan); (4) reaplicar en reversa TODAS las ediciones de la sesión sobre el `after` de disco. Cada
  reconstrucción se **verifica por hunk** (un hunk que no cuadra ⇒ drift ⇒ se aborta y queda "no
  disponible", honesto). Verificado con datos reales: `EstadisticasV2.tsx` (vía Read completo) y
  `StatsUI.tsx` (4 Edits todos con `originalFile:null` + solo Reads parciales ⇒ reverse-apply desde
  disco, before=1488 líneas) muestran su diff. **Endurecido tras auditoría adversarial** (20 agentes):
  (a) `create` (Write de archivo nuevo, `originalFile:null`, 0 hunks) ⇒ before vacío = "new file" en vez
  de "no disponible"; (b) el `Read` snapshot solo se usa si cuadra con el 1er Edit (`snapshot_consistent`),
  evitando misatribuir un cambio no rastreado entre Read y Edit; (c) `--content` sin `--session` elige la
  sesión más reciente, sin mezclar ediciones de varias sesiones; (d) hunks solapados ⇒ se aborta.
  **Límites conocidos (documentados en el código, best-effort, no mienten):** la verificación es local a
  las regiones tocadas, así que el reverse-apply desde un disco desfasado puede arrastrar cambios
  fuera-de-hunk (aparecen igual en before y after ⇒ no se atribuyen como diff); el marcador
  `\ No newline at end of file` no se modela (no aparece en `structuredPatch` real). **Posible mejora
  (Fase 3):** usar `~/.claude/file-history/<sessionId>/<sha256(path)[:16]>@v<n>` (snapshot canónico) para
  los casos con drift donde el reverse-apply aborta.
- **Build de macOS en CI** — 🟡 configurado, pendiente el primer tag real. `release.yml` es ahora una
  **matrix** (Linux `ubuntu-22.04` + `macos-14` arm64 + `macos-13` Intel) preparada para que cada tag
  `v*` compile y publique los dos `.dmg` (ad-hoc signed, sin secretos) junto al `.deb` + AppImage, en
  runners de macOS **gratis** (sin Mac física). Encima va `install.sh` (raíz), el one-liner
  `curl … | bash` que baja el `.dmg` del último Release y lo deja en `/Applications` (falla con un
  mensaje claro si todavía no hay `.dmg`). **Aún no se ha empujado ningún tag con esta matrix**, así
  que no existe un `.dmg` publicado todavía: el primer `v*` lo generará. **Pendiente:** correr ese
  primer tag, la firma/notarización (Apple Developer, ~99 USD/año) para quitar la fricción de
  Gatekeeper, y un **Homebrew Cask** (`brew install --cask`) con upgrade/uninstall.
- **Aviso de actualización (check-for-updates)** — ✅ implementado. arrow consulta el último Release de
  GitHub y avisa si hay una versión más nueva, **sin descargar ni instalar nada** (honesto: unsigned ⇒
  un auto-updater silencioso pelearía con Gatekeeper, por eso solo *avisa*). Capa de red **opt-in y
  secundaria** en `src/update.rs` (fuera del hot path del parser; shell-out a `curl`, sin dep HTTP
  nueva, time-boxed, degrada con `error` en vez de panic). Expuesto en CLI (`--check-update [--json]`),
  Tauri (comando `check_update` + `open_url` para abrir el release) y en la **VersionBadge** (punto verde
  + "Update available → vX.Y.Z" + "Open release"). Tests de orden de versión en `update.rs`.
  **Pendientes (siguen en backlog):** auto-instalación real vía `tauri-plugin-updater` (requiere keypair
  de firma del updater + `latest.json` firmado en el job de release; mejor **tras** la notarización) y el
  Homebrew Cask de arriba para el upgrade nativo en Mac.
- **macOS sin verificar en hardware**: el código ya está adaptado a Mac (titlebar nativa + font-weight
  por OS; ver [MACOS.md](MACOS.md)) y la matrix ya **construirá** el `.dmg` en CI, pero el bloque
  `cfg(target_os = "macos")` y el `install.sh` aún **no se probaron en una Mac real** → falta correr
  un tag y confirmar el look de la titlebar, que el `.dmg` monta y que el one-liner instala bien.
