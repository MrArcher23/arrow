# arrow

**Visor de auditoría de Claude Code.** Responde a una sola pregunta, de forma fiable:
*¿qué archivos tocó Claude, en qué repo, con qué diff y en qué sesión?* — sin abrir un IDE,
sin chat con IA, y **sin depender de git ni de hooks**.

> Estado: **Fase 0** (parser/CLI). Valida la capa de datos antes de invertir en la UI de escritorio.

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
`--list`, `--json`.

## Roadmap

- [x] **Fase 0** — parser nativo `JSONL → repo/sesión/archivo/diff`, salida en terminal y `--json`.
- [ ] **Fase 1** — UI web local (Vite) consumiendo el `--json`: sidebar `repos → archivos`
      (estilo Antigravity) + diff por archivo con **CodeMirror 6 + `@codemirror/merge`**.
- [ ] **Fase 2** — empaquetar en **Tauri 2.x** (`.deb`/AppImage). El parser de Rust pasa a ser
      el backend nativo (sin sidecar). Watcher por mtime para refresco en vivo.
- [ ] **Fase 3** — honestidad + git: toggle "git diff working tree", marcado de `userModified`,
      timeline point-in-time reusando `file-history`.
- [ ] **Fase 4** (postergado) — edición real con guardado a disco, integración GitHub (PRs/commits).

## Stack

Rust (parser/backend) · CodeMirror 6 (UI, Fase 1+) · Tauri 2.x (empaquetado, Fase 2+).
Sobre Linux/WebKitGTK habrá que aplicar `WEBKIT_DISABLE_DMABUF_RENDERER=1` y compensar el
bug de `font-weight` (+100) — ambos documentados para la Fase 2.

## Licencia

MIT.
