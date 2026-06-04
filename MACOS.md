# arrow en macOS

Guía exclusiva para **compilar, instalar y distribuir** arrow en macOS (`.app` / `.dmg`).
El parser y la UI son los mismos que en Linux; este documento cubre solo lo específico de Mac.
Contexto general en [README.md](README.md), convenciones en [CLAUDE.md](CLAUDE.md).

> **Importante:** un build de macOS **solo se compila en una Mac** (con Xcode Command Line
> Tools). No hay cross-compile viable desde Linux/Windows: la toolchain, WKWebView y la firma
> son de Apple. Si no tenés una Mac, la alternativa es CI con un runner `macos` (ver
> [Build en CI](#build-en-ci-sin-mac)).

## Cómo se adapta la app a macOS

arrow es **un solo código** que se ajusta al sistema operativo al arrancar (no hay un binario
"de Mac" distinto a nivel de fuente). Los ajustes específicos:

| Aspecto | Linux / Windows | macOS |
|---|---|---|
| **Titlebar** | custom (`decorations:false`) con botones `─ ▢ ✕` propios | decoración **nativa** (semáforos 🔴🟡🟢); los botones custom se ocultan |
| **font-weight** | `350` (compensa el render pesado de WebKitGTK) | peso **normal** (WKWebView ya lo renderiza bien) |
| **Fuente de datos** | `~/.claude/projects` vía `$HOME` | igual — en Mac `$HOME` siempre existe → **zero-config** |
| **Bundle** | `.deb` + `.AppImage` | `.app` + `.dmg` (se piden por CLI) |

Dónde vive cada ajuste (por si hay que tocarlo):
- **Detección de plataforma:** `web/src/lib/platform.ts` (`isMac`/`isWindows`/`isLinux` +
  `applyOsClass`, que marca `<html>` con `is-mac`/`is-windows`/`is-linux`). Se aplica en
  `web/src/main.ts` antes de montar.
- **font-weight por OS:** `web/src/app.css` → la regla `:root.is-linux { font-weight: 350 }`
  (en Mac no aplica, queda el peso normal).
- **Titlebar nativa en Mac:** `src-tauri/src/lib.rs` → en el `setup()`, bajo
  `#[cfg(target_os = "macos")]`, se llama a `win.set_decorations(true)` para restaurar los
  semáforos nativos. Los botones de ventana custom se ocultan con el guard `!isMac` en
  `web/src/components/WindowControls.svelte`.

## Requisitos (una sola vez en la Mac)

```bash
# Xcode Command Line Tools (clang, linker, SDK)
xcode-select --install

# Rust (si no está instalado)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# CLI de Tauri 2.x
cargo install tauri-cli --version "^2"

# Node 22 y npm (la versión que usa el CI; si no están). Con Homebrew:
brew install node
```

> A diferencia de Linux, macOS **no** necesita `libwebkit2gtk` ni dependencias de sistema
> aparte: WKWebView viene con el OS.

## Traer el código

```bash
git clone https://github.com/MrArcher23/arrow.git
cd arrow
npm --prefix web install
```

Si ya lo tenías clonado:

```bash
cd arrow && git pull && npm --prefix web install
```

## Generar el instalable (`.app` + `.dmg`)

```bash
cargo tauri build --bundles app dmg
```

> El flag **`--bundles app dmg` es obligatorio**: el `tauri.conf.json` declara los targets de
> Linux (`deb`, `appimage`), así que sin el flag el build de Mac fallaría/omitiría los formatos
> correctos. El flag los sobreescribe sin tocar el config (riesgo cero para el build de Linux).

Salida (la arquitectura depende del chip: `aarch64` en Apple Silicon, `x64` en Intel):

- **`.dmg`** → `src-tauri/target/release/bundle/dmg/arrow_0.1.0_<arch>.dmg`
- **`.app`** → `src-tauri/target/release/bundle/macos/arrow.app`

## Instalar / probar

Opción A — arrastrar a Aplicaciones desde el `.dmg` (lo normal para distribuir).

Opción B — ejecutar el `.app` directo:

```bash
open src-tauri/target/release/bundle/macos/arrow.app
```

### Gatekeeper (binario sin firmar)

Como el binario **no está firmado ni notarizado**, la primera vez macOS lo bloqueará
("no se puede abrir porque proviene de un desarrollador no identificado"). Soluciones:

- **Clic derecho → Abrir** sobre la app (solo la primera vez), o
- quitar el atributo de cuarentena:

  ```bash
  xattr -dr com.apple.quarantine src-tauri/target/release/bundle/macos/arrow.app
  ```

Para distribuir sin esa fricción haría falta **firma + notarización** con una cuenta de Apple
Developer (~99 USD/año) — fuera del alcance de este MVP.

## Qué deberías ver en macOS

- **Semáforos nativos** 🔴🟡🟢 arriba a la izquierda (no los botones `─ ▢ ✕` de Linux).
- **Texto con peso normal** (sin el "fino" que compensa WebKitGTK en Linux).
- El sidebar `repo → sesión → archivos` + diff, leyendo `~/.claude/projects` **sin configurar
  nada** (basta tener Claude Code con alguna sesión que haya editado archivos).

## Troubleshooting

- **"command not found: cargo"** → abrí una terminal nueva tras instalar Rust, o
  `source "$HOME/.cargo/env"`.
- **"command not found: tauri"** → `cargo tauri` se invoca vía cargo; confirmá
  `cargo tauri --version`. Si falla, reinstalá: `cargo install tauri-cli --version "^2"`.
- **El build no genera `.dmg`** → asegurate de incluir `--bundles app dmg`. Sin el flag toma
  los targets de Linux del config.
- **Pantalla en blanco** → no debería pasar en Mac (la mitigación `WEBKIT_DISABLE_DMABUF_RENDERER`
  es solo de Linux y está gateada por OS). Si ocurre, reportar con la versión de macOS.
- **Los semáforos pisan el texto "arrow"** o querés el look "contenido bajo la barra" (estilo
  `titleBarStyle: Overlay`): es un ajuste fino que se puede aplicar; abrir el tema para decidir
  entre decoración nativa simple (actual) vs. overlay.

## Build en CI (sin Mac)

**Ya configurado.** `.github/workflows/release.yml` compila el `.dmg` en la nube en cada tag `v*`:
una **matrix** con runners `macos-14` (Apple Silicon) y `macos-13` (Intel) corre
`cargo tauri build --bundles app dmg` y publica los dos `.dmg` en el GitHub Release (junto al `.deb`
+ AppImage de Linux). Los runners de macOS son **gratis en repos públicos**; no se necesita una Mac
física ni `tauri-action` (se invoca `cargo tauri` directo, igual que en Linux, porque el `package.json`
vive en `web/`). El `.dmg` se genera con **ad-hoc signing** (sin certificado), así que **no requiere
secretos** — pero queda **sin notarizar** (ver Gatekeeper arriba).

Sobre ese `.dmg` publicado, `install.sh` (raíz del repo) da el instalador de un comando:
`/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/MrArcher23/arrow/main/install.sh)"`.

## Estado

- ✅ Código adaptado a macOS (titlebar nativa + font-weight por OS).
- ✅ **`.dmg` publicado por CI** (matrix arm64 + x64 en `release.yml`) + `install.sh` (one-liner).
- ⏳ **Sin verificar en una Mac todavía:** el bloque `cfg(target_os = "macos")` solo se activa al
  compilar en macOS. Falta correr un tag real y confirmar en una Mac el look de la titlebar, que el
  `.dmg` monta y que `install.sh` deja la app en `/Applications`.
- ⏳ **Firma/notarización** (Apple Developer, ~99 USD/año) y **Homebrew Cask** siguen pendientes
  (backlog del [ROADMAP.md](ROADMAP.md)).
