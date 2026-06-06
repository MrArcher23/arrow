//! Sidebar tree: `repo → session → touched files`, ported from `Sidebar.svelte`.
//!
//! Focus repos (the active session + its ~10-min burst) render expanded at the top with a
//! green dot; the rest fold into "Other repos". When idle (no recent activity) there is no
//! focus: a quiet "No active work" note sits above the collapsed history. Selecting a file
//! returns `(session_id, path)` so the app can load its diff.
//!
//! Expansion state lives in egui memory (per-`CollapsingState` id); focus repos default to
//! open. File rows are `selectable_label`s built from a `LayoutJob` so the +added/-removed
//! counts keep their green/red colors. UI strings are English (arrow is English-branded).

use std::cell::RefCell;

use arrow::{FileOut, RepoOut, ReportOut, SessionOut};
use egui::{
    collapsing_header::CollapsingState, text::LayoutJob, Color32, FontId, RichText, TextFormat,
};

use crate::focus;

const GREEN: Color32 = Color32::from_rgb(63, 185, 80);
const RED: Color32 = Color32::from_rgb(248, 81, 73);

/// Render the tree. Returns `Some((session_id, path))` for a file clicked this frame.
pub fn show(
    ui: &mut egui::Ui,
    report: &ReportOut,
    selected: Option<(&str, &str)>,
    now: i64,
) -> Option<(String, String)> {
    // Collected via a RefCell so the nested CollapsingState closures (which each take their
    // own closure) can record a click without juggling overlapping &mut borrows.
    let clicked: RefCell<Option<(String, String)>> = RefCell::new(None);

    let focus_idx = focus::focus_repos(&report.repos, now);
    let idle = focus_idx.is_empty();

    if idle {
        // No active work right now (newest edit older than the live window): show nothing as
        // current — just a quiet note + the collapsed history below.
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("No active work").strong());
            let last = report
                .repos
                .first()
                .and_then(|r| r.sessions.first())
                .and_then(|s| s.last_activity.as_deref());
            let rel = focus::relative(last, now);
            if !rel.is_empty() {
                ui.label(RichText::new(format!("last activity {rel}")).weak().small());
            }
        });
        ui.add_space(6.0);
    }

    for &i in &focus_idx {
        repo_row(ui, &report.repos[i], true, selected, now, &clicked);
    }

    let other: Vec<usize> = (0..report.repos.len())
        .filter(|i| !focus_idx.contains(i))
        .collect();
    if !other.is_empty() {
        ui.add_space(6.0);
        let label = if idle { "HISTORY" } else { "OTHER REPOS" };
        let id = ui.make_persistent_id("other-repos");
        CollapsingState::load_with_default_open(ui.ctx(), id, false)
            .show_header(ui, |ui| {
                ui.label(
                    RichText::new(format!("{label}  ({})", other.len()))
                        .weak()
                        .small(),
                );
            })
            .body(|ui| {
                for i in other {
                    repo_row(ui, &report.repos[i], false, selected, now, &clicked);
                }
            });
    }

    clicked.into_inner()
}

fn repo_row(
    ui: &mut egui::Ui,
    repo: &RepoOut,
    in_focus: bool,
    selected: Option<(&str, &str)>,
    now: i64,
    clicked: &RefCell<Option<(String, String)>>,
) {
    let current = repo.sessions.first();
    let live = in_focus
        && current
            .map(|s| focus::is_live(s.last_activity.as_deref(), now))
            .unwrap_or(false);

    let id = ui.make_persistent_id(("repo", &repo.cwd));
    CollapsingState::load_with_default_open(ui.ctx(), id, in_focus)
        .show_header(ui, |ui| {
            if live {
                ui.label(RichText::new("●").color(GREEN).small());
            }
            ui.label(RichText::new(basename(&repo.cwd)).strong());
            if let Some(branch) = &repo.git_branch {
                ui.label(RichText::new(branch).weak().small());
            }
        })
        .body(|ui| {
            if let Some(cur) = current {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title_of(cur)).small());
                    ui.label(
                        RichText::new(focus::relative(cur.last_activity.as_deref(), now))
                            .weak()
                            .small(),
                    );
                });
                for f in &cur.files {
                    file_row(ui, &cur.session_id, f, &repo.cwd, selected, now, clicked);
                }
            }

            let rest = repo.sessions.get(1..).unwrap_or(&[]);
            if !rest.is_empty() {
                let hid = ui.make_persistent_id(("hist", &repo.cwd));
                CollapsingState::load_with_default_open(ui.ctx(), hid, false)
                    .show_header(ui, |ui| {
                        ui.label(
                            RichText::new(format!("History  ({})", rest.len()))
                                .weak()
                                .small(),
                        );
                    })
                    .body(|ui| {
                        for (bucket, sessions) in buckets(rest) {
                            let bkey = ui.make_persistent_id(("bucket", &repo.cwd, bucket));
                            CollapsingState::load_with_default_open(ui.ctx(), bkey, false)
                                .show_header(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!("{bucket}  ({})", sessions.len()))
                                            .weak()
                                            .small(),
                                    );
                                })
                                .body(|ui| {
                                    for s in sessions {
                                        let skey = ui.make_persistent_id(("sess", &s.session_id));
                                        CollapsingState::load_with_default_open(
                                            ui.ctx(),
                                            skey,
                                            false,
                                        )
                                        .show_header(ui, |ui| {
                                            ui.label(RichText::new(title_of(s)).small().weak());
                                        })
                                        .body(|ui| {
                                            for f in &s.files {
                                                file_row(
                                                    ui,
                                                    &s.session_id,
                                                    f,
                                                    &repo.cwd,
                                                    selected,
                                                    now,
                                                    clicked,
                                                );
                                            }
                                        });
                                    }
                                });
                        }
                    });
            }
        });
}

fn file_row(
    ui: &mut egui::Ui,
    session_id: &str,
    f: &FileOut,
    root: &str,
    selected: Option<(&str, &str)>,
    now: i64,
    clicked: &RefCell<Option<(String, String)>>,
) {
    let is_selected = selected == Some((session_id, f.path.as_str()));
    let fg = ui.visuals().text_color();
    let dim = ui.visuals().weak_text_color();
    let prop = FontId::proportional(13.0);
    let mono = FontId::monospace(11.0);

    let mut job = LayoutJob::default();
    job.append(
        &basename(&f.path),
        0.0,
        TextFormat::simple(prop.clone(), fg),
    );
    let dir = short_dir(&rel_dir(&f.path, root));
    if !dir.is_empty() {
        job.append(
            &format!("  {dir}"),
            0.0,
            TextFormat::simple(prop.clone(), dim),
        );
    }
    let ft = focus::relative(f.last_touched.as_deref(), now);
    if !ft.is_empty() {
        job.append(
            &format!("  {ft}"),
            0.0,
            TextFormat::simple(prop.clone(), dim),
        );
    }
    job.append(
        &format!("  +{}", f.added),
        0.0,
        TextFormat::simple(mono.clone(), GREEN),
    );
    job.append(
        &format!(" -{}", f.removed),
        0.0,
        TextFormat::simple(mono, RED),
    );
    if f.user_modified {
        job.append("  ⚠", 0.0, TextFormat::simple(prop, WARN));
    }

    let hover = if f.user_modified {
        format!("{}\n⚠ Modified outside Claude", f.path)
    } else {
        f.path.clone()
    };
    let resp = ui.selectable_label(is_selected, job).on_hover_text(hover);
    if resp.clicked() {
        *clicked.borrow_mut() = Some((session_id.to_string(), f.path.clone()));
    }
}

// Amber warning color for the ⚠ flag (matches the web's --warn).
const WARN: Color32 = Color32::from_rgb(210, 153, 34);

fn basename(p: &str) -> String {
    p.split('/')
        .rfind(|s| !s.is_empty())
        .unwrap_or(p)
        .to_string()
}

/// The file's folder relative to the repo root (without the file name).
fn rel_dir(path: &str, root: &str) -> String {
    let mut p = path;
    if !root.is_empty() {
        if let Some(stripped) = p.strip_prefix(root) {
            p = stripped.trim_start_matches('/');
        }
    }
    match p.rfind('/') {
        Some(i) => p[..i].to_string(),
        None => String::new(),
    }
}

/// Compact to the last two folders: web/src/components -> …/src/components
fn short_dir(dir: &str) -> String {
    if dir.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        parts.join("/")
    } else {
        format!("…/{}", parts[parts.len() - 2..].join("/"))
    }
}

fn title_of(s: &SessionOut) -> String {
    if let Some(t) = &s.title {
        if !t.is_empty() {
            return t.clone();
        }
    }
    if let Some(p) = &s.last_prompt {
        if !p.is_empty() {
            return p.chars().take(60).collect();
        }
    }
    s.session_id.chars().take(8).collect()
}

/// Group sessions by date bucket, in BUCKET_ORDER, keeping only non-empty buckets.
fn buckets(sessions: &[SessionOut]) -> Vec<(&'static str, Vec<&SessionOut>)> {
    focus::BUCKET_ORDER
        .iter()
        .filter_map(|&b| {
            let group: Vec<&SessionOut> = sessions
                .iter()
                .filter(|s| focus::date_bucket(s.last_activity.as_deref()) == b)
                .collect();
            (!group.is_empty()).then_some((b, group))
        })
        .collect()
}
