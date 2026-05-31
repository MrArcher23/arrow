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
use std::sync::mpsc::channel;
use std::time::Duration;

use arrow::{ContentOut, ReportOut};
use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

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

/// Watcher nativo: vigila `~/.claude/projects` y, con debounce, emite
/// `report-changed` para que el frontend refresque sin polling.
fn spawn_watcher(app: AppHandle) {
    let dir = projects_dir();
    std::thread::spawn(move || {
        let (tx, rx) = channel();
        let mut watcher =
            match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if res.is_ok() {
                    let _ = tx.send(());
                }
            }) {
                Ok(w) => w,
                Err(_) => return,
            };
        // RecursiveMode: los transcripts viven en subdirectorios por proyecto.
        if watcher
            .watch(Path::new(&dir), RecursiveMode::Recursive)
            .is_err()
        {
            return; // el directorio aún no existe: sin watcher (el polling del front cubre)
        }
        // El `watcher` se mantiene vivo mientras dure este hilo (loop infinito).
        loop {
            // Bloquea hasta la primera actividad…
            if rx.recv().is_err() {
                return; // canal cerrado: la app terminó
            }
            // …y luego drena ráfagas: emite una sola vez tras 400ms de calma.
            while rx.recv_timeout(Duration::from_millis(400)).is_ok() {}
            let _ = app.emit("report-changed", ());
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
        .invoke_handler(tauri::generate_handler![report, content])
        .setup(|app| {
            spawn_watcher(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error al arrancar la app de arrow");
}
