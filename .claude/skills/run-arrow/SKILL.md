---
description: Compila y levanta la app de escritorio nativa (egui) de arrow. Úsalo cuando el usuario quiera ejecutar, abrir, levantar o ver la app de arrow.
allowed-tools: Bash(cargo build*) Bash(cargo run*) Bash(~/.cargo/bin/cargo build*) Bash(~/.cargo/bin/cargo run*)
---

Levanta la app de escritorio de arrow (egui/eframe, Rust puro — sin webview ni Node).

1. Corre la app **dentro de `gui/`** (su propia raíz de workspace):
   `cargo run --manifest-path gui/Cargo.toml`
   — si `cargo` no está en PATH, usa `~/.cargo/bin/cargo`.
   (Para release: `cargo run --release --manifest-path gui/Cargo.toml`.)
2. Se abre una ventana nativa que lee `~/.claude/projects` directamente (sin dev-server ni HTTP).

Notas:
- En Linux la build necesita las deps de egui/eframe (apt): `libgtk-3-dev libxcb-render0-dev
  libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev`. Ya **no** se necesita WebKitGTK/Node.
- La app reusa el parser (`arrow = { path = ".." }`); tras tocar `src/lib.rs` (el parser), recompilar
  arrow-gui toma los cambios automáticamente (es una dep path).
- Para ver el render sin pantalla: lanzar en segundo plano y capturar con `grim` (Wayland), luego leer
  el PNG. Si no, pídele al usuario que confirme cómo se ve.
