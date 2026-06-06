//! Background worker: keeps the parser off the UI thread.
//!
//! `build_report` parses every transcript and `file_content` reads files from disk —
//! both can take long enough to drop a frame. So the UI never calls them directly; it
//! sends a [`Cmd`] to this worker and drains [`Msg`] results each frame. The worker
//! calls `ctx.request_repaint()` after posting a result so the UI wakes up to apply it.

use std::path::Path;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use arrow::{ContentOut, ReportOut, UpdateStatus};
use notify::{RecursiveMode, Watcher};

/// A request from the UI to the worker.
pub enum Cmd {
    /// (Re)build the full report from `~/.claude/projects`.
    LoadReport,
    /// Load the before/after diff content for a file (optionally pinned to a session).
    LoadContent {
        file: String,
        session: Option<String>,
    },
    /// Check GitHub for a newer release (network, time-boxed, read-only).
    CheckUpdate,
}

/// A result from the worker back to the UI.
pub enum Msg {
    Report(ReportOut),
    /// Content for a file. Carries `file`/`session` (inside `ContentOut`) so the UI can
    /// drop a stale result when the user has already clicked another file.
    Content(ContentOut),
    Update(UpdateStatus),
}

pub struct Worker {
    tx: Sender<Cmd>,
    rx: Receiver<Msg>,
}

impl Worker {
    /// Spawn the worker thread. `ctx` is used to wake the UI when a result is ready;
    /// `projects_dir` is captured once (it never changes during a run).
    pub fn spawn(ctx: egui::Context, projects_dir: String) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (msg_tx, msg_rx) = channel::<Msg>();
        std::thread::spawn(move || {
            // Exits when the UI drops the command sender (app shutdown).
            while let Ok(cmd) = cmd_rx.recv() {
                let msg = match cmd {
                    Cmd::LoadReport => Msg::Report(arrow::build_report(&projects_dir)),
                    Cmd::LoadContent { file, session } => Msg::Content(arrow::file_content(
                        &projects_dir,
                        &file,
                        session.as_deref(),
                    )),
                    Cmd::CheckUpdate => Msg::Update(arrow::check_update(env!("CARGO_PKG_VERSION"))),
                };
                if msg_tx.send(msg).is_err() {
                    break; // UI gone
                }
                ctx.request_repaint();
            }
        });
        Self {
            tx: cmd_tx,
            rx: msg_rx,
        }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    /// A clonable handle to enqueue commands from another thread (the file watcher).
    pub fn sender(&self) -> Sender<Cmd> {
        self.tx.clone()
    }

    /// Pull the next ready result without blocking; `None` when the queue is empty
    /// (or the worker has gone away).
    pub fn try_recv(&self) -> Option<Msg> {
        self.rx.try_recv().ok()
    }
}

/// Native watcher over `~/.claude/projects`: on a debounced filesystem change it enqueues a
/// `LoadReport` so the UI refreshes without polling. Ported from the Tauri backend's
/// `spawn_watcher`; instead of emitting an event it pushes onto the worker's command queue
/// (the worker calls `ctx.request_repaint()` when it posts the fresh report).
///
/// Resilient to the directory's lifecycle: if `~/.claude/projects` doesn't exist yet (fresh
/// install) or is deleted and recreated, it re-establishes the watch instead of giving up.
pub fn spawn_watcher(dir: String, tx: Sender<Cmd>) {
    const RETRY: Duration = Duration::from_secs(5);
    const DEBOUNCE: Duration = Duration::from_millis(400);
    const REVALIDATE: Duration = Duration::from_secs(30);

    std::thread::spawn(move || {
        // Outer loop: (re)establish the watch. `continue 'establish` drops the current
        // watcher and starts over (e.g. after the directory is deleted).
        'establish: loop {
            if !Path::new(&dir).exists() {
                std::thread::sleep(RETRY);
                continue;
            }
            let (fs_tx, fs_rx) = channel();
            let mut watcher =
                match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    if res.is_ok() {
                        let _ = fs_tx.send(());
                    }
                }) {
                    Ok(w) => w,
                    Err(_) => {
                        std::thread::sleep(RETRY);
                        continue;
                    }
                };
            // Transcripts live in per-project subdirectories.
            if watcher
                .watch(Path::new(&dir), RecursiveMode::Recursive)
                .is_err()
            {
                std::thread::sleep(RETRY);
                continue;
            }
            loop {
                match fs_rx.recv_timeout(REVALIDATE) {
                    Ok(()) => {
                        // Drain bursts: enqueue one refresh after DEBOUNCE of calm.
                        while fs_rx.recv_timeout(DEBOUNCE).is_ok() {}
                        if tx.send(Cmd::LoadReport).is_err() {
                            return; // app gone
                        }
                    }
                    // Idle: revalidate the directory still exists (inotify loses the watch
                    // if it's deleted and recreated without notice).
                    Err(RecvTimeoutError::Timeout) => {
                        if !Path::new(&dir).exists() {
                            continue 'establish;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => continue 'establish,
                }
            }
        }
    });
}
