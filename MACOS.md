# arrow en macOS

Guía para **compilar, instalar y distribuir** arrow en macOS (`.app` / `.dmg`). El parser y la
app egui son los mismos que en Linux; este documento cubre solo lo específico de Mac. Contexto
general en [README.md](README.md), convenciones en [CLAUDE.md](CLAUDE.md).

> **Importante:** un build de macOS **solo se compila en una Mac** (con Xcode Command Line Tools).
> No hay cross-compile viable desde Linux. Si no tenés una Mac, la alternativa es CI con un runner
> `macos` (ver [Build en CI](#build-en-ci-sin-mac)).

## Cómo se adapta la app a macOS

Tras la migración a **egui/eframe**, macOS es casi *zero-config*: eframe crea una **ventana nativa
real** (winit), así que la titlebar con semáforos 🔴🟡🟢, el render de fuentes y el DPI los maneja el
SO sin código específico. Ya **no** hay titlebar custom, ni hack de `font-weight`, ni WebKitGTK, ni
mitigaciones de webview (todo eso era de la época Tauri). La fuente de datos es `~/.claude/projects`
vía `$HOME` (siempre existe en Mac). Lo único distinto: el bundle es `.app`/`.dmg` (vs `.deb`/AppImage).

## Requisitos (una sola vez en la Mac)

```bash
# Xcode Command Line Tools (clang, linker, SDK)
xcode-select --install

# Rust (si no está instalado)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# cargo-bundle (genera el .app)
cargo install cargo-bundle
```

> A diferencia de Linux, macOS **no** necesita librerías de sistema aparte para egui.

## Traer el código

```bash
git clone https://github.com/MrArcher23/arrow.git
cd arrow/gui      # la app es su propia raíz de workspace
```

## Generar el instalable (`.app` + `.dmg`)

```bash
# desde gui/
cargo bundle --release
# → target/release/bundle/osx/arrow.app

# empaquetar en .dmg (lo que hace CI)
hdiutil create -volname "arrow" \
  -srcfolder target/release/bundle/osx/arrow.app \
  -ov -format UDZO arrow.dmg
```

La metadata del bundle (nombre, identifier `dev.diegosanchez.arrow`, icono) vive en
`[package.metadata.bundle]` de `gui/Cargo.toml`.

## Instalar / probar

- Arrastrar `arrow.app` a **Aplicaciones** desde el `.dmg`, o ejecutarlo directo:
  `open target/release/bundle/osx/arrow.app`.

### Gatekeeper (binario sin firmar)

El binario **no está firmado ni notarizado** (CI lo firma *ad-hoc*), así que la primera vez macOS lo
bloqueará. Soluciones: **clic derecho → Abrir** (solo la primera vez), o quitar la cuarentena:

```bash
xattr -dr com.apple.quarantine /Applications/arrow.app
```

Para distribuir sin esa fricción haría falta **firma + notarización** con una cuenta de Apple
Developer (~99 USD/año) — fuera del alcance del MVP.

## Build en CI (sin Mac)

`.github/workflows/release.yml` está preparado para compilar el `.dmg` en la nube en cada tag `v*`:
una **matrix** con runners `macos-14` (Apple Silicon) y `macos-13` (Intel) corre `cargo bundle
--release`, firma *ad-hoc* y empaqueta con `hdiutil`, publicando `arrow_aarch64.dmg` /
`arrow_x64.dmg` en el GitHub Release (junto al `.deb` + AppImage de Linux). Los runners de macOS son
**gratis en repos públicos**. **Pendiente de validar con un tag real.**

Sobre ese `.dmg`, `install.sh` (raíz del repo) da el instalador de un comando:
`/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/MrArcher23/arrow/main/install.sh)"`
(busca el `.dmg` con sufijo `_aarch64`/`_x64` según el chip).

## Estado

- ✅ Código nativo egui — sin ajustes específicos de Mac salvo el empaquetado (winit da la ventana
  nativa). Mucho más simple que la era Tauri.
- 🟡 **Matrix de CI + `install.sh` escritos** (cargo-bundle + hdiutil) pero **aún no ejecutados** — el
  primer tag `v*` publicará los `.dmg`.
- ⏳ **Sin verificar en una Mac todavía:** falta correr un tag real y confirmar que el `.dmg` monta,
  abre y que `install.sh` deja la app en `/Applications`.
- ⏳ **Firma/notarización** (Apple Developer) y **Homebrew Cask** siguen pendientes (ver
  [ROADMAP.md](ROADMAP.md)).
