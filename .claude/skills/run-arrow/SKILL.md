---
description: Compila el parser de Rust y levanta la UI web de arrow en http://localhost:5173. Úsalo cuando el usuario quiera ejecutar, abrir, levantar o ver la app de arrow.
allowed-tools: Bash(cargo build*) Bash(~/.cargo/bin/cargo build*) Bash(npm*)
---

Levanta arrow para verlo en el navegador.

1. Compila el parser (el dev-server ejecuta `target/release/arrow`):
   `cargo build --release`  — si `cargo` no está en PATH: `~/.cargo/bin/cargo build --release`.
2. Si falta `web/node_modules`, instala deps: `npm --prefix web install`.
3. Levanta el dev-server (déjalo en segundo plano): `npm --prefix web run dev`.
4. Dile al usuario que abra **http://localhost:5173**.

Notas:
- Tras cambiar `src/main.rs`, **recompila** (paso 1): el dev-server re-ejecuta el binario en cada
  request, así que la UI tomará los cambios sin reiniciar el server.
- El frontend usa HMR: cambios en `web/src/**` se reflejan solos.
- No tengo navegador para verificar el render visual: pídele al usuario que confirme cómo se ve.
