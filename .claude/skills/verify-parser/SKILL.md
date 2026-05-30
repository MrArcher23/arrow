---
description: Recompila el parser de arrow y lo ejecuta contra los datos reales de ~/.claude para verificar que funciona. No hay tests; esta es la verificación pass/fail. Úsalo tras cambiar src/main.rs o cuando dudes de la salida del parser.
allowed-tools: Bash(cargo build*) Bash(~/.cargo/bin/cargo build*) Bash(./target/release/arrow*) Bash(jq*)
---

Verifica el parser contra datos reales. **No afirmes que funciona: ejecútalo y muestra la salida.**

1. Compila release: `cargo build --release` (o `~/.cargo/bin/cargo build --release`).
2. Resumen general: `./target/release/arrow --list | head -40`.
   Debe listar `repo → sesión` (con **título legible**, no UUID) → archivos con `+/-`.
3. Invariantes con `--json` (deben cumplirse siempre):
   - **0 archivos internos de Claude** (filtro del HOME):
     `./target/release/arrow --json | jq --arg h "$HOME/.claude/" '[.repos[].sessions[].files[].path|select(startswith($h))]|length'` → **0**
   - **Sin repos fantasma**: revisa que `[.repos[].cwd|split("/")|last]` sean repos reales,
     no subcarpetas sueltas (`utils`, `layout`, `Stats`…). Señal de bug en la agrupación `git_root`.
   - Repos ordenados por recencia (el de actividad más nueva primero).
4. Diff de un archivo concreto:
   `./target/release/arrow --content --session <id> --file <ruta> | jq '{beforeAvailable,afterAvailable,ops}'`.
5. Reporta la **evidencia** (salida real de cada chequeo), no un "funciona".

Si tocaste la agrupación o el filtrado, compara el `repoCount` antes/después para detectar regresiones.
