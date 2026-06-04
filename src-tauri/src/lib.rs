//! arrow-app — backend nativo de Tauri.
//!
//! Envuelve la librería `arrow` (el parser) en dos comandos `invoke` que
//! devuelven EXACTAMENTE el mismo JSON que consume la UI web hoy:
//!   - `report()`  -> ReportOut  (equivale a `arrow --json`)
//!   - `content()` -> ContentOut (equivale a `arrow --content --file …`)
//!
//! No hay sidecar ni servidor HTTP: la UI llama a Rust directamente. El refresco
//! en vivo lo provee un watcher `notify` sobre `~/.claude/projects` que emite el
//! evento `report-changed` (con debounce) al frontend.

use std::path::Path;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;

use arrow::{ContentOut, ReportOut};
use notify::{RecursiveMode, Watcher};
#[cfg(target_os = "macos")]
use tauri::Manager;
use tauri::{AppHandle, Emitter};

mod editor;
mod worktrees;

/// `~/.claude/projects` (fuente de verdad nativa de Claude Code).
fn projects_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    format!("{home}/.claude/projects")
}

/// Reporte completo repo→sesión→archivo (sin filtros). Contrato idéntico a `--json`.
#[tauri::command]
fn report() -> ReportOut {
    arrow::build_report(&projects_dir())
}

/// Before/after de un archivo para la vista de diff. Idéntico a `--content --file`.
#[tauri::command]
fn content(file: String, session: Option<String>) -> ContentOut {
    arrow::file_content(&projects_dir(), &file, session.as_deref())
}

/// Editors detected on `$PATH`, for the "Open in editor" picker. Tauri-only
/// (the browser dev-server has no equivalent — opening a local editor only
/// makes sense from the native app).
#[tauri::command]
fn editors() -> Vec<editor::EditorOut> {
    editor::detect_editors()
}

/// Open `file` at `line`:`col` in the chosen editor (fire-and-forget). Delegates
/// to the editor's CLI; arrow never embeds one. Returns an error string the UI
/// can surface if the editor isn't found or fails to launch.
#[tauri::command]
fn open_in_editor(editor_id: String, file: String, line: i64, col: i64) -> Result<(), String> {
    editor::open_in_editor(&editor_id, &file, line, col)
}

/// Worktree inventory for the given repo roots (the ones the report already
/// knows). Read-only: lists + classifies git worktrees (merged/active/stale is
/// finished off in the frontend); arrow never removes one. Tauri-only — the
/// browser dev-server has no equivalent.
#[tauri::command]
fn worktrees(repo_roots: Vec<String>) -> Vec<worktrees::RepoWorktreesOut> {
    worktrees::list_worktrees(&repo_roots)
}

/// On-demand disk size (approx, apparent bytes) per worktree path, for the
/// "Calculate sizes" button. Separate from `worktrees()` so opening the modal
/// stays instant — walking a full checkout is the one expensive part.
#[tauri::command]
fn worktree_sizes(paths: Vec<String>) -> std::collections::HashMap<String, u64> {
    worktrees::worktree_sizes(&paths)
}

/// Watcher nativo: vigila `~/.claude/projects` y, con debounce, emite
/// `report-changed` para que el frontend refresque sin polling.
///
/// Resiliente al ciclo de vida del directorio: si `~/.claude/projects` aún no
/// existe (instalación nueva) o se borra y se recrea, el watcher reintenta
/// establecerse cada `RETRY` en vez de rendirse para siempre. El polling lento
/// del frontend (App.svelte) es el respaldo último si el watcher fallara.
fn spawn_watcher(app: AppHandle) {
    const RETRY: Duration = Duration::from_secs(5);
    const DEBOUNCE: Duration = Duration::from_millis(400);
    const REVALIDATE: Duration = Duration::from_secs(30);

    let dir = projects_dir();
    std::thread::spawn(move || {
        // Bucle externo: (re)establecer el watch. `continue 'establish` suelta el
        // watcher actual y vuelve a empezar (p.ej. tras borrarse el directorio).
        'establish: loop {
            if !Path::new(&dir).exists() {
                std::thread::sleep(RETRY); // aún no existe: reintenta más tarde
                continue;
            }
            let (tx, rx) = channel();
            let mut watcher =
                match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    if res.is_ok() {
                        let _ = tx.send(());
                    }
                }) {
                    Ok(w) => w,
                    Err(_) => {
                        std::thread::sleep(RETRY);
                        continue;
                    }
                };
            // RecursiveMode: los transcripts viven en subdirectorios por proyecto.
            if watcher
                .watch(Path::new(&dir), RecursiveMode::Recursive)
                .is_err()
            {
                std::thread::sleep(RETRY);
                continue;
            }
            // Armado. El `watcher` se mantiene vivo en este scope mientras dure el
            // bucle interno; al hacer `continue 'establish` se libera y se re-crea.
            loop {
                match rx.recv_timeout(REVALIDATE) {
                    Ok(()) => {
                        // Drena ráfagas: emite una sola vez tras DEBOUNCE de calma.
                        while rx.recv_timeout(DEBOUNCE).is_ok() {}
                        let _ = app.emit("report-changed", ());
                    }
                    // Idle: revalida que el directorio siga existiendo (inotify
                    // pierde el watch si el dir se borra y recrea sin avisar).
                    Err(RecvTimeoutError::Timeout) => {
                        if !Path::new(&dir).exists() {
                            continue 'establish; // se borró: re-establecer al volver
                        }
                    }
                    // El watcher se cayó (canal cerrado): re-establecer.
                    Err(RecvTimeoutError::Disconnected) => continue 'establish,
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Mitigaciones WebKitGTK en Linux (pantalla en blanco por DMABUF/NVIDIA/Wayland).
    // DEBEN fijarse ANTES de construir la app. El bug de font-weight (+100) ya está
    // compensado en web/src/app.css (font-weight: 350).
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        // Si aparece pantalla en blanco en Wayland, descomentar:
        // std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            report,
            content,
            editors,
            open_in_editor,
            worktrees,
            worktree_sizes
        ])
        .setup(|app| {
            spawn_watcher(app.handle().clone());
            // macOS: restaurar la decoración nativa (semáforos rojo/amarillo/verde). En
            // Linux/Windows mantenemos la titlebar custom (decorations:false del config),
            // porque ahí el WM no pinta los botones de min/max de forma fiable.
            // Además forzamos la titlebar nativa a OSCURO: por defecto sigue la apariencia del
            // sistema (modo claro ⇒ barra BLANCA) y choca con el tema oscuro de arrow. El
            // título nativo se oculta vía `hiddenTitle` en tauri.conf.json (evita el "arrow"
            // duplicado). Trade-off aceptado: con un tema CLARO de arrow la barra desentona un
            // poco. (Sin verificar en hardware Mac — ver ROADMAP.)
            #[cfg(target_os = "macos")]
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_decorations(true);
                let _ = win.set_theme(Some(tauri::Theme::Dark));
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error al arrancar la app de arrow");
}
