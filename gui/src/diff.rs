//! Side-by-side diff view, the egui replacement for CodeMirror's MergeView.
//!
//! egui has no merge/diff widget, so we build one: line-align before/after with `similar`,
//! syntax-highlight each side once with syntect (themes/syntaxes bundled by `two-face`), and
//! render the aligned rows in a single virtualized `ScrollArea` — one scroll area for both
//! columns gives synchronized scrolling for free. Highlighting is done once per loaded file
//! (cached as `LayoutJob`s in [`DiffDoc`]); only the visible rows are painted each frame via
//! `show_rows`. This is the "pragmatic" v1: no collapse-unchanged and no in-diff find yet.
//!
//! Honesty is preserved from the web (`DiffView.svelte::classify`): a missing "before" is
//! never shown as a new file — it gets an explicit banner and the current file in one column.

use arrow::ContentOut;
use egui::{text::LayoutJob, Color32, FontId, Sense, TextFormat};
use similar::{ChangeTag, TextDiff};
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxSet;
use two_face::theme::EmbeddedThemeName;

const SIZE: f32 = 12.0;
const GUTTER: f32 = 48.0;

// Semi-transparent +/- backgrounds, readable over both dark and light panels.
const ADD_BG: Color32 = Color32::from_rgba_premultiplied(40, 90, 50, 90);
const DEL_BG: Color32 = Color32::from_rgba_premultiplied(95, 40, 40, 90);

/// Owns the syntax/theme assets; builds a [`DiffDoc`] per loaded file. Created once.
pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            // `no_newlines` pairs with our line-by-line splitting (lines carry no '\n').
            syntaxes: two_face::syntax::extra_no_newlines(),
            theme: two_face::theme::extra()
                .get(EmbeddedThemeName::ColdarkDark)
                .clone(),
        }
    }

    /// Pick the theme used to color code (Phase 4 swaps this when the user changes themes).
    #[allow(dead_code)]
    pub fn set_theme(&mut self, name: EmbeddedThemeName) {
        self.theme = two_face::theme::extra().get(name).clone();
    }

    /// Build the renderable diff for a loaded file: classify it, then either align both
    /// sides (a real diff) or highlight a single column (created/deleted/unknown).
    pub fn build(&self, content: &ContentOut) -> DiffDoc {
        let banner = banner_for(content);
        let body = match classify(content) {
            Mode::None => Body::Empty,
            Mode::Diff => Body::Split(self.split(content)),
            Mode::Created | Mode::Unknown => {
                Body::Single(self.highlight_all(&content.after, &content.file))
            }
            Mode::Deleted => Body::Single(self.highlight_all(&content.before, &content.file)),
        };
        DiffDoc { banner, body }
    }

    /// Highlight every line of `text` into one `LayoutJob` per line, keyed by 0-based line.
    fn highlight_all(&self, text: &str, file: &str) -> Vec<LayoutJob> {
        let syntax = self
            .syntaxes
            .find_syntax_for_file(file)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());
        let mut hl = HighlightLines::new(syntax, &self.theme);
        let font = FontId::monospace(SIZE);
        text.lines()
            .map(|line| {
                let mut job = LayoutJob::default();
                job.wrap.max_width = f32::INFINITY; // no wrap; cells clip overflow
                match hl.highlight_line(line, &self.syntaxes) {
                    Ok(ranges) => {
                        for (style, piece) in ranges {
                            let c = style.foreground;
                            job.append(
                                piece,
                                0.0,
                                TextFormat::simple(font.clone(), Color32::from_rgb(c.r, c.g, c.b)),
                            );
                        }
                    }
                    // A failed highlight degrades to plain text, never panics.
                    Err(_) => {
                        job.append(line, 0.0, TextFormat::simple(font.clone(), Color32::GRAY))
                    }
                }
                job
            })
            .collect()
    }

    /// Align before/after into side-by-side rows (the `Mode::Diff` case).
    fn split(&self, content: &ContentOut) -> Vec<RenderRow> {
        let left = self.highlight_all(&content.before, &content.file);
        let right = self.highlight_all(&content.after, &content.file);
        let diff = TextDiff::from_lines(content.before.as_str(), content.after.as_str());

        let mut rows = Vec::new();
        // Buffer consecutive deletes/inserts so a replaced block pairs left↔right per row.
        let mut dels: Vec<usize> = Vec::new();
        let mut inss: Vec<usize> = Vec::new();
        let flush = |rows: &mut Vec<Row>, dels: &mut Vec<usize>, inss: &mut Vec<usize>| {
            let n = dels.len().max(inss.len());
            for k in 0..n {
                match (dels.get(k), inss.get(k)) {
                    (Some(&l), Some(&r)) => rows.push(Row::new(Some(l), Some(r), RowKind::Replace)),
                    (Some(&l), None) => rows.push(Row::new(Some(l), None, RowKind::Delete)),
                    (None, Some(&r)) => rows.push(Row::new(None, Some(r), RowKind::Insert)),
                    (None, None) => {}
                }
            }
            dels.clear();
            inss.clear();
        };

        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Equal => {
                    flush(&mut rows, &mut dels, &mut inss);
                    rows.push(Row::new(
                        change.old_index(),
                        change.new_index(),
                        RowKind::Equal,
                    ));
                }
                ChangeTag::Delete => dels.push(change.old_index().unwrap_or(0)),
                ChangeTag::Insert => inss.push(change.new_index().unwrap_or(0)),
            }
        }
        flush(&mut rows, &mut dels, &mut inss);

        Rows { rows, left, right }.into_render()
    }
}

/// A logical aligned row: 0-based line indices into the left/right highlighted vectors.
struct Row {
    left_idx: Option<usize>,
    right_idx: Option<usize>,
    kind: RowKind,
}

impl Row {
    fn new(left_idx: Option<usize>, right_idx: Option<usize>, kind: RowKind) -> Self {
        Self {
            left_idx,
            right_idx,
            kind,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum RowKind {
    Equal,
    Delete,
    Insert,
    Replace,
}

/// Intermediate bundle so we can move the highlighted line vectors into render rows.
struct Rows {
    rows: Vec<Row>,
    left: Vec<LayoutJob>,
    right: Vec<LayoutJob>,
}

impl Rows {
    fn into_render(self) -> Vec<RenderRow> {
        let Rows { rows, left, right } = self;
        rows.into_iter()
            .map(|r| RenderRow {
                left_no: r.left_idx.map(|i| i + 1),
                right_no: r.right_idx.map(|i| i + 1),
                left: r.left_idx.and_then(|i| left.get(i).cloned()),
                right: r.right_idx.and_then(|i| right.get(i).cloned()),
                kind: r.kind,
            })
            .collect()
    }
}

/// A row ready to paint: 1-based line numbers + the highlighted jobs for each side.
struct RenderRow {
    left_no: Option<usize>,
    right_no: Option<usize>,
    left: Option<LayoutJob>,
    right: Option<LayoutJob>,
    kind: RowKind,
}

enum Body {
    Empty,
    /// One column (created → after, deleted → before, unknown → after).
    Single(Vec<LayoutJob>),
    /// Two columns aligned side by side.
    Split(Vec<RenderRow>),
}

#[derive(Clone, Copy)]
enum Banner {
    Created,
    Deleted,
    WorktreeDeleted,
    Unknown,
}

enum Mode {
    None,
    Created,
    Deleted,
    Unknown,
    Diff,
}

/// The built, cached diff for the open file. Held by the app and rebuilt on file/theme change.
pub struct DiffDoc {
    banner: Option<Banner>,
    body: Body,
}

/// Ported from `DiffView.svelte::classify`: decide what we can honestly show.
fn classify(c: &ContentOut) -> Mode {
    let b = c.before.as_str();
    let a = c.after.as_str();
    // No "before" recorded: never pretend the file is new — show the current file (unknown)
    // if we have it, otherwise nothing.
    if !c.before_available {
        return if c.after_available {
            Mode::Unknown
        } else {
            Mode::None
        };
    }
    // We have a "before". If the file is gone from disk, it was deleted.
    if !c.after_available {
        return if b.is_empty() {
            Mode::None
        } else {
            Mode::Deleted
        };
    }
    // Both sides present.
    if b.is_empty() {
        return if a.is_empty() {
            Mode::None
        } else {
            Mode::Created
        };
    }
    Mode::Diff
}

fn banner_for(c: &ContentOut) -> Option<Banner> {
    match classify(c) {
        Mode::Created => Some(Banner::Created),
        Mode::Unknown => Some(Banner::Unknown),
        Mode::Deleted => {
            // The on-disk copy is gone because its git worktree was deleted: be honest —
            // this is the last recorded state, not a live diff.
            if c.file.contains("/.claude/worktrees/") {
                Some(Banner::WorktreeDeleted)
            } else {
                Some(Banner::Deleted)
            }
        }
        _ => None,
    }
}

/// Render the cached diff. Called every frame; only visible rows are painted.
pub fn show(ui: &mut egui::Ui, doc: &DiffDoc) {
    if let Some(banner) = doc.banner {
        draw_banner(ui, banner);
    }

    // Uniform row height (show_rows needs it). Measured from a sample monospace galley.
    let row_h = ui
        .painter()
        .layout_no_wrap("X".to_owned(), FontId::monospace(SIZE), Color32::GRAY)
        .size()
        .y
        + 2.0;

    match &doc.body {
        Body::Empty => {
            ui.centered_and_justified(|ui| ui.weak("Nothing to show for this file."));
        }
        Body::Single(lines) => {
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show_rows(ui, row_h, lines.len(), |ui, range| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    let w = ui.available_width();
                    for i in range {
                        ui.horizontal(|ui| {
                            gutter(ui, row_h, Some(i + 1));
                            cell(ui, w - GUTTER, row_h, lines.get(i), None);
                        });
                    }
                });
        }
        Body::Split(rows) => {
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show_rows(ui, row_h, rows.len(), |ui, range| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    let col = ((ui.available_width() - 2.0 * GUTTER) / 2.0).max(40.0);
                    for i in range {
                        let r = &rows[i];
                        let (lbg, rbg) = backgrounds(r.kind);
                        ui.horizontal(|ui| {
                            gutter(ui, row_h, r.left_no);
                            cell(ui, col, row_h, r.left.as_ref(), lbg);
                            gutter(ui, row_h, r.right_no);
                            cell(ui, col, row_h, r.right.as_ref(), rbg);
                        });
                    }
                });
        }
    }
}

fn backgrounds(kind: RowKind) -> (Option<Color32>, Option<Color32>) {
    match kind {
        RowKind::Equal => (None, None),
        RowKind::Delete => (Some(DEL_BG), None),
        RowKind::Insert => (None, Some(ADD_BG)),
        RowKind::Replace => (Some(DEL_BG), Some(ADD_BG)),
    }
}

/// A right-aligned line-number gutter cell.
fn gutter(ui: &mut egui::Ui, row_h: f32, n: Option<usize>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(GUTTER, row_h), Sense::hover());
    if let Some(n) = n {
        let dim = ui.visuals().weak_text_color();
        let galley = ui
            .painter()
            .layout_no_wrap(n.to_string(), FontId::monospace(SIZE - 1.0), dim);
        let pos = egui::pos2(rect.right() - galley.size().x - 6.0, rect.top() + 1.0);
        ui.painter().galley(pos, galley, dim);
    }
}

/// A code cell: optional background tint + the (clipped, non-wrapping) highlighted line.
fn cell(ui: &mut egui::Ui, w: f32, row_h: f32, job: Option<&LayoutJob>, bg: Option<Color32>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, row_h), Sense::hover());
    if let Some(bg) = bg {
        ui.painter().rect_filled(rect, 0.0, bg);
    }
    if let Some(job) = job {
        let galley = ui.painter().layout_job(job.clone());
        ui.painter().with_clip_rect(rect).galley(
            rect.left_top() + egui::vec2(4.0, 1.0),
            galley,
            Color32::GRAY,
        );
    }
}

fn draw_banner(ui: &mut egui::Ui, banner: Banner) {
    let green = Color32::from_rgb(63, 185, 80);
    let red = Color32::from_rgb(248, 81, 73);
    let amber = Color32::from_rgb(210, 153, 34);
    let (text, color) = match banner {
        Banner::Created => ("＋ new file", green),
        Banner::Deleted => ("－ deleted file", red),
        Banner::WorktreeDeleted => (
            "⚠ worktree deleted — showing the last recorded state, not a live diff",
            amber,
        ),
        Banner::Unknown => (
            "⚠ no prior snapshot for this edit — showing the current file",
            amber,
        ),
    };
    ui.add_space(2.0);
    ui.colored_label(color, text);
    ui.separator();
}
