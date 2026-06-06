//! arrow-gui — native egui/eframe desktop app.
//!
//! Replaces the Tauri + Svelte/CodeMirror stack with a single native Rust binary:
//! no Node/Vite, no system webview. It reuses the parser library (`arrow`) directly
//! — `build_report` / `file_content` return plain Rust structs, so there is no IPC and
//! no JSON contract between a backend and a frontend; the UI consumes the structs as-is.

mod diff;
mod editor;
mod focus;
mod sidebar;
mod sys;
mod theme;
mod worker;
mod worktrees;
mod worktrees_modal;

use std::time::Duration;

use arrow::{ContentOut, ReportOut, UpdateStatus};
use worker::{Cmd, Msg, Worker};

const ZOOM_STEP: f32 = 0.1;
const ZOOM_MIN: f32 = 0.6;
const ZOOM_MAX: f32 = 2.0;
const GREEN: egui::Color32 = egui::Color32::from_rgb(63, 185, 80);
const AMBER: egui::Color32 = egui::Color32::from_rgb(210, 153, 34);
const RED: egui::Color32 = egui::Color32::from_rgb(248, 81, 73);

/// `~/.claude/projects` — Claude Code's native source of truth (same as the CLI/Tauri).
fn projects_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    format!("{home}/.claude/projects")
}

/// Whether the open (session, path) still exists in a freshly fetched report — used to
/// decide if the diff can be refreshed in place or the selection has aged out.
fn file_still_in_report(report: &ReportOut, session: &str, path: &str) -> bool {
    report.repos.iter().any(|repo| {
        repo.sessions
            .iter()
            .any(|s| s.session_id == session && s.files.iter().any(|f| f.path == path))
    })
}

struct ArrowApp {
    worker: Worker,
    report: Option<ReportOut>,
    error: Option<String>,
    /// (session_id, path) of the open file.
    selected: Option<(String, String)>,
    content: Option<ContentOut>,
    /// Built (highlighted, line-aligned) diff for the open file; rebuilt on file change.
    diff_doc: Option<diff::DiffDoc>,
    highlighter: diff::Highlighter,
    loading_content: bool,
    sidebar_collapsed: bool,
    /// Current theme id; `applied_theme` tracks what's been pushed to the context so we
    /// re-apply (and re-highlight) only when it changes.
    theme_id: String,
    applied_theme: String,
    /// UI zoom factor (VS Code-style Ctrl +/−/0), persisted across runs.
    zoom: f32,
    /// Latest release-check result (the version badge's "update available" hint).
    update: Option<UpdateStatus>,
    worktrees_modal: Option<worktrees_modal::WorktreesModal>,
    /// Editors detected on $PATH, and the remembered choice (for "Open in editor").
    editors: Vec<editor::EditorOut>,
    editor_id: String,
    /// Serialized last report, to skip re-rendering when a refresh changed nothing
    /// (mirrors the web's JSON.stringify comparison).
    last_report_json: String,
    /// Wall-clock now (epoch-millis), sampled each frame so relative times age in place.
    now: i64,
}

impl ArrowApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Restore persisted UI state (replaces the web's localStorage).
        let theme_id = cc
            .storage
            .and_then(|s| eframe::get_value::<String>(s, "theme"))
            .unwrap_or_else(|| theme::DEFAULT.to_string());
        let zoom = cc
            .storage
            .and_then(|s| eframe::get_value::<f32>(s, "zoom"))
            .unwrap_or(1.0)
            .clamp(ZOOM_MIN, ZOOM_MAX);
        let editors = editor::detect_editors();
        let editor_id = cc
            .storage
            .and_then(|s| eframe::get_value::<String>(s, "editor"))
            .filter(|id| editors.iter().any(|e| e.id == *id))
            .or_else(|| editors.first().map(|e| e.id.clone()))
            .unwrap_or_default();

        theme::apply(&cc.egui_ctx, &theme_id);
        cc.egui_ctx.set_zoom_factor(zoom);
        let mut highlighter = diff::Highlighter::new();
        highlighter.set_theme(theme::def(&theme_id).syntect);

        let worker = Worker::spawn(cc.egui_ctx.clone(), projects_dir());
        worker.send(Cmd::LoadReport);
        worker.send(Cmd::CheckUpdate);
        // Native live refresh: a watcher enqueues LoadReport on edits (debounced). A slow
        // poll isn't needed — the watcher is resilient — but the worker repaints on results.
        worker::spawn_watcher(projects_dir(), worker.sender());

        Self {
            worker,
            report: None,
            error: None,
            selected: None,
            content: None,
            diff_doc: None,
            highlighter,
            loading_content: false,
            sidebar_collapsed: false,
            applied_theme: theme_id.clone(),
            theme_id,
            zoom,
            update: None,
            worktrees_modal: None,
            editors,
            editor_id,
            last_report_json: String::new(),
            now: focus::now_ms(),
        }
    }

    /// Open a file's diff: remember the selection and ask the worker for its content.
    fn select(&mut self, session: String, path: String) {
        self.selected = Some((session.clone(), path.clone()));
        self.content = None;
        self.diff_doc = None;
        self.loading_content = true;
        self.worker.send(Cmd::LoadContent {
            file: path,
            session: Some(session),
        });
    }

    /// Apply any results the worker has posted since the last frame.
    fn drain_worker(&mut self) {
        while let Some(msg) = self.worker.try_recv() {
            match msg {
                Msg::Report(r) => {
                    // Skip if nothing changed (a watcher burst that didn't alter the report):
                    // don't re-render or disturb the open diff (mirrors the web's stringify guard).
                    let json = serde_json::to_string(&r).unwrap_or_default();
                    if json == self.last_report_json {
                        continue;
                    }
                    let first = self.report.is_none();
                    self.last_report_json = json;
                    self.report = Some(r);

                    if first {
                        // Initial auto-open: only when there is ACTIVE work (a recent edit),
                        // open the top file. On an idle launch show nothing as current (honest).
                        let pick = self.report.as_ref().and_then(|rep| {
                            let s = rep.repos.first()?.sessions.first()?;
                            if !focus::is_live(s.last_activity.as_deref(), self.now) {
                                return None;
                            }
                            let f = s.files.first()?;
                            Some((s.session_id.clone(), f.path.clone()))
                        });
                        if let Some((session, path)) = pick {
                            self.select(session, path);
                        }
                    } else if let Some((session, path)) = self.selected.clone() {
                        let report = self.report.as_ref().unwrap();
                        if file_still_in_report(report, &session, &path) {
                            // Live refresh: re-fetch the OPEN file's diff in place (same
                            // selection, no blank flash — the current diff stays until the
                            // fresh one arrives and replaces it).
                            self.worker.send(Cmd::LoadContent {
                                file: path,
                                session: Some(session),
                            });
                        } else {
                            // The open file aged out of the report → drop it (don't show a
                            // stale diff); the panel returns to a clean state.
                            self.selected = None;
                            self.content = None;
                            self.diff_doc = None;
                        }
                    }
                }
                Msg::Content(c) => {
                    // Drop stale results: only apply content for the file currently open
                    // (the user may have clicked another file while this was loading).
                    let matches = self
                        .selected
                        .as_ref()
                        .is_some_and(|(_, path)| *path == c.file);
                    if matches {
                        // Build the highlighted, line-aligned diff now (once), so each frame
                        // only paints the visible rows.
                        self.diff_doc = Some(self.highlighter.build(&c));
                        self.content = Some(c);
                        self.loading_content = false;
                    }
                }
                Msg::Update(u) => self.update = Some(u),
            }
        }
    }

    /// Re-apply the theme (and re-highlight the open diff) when the selection changed.
    fn sync_theme(&mut self, ctx: &egui::Context) {
        if self.theme_id == self.applied_theme {
            return;
        }
        theme::apply(ctx, &self.theme_id);
        self.highlighter
            .set_theme(theme::def(&self.theme_id).syntect);
        if let Some(c) = &self.content {
            self.diff_doc = Some(self.highlighter.build(c));
        }
        self.applied_theme = self.theme_id.clone();
    }

    fn set_zoom(&mut self, ctx: &egui::Context, z: f32) {
        self.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
        ctx.set_zoom_factor(self.zoom);
    }

    /// Number of focus repos with recent activity (the green-dot count in the topbar).
    fn live_count(&self) -> usize {
        let Some(report) = &self.report else {
            return 0;
        };
        focus::focus_repos(&report.repos, self.now)
            .into_iter()
            .filter(|&i| {
                report.repos[i]
                    .sessions
                    .first()
                    .is_some_and(|s| focus::is_live(s.last_activity.as_deref(), self.now))
            })
            .count()
    }

    fn version_badge(&self, ui: &mut egui::Ui) {
        let ver = env!("CARGO_PKG_VERSION");
        match &self.update {
            Some(u) if u.update_available => {
                let latest = u.latest.as_deref().unwrap_or("");
                let resp = ui
                    .button(egui::RichText::new(format!("v{ver} ●")).color(GREEN))
                    .on_hover_text(format!(
                        "Update available → v{latest}\nClick to open the release"
                    ));
                if resp.clicked() {
                    if let Some(url) = &u.url {
                        let _ = sys::open_url(url);
                    }
                }
            }
            _ => {
                ui.label(egui::RichText::new(format!("v{ver}")).weak());
            }
        }
    }

    fn theme_menu(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_id_salt("theme")
            .selected_text(theme::def(&self.theme_id).label)
            .show_ui(ui, |ui| {
                for t in theme::THEMES {
                    ui.selectable_value(&mut self.theme_id, t.id.to_string(), t.label);
                }
            });
    }

    /// "Open in editor": a picker of detected editors + an Open button that delegates to the
    /// editor's CLI at the first changed line. Disabled when the file is gone from disk
    /// (honest: it opens the current on-disk file, never the reconstructed snapshot).
    fn open_in_editor_bar(&mut self, ui: &mut egui::Ui, file: &str, line: i64, enabled: bool) {
        if self.editors.is_empty() {
            return;
        }
        // Copy the list so the combo can mutate self.editor_id without borrowing self.editors.
        let items: Vec<(String, String)> = self
            .editors
            .iter()
            .map(|e| (e.id.clone(), e.name.clone()))
            .collect();
        let label = items
            .iter()
            .find(|(id, _)| *id == self.editor_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_default();

        let open = ui.add_enabled(enabled, egui::Button::new("Open in editor"));
        if !enabled {
            open.clone().on_hover_text("The file is no longer on disk");
        }
        if open.clicked() {
            let _ = editor::open_in_editor(&self.editor_id, file, line, 1);
        }
        egui::ComboBox::from_id_salt("editor")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for (id, name) in &items {
                    ui.selectable_value(&mut self.editor_id, id.clone(), name);
                }
            });
    }

    fn zoom_widget(&mut self, ui: &mut egui::Ui) {
        // RTL layout: add right-to-left, so push "+" first to land on the right.
        if ui.button("+").on_hover_text("Zoom in (Ctrl +)").clicked() {
            let z = self.zoom;
            self.set_zoom(&ui.ctx().clone(), z + ZOOM_STEP);
        }
        if ui
            .button(format!("{}%", (self.zoom * 100.0).round() as i32))
            .on_hover_text("Reset zoom (Ctrl 0)")
            .clicked()
        {
            self.set_zoom(&ui.ctx().clone(), 1.0);
        }
        if ui.button("−").on_hover_text("Zoom out (Ctrl −)").clicked() {
            let z = self.zoom;
            self.set_zoom(&ui.ctx().clone(), z - ZOOM_STEP);
        }
    }
}

impl eframe::App for ArrowApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.now = focus::now_ms();
        self.drain_worker();
        let ctx = ui.ctx().clone();
        self.sync_theme(&ctx);
        // Clock tick: keep relative times aging / the green dot expiring even when idle.
        // Only schedule it while focused — when the window is hidden there's nothing to age
        // on screen, and egui repaints on refocus (catching `now` up). Mirrors the web's
        // visibilitychange pause.
        if ctx.input(|i| i.focused) {
            ctx.request_repaint_after(Duration::from_secs(30));
        }

        // Zoom keys (VS Code-style Ctrl +/−/0).
        let (zoom_in, zoom_out, zoom_reset) = ctx.input(|i| {
            let c = i.modifiers.command;
            (
                c && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)),
                c && i.key_pressed(egui::Key::Minus),
                c && i.key_pressed(egui::Key::Num0),
            )
        });
        if zoom_in {
            self.set_zoom(&ctx, self.zoom + ZOOM_STEP);
        }
        if zoom_out {
            self.set_zoom(&ctx, self.zoom - ZOOM_STEP);
        }
        if zoom_reset {
            self.set_zoom(&ctx, 1.0);
        }

        // A file click bubbles out of the sidebar closure (which only borrows self
        // immutably); we act on it after the panels so `select()` can borrow self mut.
        let mut click: Option<(String, String)> = None;

        egui::Panel::top("topbar").show_inside(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui.button("☰").on_hover_text("Toggle sidebar").clicked() {
                    self.sidebar_collapsed = !self.sidebar_collapsed;
                }
                ui.heading("ARROW");
                ui.label(egui::RichText::new("audit of Claude Code changes").weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.version_badge(ui);
                    self.theme_menu(ui);
                    self.zoom_widget(ui);
                    if self.report.is_some()
                        && ui
                            .button("Worktrees")
                            .on_hover_text("Worktree inventory")
                            .clicked()
                    {
                        // Unique repo roots (the report's repos are already distinct git roots).
                        let mut roots: Vec<String> = Vec::new();
                        if let Some(r) = &self.report {
                            for repo in &r.repos {
                                if !roots.contains(&repo.cwd) {
                                    roots.push(repo.cwd.clone());
                                }
                            }
                        }
                        self.worktrees_modal = Some(worktrees_modal::WorktreesModal::open(roots));
                    }
                    if let Some(r) = &self.report {
                        ui.label(egui::RichText::new(format!("{} repos", r.repo_count)).weak());
                    }
                    let live = self.live_count();
                    if live > 0 {
                        ui.colored_label(GREEN, format!("● {live}"));
                    }
                });
            });
            ui.add_space(2.0);
        });

        if !self.sidebar_collapsed {
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(340.0)
                .size_range(egui::Rangef::new(180.0, 640.0))
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(err) = &self.error {
                            ui.colored_label(RED, err);
                        }
                        if let Some(report) = &self.report {
                            let sel = self
                                .selected
                                .as_ref()
                                .map(|(s, p)| (s.as_str(), p.as_str()));
                            click = sidebar::show(ui, report, sel, self.now);
                        } else if self.error.is_none() {
                            ui.weak("Loading sessions…");
                        }
                    });
                });
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            // Snapshot the bits the filebar needs as owned/copy values so we don't hold a
            // borrow on self.selected/self.content while the editor combo mutates self.
            let info = self.selected.as_ref().map(|(_, p)| {
                let c = self.content.as_ref();
                (
                    p.clone(),
                    c.is_some_and(|c| c.user_modified),
                    c.is_some_and(|c| c.after_available),
                    c.and_then(|c| c.first_changed_line).unwrap_or(1),
                )
            });
            if let Some((path, user_modified, after_available, line)) = info {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.monospace(&path);
                    if user_modified {
                        ui.colored_label(AMBER, "⚠ modified outside Claude");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.open_in_editor_bar(ui, &path, line, after_available);
                    });
                });
                ui.separator();
                if self.loading_content {
                    ui.weak("Loading diff…");
                } else if let Some(doc) = &self.diff_doc {
                    diff::show(ui, doc);
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.weak("Select a file in the sidebar to view its diff.");
                });
            }
        });

        // Worktrees inventory + cleanup (the only disk-mutating action, gated by a dry-run +
        // confirmation inside the modal). A real removal flags `changed` → we refresh the report.
        let now = self.now;
        let mut close_modal = false;
        let mut need_refresh = false;
        if let (Some(modal), Some(report)) = (self.worktrees_modal.as_mut(), self.report.as_ref()) {
            let keep = modal.show(&ctx, report, now);
            need_refresh = modal.take_changed();
            close_modal = !keep;
        }
        if need_refresh {
            self.worker.send(Cmd::LoadReport);
        }
        if close_modal {
            self.worktrees_modal = None;
        }

        if let Some((session, path)) = click {
            self.select(session, path);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "theme", &self.theme_id);
        eframe::set_value(storage, "zoom", &self.zoom);
        eframe::set_value(storage, "editor", &self.editor_id);
    }
}

/// Decode the bundled PNG into an egui window icon.
fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()?
        .into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("arrow")
        .with_inner_size([1100.0, 720.0])
        .with_min_inner_size([640.0, 400.0]);
    // Window/taskbar icon (decoded from the bundled PNG).
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "arrow",
        options,
        Box::new(|cc| Ok(Box::new(ArrowApp::new(cc)))),
    )
}
