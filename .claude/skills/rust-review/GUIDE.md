# Guía de best-practices de Rust para arrow

Referencia para `/rust-review`. Criterios derivados de la documentación oficial de Rust
(<https://doc.rust-lang.org/stable/> — The Book, Rust by Example, la API std y las lints de clippy)
**afinados al caso de arrow**: un parser que lee un formato JSONL no documentado y volátil, expuesto
como CLI y como backend de Tauri. No es un resumen de todo Rust; es lo que importa para *este* código.

Norte del proyecto: **honestidad y robustez sobre cleverness**. El parser nunca debe romper ante datos
raros (se auto-borran a ~30 días y cambian entre versiones de Claude Code), y nunca debe afirmar más de
lo que sabe. Ver `CLAUDE.md`.

---

## 1. Manejo de errores

**Regla de oro:** los datos externos (disco, JSON del transcript, variables de entorno) son *no
confiables*. Nunca `.unwrap()`/`.expect()`/`panic!` sobre ellos.

- **Parseo de JSONL** → siempre a `serde_json::Value` (no a structs rígidos con `#[derive(Deserialize)]`).
  Una línea que no parsea se cuenta como `lines_skipped` y se ignora. Patrón canónico en `collect`:
  ```rust
  match serde_json::from_str::<Value>(&line) {
      Ok(v) => ingest(&v, ...),
      Err(_) => c.lines_skipped += 1, // parsing defensivo: formato no documentado
  }
  ```
- **Acceso a campos** → `.get("campo").and_then(Value::as_str)` con un `unwrap_or`/`unwrap_or_default`
  o un `match`/`if let` que continúe ante la ausencia. Nunca indexar `v["campo"]` (eso *sí* paniquea).
- **IO de archivos** → `File::open(...)` que falla hace `continue`/`return`, no propaga pánico. En el
  modo contenido, si el archivo no está en disco → `after_available: false`, string vacío. Degradar, no romper.
- **CLI (`src/main.rs`)**: `fn main() -> anyhow::Result<()>`; usa `?` para errores de presentación
  (serialización, flag `--content` sin `--file`). `anyhow` es correcto aquí: es una app, no una lib que
  otros consuman programáticamente.
- **Comandos Tauri (`src-tauri`)**: devuelven el struct (`ReportOut`/`ContentOut`) directamente. No
  propagan `Result` a la UI: cualquier fallo de IO ya está absorbido dentro de la lib como campos
  vacíos. La ventana nunca debe ver un error de Rust por un transcript raro.
- **`.expect()` permitido** solo en invariantes del *programa*, no de los *datos*: p.ej.
  `tauri::generate_context!()` o `.run(...).expect("error al arrancar la app")` — si eso falla, el
  binario está mal construido, no es culpa de un dato. Justifica cada uno.

## 2. Ownership, borrowing y tipos

- **Parámetros**: acepta `&str` en vez de `String` cuando solo lees (`file_content(projects_dir: &str, ...)`).
  Acepta `&[T]` en vez de `&Vec<T>`. Devuelve owned (`String`, structs) cuando el llamador se queda el dato.
- **`Option<&str>` para filtros opcionales** (`session: Option<&str>`): es el idiom para "puede o no venir".
  La CLI mapea `cli.session.as_deref()`.
- **Evita `.clone()` en rutas calientes.** El barrido de transcripts corre sobre miles de líneas; clonar
  ahí se nota. Clonar al *construir la salida* (`build_report_from`) es aceptable: ocurre una vez.
- **`&mut` acumuladores** (como `scan_transcript(..., before: &mut Option<String>, ...)`) es preferible a
  devolver y fusionar structs en un bucle caliente.
- **Cachés**: `HashMap` para lookup O(1) sin orden (cache `cwd → git_root`); `BTreeMap` cuando el orden
  determinista importa (repos/sesiones/archivos: la salida JSON debe ser estable entre corridas).

## 3. Iteradores sobre índices

Preferir cadenas de iterador a bucles con índice manual — más claro y sin riesgo de out-of-bounds:
```rust
let lines: Vec<String> = h.get("lines").and_then(Value::as_array)
    .map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect())
    .unwrap_or_default();
```
`filter_map(|e| e.ok())` para quedarte con los `Ok` de un walker. `map_while(Result::ok)` para leer
líneas hasta el primer error de IO. Evita `for i in 0..v.len() { v[i] }`.

## 4. Estructura de crates (lo específico de arrow)

- **Lógica en la lib, no en `main.rs`.** `src/lib.rs` expone funciones puras (`build_report`,
  `file_content`, `collect`, `build_report_from`); `src/main.rs` es una cáscara: parsea flags con `clap`,
  llama a la lib, presenta (terminal o JSON). Esto permite que `src-tauri` reuse el parser **sin duplicarlo**
  (`arrow = { path = ".." }`). Si aparece lógica de parsing en `main.rs` o en `src-tauri`, muévela a la lib.
- **Visibilidad**: `pub` solo lo que la CLI o Tauri necesitan. Los helpers internos (`ingest`,
  `scan_transcript`, `git_root`) quedan privados del módulo.
- **`src-tauri` es su propia raíz de workspace** (`[workspace]` vacío en su `Cargo.toml`): así
  `cargo build` desde la raíz compila solo el parser y no arrastra `libwebkit2gtk`. No lo conviertas en
  miembro del workspace raíz.

## 5. El contrato de serialización (no romperlo)

Los structs `*Out` (`ReportOut`, `RepoOut`, `SessionOut`, `FileOut`, `ContentOut`) son el **contrato con
la UI**. Reglas:
- `#[derive(Serialize)]` + `#[serde(rename_all = "camelCase")]`: Rust usa `snake_case`, la UI espera
  `camelCase`. Mantenerlo.
- Renombrar o quitar un campo, o cambiar su tipo, **rompe** `web/src/lib/types.ts` y los componentes
  Svelte. Si cambias un `*Out`, actualiza `types.ts` en el mismo cambio.
- `Option<T>` serializa a `T | null`: la UI ya lo maneja (`title: string | null`). Es la forma honesta
  de decir "puede no haber dato".

## 6. clippy y rustfmt como red de seguridad

- **clippy** detecta lo que el compilador no: `needless_return`, `redundant_clone`, `single_char_pattern`,
  `manual_map`, etc. Trátalo como obligatorio (0 warnings), no como sugerencia. Si un lint es un falso
  positivo, `#[allow(clippy::lint_name)]` **con un comentario** que explique por qué.
- **rustfmt** es el formato canónico: no discutas estilo, ejecútalo. Tras `cargo fmt`, **recompila y
  corre `/verify-parser`**: el formato jamás debe alterar la salida del binario (verificación: md5 de
  `--json`/`--list` idéntico antes/después).

## 7. Lo que NO aplica a arrow (no lo impongas)

- **`async`/await**: la lib es IO síncrono sobre archivos locales; no necesita Tokio. (El watcher de
  Tauri usa un hilo dedicado con `std::thread`, no async — correcto para su caso.)
- **Generics/traits elaborados**: el dominio es concreto (transcripts → structs). No generalices de más.
- **`unsafe`**: no hay ninguna razón para usarlo aquí. Si aparece, es un error.
- **Micro-optimización prematura**: el cuello de botella real era O(historial) por clic (resuelto
  abriendo el transcript por `sessionId`). Perfila antes de optimizar; no añadas complejidad sin medir.
