//! Worktree inventory + cleanup, ported from `WorktreesModal.svelte`.
//!
//! Lists and classifies the git worktrees Claude Code creates per session (active / stale /
//! "safe to remove"), with on-demand disk sizes and a `copy cmd` per row. The `Clean`/`Prune`
//! button is the ONLY action in arrow that mutates disk: it runs `git worktree remove` WITHOUT
//! `--force` (git refuses a locked/dirty worktree), gated behind a dry-run + confirmation, and
//! only on rows git would actually clean. All git shelling lives in `worktrees.rs`.
//!
//! Threading: listing + clean/prune are fast (git plumbing, ~ms) and run synchronously; only
//! the disk-size walk is slow, so it runs off-thread and posts back over a channel.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

use arrow::ReportOut;

use crate::focus;
use crate::worktrees::{self, RepoWorktreesOut, WorktreeOut};

const GREEN: egui::Color32 = egui::Color32::from_rgb(63, 185, 80);
const AMBER: egui::Color32 = egui::Color32::from_rgb(210, 153, 34);
const RED: egui::Color32 = egui::Color32::from_rgb(248, 81, 73);
const DIM: egui::Color32 = egui::Color32::from_gray(140);

#[derive(Clone, Copy, PartialEq)]
enum Klass {
    Safe,
    Stale,
    Active,
}

/// A previewed clean awaiting confirmation (the dry-run output is shown before running).
struct Confirm {
    path: String,
    cmd: String,
    note: String,
    prune: bool,
    repo: String,
}

/// Mutating actions collected during render, applied after (to avoid borrowing self twice).
enum Action {
    Reload,
    CalcSizes,
    StartClean {
        repo: String,
        path: String,
        prune: bool,
    },
    Confirm,
    Cancel,
}

pub struct WorktreesModal {
    repo_roots: Vec<String>,
    groups: Vec<RepoWorktreesOut>,
    error: Option<String>,
    sizes: HashMap<String, u64>,
    sizing: bool,
    rx_sizes: Option<Receiver<HashMap<String, u64>>>,
    confirming: Option<Confirm>,
    /// (row path, ok, message) — the last per-row outcome (git's verbatim refusal on failure).
    clean_msg: Option<(String, bool, String)>,
    /// Set when a real removal/prune succeeded; the app reads it to refresh the report.
    changed: bool,
}

impl WorktreesModal {
    /// Open the inventory for the given repo roots (loads synchronously — git plumbing).
    pub fn open(repo_roots: Vec<String>) -> Self {
        let groups = worktrees::list_worktrees(&repo_roots);
        Self {
            repo_roots,
            groups,
            error: None,
            sizes: HashMap::new(),
            sizing: false,
            rx_sizes: None,
            confirming: None,
            clean_msg: None,
            changed: false,
        }
    }

    /// True if a real removal happened since last checked (caller refreshes the report).
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn reload(&mut self) {
        self.groups = worktrees::list_worktrees(&self.repo_roots);
        self.confirming = None;
    }

    fn calc_sizes(&mut self, ctx: &egui::Context) {
        self.sizing = true;
        let paths: Vec<String> = self
            .groups
            .iter()
            .flat_map(|g| {
                g.worktrees
                    .iter()
                    .filter(|w| !w.is_main)
                    .map(|w| w.path.clone())
            })
            .collect();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let sizes = worktrees::worktree_sizes(&paths);
            let _ = tx.send(sizes);
            ctx.request_repaint();
        });
        self.rx_sizes = Some(rx);
    }

    fn start_clean(&mut self, repo: String, path: String, prune: bool) {
        let res = if prune {
            worktrees::prune_worktrees(&repo, true)
        } else {
            worktrees::remove_worktree(&repo, &path, true)
        };
        self.confirming = Some(Confirm {
            path,
            cmd: res.command,
            note: res.output,
            prune,
            repo,
        });
        self.clean_msg = None;
    }

    fn confirm_clean(&mut self) {
        let Some(c) = self.confirming.take() else {
            return;
        };
        let res = if c.prune {
            worktrees::prune_worktrees(&c.repo, false)
        } else {
            worktrees::remove_worktree(&c.repo, &c.path, false)
        };
        let text = if res.ok {
            if c.prune {
                "pruned".into()
            } else {
                "removed".into()
            }
        } else {
            res.output
        };
        self.clean_msg = Some((c.path, res.ok, text));
        if res.ok {
            self.changed = true;
            self.reload();
        }
    }

    /// Render the modal. Returns `false` when it should close.
    pub fn show(&mut self, ctx: &egui::Context, report: &ReportOut, now: i64) -> bool {
        // Pull in any finished size walk.
        if let Some(rx) = &self.rx_sizes {
            if let Ok(sizes) = rx.try_recv() {
                self.sizes = sizes;
                self.sizing = false;
                self.rx_sizes = None;
            }
        }

        // Flatten the report's files once for active/stale classification (most recent edit
        // under each worktree path).
        let files: Vec<(&str, &str)> = report
            .repos
            .iter()
            .flat_map(|r| r.sessions.iter().flat_map(|s| s.files.iter()))
            .filter_map(|f| f.last_touched.as_deref().map(|t| (f.path.as_str(), t)))
            .collect();

        let mut open = true;
        let mut action: Option<Action> = None;

        let resp = egui::Modal::new(egui::Id::new("worktrees-modal")).show(ctx, |ui| {
            ui.set_width(720.0);

            ui.horizontal(|ui| {
                ui.heading("Worktree cleanup");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕").on_hover_text("Close").clicked() {
                        open = false;
                    }
                    if ui.button("↻").on_hover_text("Refresh").clicked() {
                        action = Some(Action::Reload);
                    }
                });
            });
            ui.separator();

            // Legend + Calculate sizes.
            ui.horizontal(|ui| {
                legend_dot(ui, GREEN, "safe to remove");
                legend_dot(ui, AMBER, "stale (old, unmerged)");
                legend_dot(ui, DIM, "active");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let enabled = !self.sizing && !self.groups.is_empty();
                    let label = if self.sizing {
                        "Calculating…"
                    } else {
                        "Calculate sizes"
                    };
                    if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                        action = Some(Action::CalcSizes);
                    }
                });
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .max_height(460.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(err) = &self.error {
                        ui.colored_label(RED, err);
                    } else if self.groups.iter().all(|g| linked_count(g) == 0) {
                        ui.add_space(16.0);
                        ui.weak("No linked worktrees found.");
                    } else {
                        for g in &self.groups {
                            if linked_count(g) == 0 {
                                continue;
                            }
                            self.render_group(ui, g, &files, now, &mut action);
                        }
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(
                                "“Clean” runs `git worktree remove` without --force (git refuses a \
                                 locked or dirty worktree), after a dry-run and confirmation. \
                                 “merged” shows only when the branch is provably an ancestor of the \
                                 default branch; squash/rebase merges read as “can't confirm”. \
                                 Sizes are approximate.",
                            )
                            .weak()
                            .small(),
                        );
                    }
                });
        });

        if resp.should_close() {
            open = false;
        }

        match action {
            Some(Action::Reload) => self.reload(),
            Some(Action::CalcSizes) => self.calc_sizes(ctx),
            Some(Action::StartClean { repo, path, prune }) => self.start_clean(repo, path, prune),
            Some(Action::Confirm) => self.confirm_clean(),
            Some(Action::Cancel) => self.confirming = None,
            None => {}
        }

        open
    }

    fn render_group(
        &self,
        ui: &mut egui::Ui,
        g: &RepoWorktreesOut,
        files: &[(&str, &str)],
        now: i64,
        action: &mut Option<Action>,
    ) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(repo_name(&g.repo_root)).strong());
            if let Some(b) = &g.default_branch {
                ui.label(egui::RichText::new(b).weak().small());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} wt", linked_count(g)))
                        .weak()
                        .small(),
                );
            });
        });

        for w in g.worktrees.iter().filter(|w| !w.is_main) {
            let latest = latest_edit_under(&w.path, files);
            let klass = classify(w, latest.as_deref(), now);
            ui.horizontal(|ui| {
                ui.colored_label(klass_color(klass), "●");
                ui.monospace(w.branch.as_deref().unwrap_or("(detached)"));
                ui.label(egui::RichText::new(merge_text(g, w)).weak());
                if w.dirty {
                    ui.colored_label(AMBER, "· uncommitted");
                }
                if w.locked {
                    ui.colored_label(AMBER, "· locked");
                }
                if w.prunable {
                    ui.colored_label(AMBER, "· missing (prunable)");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Clean / Prune (only on rows git would act on).
                    if can_clean(w, latest.as_deref(), now) {
                        let label = if w.prunable { "Prune" } else { "Clean" };
                        if ui.button(label).clicked() {
                            *action = Some(Action::StartClean {
                                repo: g.repo_root.clone(),
                                path: w.path.clone(),
                                prune: w.prunable,
                            });
                        }
                    }
                    let cmd = cmd_for(g, w);
                    if ui.button("copy cmd").on_hover_text(&cmd).clicked() {
                        ui.ctx().copy_text(cmd);
                    }
                    if let Some(sz) = self.sizes.get(&w.path) {
                        ui.label(egui::RichText::new(hsize(*sz)).monospace());
                    }
                    ui.label(
                        egui::RichText::new(last_edit_label(latest.as_deref(), now))
                            .weak()
                            .small(),
                    );
                });
            });

            // Confirmation bar / outcome message for this row.
            if self.confirming.as_ref().is_some_and(|c| c.path == w.path) {
                let c = self.confirming.as_ref().unwrap();
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.monospace(&c.cmd);
                    if !c.note.is_empty() {
                        ui.label(egui::RichText::new(&c.note).weak().small());
                    }
                    ui.horizontal(|ui| {
                        let go = if c.prune { "Prune" } else { "Remove" };
                        if ui.button(go).clicked() {
                            *action = Some(Action::Confirm);
                        }
                        if ui.button("Cancel").clicked() {
                            *action = Some(Action::Cancel);
                        }
                    });
                });
            } else if let Some((path, ok, text)) = &self.clean_msg {
                if path == &w.path {
                    let color = if *ok { GREEN } else { RED };
                    ui.colored_label(
                        color,
                        if *ok {
                            format!("✓ {text}")
                        } else {
                            text.clone()
                        },
                    );
                }
            }
        }
    }
}

fn legend_dot(ui: &mut egui::Ui, color: egui::Color32, label: &str) {
    ui.colored_label(color, "●");
    ui.label(egui::RichText::new(label).weak().small());
    ui.add_space(8.0);
}

fn klass_color(k: Klass) -> egui::Color32 {
    match k {
        Klass::Safe => GREEN,
        Klass::Stale => AMBER,
        Klass::Active => DIM,
    }
}

fn linked_count(g: &RepoWorktreesOut) -> usize {
    g.worktrees.iter().filter(|w| !w.is_main).count()
}

/// Precedence: actively-edited ⇒ active (you're working in it); proven-merged + clean ⇒ safe;
/// else stale.
fn classify(w: &WorktreeOut, latest: Option<&str>, now: i64) -> Klass {
    if focus::is_active(latest, now) {
        return Klass::Active;
    }
    if w.is_merged && !w.dirty && !w.locked {
        return Klass::Safe;
    }
    Klass::Stale
}

/// Offer Clean only for a row git would act on: a phantom (prunable) or a proven-merged,
/// clean, unlocked worktree — and never one you're actively editing.
fn can_clean(w: &WorktreeOut, latest: Option<&str>, now: i64) -> bool {
    if focus::is_active(latest, now) {
        return false;
    }
    w.prunable || (w.is_merged && !w.dirty && !w.locked)
}

fn merge_text(g: &RepoWorktreesOut, w: &WorktreeOut) -> String {
    if w.branch.is_none() {
        return "detached HEAD".into();
    }
    let Some(default) = &g.default_branch else {
        return "can't tell (no default branch)".into();
    };
    if w.is_merged {
        return format!("merged into {default}");
    }
    let ahead = match w.ahead {
        Some(n) if n > 0 => format!("{n} ahead"),
        _ => "diverged".into(),
    };
    format!("{ahead} of {default} · can't confirm merged")
}

/// Most recent edit (ISO) of any tracked file under `path`. A linked worktree never nests
/// another, so a prefix match is unambiguous.
fn latest_edit_under(path: &str, files: &[(&str, &str)]) -> Option<String> {
    let prefix = if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    };
    files
        .iter()
        .filter(|(p, _)| p.starts_with(&prefix))
        .map(|(_, t)| *t)
        .max() // ISO 8601 sorts lexicographically == chronologically
        .map(|s| s.to_string())
}

fn last_edit_label(latest: Option<&str>, now: i64) -> String {
    match latest {
        None => "no recent edits".into(),
        Some(iso) => {
            let rel = focus::relative(Some(iso), now);
            if rel.is_empty() {
                "edited".into()
            } else {
                format!("edited {rel}")
            }
        }
    }
}

fn repo_name(root: &str) -> String {
    root.trim_end_matches('/')
        .split('/')
        .next_back()
        .unwrap_or(root)
        .to_string()
}

/// Single-quote a path for a shell command the USER will paste (arrow never runs it).
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn cmd_for(g: &RepoWorktreesOut, w: &WorktreeOut) -> String {
    if w.prunable {
        format!("git -C {} worktree prune", q(&g.repo_root))
    } else {
        format!("git -C {} worktree remove {}", q(&g.repo_root), q(&w.path))
    }
}

fn hsize(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut n = bytes as f64;
    let mut i = 0;
    while n >= 1024.0 && i < units.len() - 1 {
        n /= 1024.0;
        i += 1;
    }
    let decimals = if n < 10.0 && i > 0 { 1 } else { 0 };
    format!("{n:.decimals$} {}", units[i])
}
