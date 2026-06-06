---
description: Revisa la calidad del código Rust de arrow (el parser/lib en src/ y la app egui en gui/) aplicando clippy + rustfmt y un checklist de best-practices afinado al proyecto. Úsalo tras tocar src/lib.rs, src/main.rs o gui/, o cuando quieras endurecer el código Rust. No reemplaza a /verify-parser (correctitud de datos); este mira la calidad del código.
allowed-tools: Bash(cargo build*) Bash(~/.cargo/bin/cargo build*) Bash(cargo clippy*) Bash(~/.cargo/bin/cargo clippy*) Bash(cargo fmt*) Bash(~/.cargo/bin/cargo fmt*) Bash(./target/release/arrow*) Read
---

Revisa la calidad del Rust de arrow. **No afirmes "está bien": ejecuta las herramientas y muestra su salida.**

El proyecto tiene **dos crates** (cada uno su propia raíz de workspace):
- raíz `/` → la lib del parser (`src/lib.rs`) + la CLI (`src/main.rs`).
- `gui/` → la app de escritorio nativa (egui/eframe).
Hay que revisar **ambos**. La guía de criterios está en `GUIDE.md` (junto a este archivo) — léela antes de juzgar.

Si `cargo` no está en PATH, usa `~/.cargo/bin/cargo`. Requiere `clippy` y `rustfmt`
(`rustup component add clippy rustfmt` si faltan).

## 1. Lint con clippy (best-practices del compilador)
- Crate raíz: `cargo clippy --release --all-targets` → **0 warnings**.
- App egui: `(cd gui && cargo clippy --release --all-targets)` → **0 warnings**.
- Reporta cada warning con su `-->` archivo:línea. No los silencies con `#[allow]` sin justificarlo.

## 2. Formato con rustfmt
- `cargo fmt --check` en la raíz y en `gui/`. Si hay diffs, muéstralos.
- Para aplicarlos: `cargo fmt` en cada crate. Tras formatear, **recompila** y confirma con
  `/verify-parser` que la salida del binario es idéntica (el formato no debe cambiar el comportamiento).

## 3. Checklist afinado a arrow (revisión manual, ver GUIDE.md)
Lee los archivos y verifica, citando `archivo:línea` como evidencia:
- **Parsing defensivo**: toda lectura de JSONL parsea a `serde_json::Value` y una línea inválida se
  ignora (nunca `.unwrap()`/`panic!` sobre datos del transcript). El formato es volátil.
- **Sin `.unwrap()`/`.expect()` sobre datos externos** (disco, JSON, entorno). Permitido solo en
  invariantes del programa. Señala cada caso y clasifícalo.
- **Errores**: la CLI usa `anyhow::Result`; la app egui consume los structs directo (cualquier
  fallo de IO degrada a campos vacíos / `*_available:false`, nunca rompe la ventana). El parsing y
  la lectura de disco corren en el worker thread (`gui/src/worker.rs`), no en el frame.
- **La lógica del parser NO se duplica**: `arrow-gui` y la CLI llaman a la misma lib (`arrow::`).
  Si ves lógica de parsing copiada, es un bug de diseño. El parser `src/lib.rs` **nunca invoca git**
  (el shelling vive en `gui/src/{editor,worktrees}.rs`).
- **Contrato de datos**: la app egui usa los structs `*Out` del parser **directo** (sin IPC/JSON), así
  que un cambio de campo se ve en compile-time en `gui/` (no hay un `types.ts` que sincronizar).
- **Idioms**: preferir iteradores a índices, `&str` sobre `String` en parámetros, evitar `.clone()`
  innecesarios en rutas calientes (el barrido de transcripts), `match`/`if let` sobre comparaciones frágiles.

## 4. Reporta evidencia
Salida real de clippy/fmt + lista de hallazgos del checklist con `archivo:línea`. Si todo pasa,
dilo con la prueba (el "Finished … 0 warnings" y el `fmt --check` sin diffs), no con un "se ve bien".
