# ROADMAP / workfile de arrow

Archivo de seguimiento entre sesiones. El **roadmap canónico de fases** vive en
[README.md](README.md#roadmap); aquí va el detalle operativo: estado fino, deuda técnica y un
backlog de ideas que aún no están comprometidas a ninguna fase. Las **convenciones** del proyecto
están en [CLAUDE.md](CLAUDE.md); el contrato de datos en [SPEC.md](SPEC.md).

> Última actualización: 2026-05-30 (cierre de Fase 2 + pulido).

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
- **Atajos de teclado** — navegar el árbol y abrir diffs sin ratón (j/k, enter, etc.).

## Deuda técnica / notas

- **Semántica del badge `live`**: hoy es "actividad reciente" (<20 min), no "sesión en ejecución".
  Decisión de diseño pendiente documentada en [SPEC.md](SPEC.md) (sección del watcher) — se aborda
  al avanzar Fase 3.
- **Tiempo relativo congelado**: `relative()`/`isLive()` solo se recalculan al re-render; si el
  report no cambia, el "Nm ago" no avanza. Un tick de reloj independiente lo resolvería (también
  anotado en SPEC.md).
- **Sin CI**: los tests y `/rust-review` se corren a mano. Si el proyecto crece, valdría un workflow
  de GitHub Actions (`cargo test` + `cargo clippy` + `cargo fmt --check`).
