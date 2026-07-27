//! arrow — librería de auditoría de Claude Code
//!
//! Reconstruye "repo -> sesión -> archivo -> diff" leyendo los transcripts
//! NATIVOS de Claude Code en `~/.claude/projects/*/<sessionId>.jsonl`, sin hooks
//! y sin git.
//!
//! Fuente de verdad (verificada en disco, Claude Code v2.1.x):
//!   - Cada `Edit`/`Write`/`MultiEdit` emite, en un record `type:"user"`, un campo
//!     top-level `toolUseResult` con: filePath, structuredPatch (hunks exactos),
//!     userModified y, para Write, `type` = "create"/"update".
//!   - El `cwd` de cada record es la ruta real del repo.
//!   - Metadatos legibles por sesión: `type:"ai-title"` -> aiTitle (título humano),
//!     `type:"last-prompt"` -> lastPrompt, y `timestamp` en cada record (actividad).
//!
//! Solo se consideran "sesiones" los transcripts de PRIMER NIVEL dentro de cada
//! directorio de proyecto; los .jsonl anidados (subagentes, workflows) se ignoran.
//!
//! Límite honesto: solo captura lo que pasa por Edit/Write/MultiEdit. Cambios vía
//! comandos Bash (sed, prettier, build, mv, rm) NO aparecen aquí.
//!
//! Esta es la capa de datos COMPARTIDA por la CLI (`src/main.rs`) y el backend de
//! Tauri (`src-tauri/`). El comportamiento del parser es idéntico en ambos: la CLI
//! solo añade filtros (`--repo`/`--session`) y la presentación en terminal.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use walkdir::WalkDir;

// Opt-in, secondary network layer (release-update check). Kept out of the parser
// hot path — `build_report`/`file_content` never touch it; same separation as the
// git worktree layer.
pub mod update;
pub use update::{check_update, UpdateStatus};

// ---------------------------------------------------------------------------
// Modelo interno (acumulación)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct FileChange {
    pub write_type: Option<String>,
    pub user_modified: bool,
    pub ops: usize,
    pub added: usize,
    pub removed: usize,
    pub hunks: Vec<Hunk>,
    /// Most recent edit timestamp for this file within the session (max over all
    /// its Edit/Write/MultiEdit records). Drives recency ordering in the UI: the
    /// file edited last bubbles to the top, and re-touching one lifts it back up.
    pub last_ts: Option<String>,
}

pub struct Hunk {
    pub old_start: i64,
    pub old_lines: i64,
    pub new_start: i64,
    pub new_lines: i64,
    pub lines: Vec<String>,
}

#[derive(Default)]
pub struct Session {
    pub files: BTreeMap<String, FileChange>,
}

#[derive(Default)]
pub struct Repo {
    pub git_branch: Option<String>,
    pub sessions: BTreeMap<String, Session>,
}

/// Metadatos por sesión, recogidos de cualquier record con `sessionId`.
#[derive(Default)]
pub struct SessionMeta {
    pub title: Option<String>,
    pub last_prompt: Option<String>,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
    /// First HUMAN prompt of the session (a `user` record whose `message.content`
    /// is a plain string, not `isMeta`/`isCompactSummary`, not starting with "<").
    /// Title fallback when the session never got an `ai-title`.
    pub first_user_prompt: Option<String>,
    /// `cwd` of the LAST record that carried one — covers sessions resumed from a
    /// different folder than the one their transcript dir encodes.
    pub resume_cwd: Option<String>,
    /// Last 2 DISTINCT `last-prompt` values, most recent first, each truncated to
    /// 200 chars (char boundary, never splits UTF-8).
    pub last_prompts: Vec<String>,
    /// `(prNumber, prUrl)` from `pr-link` records, deduped by number, in
    /// chronological order.
    pub pr_links: Vec<(u64, String)>,
    /// Size of the session's `<sessionId>.jsonl` on disk.
    pub size_bytes: u64,
}

impl SessionMeta {
    /// Effective title: last `ai-title` wins; falls back to the first human
    /// prompt; `None` when neither exists (the UI shows "(no title)").
    pub fn effective_title(&self) -> Option<&str> {
        self.title.as_deref().or(self.first_user_prompt.as_deref())
    }
}

/// Resultado crudo de recorrer los transcripts: el modelo acumulado más las
/// estadísticas de parsing (para la cabecera de la salida en terminal).
#[derive(Default)]
pub struct Collected {
    pub repos: BTreeMap<String, Repo>,
    pub metas: BTreeMap<String, SessionMeta>,
    pub jsonl_files: usize,
    pub lines_total: usize,
    pub lines_skipped: usize,
}

// ---------------------------------------------------------------------------
// Salida JSON (contrato hacia la UI)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportOut {
    pub projects_dir: String,
    pub repo_count: usize,
    /// Claude Code's transcript retention (`cleanupPeriodDays` in
    /// `<claude root>/settings.json`; defaults to 30 when absent/invalid). The
    /// claude root is derived as the PARENT of the projects dir, so it follows
    /// `--projects-dir` overrides.
    pub retention_days: u32,
    pub repos: Vec<RepoOut>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoOut {
    pub cwd: String,
    pub git_branch: Option<String>,
    pub sessions: Vec<SessionOut>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOut {
    pub session_id: String,
    /// Last `ai-title`; falls back to the first human prompt; `None` when neither
    /// exists (the frontend shows "(no title)").
    pub title: Option<String>,
    pub last_prompt: Option<String>,
    pub first_activity: Option<String>,
    pub last_activity: Option<String>,
    /// Size of the session's `<sessionId>.jsonl` on disk, in bytes.
    pub size_bytes: u64,
    /// `cwd` of the LAST record that carried one — where the session actually
    /// ran last (covers sessions resumed from a different folder).
    pub resume_cwd: Option<String>,
    /// Last 2 DISTINCT `last-prompt` values, most recent first, each truncated
    /// to 200 chars (char boundary, UTF-8 safe).
    pub last_prompts: Vec<String>,
    /// PRs linked during the session (`pr-link` records), deduped by number,
    /// chronological order.
    pub pr_links: Vec<PrLink>,
    /// A running Claude Code process currently registers this session in
    /// `<claude root>/sessions/*.json` (verified against `/proc/<pid>` when
    /// possible; otherwise a fresh `updatedAt` < 10 min). Best-effort.
    pub live: bool,
    pub file_count: usize,
    pub files: Vec<FileOut>,
}

/// A PR linked from a session (from `pr-link` records).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PrLink {
    pub number: u64,
    pub url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOut {
    pub path: String,
    pub write_type: Option<String>,
    pub user_modified: bool,
    pub ops: usize,
    pub added: usize,
    pub removed: usize,
    /// ISO 8601 of the file's most recent edit in the session; `None` if no
    /// record carried a timestamp. Files are emitted most-recent-first.
    pub last_touched: Option<String>,
}

/// Salida del modo `--content`: el "antes" (estado previo a la primera edición de
/// la sesión, ver [`resolve_before`]) y el "después" (archivo actual en disco),
/// para alimentar @codemirror/merge.
/// `before_available = false` cuando no se pudo recuperar/reconstruir el "antes".
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentOut {
    pub file: String,
    pub session: Option<String>,
    pub before: String,
    pub after: String,
    pub before_available: bool,
    pub after_available: bool,
    pub user_modified: bool,
    pub ops: usize,
    /// 1-based first actually-changed line in the AFTER file across all of the
    /// session's edits — each hunk's `newStart` advanced past its leading context
    /// (the raw `newStart` points at the top of the context window, ~3 lines high).
    /// For "Open in editor" to jump to the first change. `None` when there are no
    /// hunks (e.g. a `create`). Points at the current on-disk file, not the before.
    pub first_changed_line: Option<i64>,
}

/// Outcome of [`trash_session`] (or its dry-run preview). `trashed` lists the
/// absolute paths sent to the system trash — or, on a dry-run, the paths that
/// WOULD be sent. Nothing in arrow ever `rm -rf`s these.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrashOut {
    /// The FULL session id that was resolved (the caller may pass a prefix).
    pub session_id: String,
    /// Preview only; disk untouched.
    pub dry_run: bool,
    /// Artifacts located: the transcript `<sessionId>.jsonl`, its sibling
    /// `<sessionId>/` dir (subagents/workflows), and the session's
    /// `file-history/` and `session-env/` dirs under the claude root.
    pub trashed: Vec<String>,
}

// ---------------------------------------------------------------------------
// API pública de la lib
// ---------------------------------------------------------------------------

/// Construye el reporte completo (sin filtros) listo para serializar a JSON.
/// Es el contrato que consume la UI (web vía `--json`, Tauri vía `invoke`).
pub fn build_report(projects_dir: &str) -> ReportOut {
    let collected = collect(projects_dir, None, None);
    build_report_from(projects_dir, &collected.repos, &collected.metas)
}

/// One target edit captured during a session scan: the inline `originalFile`
/// snapshot (if Claude Code emitted it), the patch hunks, and whether the edit
/// CREATED the file (`toolUseResult.type == "create"`). Together these let us
/// reconstruct the "before" even when the first edit lacks an inline original.
struct EditRec {
    original: Option<String>,
    hunks: Vec<Hunk>,
    is_create: bool,
}

/// Mutable accumulator used while scanning a session for one file's history.
///
/// The "before" (state prior to the session's FIRST edit) is resolved later by
/// [`resolve_before`] from these fields; `None` there means we couldn't recover
/// it — the UI must then say so instead of pretending the file is new (honesty).
#[derive(Default)]
struct BeforeScan {
    /// All edits of the target in the session, in chronological order.
    edits: Vec<EditRec>,
    /// Newest FULL `Read` snapshot of the target captured BEFORE the first edit
    /// (a good "before" when the first edit's `originalFile` is `null`).
    read_before: Option<String>,
    /// Working slot: newest full `Read` so far, frozen into `read_before` at the
    /// first edit.
    pending_read: Option<String>,
    first_edit_seen: bool,
    /// Latest edit timestamp in this transcript — used to pick the most recent
    /// session when `file_content` runs without a session filter.
    last_ts: Option<String>,
    ops: usize,
    user_modified: bool,
}

/// Reúne el "antes" y el "después" (archivo actual en disco) de UN archivo, para
/// la vista de diff.
///
/// El "antes" es el contenido del archivo justo antes de la PRIMERA edición de la
/// sesión, resuelto por [`resolve_before`] en cascada (la fuente más fiable
/// primero): `originalFile` inline del primer Edit → snapshot exacto más temprano
/// (un `originalFile` posterior o un `Read` completo) reaplicando en reversa las
/// ediciones previas → `Read` completo → reaplicar en reversa TODAS las ediciones
/// sobre el `after` de disco. Cada reconstrucción se VERIFICA contra el contenido;
/// si nada cuadra, el "antes" queda NO disponible (`before_available = false`) en
/// vez de fingir vacío o inventar un diff.
///
/// Recorre SOLO transcripts de PRIMER NIVEL (igual que `collect`/el report; los
/// `.jsonl` anidados de subagentes se ignoran). Cuando se indica `session`, abre
/// directamente su transcript `<sessionId>.jsonl` en vez de barrer todo
/// `~/.claude/projects`: el coste pasa de O(todo el historial) a O(1 archivo), lo
/// que elimina el lag al navegar archivo por archivo en la UI (la UI siempre pasa
/// la sesión del archivo seleccionado).
pub fn file_content(projects_dir: &str, file: &str, session: Option<&str>) -> ContentOut {
    let target = file.to_string();
    let projects_path = Path::new(projects_dir);

    // Scan each top-level transcript INDEPENDENTLY and keep the one whose edits
    // are most recent. One transcript = one session, so a file edited by several
    // sessions is never blended into a fabricated cross-session history (matters
    // for the CLI's no-`--session` path; the UI always passes a session).
    let mut scan = BeforeScan::default();
    for entry in WalkDir::new(projects_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.extension().map(|e| e == "jsonl").unwrap_or(false) {
            continue;
        }
        // Solo transcripts de primer nivel: <projectdir>/<sessionId>.jsonl.
        let top_level = path
            .strip_prefix(projects_path)
            .map(|r| r.components().count() == 2)
            .unwrap_or(false);
        if !top_level {
            continue;
        }
        // Atajo: con sesión, abrimos solo el transcript cuyo nombre ES ese sessionId
        // (acepta prefijo, igual que `--session`). El resto se salta sin leerlo.
        if let Some(sf) = session {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if !stem.starts_with(sf) {
                continue;
            }
        }
        let mut one = BeforeScan::default();
        scan_transcript(path, &target, session, &mut one);
        if !one.first_edit_seen {
            continue; // this transcript never edited the file
        }
        if one.last_ts > scan.last_ts {
            scan = one; // most recent session wins
        }
    }

    let (after, after_available) = match std::fs::read_to_string(&target) {
        Ok(s) => (s, true),
        Err(_) => (String::new(), false),
    };

    let before = resolve_before(&scan.edits, scan.read_before.as_deref(), &after);
    let before_available = before.is_some();
    // Earliest changed line (after side) across all of this session's edits, so
    // "Open in editor" lands on the first change instead of the file top. The
    // hunks exist regardless of whether the `before` could be reconstructed.
    let first_changed_line = scan
        .edits
        .iter()
        .flat_map(|e| e.hunks.iter())
        .map(|h| {
            // `new_start` is the first line of the hunk's CONTEXT window, not the
            // first changed line. Advance past the leading context lines (those
            // prefixed with ' ') so the cursor lands on the first actually-changed
            // line on the after side (~3 lines lower for a typical hunk).
            let lead = h.lines.iter().take_while(|l| l.starts_with(' ')).count() as i64;
            h.new_start + lead
        })
        .filter(|&n| n >= 1)
        .min();
    ContentOut {
        file: target,
        session: session.map(str::to_string),
        before_available,
        before: before.unwrap_or_default(),
        after,
        after_available,
        user_modified: scan.user_modified,
        ops: scan.ops,
        first_changed_line,
    }
}

/// Escanea un transcript acumulando, para `target`: las ediciones (con su
/// `originalFile` y hunks), el último `Read` completo previo a la 1ª edición, el
/// nº de `ops` y el flag `userModified`. Filtra por `session` (prefijo) si se da.
///
/// Mira dos clases de record: los `Read` (cuyo snapshot completo respalda el
/// "antes") y los Edit/Write/MultiEdit (que aportan `originalFile` + hunks).
fn scan_transcript(path: &Path, target: &str, session: Option<&str>, scan: &mut BeforeScan) {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };
    for line in BufReader::new(f).lines().map_while(std::result::Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let tur = match v.get("toolUseResult") {
            Some(t) if t.is_object() => t,
            _ => continue,
        };
        // Session filter (prefix), applied to BOTH Read and Edit records.
        if let Some(sf) = session {
            let sid = v.get("sessionId").and_then(Value::as_str).unwrap_or("");
            if !sid.starts_with(sf) {
                continue;
            }
        }
        // (1) Read of the target: remember its full-file snapshot (only matters
        //     until the first edit, which freezes it into `read_before`).
        if !scan.first_edit_seen {
            if let Some(content) = read_snapshot(tur, target) {
                scan.pending_read = Some(content);
            }
        }
        // (2) Edit/Write/MultiEdit of the target (top-level `filePath`).
        if tur.get("filePath").and_then(Value::as_str) != Some(target) {
            continue;
        }
        if !scan.first_edit_seen {
            scan.read_before = scan.pending_read.take();
            scan.first_edit_seen = true;
        }
        scan.ops += 1;
        if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
            if scan.last_ts.as_deref().map(|l| ts > l).unwrap_or(true) {
                scan.last_ts = Some(ts.to_string());
            }
        }
        if tur
            .get("userModified")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            scan.user_modified = true;
        }
        let original = tur
            .get("originalFile")
            .and_then(Value::as_str)
            .map(str::to_string);
        scan.edits.push(EditRec {
            original,
            hunks: parse_hunks(tur),
            is_create: tur.get("type").and_then(Value::as_str) == Some("create"),
        });
    }
}

/// If `tur` is a `Read` result for `target` covering the WHOLE file, returns its
/// captured content. A `Read` result is `{ file: { filePath, content, startLine,
/// numLines, totalLines }, type: "text" }`. We reject offset/truncated reads
/// (`startLine > 1` or `numLines != totalLines`): a partial snapshot would yield a
/// wrong "before" and spurious diff lines, so we'd rather report it unavailable.
fn read_snapshot(tur: &Value, target: &str) -> Option<String> {
    let file = tur.get("file")?;
    if file.get("filePath").and_then(Value::as_str) != Some(target) {
        return None;
    }
    let start = file.get("startLine").and_then(Value::as_i64).unwrap_or(1);
    let full = start <= 1
        && match (
            file.get("numLines").and_then(Value::as_i64),
            file.get("totalLines").and_then(Value::as_i64),
        ) {
            (Some(n), Some(t)) => n == t,
            _ => true, // fields absent: assume the read covers the file
        };
    if !full {
        return None;
    }
    file.get("content")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Parses `toolUseResult.structuredPatch` into hunks. Defensive: a missing or
/// malformed patch yields an empty vec.
fn parse_hunks(tur: &Value) -> Vec<Hunk> {
    let arr = match tur.get("structuredPatch").and_then(Value::as_array) {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .map(|h| Hunk {
            old_start: h.get("oldStart").and_then(Value::as_i64).unwrap_or(0),
            old_lines: h.get("oldLines").and_then(Value::as_i64).unwrap_or(0),
            new_start: h.get("newStart").and_then(Value::as_i64).unwrap_or(0),
            new_lines: h.get("newLines").and_then(Value::as_i64).unwrap_or(0),
            lines: h
                .get("lines")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|l| l.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

/// Resolves the file's "before" (state prior to the session's FIRST edit) from
/// the captured `edits`, an optional full `Read` snapshot, and the on-disk
/// `after`. Most authoritative source first:
///   0. a CREATE (`type:"create"`) → the file didn't exist, so "before" is empty;
///   1. the first edit's inline `originalFile` (an exact pre-edit snapshot);
///   2. the earliest LATER inline snapshot, reverse-applying the edits before it;
///   3. a full `Read` snapshot, but only if it lines up with the first edit;
///   4. reverse-apply ALL edits onto the on-disk `after`.
///
/// Sources 2 and 4 are reconstructions verified PER HUNK by [`reverse_apply`]
/// (a mismatch yields `None`), and source 3 is checked against the first edit by
/// [`snapshot_consistent`]. Verification is LOCAL to the touched regions, so a
/// case-4 reconstruction off a stale disk is best-effort: out-of-band changes in
/// regions no hunk touched are carried through (they appear identically in before
/// and after, so they're never attributed as a diff). `None` overall means
/// honestly "unavailable".
fn resolve_before(edits: &[EditRec], read_before: Option<&str>, after: &str) -> Option<String> {
    if let Some(first) = edits.first() {
        // 0. A brand-new file (Write create): nothing existed before it.
        if first.is_create {
            return Some(String::new());
        }
        // 1. The first edit already carries the exact pre-edit content.
        if let Some(orig) = &first.original {
            return Some(orig.clone());
        }
    }
    // 2. Earliest LATER inline snapshot (F_k) → reverse the k edits before it.
    //    Handles a first edit whose `originalFile` came back `null`.
    for k in 1..edits.len() {
        if let Some(anchor) = edits[k].original.as_deref() {
            if let Some(before) = reverse_apply(anchor, &edits[..k]) {
                return Some(before);
            }
            break; // anchor found but reconstruction failed; fall through
        }
    }
    // 3. A full Read snapshot captured before the first edit — trusted only if it
    //    lines up with the first edit's "before" side. An untracked change between
    //    the Read and the edit would otherwise be misattributed to this session.
    if let Some(r) = read_before {
        if edits
            .first()
            .map(|e| snapshot_consistent(r, e))
            .unwrap_or(true)
        {
            return Some(r.to_string());
        }
    }
    // 4. Last resort: undo EVERY edit from the current disk content. Only when at
    //    least one edit has hunks (otherwise we'd just echo `after`).
    if edits.iter().any(|e| !e.hunks.is_empty()) {
        if let Some(before) = reverse_apply(after, edits) {
            return Some(before);
        }
    }
    None
}

/// Checks that `snapshot` (a candidate pre-edit state) is consistent with `edit`
/// being the next change: every hunk's "before" view (context + removed) must
/// appear at the hunk's OLD coordinates. Lets [`resolve_before`] reject a `Read`
/// snapshot that drifted from what the edit actually saw.
fn snapshot_consistent(snapshot: &str, edit: &EditRec) -> bool {
    let lines: Vec<&str> = snapshot.split('\n').collect();
    for h in &edit.hunks {
        if h.old_start < 1 {
            return false;
        }
        let start = (h.old_start - 1) as usize;
        let before_view: Vec<&str> = h
            .lines
            .iter()
            .filter_map(|l| match l.as_bytes().first() {
                Some(b'+') | Some(b'\\') => None,
                Some(b'-') => Some(&l[1..]),
                _ => Some(l.strip_prefix(' ').unwrap_or(l)),
            })
            .collect();
        let end = start + before_view.len();
        if end > lines.len() || lines[start..end] != before_view[..] {
            return false;
        }
    }
    true
}

/// Reverse-applies `edits` (chronological — each transforms F_i → F_{i+1}) onto
/// `anchor` (the content AFTER the last edit in the slice, i.e. F_n), returning
/// the content BEFORE the first edit (F_0).
///
/// Each hunk's "after" view is verified against the current reconstruction at its
/// `newStart`; any mismatch (drift, stale disk, a non-tool edit in a touched
/// region, or overlapping hunks) returns `None` so the caller can fall back
/// rather than show a wrong "before". Verification is LOCAL: changes in regions
/// NO hunk touches aren't checked — when `anchor` is a stale disk they pass
/// through unchanged (and thus never surface as a diff). Patch line prefixes:
/// ' ' context, '+' added (after only), '-' removed (before only). The
/// "\ No newline at end of file" marker is NOT modeled — it never appears inside
/// `structuredPatch.lines` in real top-level transcripts; if it ever did, the
/// reconstructed trailing newline could be off by one line in the last hunk.
fn reverse_apply(anchor: &str, edits: &[EditRec]) -> Option<String> {
    // `split('\n')` keeps a trailing "" when the content ends in a newline, and
    // `join("\n")` restores it exactly.
    let mut lines: Vec<String> = anchor.split('\n').map(str::to_string).collect();
    for edit in edits.iter().rev() {
        // Highest newStart first, so earlier splices don't shift later indices
        // within the same edit. `min_processed` is the lowest index already
        // spliced this edit: a hunk reaching into it means overlapping after-view
        // ranges — internally inconsistent, so bail instead of inventing.
        let mut hunks: Vec<&Hunk> = edit.hunks.iter().collect();
        hunks.sort_by_key(|h| std::cmp::Reverse(h.new_start));
        let mut min_processed = usize::MAX;
        for h in hunks {
            if h.new_start < 1 {
                return None;
            }
            let start = (h.new_start - 1) as usize;
            // Build the hunk's "after" view (context + added) and "before" view
            // (context + removed) from the prefixed patch lines.
            let mut after_view: Vec<String> = Vec::new();
            let mut before_view: Vec<String> = Vec::new();
            for l in &h.lines {
                match l.as_bytes().first() {
                    Some(b'+') => after_view.push(l[1..].to_string()),
                    Some(b'-') => before_view.push(l[1..].to_string()),
                    Some(b'\\') => {} // "\ No newline at end of file": ignore
                    _ => {
                        // Context line: one leading space (or a blank line).
                        let c = l.strip_prefix(' ').unwrap_or(l);
                        after_view.push(c.to_string());
                        before_view.push(c.to_string());
                    }
                }
            }
            let end = start + after_view.len();
            if end > lines.len() || end > min_processed || lines[start..end] != after_view[..] {
                return None; // doesn't line up, or overlaps a sibling hunk: bail
            }
            lines.splice(start..end, before_view);
            min_processed = start;
        }
    }
    Some(lines.join("\n"))
}

/// Recorre los transcripts de primer nivel de `projects_dir` y acumula el modelo.
/// `repo_filter` y `session_filter` replican EXACTAMENTE los flags `--repo`/`--session`
/// de la CLI (substring de la raíz git / prefijo del sessionId). La UI los pasa como
/// `None` (sin filtros).
pub fn collect(
    projects_dir: &str,
    repo_filter: Option<&str>,
    session_filter: Option<&str>,
) -> Collected {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let projects_path = Path::new(projects_dir);
    // Solo se filtra el ALMACÉN GLOBAL de Claude (~/.claude/), no cualquier carpeta
    // .claude/: un `.claude/` dentro de un repo (settings, skills…) SÍ es tu código.
    let claude_home = format!("{home}/.claude/");
    let mut c = Collected::default();
    let mut roots: HashMap<String, String> = HashMap::new(); // cache cwd -> raíz git

    for entry in WalkDir::new(projects_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.extension().map(|e| e == "jsonl").unwrap_or(false) {
            continue;
        }
        // Solo transcripts de primer nivel: <projectdir>/<sessionId>.jsonl
        let top_level = path
            .strip_prefix(projects_path)
            .map(|r| r.components().count() == 2)
            .unwrap_or(false);
        if !top_level {
            continue;
        }

        c.jsonl_files += 1;
        // Transcript identity: the file name IS the sessionId, and the first
        // relative component is the encoded project dir name (the last-resort
        // anchor when no record in the session carries a cwd).
        let sid_from_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let encoded_dir = path
            .strip_prefix(projects_path)
            .ok()
            .and_then(|r| r.components().next())
            .and_then(|comp| comp.as_os_str().to_str())
            .unwrap_or("")
            .to_string();
        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);

        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for line in BufReader::new(file)
            .lines()
            .map_while(std::result::Result::ok)
        {
            if line.trim().is_empty() {
                continue;
            }
            c.lines_total += 1;
            match serde_json::from_str::<Value>(&line) {
                Ok(v) => ingest(
                    &v,
                    repo_filter,
                    session_filter,
                    &mut c.repos,
                    &mut c.metas,
                    &mut roots,
                    &claude_home,
                ),
                Err(_) => c.lines_skipped += 1, // parsing defensivo: formato no documentado
            }
        }

        // --- every top-level transcript IS a session, edits or not ---
        // A session used to exist only once an Edit/Write/MultiEdit showed up;
        // now the `<sessionId>.jsonl` file itself creates it (empty `files`,
        // fileCount 0), so the Sessions tab can list ALL sessions.
        if sid_from_name.is_empty() {
            continue;
        }
        if !session_filter
            .map(|sf| sid_from_name.starts_with(sf))
            .unwrap_or(true)
        {
            continue;
        }
        let resume_cwd = {
            let meta = c.metas.entry(sid_from_name.clone()).or_default();
            meta.size_bytes = size_bytes;
            meta.resume_cwd.clone()
        };
        let already_listed = c
            .repos
            .values()
            .any(|r| r.sessions.contains_key(&sid_from_name));
        if !already_listed {
            // Anchor: git root of the LAST cwd seen (covers sessions resumed
            // from another folder — their records' cwd differs from the encoded
            // dir name); a non-git cwd anchors as-is (git_root returns it);
            // with no cwd in any record, best-effort decode of the dir name.
            let anchor = match &resume_cwd {
                Some(cwd) => git_root(cwd, &mut roots),
                None => decode_project_dir(&encoded_dir),
            };
            if repo_filter.map(|rf| anchor.contains(rf)).unwrap_or(true) {
                c.repos
                    .entry(anchor)
                    .or_default()
                    .sessions
                    .entry(sid_from_name)
                    .or_default();
            }
        }
    }

    c
}

/// Procesa un record JSONL: actualiza metadatos de sesión y, si representa una
/// edición de archivo, la acumula.
fn ingest(
    v: &Value,
    repo_filter: Option<&str>,
    session_filter: Option<&str>,
    repos: &mut BTreeMap<String, Repo>,
    metas: &mut BTreeMap<String, SessionMeta>,
    roots: &mut HashMap<String, String>,
    claude_home: &str,
) {
    // --- metadatos: cualquier record con sessionId ---
    if let Some(sid) = v.get("sessionId").and_then(Value::as_str) {
        let pass = session_filter.map(|sf| sid.starts_with(sf)).unwrap_or(true);
        if pass {
            match v.get("type").and_then(Value::as_str) {
                Some("ai-title") => {
                    // Overwrite on every record: the LAST ai-title wins.
                    if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
                        metas.entry(sid.to_string()).or_default().title = Some(t.to_string());
                    }
                }
                Some("last-prompt") => {
                    if let Some(p) = v.get("lastPrompt").and_then(Value::as_str) {
                        let e = metas.entry(sid.to_string()).or_default();
                        e.last_prompt = Some(p.to_string());
                        // Keep the last 2 DISTINCT prompts, most recent first.
                        // Compared/stored truncated to 200 chars (char boundary,
                        // so a multi-byte prompt never gets split mid-UTF-8).
                        let t = truncate_chars(p, 200);
                        if e.last_prompts.first() != Some(&t) {
                            e.last_prompts.insert(0, t);
                            e.last_prompts.truncate(2);
                        }
                    }
                }
                Some("pr-link") => {
                    // PR created/linked during the session. Claude Code re-emits
                    // the same PR on later turns → dedupe by number, keep the
                    // first occurrence (chronological order).
                    if let (Some(n), Some(u)) = (
                        v.get("prNumber").and_then(Value::as_u64),
                        v.get("prUrl").and_then(Value::as_str),
                    ) {
                        let e = metas.entry(sid.to_string()).or_default();
                        if !e.pr_links.iter().any(|(num, _)| *num == n) {
                            e.pr_links.push((n, u.to_string()));
                        }
                    }
                }
                Some("user") => {
                    // Title fallback: the FIRST human prompt — a plain-string
                    // content that isn't meta bookkeeping (`isMeta`, compact
                    // summaries) and doesn't start with "<" (slash-command XML).
                    let is_meta = v.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
                        || v.get("isCompactSummary")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                    if !is_meta {
                        if let Some(text) = v
                            .get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(Value::as_str)
                        {
                            let t = text.trim();
                            if !t.is_empty() && !t.starts_with('<') {
                                let e = metas.entry(sid.to_string()).or_default();
                                if e.first_user_prompt.is_none() {
                                    e.first_user_prompt = Some(truncate_chars(t, 200));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            // Last cwd seen wins: where the session actually ran last (a resumed
            // session's later records carry the new folder).
            if let Some(cwd) = v.get("cwd").and_then(Value::as_str) {
                if !cwd.is_empty() {
                    metas.entry(sid.to_string()).or_default().resume_cwd = Some(cwd.to_string());
                }
            }
            if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
                let e = metas.entry(sid.to_string()).or_default();
                if e.first_ts.as_deref().map(|f| ts < f).unwrap_or(true) {
                    e.first_ts = Some(ts.to_string());
                }
                if e.last_ts.as_deref().map(|l| ts > l).unwrap_or(true) {
                    e.last_ts = Some(ts.to_string());
                }
            }
        }
    }

    // --- cambio de archivo: toolUseResult ---
    let tur = match v.get("toolUseResult") {
        Some(t) if t.is_object() => t,
        _ => return,
    };
    let file_path = match tur.get("filePath").and_then(Value::as_str) {
        Some(s) => s.to_string(),
        None => return,
    };
    // Almacén global de Claude (memoria, historial…) NO es tu código. Pero un
    // `.claude/` DENTRO de un repo (settings, skills) sí: por eso filtramos por el
    // prefijo del HOME, no por la subcadena "/.claude/".
    if file_path.starts_with(claude_home) {
        return;
    }
    let cwd = v
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("(cwd desconocido)")
        .to_string();
    let session = v
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("(sin sesión)")
        .to_string();
    let branch = v
        .get("gitBranch")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // El repo es la raíz git del cwd de la sesión (fusiona subdirectorios como
    // web/ con su repo). Estable aunque el proyecto no sea git.
    let repo_key = git_root(&cwd, roots);

    if let Some(rf) = repo_filter {
        if !repo_key.contains(rf) {
            return;
        }
    }
    if let Some(sf) = session_filter {
        if !session.starts_with(sf) {
            return;
        }
    }

    let repo = repos.entry(repo_key).or_default();
    if repo.git_branch.is_none() {
        repo.git_branch = branch;
    }
    let sess = repo.sessions.entry(session).or_default();
    let fc = sess.files.entry(file_path).or_default();

    fc.ops += 1;
    // Track the latest edit time for this file (record-level `timestamp`, same
    // field the session meta uses). Kept as max so a re-touch updates recency.
    if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
        if fc.last_ts.as_deref().map(|l| ts > l).unwrap_or(true) {
            fc.last_ts = Some(ts.to_string());
        }
    }
    if let Some(t) = tur.get("type").and_then(Value::as_str) {
        fc.write_type = Some(t.to_string());
    }
    if tur
        .get("userModified")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        fc.user_modified = true;
    }

    for h in parse_hunks(tur) {
        for l in &h.lines {
            match l.as_bytes().first() {
                Some(b'+') => fc.added += 1,
                Some(b'-') => fc.removed += 1,
                _ => {}
            }
        }
        fc.hunks.push(h);
    }
}

/// Construye el reporte JSON, ordenando sesiones y repos por actividad reciente.
pub fn build_report_from(
    projects_dir: &str,
    repos: &BTreeMap<String, Repo>,
    metas: &BTreeMap<String, SessionMeta>,
) -> ReportOut {
    // Claude root = PARENT of the projects dir (follows --projects-dir).
    // `sessions/` (live processes) and `settings.json` (retention) are read
    // ONCE per build here — never per session.
    let claude_root = Path::new(projects_dir)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf);
    let live = claude_root
        .as_deref()
        .map(live_sessions)
        .unwrap_or_default();
    let retention_days = claude_root
        .as_deref()
        .map(read_retention_days)
        .unwrap_or(30);

    let mut repos_out: Vec<RepoOut> = repos
        .iter()
        .map(|(cwd, repo)| {
            let mut sessions: Vec<SessionOut> = repo
                .sessions
                .iter()
                .map(|(sid, sess)| {
                    let m = metas.get(sid);
                    // Files: most-recently-edited first (last_touched desc), mirroring how
                    // sessions/repos are already ordered by recency. The file you're editing
                    // now sits at the top, and a re-touch lifts it back up on the next live
                    // refresh. Undated files sink to the end; ties break by path for a stable
                    // order. Recency is anchored to the data timestamp, not the wall clock.
                    let mut files: Vec<FileOut> = sess
                        .files
                        .iter()
                        .map(|(path, fc)| FileOut {
                            path: path.clone(),
                            write_type: fc.write_type.clone(),
                            user_modified: fc.user_modified,
                            ops: fc.ops,
                            added: fc.added,
                            removed: fc.removed,
                            last_touched: fc.last_ts.clone(),
                        })
                        .collect();
                    files.sort_by(|a, b| {
                        b.last_touched
                            .cmp(&a.last_touched)
                            .then_with(|| a.path.cmp(&b.path))
                    });
                    SessionOut {
                        session_id: sid.clone(),
                        title: m.and_then(|m| m.effective_title().map(str::to_string)),
                        last_prompt: m.and_then(|m| m.last_prompt.clone()),
                        first_activity: m.and_then(|m| m.first_ts.clone()),
                        last_activity: m.and_then(|m| m.last_ts.clone()),
                        size_bytes: m.map(|m| m.size_bytes).unwrap_or(0),
                        resume_cwd: m.and_then(|m| m.resume_cwd.clone()),
                        last_prompts: m.map(|m| m.last_prompts.clone()).unwrap_or_default(),
                        pr_links: m
                            .map(|m| {
                                m.pr_links
                                    .iter()
                                    .map(|(n, u)| PrLink {
                                        number: *n,
                                        url: u.clone(),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        live: live.contains(sid),
                        file_count: files.len(),
                        files,
                    }
                })
                .collect();
            // Sesiones: más reciente primero (last_activity desc; None al final).
            sessions.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
            RepoOut {
                cwd: cwd.clone(),
                git_branch: repo.git_branch.clone(),
                sessions,
            }
        })
        .collect();

    // Repos: por la actividad de su sesión más reciente (desc).
    repos_out.sort_by(|a, b| {
        let ka = a.sessions.first().and_then(|s| s.last_activity.clone());
        let kb = b.sessions.first().and_then(|s| s.last_activity.clone());
        kb.cmp(&ka)
    });

    ReportOut {
        projects_dir: projects_dir.to_string(),
        repo_count: repos_out.len(),
        retention_days,
        repos: repos_out,
    }
}

/// Resuelve la raíz del repo git que contiene `cwd` (sube buscando `.git`).
/// Así un cwd que derivó a un subdirectorio (p.ej. .../arrow/web) se agrupa con
/// su repo (.../arrow). Si no hay `.git`, usa el propio cwd. Cachea por cwd.
fn git_root(cwd: &str, cache: &mut HashMap<String, String>) -> String {
    if let Some(r) = cache.get(cwd) {
        return r.clone();
    }
    let mut dir = PathBuf::from(cwd);
    let mut root = cwd.to_string();
    loop {
        let dot_git = dir.join(".git");
        if dot_git.is_dir() {
            // Normal repo: the directory holding `.git/` is the root.
            root = dir.to_string_lossy().into_owned();
            break;
        }
        if dot_git.is_file() {
            // Linked git worktree: `.git` is a FILE, not a dir — a pointer
            // `gitdir: <main>/.git/worktrees/<name>`. Re-anchor to the MAIN repo so a
            // session whose cwd lives inside `<repo>/.claude/worktrees/<name>/` folds
            // under `<repo>`, instead of surfacing the worktree as a phantom top-level
            // repo (which double-counted it and mixed it with real repos). Falls back
            // to the worktree dir if the pointer isn't a recognizable worktree shape.
            // Filesystem-only (reads one small dotfile) — the lib still never spawns git.
            root =
                worktree_main_root(&dot_git).unwrap_or_else(|| dir.to_string_lossy().into_owned());
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    cache.insert(cwd.to_string(), root.clone());
    root
}

/// Given a linked worktree's `.git` FILE (`gitdir: <main>/.git/worktrees/<name>`),
/// return the MAIN repo root (`<main>`). Returns `None` for a submodule pointer
/// (`gitdir: <main>/.git/modules/<name>`) or any shape we don't recognize, so the
/// caller keeps the worktree dir rather than guessing. Pure filesystem read.
fn worktree_main_root(dot_git_file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dot_git_file).ok()?;
    let gitdir = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    // `<main>/.git/worktrees/<name>` -> `<main>`. A submodule's `.git/modules/…`
    // pointer has no `/.git/worktrees/` segment, so `split` yields the whole string
    // and we bail (a submodule is legitimately its own repo root).
    let main = gitdir.split("/.git/worktrees/").next()?;
    if main.is_empty() || main == gitdir {
        return None;
    }
    Some(main.to_string())
}

/// Truncates to `max` CHARACTERS (a char boundary, never a byte offset — so a
/// multi-byte UTF-8 sequence is never split).
fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Best-effort decode of an encoded project dir name ("-home-user-proj" →
/// "/home/user/proj"). Ambiguous by design — a real "-" in a folder name is
/// indistinguishable from the separator — so it's only the LAST-RESORT anchor,
/// used when no record in the session carried a cwd.
fn decode_project_dir(name: &str) -> String {
    let decoded = name.trim_start_matches('-').replace('-', "/");
    format!("/{decoded}")
}

/// Session ids registered by RUNNING Claude Code processes, read from
/// `<claude root>/sessions/*.json` ({pid, sessionId, updatedAt, ...}) in ONE
/// pass. A session counts as live when its process is verifiably alive
/// (`/proc/<pid>` exists, on systems that have /proc) or — when that can't be
/// checked — when its `updatedAt` is fresher than 10 minutes. Best-effort:
/// a stale registry file whose pid got recycled could false-positive.
fn live_sessions(claude_root: &Path) -> HashSet<String> {
    let mut live = HashSet::new();
    let entries = match std::fs::read_dir(claude_root.join("sessions")) {
        Ok(e) => e,
        Err(_) => return live, // no registry dir: nothing is live
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            // Defensive: an unreadable/malformed registry file is skipped.
            let v: Value = match std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
            {
                Some(v) => v,
                None => continue,
            };
            let sid = match v.get("sessionId").and_then(Value::as_str) {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            };
            let pid = v.get("pid").and_then(Value::as_u64);
            let updated_ms = v.get("updatedAt").and_then(Value::as_u64);
            if session_process_alive(pid, updated_ms, now_ms) {
                live.insert(sid.to_string());
            }
        }
    }
    live
}

/// Is the registered process alive? `/proc/<pid>` when /proc exists (Linux);
/// without procfs (macOS) ask `ps -p <pid>` (argv-direct, the same shell-out
/// pattern as gio/curl). Only when the pid can't be checked at all fall back to
/// "updatedAt is fresher than 10 minutes" — Claude Code does NOT heartbeat
/// updatedAt while a session sits idle, so that fallback alone would call a
/// running-but-idle session dead (and let `trash_session` delete it).
fn session_process_alive(pid: Option<u64>, updated_ms: Option<u64>, now_ms: u64) -> bool {
    if let Some(pid) = pid {
        if Path::new("/proc").is_dir() {
            return Path::new(&format!("/proc/{pid}")).exists();
        }
        if let Ok(status) = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "pid="])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            return status.success();
        }
    }
    const TEN_MIN_MS: u64 = 10 * 60 * 1000;
    updated_ms
        .map(|u| now_ms.saturating_sub(u) < TEN_MIN_MS)
        .unwrap_or(false)
}

/// Claude Code's transcript retention: `cleanupPeriodDays` from
/// `<claude root>/settings.json`. Defensive: a missing file, invalid JSON, or a
/// null/non-numeric value all yield the documented default of 30.
fn read_retention_days(claude_root: &Path) -> u32 {
    std::fs::read_to_string(claude_root.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("cleanupPeriodDays").and_then(Value::as_u64))
        .map(|d| d.min(u32::MAX as u64) as u32)
        .unwrap_or(30)
}

// ---------------------------------------------------------------------------
// Trash (the lib's ONLY disk mutation — and a reversible one)
// ---------------------------------------------------------------------------

/// Sends ALL of a session's artifacts to the system trash (never `rm -rf`):
/// the transcript `<sessionId>.jsonl` (searched across every `projects/<dir>/`),
/// its sibling `<sessionId>/` dir (subagents/workflows), and the session's
/// `file-history/<sessionId>/` and `session-env/<sessionId>/` dirs under the
/// claude root (= parent of `projects_dir`).
///
/// `session_id` may be a prefix, but it must resolve to exactly ONE session
/// (ambiguity is an error — this deletes things). REFUSES to touch a session a
/// running Claude Code process has registered (see [`live_sessions`]).
///
/// With `execute == false` this is a DRY-RUN: it only locates and returns the
/// paths, touching nothing. With `execute == true` it shells out to the
/// platform trash (`gio trash` on Linux; a move into `~/.Trash` on macOS,
/// suffixing on name collisions). If the trash tool is missing or fails, the
/// call returns an error WITHOUT falling back to any destructive delete.
pub fn trash_session(
    projects_dir: &str,
    session_id: &str,
    execute: bool,
) -> anyhow::Result<TrashOut> {
    anyhow::ensure!(!session_id.is_empty(), "empty session id");
    // A session id (or prefix) is only ever alphanumeric plus dashes. Rejecting
    // anything else up front (dots, path separators, globs…) guarantees the
    // artifact paths built below can never escape their intended directories
    // (`--trash-session ..` must not resolve to the whole claude root).
    anyhow::ensure!(
        session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "invalid session id '{session_id}' — expected letters, digits and dashes only"
    );
    let projects_path = Path::new(projects_dir);

    // (a) Locate the transcript(s): top-level `<dir>/<sessionId>.jsonl` only,
    // same rule as the report (nested subagent transcripts are not sessions).
    let mut matched: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in WalkDir::new(projects_path)
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        // Transcripts are FILES; a directory named `x.jsonl` is not a session.
        if !path.is_file() || !path.extension().map(|e| e == "jsonl").unwrap_or(false) {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        // Same charset rule as the input id: a stem like `..` (from a planted
        // `...jsonl`) must never become a trash target.
        if stem.is_empty() || !stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            continue;
        }
        if stem.starts_with(session_id) {
            matched
                .entry(stem.to_string())
                .or_default()
                .push(path.to_path_buf());
        }
    }
    if matched.is_empty() {
        anyhow::bail!("session '{session_id}' not found under {projects_dir} — nothing to trash");
    }
    if matched.len() > 1 {
        // Char-based truncation (a byte slice could split a UTF-8 boundary).
        let ids: Vec<String> = matched
            .keys()
            .map(|s| s.chars().take(8).collect())
            .collect();
        anyhow::bail!(
            "'{session_id}' is ambiguous — it matches {} sessions ({}…); pass a longer prefix",
            matched.len(),
            ids.join(", ")
        );
    }
    let (sid, transcripts) = matched.into_iter().next().expect("checked non-empty");

    // (b) Refuse a live session (a running process still writes these files).
    // Canonicalize first so a relative `--projects-dir` still yields the claude
    // root (its parent) and this guard is never skipped silently.
    let canonical =
        std::fs::canonicalize(projects_path).unwrap_or_else(|_| projects_path.to_path_buf());
    let claude_root = canonical
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf);
    if let Some(root) = &claude_root {
        if live_sessions(root).contains(&sid) {
            anyhow::bail!(
                "session {sid} is live (a running Claude Code process has it registered in \
                 {}/sessions) — refusing to trash it",
                root.display()
            );
        }
    }

    // (c) Gather every existing artifact.
    let mut targets: Vec<PathBuf> = Vec::new();
    for jsonl in &transcripts {
        targets.push(jsonl.clone());
        // Sibling `<sessionId>/` dir: subagents/ and workflows/.
        if let Some(dir) = jsonl.parent().map(|d| d.join(&sid)) {
            if dir.is_dir() {
                targets.push(dir);
            }
        }
    }
    if let Some(root) = &claude_root {
        for sub in ["file-history", "session-env"] {
            let p = root.join(sub).join(&sid);
            if p.exists() {
                targets.push(p);
            }
        }
    }

    // (d) Dry-run returns the located paths untouched; execute trashes them.
    if execute {
        send_to_trash(&targets)?;
    }
    Ok(TrashOut {
        session_id: sid,
        dry_run: !execute,
        trashed: targets
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
    })
}

/// Linux (and other non-mac unixes): `gio trash -- <paths…>` — the freedesktop
/// trash, recoverable from the file manager. Same shell-out pattern as
/// `update.rs`/curl: argv-direct, no shell. A missing gio or a non-zero exit is
/// an ERROR — there is deliberately no `rm -rf` fallback.
#[cfg(not(target_os = "macos"))]
fn send_to_trash(paths: &[PathBuf]) -> anyhow::Result<()> {
    let out = std::process::Command::new("gio")
        .arg("trash")
        .arg("--")
        .args(paths)
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "couldn't run `gio trash` ({e}) — nothing was deleted; install gio (glib2) \
                 or trash the paths manually"
            )
        })?;
    if !out.status.success() {
        anyhow::bail!(
            "`gio trash` failed: {} — arrow never falls back to a destructive delete",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// macOS: move into `~/.Trash/` (same volume as `~/.claude`, so a plain rename
/// works), suffixing the name if it collides with something already trashed.
#[cfg(target_os = "macos")]
fn send_to_trash(paths: &[PathBuf]) -> anyhow::Result<()> {
    let home =
        std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME not set — nothing was deleted"))?;
    let trash = Path::new(&home).join(".Trash");
    anyhow::ensure!(
        trash.is_dir(),
        "~/.Trash not found — nothing was deleted; trash the paths manually"
    );
    for p in paths {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("unnameable path {} — nothing moved", p.display()))?;
        let mut dest = trash.join(name);
        let mut n = 1u32;
        while dest.exists() {
            dest = trash.join(format!("{name} {n}"));
            n += 1;
        }
        std::fs::rename(p, &dest)
            .map_err(|e| anyhow::anyhow!("couldn't move {} to the Trash: {e}", p.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
//
// Verifican el comportamiento NO obvio documentado en CLAUDE.md/README contra
// transcripts-fixture escritos en un directorio temporal (no tocan ~/.claude
// real). Complementan a `/verify-parser` (que corre contra datos reales): estos
// blindan el parser ante regresiones cuando se extienda el formato.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Directorio temporal único por test (contador atómico evita colisiones
    // entre tests que corren en paralelo). Se limpia al entrar para idempotencia.
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    fn tmpdir(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut d = std::env::temp_dir();
        d.push(format!("arrow_test_{tag}_{n}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    // Escribe un transcript de PRIMER NIVEL: <projects>/<projsubdir>/<sid>.jsonl
    fn write_top_level(projects: &Path, projsubdir: &str, sid: &str, records: &[String]) {
        let dir = projects.join(projsubdir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{sid}.jsonl")), records.join("\n")).unwrap();
    }

    // Record de edición (toolUseResult con filePath + structuredPatch).
    fn edit_record(
        sid: &str,
        cwd: &str,
        file_path: &str,
        ts: &str,
        original: &str,
        patch_lines: &[&str],
    ) -> String {
        json!({
            "type": "user",
            "sessionId": sid,
            "cwd": cwd,
            "gitBranch": "main",
            "timestamp": ts,
            "toolUseResult": {
                "filePath": file_path,
                "type": "update",
                "userModified": false,
                "originalFile": original,
                "structuredPatch": [{
                    "oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": patch_lines.len(),
                    "lines": patch_lines,
                }],
            }
        })
        .to_string()
    }

    #[test]
    fn parsing_defensivo_ignora_lineas_invalidas() {
        let dir = tmpdir("defensivo");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("a.txt");
        fs::write(&file, "after\n").unwrap();

        let records = vec![
            "esto no es json".to_string(),
            "{ json roto".to_string(),
            String::new(), // línea vacía: se salta sin contar
            edit_record(
                "s1",
                repo.to_str().unwrap(),
                file.to_str().unwrap(),
                "2026-01-01T00:00:00Z",
                "before\n",
                &["-before", "+after"],
            ),
        ];
        write_top_level(&dir, "proj", "s1", &records);

        let c = collect(dir.to_str().unwrap(), None, None);
        // Las dos líneas inválidas se cuentan como skipped, NO rompen el parseo.
        assert_eq!(c.lines_skipped, 2, "deben ignorarse las 2 líneas inválidas");
        // El record válido sí se ingirió.
        assert_eq!(c.repos.len(), 1, "el record válido debe producir 1 repo");
    }

    #[test]
    fn solo_transcripts_de_primer_nivel() {
        let dir = tmpdir("toplevel");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("a.txt");
        fs::write(&file, "x\n").unwrap();
        let rec = edit_record(
            "s1",
            repo.to_str().unwrap(),
            file.to_str().unwrap(),
            "2026-01-01T00:00:00Z",
            "",
            &["+x"],
        );
        // Top-level: cuenta. `rec` se reutiliza abajo, así que pasamos un slice
        // prestado en vez de clonar (clippy: useless clone).
        write_top_level(&dir, "proj", "s1", std::slice::from_ref(&rec));
        // Anidado (subagente): <projects>/proj/nested/s2.jsonl → 3 componentes, se ignora.
        let nested = dir.join("proj").join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("s2.jsonl"), &rec).unwrap();

        let c = collect(dir.to_str().unwrap(), None, None);
        assert_eq!(
            c.jsonl_files, 1,
            "solo el transcript de primer nivel cuenta"
        );
    }

    #[test]
    fn agrupa_por_raiz_git() {
        let dir = tmpdir("gitroot");
        let repo = dir.join("myrepo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let sub = repo.join("web");
        fs::create_dir_all(&sub).unwrap();
        let file = sub.join("App.svelte");
        fs::write(&file, "x\n").unwrap();
        // cwd es el subdirectorio web/, pero el repo es la raíz git (myrepo).
        let rec = edit_record(
            "s1",
            sub.to_str().unwrap(),
            file.to_str().unwrap(),
            "2026-01-01T00:00:00Z",
            "",
            &["+x"],
        );
        write_top_level(&dir, "proj", "s1", &[rec]);

        let c = collect(dir.to_str().unwrap(), None, None);
        let keys: Vec<&String> = c.repos.keys().collect();
        assert_eq!(keys.len(), 1);
        assert_eq!(
            keys[0],
            &repo.to_string_lossy(),
            "el cwd en web/ debe agruparse bajo la raíz git myrepo"
        );
    }

    #[test]
    fn worktree_se_agrupa_bajo_el_repo_principal() {
        // A linked git worktree has a `.git` FILE (pointer), not a `.git/` dir. A
        // session whose cwd lives inside `<repo>/.claude/worktrees/<name>/` must fold
        // under the MAIN repo, not surface as a phantom top-level repo. (This is how
        // Claude Code's per-session worktrees look on disk.)
        let dir = tmpdir("worktree");
        let repo = dir.join("myrepo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let wt = repo.join(".claude").join("worktrees").join("feat");
        fs::create_dir_all(&wt).unwrap();
        // The worktree's `.git` is a FILE pointing at <repo>/.git/worktrees/feat.
        fs::write(
            wt.join(".git"),
            format!("gitdir: {}/.git/worktrees/feat\n", repo.to_string_lossy()),
        )
        .unwrap();
        let file = wt.join("src").join("App.tsx");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "x\n").unwrap();
        let rec = edit_record(
            "s1",
            wt.to_str().unwrap(),
            file.to_str().unwrap(),
            "2026-01-01T00:00:00Z",
            "",
            &["+x"],
        );
        write_top_level(&dir, "proj", "s1", &[rec]);

        let c = collect(dir.to_str().unwrap(), None, None);
        let keys: Vec<&String> = c.repos.keys().collect();
        assert_eq!(keys.len(), 1);
        assert_eq!(
            keys[0],
            &repo.to_string_lossy(),
            "un cwd dentro de un worktree enlazado debe agruparse bajo el repo principal"
        );
    }

    #[test]
    fn cuenta_added_y_removed() {
        let dir = tmpdir("addremoved");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("a.txt");
        fs::write(&file, "x\n").unwrap();
        // 2 añadidas, 1 borrada (el contexto " ctx" no cuenta).
        let rec = edit_record(
            "s1",
            repo.to_str().unwrap(),
            file.to_str().unwrap(),
            "2026-01-01T00:00:00Z",
            "old\n",
            &["-old", "+new1", "+new2", " ctx"],
        );
        write_top_level(&dir, "proj", "s1", &[rec]);

        let report = build_report(dir.to_str().unwrap());
        let f = &report.repos[0].sessions[0].files[0];
        assert_eq!(f.added, 2);
        assert_eq!(f.removed, 1);
    }

    #[test]
    fn filtra_solo_el_home_global_de_claude() {
        let dir = tmpdir("claudefilter");
        let home = std::env::var("HOME").unwrap();
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();

        // (a) filePath dentro del HOME global ~/.claude/: NO es código del usuario.
        let claude_file = format!("{home}/.claude/projects/internal.jsonl");
        // (b) filePath en un .claude/ DENTRO del repo: SÍ es código del usuario.
        let repo_claude = repo.join(".claude").join("settings.json");

        let recs = vec![
            edit_record(
                "s1",
                repo.to_str().unwrap(),
                &claude_file,
                "2026-01-01T00:00:00Z",
                "",
                &["+x"],
            ),
            edit_record(
                "s1",
                repo.to_str().unwrap(),
                repo_claude.to_str().unwrap(),
                "2026-01-01T00:00:01Z",
                "",
                &["+y"],
            ),
        ];
        write_top_level(&dir, "proj", "s1", &recs);

        let c = collect(dir.to_str().unwrap(), None, None);
        let files: Vec<&String> = c
            .repos
            .values()
            .flat_map(|r| r.sessions.values())
            .flat_map(|s| s.files.keys())
            .collect();
        assert!(
            !files
                .iter()
                .any(|p| p.starts_with(&format!("{home}/.claude/"))),
            "el HOME global ~/.claude/ debe filtrarse"
        );
        assert!(
            files.iter().any(|p| p.ends_with(".claude/settings.json")),
            "un .claude/ dentro del repo SÍ debe aparecer"
        );
    }

    #[test]
    fn repos_ordenados_por_recencia() {
        let dir = tmpdir("recencia");
        // Dos repos con git root distinto; el de timestamp mayor va primero.
        for (name, ts) in [
            ("viejo", "2026-01-01T00:00:00Z"),
            ("nuevo", "2026-06-01T00:00:00Z"),
        ] {
            let repo = dir.join(name);
            fs::create_dir_all(repo.join(".git")).unwrap();
            let file = repo.join("a.txt");
            fs::write(&file, "x\n").unwrap();
            let rec = edit_record(
                name,
                repo.to_str().unwrap(),
                file.to_str().unwrap(),
                ts,
                "",
                &["+x"],
            );
            write_top_level(&dir, name, name, &[rec]);
        }
        let report = build_report(dir.to_str().unwrap());
        assert_eq!(report.repo_count, 2);
        assert!(
            report.repos[0].cwd.ends_with("nuevo"),
            "el repo con actividad más reciente debe ir primero"
        );
    }

    #[test]
    fn archivos_ordenados_por_ultima_edicion() {
        // El archivo editado más recientemente va primero; re-tocar uno lo sube de
        // nuevo al tope (acumulado en una sola entrada, no duplicado).
        let dir = tmpdir("file_recency");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let a = repo.join("a.txt");
        let b = repo.join("b.txt");
        fs::write(&a, "x\n").unwrap();
        fs::write(&b, "y\n").unwrap();
        let cwd = repo.to_str().unwrap();
        // Toco a (T0), luego b (T1), luego vuelvo a tocar a (T2 = el más reciente).
        let recs = vec![
            edit_record(
                "s1",
                cwd,
                a.to_str().unwrap(),
                "2026-06-02T00:00:00Z",
                "",
                &["+x"],
            ),
            edit_record(
                "s1",
                cwd,
                b.to_str().unwrap(),
                "2026-06-02T00:01:00Z",
                "",
                &["+y"],
            ),
            edit_record(
                "s1",
                cwd,
                a.to_str().unwrap(),
                "2026-06-02T00:02:00Z",
                "",
                &["+x2"],
            ),
        ];
        write_top_level(&dir, "proj", "s1", &recs);

        let report = build_report(dir.to_str().unwrap());
        let files = &report.repos[0].sessions[0].files;
        assert_eq!(files.len(), 2, "a.txt deduplicado en una entrada + b.txt");
        assert!(
            files[0].path.ends_with("a.txt"),
            "el archivo re-tocado más tarde (a.txt) debe burbujear al tope"
        );
        assert_eq!(
            files[0].last_touched.as_deref(),
            Some("2026-06-02T00:02:00Z")
        );
        assert_eq!(
            files[0].ops, 2,
            "las dos ediciones de a.txt se acumulan en una entrada"
        );
        assert!(files[1].path.ends_with("b.txt"));
        assert_eq!(
            files[1].last_touched.as_deref(),
            Some("2026-06-02T00:01:00Z")
        );
    }

    #[test]
    fn metadata_titulo_se_asocia_a_la_sesion() {
        let dir = tmpdir("titulo");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("a.txt");
        fs::write(&file, "x\n").unwrap();
        let title_rec = json!({
            "type": "ai-title", "sessionId": "s1", "aiTitle": "Mi título legible",
            "timestamp": "2026-01-01T00:00:00Z"
        })
        .to_string();
        let edit = edit_record(
            "s1",
            repo.to_str().unwrap(),
            file.to_str().unwrap(),
            "2026-01-01T00:00:01Z",
            "",
            &["+x"],
        );
        write_top_level(&dir, "proj", "s1", &[title_rec, edit]);

        let report = build_report(dir.to_str().unwrap());
        assert_eq!(
            report.repos[0].sessions[0].title.as_deref(),
            Some("Mi título legible")
        );
    }

    #[test]
    fn file_content_before_after_y_atajo_por_sesion() {
        let dir = tmpdir("content");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("a.txt");
        fs::write(&file, "AFTER en disco\n").unwrap();
        let fp = file.to_str().unwrap();

        // Dos sesiones tocan el mismo archivo con originalFile distinto.
        write_top_level(
            &dir,
            "proj",
            "sessA",
            &[edit_record(
                "sessA",
                repo.to_str().unwrap(),
                fp,
                "2026-01-01T00:00:00Z",
                "BEFORE de A\n",
                &["+x"],
            )],
        );
        write_top_level(
            &dir,
            "proj",
            "sessB",
            &[edit_record(
                "sessB",
                repo.to_str().unwrap(),
                fp,
                "2026-02-01T00:00:00Z",
                "BEFORE de B\n",
                &["+y"],
            )],
        );

        // Con filtro de sesión: before es el de ESA sesión; after viene del disco.
        let out = file_content(dir.to_str().unwrap(), fp, Some("sessB"));
        assert_eq!(out.before, "BEFORE de B\n");
        assert!(out.before_available);
        assert_eq!(out.after, "AFTER en disco\n");
        assert!(out.after_available);
        assert_eq!(out.ops, 1, "solo la sesión filtrada aporta ops");
    }

    #[test]
    fn file_content_archivo_inexistente_degrada_sin_romper() {
        let dir = tmpdir("noafter");
        // No hay transcripts ni archivo en disco: debe degradar, no paniquear.
        let out = file_content(dir.to_str().unwrap(), "/ruta/que/no/existe.txt", None);
        assert!(!out.before_available);
        assert!(!out.after_available);
        assert_eq!(out.ops, 0);
        assert_eq!(out.after, "");
    }

    // Edit record whose `originalFile` is JSON `null` (Claude Code emits this on
    // some edits even though a `structuredPatch` is present).
    fn edit_record_null_original(
        sid: &str,
        cwd: &str,
        file_path: &str,
        ts: &str,
        patch_lines: &[&str],
    ) -> String {
        json!({
            "type": "user",
            "sessionId": sid,
            "cwd": cwd,
            "gitBranch": "main",
            "timestamp": ts,
            "toolUseResult": {
                "filePath": file_path,
                "userModified": false,
                "originalFile": null,
                "structuredPatch": [{
                    "oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": patch_lines.len(),
                    "lines": patch_lines,
                }],
            }
        })
        .to_string()
    }

    // Read result record: `{ file: { filePath, content, startLine, numLines,
    // totalLines }, type: "text" }`. `num`/`total` let a test simulate a full vs
    // truncated read.
    fn read_record(
        sid: &str,
        cwd: &str,
        file_path: &str,
        ts: &str,
        content: &str,
        num: i64,
        total: i64,
    ) -> String {
        json!({
            "type": "user",
            "sessionId": sid,
            "cwd": cwd,
            "timestamp": ts,
            "toolUseResult": {
                "type": "text",
                "file": {
                    "filePath": file_path,
                    "content": content,
                    "startLine": 1,
                    "numLines": num,
                    "totalLines": total,
                },
            }
        })
        .to_string()
    }

    #[test]
    fn file_content_usa_snapshot_del_read_si_originalfile_es_null() {
        let dir = tmpdir("readfallback");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("Comp.tsx");
        fs::write(&file, "line A\nEDITED\nline C\n").unwrap();
        let fp = file.to_str().unwrap();

        // Read (full, CONSISTENT with the edit) then Edit with originalFile:null.
        let read = read_record(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:00:00Z",
            "line A\nline B\nline C\n",
            3,
            3,
        );
        let edit = edit_hunk(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:01:00Z",
            None,
            1,
            1,
            &[" line A", "-line B", "+EDITED", " line C"],
        );
        write_top_level(&dir, "proj", "s1", &[read, edit]);

        let out = file_content(dir.to_str().unwrap(), fp, Some("s1"));
        assert!(
            out.before_available,
            "el snapshot del Read debe rellenar el before cuando originalFile es null"
        );
        assert_eq!(out.before, "line A\nline B\nline C\n");
        assert_eq!(out.after, "line A\nEDITED\nline C\n");
        assert_eq!(out.ops, 1, "solo el Edit cuenta como op (el Read no)");
    }

    // Realistic single-hunk edit. `original` -> originalFile (None = JSON null).
    // oldLines/newLines are derived from the prefixed `lines` so the patch is
    // internally consistent (context ' ' counts in both, '-' only old, '+' only new).
    #[allow(clippy::too_many_arguments)] // a focused test fixture builder
    fn edit_hunk(
        sid: &str,
        cwd: &str,
        fp: &str,
        ts: &str,
        original: Option<&str>,
        old_start: i64,
        new_start: i64,
        lines: &[&str],
    ) -> String {
        let old_lines = lines.iter().filter(|l| !l.starts_with('+')).count();
        let new_lines = lines.iter().filter(|l| !l.starts_with('-')).count();
        let of = match original {
            Some(s) => json!(s),
            None => Value::Null,
        };
        json!({
            "type": "user", "sessionId": sid, "cwd": cwd, "gitBranch": "main", "timestamp": ts,
            "toolUseResult": {
                "filePath": fp, "userModified": false, "originalFile": of,
                "structuredPatch": [{
                    "oldStart": old_start, "oldLines": old_lines,
                    "newStart": new_start, "newLines": new_lines, "lines": lines,
                }],
            }
        })
        .to_string()
    }

    #[test]
    fn file_content_reconstruye_before_revirtiendo_patch_desde_disco() {
        let dir = tmpdir("reverse_disk");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("Comp.tsx");
        // F0 (pre-edición) = "a\nB\nc\n"; en disco quedó "a\nX\nc\n".
        fs::write(&file, "a\nX\nc\n").unwrap();
        let fp = file.to_str().unwrap();
        // Edit con originalFile:null y SIN Read: el before se reconstruye revirtiendo
        // el structuredPatch sobre el contenido de disco.
        let edit = edit_hunk(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:00:00Z",
            None,
            1,
            1,
            &[" a", "-B", "+X", " c"],
        );
        write_top_level(&dir, "proj", "s1", &[edit]);

        let out = file_content(dir.to_str().unwrap(), fp, Some("s1"));
        assert!(
            out.before_available,
            "reverse-apply debe reconstruir el before desde disco"
        );
        assert_eq!(out.before, "a\nB\nc\n");
        assert_eq!(out.after, "a\nX\nc\n");
        assert_eq!(out.ops, 1);
    }

    #[test]
    fn file_content_reconstruye_before_anclando_en_un_original_posterior() {
        // Caso StatsUI: la 1ª edición trae originalFile:null, pero una posterior sí
        // trae un snapshot exacto; se ancla ahí y se revierten las ediciones previas.
        let dir = tmpdir("reverse_anchor");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("Comp.tsx");
        fs::write(&file, "L1\nL2x\nL3y\n").unwrap(); // F2 (disco)
        let fp = file.to_str().unwrap();
        // edit1: L2 -> L2x (originalFile null). edit2: L3 -> L3y (originalFile = F1).
        let e1 = edit_hunk(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:00:00Z",
            None,
            1,
            1,
            &[" L1", "-L2", "+L2x", " L3"],
        );
        let e2 = edit_hunk(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:01:00Z",
            Some("L1\nL2x\nL3\n"),
            1,
            1,
            &[" L1", " L2x", "-L3", "+L3y"],
        );
        write_top_level(&dir, "proj", "s1", &[e1, e2]);

        let out = file_content(dir.to_str().unwrap(), fp, Some("s1"));
        assert!(out.before_available);
        assert_eq!(
            out.before, "L1\nL2\nL3\n",
            "before = pre-sesión, revirtiendo edit1 desde el snapshot exacto de edit2"
        );
        assert_eq!(out.after, "L1\nL2x\nL3y\n");
        assert_eq!(out.ops, 2);
    }

    #[test]
    fn file_content_before_no_disponible_si_el_patch_no_cuadra() {
        let dir = tmpdir("drift");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("Comp.tsx");
        // El disco NO coincide con el "after" del patch (drift: edición externa/Bash
        // entre la última herramienta y ahora). Honestidad: NO inventar un before.
        fs::write(&file, "totally\ndifferent\ncontent\n").unwrap();
        let fp = file.to_str().unwrap();
        let edit = edit_hunk(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:00:00Z",
            None,
            1,
            1,
            &[" a", "-B", "+X", " c"],
        );
        write_top_level(&dir, "proj", "s1", &[edit]);

        let out = file_content(dir.to_str().unwrap(), fp, Some("s1"));
        assert!(
            !out.before_available,
            "ante drift, el before queda NO disponible (no se inventa)"
        );
    }

    #[test]
    fn file_content_ignora_read_truncado_como_before() {
        let dir = tmpdir("truncread");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("Big.tsx");
        fs::write(&file, "full file on disk\n").unwrap();
        let fp = file.to_str().unwrap();

        // Read truncado (numLines != totalLines): NO sirve de before. Como el Edit
        // trae originalFile:null y no hay otro Read completo, before queda no disponible.
        let read = read_record(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:00:00Z",
            "only first 100 lines...\n",
            100,
            500,
        );
        let edit = edit_record_null_original(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:01:00Z",
            &["+x"],
        );
        write_top_level(&dir, "proj", "s1", &[read, edit]);

        let out = file_content(dir.to_str().unwrap(), fp, Some("s1"));
        assert!(
            !out.before_available,
            "un Read truncado no debe usarse como before"
        );
    }

    #[test]
    fn file_content_archivo_creado_se_marca_como_nuevo() {
        let dir = tmpdir("create");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("New.tsx");
        fs::write(&file, "brand new content\n").unwrap();
        let fp = file.to_str().unwrap();
        // Write that CREATES the file: type:"create", originalFile null, no patch.
        let create = json!({
            "type": "user", "sessionId": "s1", "cwd": repo.to_str().unwrap(),
            "gitBranch": "main", "timestamp": "2026-06-02T00:00:00Z",
            "toolUseResult": {
                "filePath": fp, "type": "create", "userModified": false,
                "originalFile": null, "structuredPatch": [],
            }
        })
        .to_string();
        write_top_level(&dir, "proj", "s1", &[create]);

        let out = file_content(dir.to_str().unwrap(), fp, Some("s1"));
        // A created file has an empty "before" available (the frontend then shows
        // "+ new file"), NOT an "unavailable" / honest-banner state.
        assert!(
            out.before_available,
            "un archivo creado tiene before disponible"
        );
        assert_eq!(out.before, "", "el before de un create es vacío");
        assert_eq!(out.after, "brand new content\n");
        assert_eq!(out.ops, 1);
    }

    #[test]
    fn file_content_expone_primera_linea_cambiada() {
        // first_changed_line = primer cambio REAL (lado after) = newStart + contexto
        // líder del hunk, NO el newStart crudo (que apunta al inicio del contexto).
        let dir = tmpdir("firstline");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("Comp.tsx");
        fs::write(&file, "a\nb\nc\n").unwrap();
        let fp = file.to_str().unwrap();
        // Hunk A: newStart=5 con 3 líneas de contexto líder → primer cambio = 8.
        // Hunk B: newStart=10 sin contexto → primer cambio = 10.
        // El min se toma sobre el valor AJUSTADO (8), no sobre el newStart crudo (5).
        let edit = json!({
            "type": "user", "sessionId": "s1", "cwd": repo.to_str().unwrap(),
            "gitBranch": "main", "timestamp": "2026-06-02T00:00:00Z",
            "toolUseResult": {
                "filePath": fp, "userModified": false, "originalFile": "a\nb\nc\n",
                "structuredPatch": [
                    { "oldStart": 5, "oldLines": 4, "newStart": 5, "newLines": 4,
                      "lines": [" ctx1", " ctx2", " ctx3", "-old", "+new"] },
                    { "oldStart": 10, "oldLines": 1, "newStart": 10, "newLines": 1,
                      "lines": ["+added"] }
                ],
            }
        })
        .to_string();
        write_top_level(&dir, "proj", "s1", &[edit]);

        let out = file_content(dir.to_str().unwrap(), fp, Some("s1"));
        assert_eq!(
            out.first_changed_line,
            Some(8),
            "primera línea cambiada = newStart + contexto líder (5+3), no el crudo (5)"
        );
    }

    #[test]
    fn reverse_apply_aborta_con_hunks_solapados() {
        let dir = tmpdir("overlap");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("Comp.tsx");
        fs::write(&file, "a\nX\nY\nb\n").unwrap();
        let fp = file.to_str().unwrap();
        // Two hunks whose after-view ranges overlap (indices [1,3) and [2,4)).
        // Internally inconsistent: reverse_apply must bail, before unavailable.
        let edit = json!({
            "type": "user", "sessionId": "s1", "cwd": repo.to_str().unwrap(),
            "gitBranch": "main", "timestamp": "2026-06-02T00:00:00Z",
            "toolUseResult": {
                "filePath": fp, "userModified": false, "originalFile": null,
                "structuredPatch": [
                    { "oldStart": 2, "oldLines": 2, "newStart": 2, "newLines": 2,
                      "lines": [" X", "-P", "+Y"] },
                    { "oldStart": 3, "oldLines": 2, "newStart": 3, "newLines": 2,
                      "lines": [" Y", "-Q", "+b"] },
                ],
            }
        })
        .to_string();
        write_top_level(&dir, "proj", "s1", &[edit]);

        let out = file_content(dir.to_str().unwrap(), fp, Some("s1"));
        assert!(
            !out.before_available,
            "hunks solapados no deben producir un before inventado"
        );
    }

    #[test]
    fn file_content_descarta_read_que_no_cuadra_con_el_edit() {
        // Drift entre el Read y el Edit: un cambio NO rastreado (Bash/otra sesión)
        // alteró "line A" -> "line X" en disco. El Read viejo ("line A...") NO debe
        // usarse; se cae al reverse-apply desde disco, que no atribuye ese cambio.
        let dir = tmpdir("staleread");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("Comp.tsx");
        fs::write(&file, "line X\nEDITED\nline C\n").unwrap();
        let fp = file.to_str().unwrap();
        // Read capturó el estado viejo (line A); el Edit (originalFile null) vio el
        // disco real (line X) según su contexto.
        let read = read_record(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:00:00Z",
            "line A\nline B\nline C\n",
            3,
            3,
        );
        let edit = edit_hunk(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:01:00Z",
            None,
            1,
            1,
            &[" line X", "-line B", "+EDITED", " line C"],
        );
        write_top_level(&dir, "proj", "s1", &[read, edit]);

        let out = file_content(dir.to_str().unwrap(), fp, Some("s1"));
        assert!(out.before_available);
        assert_eq!(
            out.before, "line X\nline B\nline C\n",
            "el before honesto sale del reverse-apply, NO del Read drifteado"
        );
    }

    #[test]
    fn file_content_sin_sesion_elige_la_mas_reciente() {
        // El mismo archivo editado por dos sesiones. Sin filtro de sesión, NO se
        // mezclan: gana la sesión con la edición más reciente.
        let dir = tmpdir("nosession");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let file = repo.join("Shared.tsx");
        fs::write(&file, "DISK\n").unwrap();
        let fp = file.to_str().unwrap();
        let old = edit_hunk(
            "sessOld",
            repo.to_str().unwrap(),
            fp,
            "2026-01-01T00:00:00Z",
            Some("OLD_A\n"),
            1,
            1,
            &["-OLD_A", "+DISK"],
        );
        let new = edit_hunk(
            "sessNew",
            repo.to_str().unwrap(),
            fp,
            "2026-06-01T00:00:00Z",
            Some("OLD_B\n"),
            1,
            1,
            &["-OLD_B", "+DISK"],
        );
        write_top_level(&dir, "projOld", "sessOld", &[old]);
        write_top_level(&dir, "projNew", "sessNew", &[new]);

        let out = file_content(dir.to_str().unwrap(), fp, None);
        assert_eq!(
            out.before, "OLD_B\n",
            "sin sesión, gana la edición más reciente (sessNew), sin mezclar"
        );
        assert_eq!(out.ops, 1, "solo cuenta las ops de la sesión elegida");
    }

    // ------------------------------------------------------------------
    // Sessions tab: sesiones sin ediciones, metadatos nuevos, retención,
    // live y borrado a papelera (dry-run).
    // ------------------------------------------------------------------

    // Plain user record (a human prompt), with cwd + timestamp.
    fn user_record(sid: &str, cwd: &str, ts: &str, content: &str) -> String {
        json!({
            "type": "user", "sessionId": sid, "cwd": cwd, "timestamp": ts,
            "message": { "role": "user", "content": content }
        })
        .to_string()
    }

    fn last_prompt_record(sid: &str, prompt: &str) -> String {
        json!({ "type": "last-prompt", "sessionId": sid, "lastPrompt": prompt, "leafUuid": "x" })
            .to_string()
    }

    fn pr_link_record(sid: &str, number: u64, url: &str, ts: &str) -> String {
        json!({
            "type": "pr-link", "sessionId": sid, "prNumber": number, "prUrl": url,
            "prRepository": "owner/repo", "timestamp": ts
        })
        .to_string()
    }

    // Nested layout `<tmp>/claude/projects` so the claude root (parent of the
    // projects dir) is OURS — live/retention tests never touch the real one.
    fn claude_layout(tag: &str) -> (PathBuf, PathBuf) {
        let root = tmpdir(tag);
        let claude = root.join("claude");
        let projects = claude.join("projects");
        fs::create_dir_all(&projects).unwrap();
        (claude, projects)
    }

    #[test]
    fn sesion_sin_ediciones_se_lista_con_titulo_de_ai_title() {
        // Un transcript SIN Edit/Write/MultiEdit debe existir igual como sesión
        // (files vacío, fileCount 0), anclado a la raíz git de su cwd.
        let dir = tmpdir("no_edits");
        let repo = dir.join("myrepo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let recs = vec![
            json!({ "type": "ai-title", "sessionId": "s1", "aiTitle": "Investigar el bug" })
                .to_string(),
            user_record(
                "s1",
                repo.to_str().unwrap(),
                "2026-06-02T00:00:00Z",
                "hola, mira este error",
            ),
        ];
        write_top_level(&dir, "proj", "s1", &recs);

        let report = build_report(dir.to_str().unwrap());
        assert_eq!(report.repo_count, 1);
        assert_eq!(
            report.repos[0].cwd,
            repo.to_string_lossy(),
            "la sesión sin ediciones se agrupa por la raíz git de su cwd"
        );
        let s = &report.repos[0].sessions[0];
        assert_eq!(s.session_id, "s1");
        assert_eq!(s.title.as_deref(), Some("Investigar el bug"));
        assert_eq!(s.file_count, 0);
        assert!(s.files.is_empty());
        assert!(s.size_bytes > 0, "sizeBytes = tamaño del .jsonl");
        assert!(!s.live);
    }

    #[test]
    fn titulo_fallback_al_primer_prompt_humano() {
        // Sin ai-title: el título cae al PRIMER user con content string que no
        // sea isMeta/isCompactSummary ni empiece con "<" (XML de slash-commands).
        let dir = tmpdir("title_fallback");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let cwd = repo.to_str().unwrap();
        let recs = vec![
            // isMeta: se ignora como prompt humano.
            json!({
                "type": "user", "sessionId": "s1", "cwd": cwd, "isMeta": true,
                "timestamp": "2026-06-02T00:00:00Z",
                "message": { "role": "user", "content": "caveat interno" }
            })
            .to_string(),
            // Empieza con "<": salida de slash-command, no un prompt humano.
            user_record(
                "s1",
                cwd,
                "2026-06-02T00:00:01Z",
                "<command-name>/model</command-name>",
            ),
            // Contenido array (tool results): no es string, se ignora.
            json!({
                "type": "user", "sessionId": "s1", "cwd": cwd,
                "timestamp": "2026-06-02T00:00:02Z",
                "message": { "role": "user", "content": [{ "type": "tool_result" }] }
            })
            .to_string(),
            // El primer prompt humano real: ESTE es el título.
            user_record("s1", cwd, "2026-06-02T00:00:03Z", "Arregla el bug de login"),
            user_record("s1", cwd, "2026-06-02T00:00:04Z", "otro prompt posterior"),
        ];
        write_top_level(&dir, "proj", "s1", &recs);

        let report = build_report(dir.to_str().unwrap());
        let s = &report.repos[0].sessions[0];
        assert_eq!(s.title.as_deref(), Some("Arregla el bug de login"));
    }

    #[test]
    fn resume_cwd_es_el_ultimo_cwd_y_ancla_el_repo() {
        // Caso real 081c7c8f: transcript guardado bajo la carpeta A pero
        // reanudado desde B — el ÚLTIMO cwd gana y ancla la agrupación.
        let dir = tmpdir("resume_cwd");
        let repo_a = dir.join("repoA");
        let repo_b = dir.join("repoB");
        fs::create_dir_all(repo_a.join(".git")).unwrap();
        fs::create_dir_all(repo_b.join(".git")).unwrap();
        let recs = vec![
            user_record(
                "s1",
                repo_a.to_str().unwrap(),
                "2026-06-02T00:00:00Z",
                "empezamos aquí",
            ),
            user_record(
                "s1",
                repo_b.to_str().unwrap(),
                "2026-06-02T00:01:00Z",
                "seguimos en otra carpeta",
            ),
        ];
        write_top_level(&dir, "proj", "s1", &recs);

        let report = build_report(dir.to_str().unwrap());
        assert_eq!(report.repo_count, 1, "una sesión vive en UN repo");
        assert_eq!(
            report.repos[0].cwd,
            repo_b.to_string_lossy(),
            "el anclaje usa el ÚLTIMO cwd (sesión reanudada)"
        );
        let s = &report.repos[0].sessions[0];
        assert_eq!(s.resume_cwd.as_deref(), repo_b.to_str());
    }

    #[test]
    fn last_prompts_ultimos_dos_distintos_truncados() {
        let dir = tmpdir("last_prompts");
        // 300 chars multibyte (á = 2 bytes): el truncado es por CHAR, no por byte.
        let long = "á".repeat(300);
        let recs = vec![
            last_prompt_record("s1", "primer prompt"),
            last_prompt_record("s1", "segundo prompt"),
            last_prompt_record("s1", "segundo prompt"), // repetido: no duplica
            last_prompt_record("s1", &long),
        ];
        write_top_level(&dir, "proj", "s1", &recs);

        let report = build_report(dir.to_str().unwrap());
        let s = &report.repos[0].sessions[0];
        assert_eq!(s.last_prompts.len(), 2, "solo los últimos 2 DISTINTOS");
        assert_eq!(
            s.last_prompts[0].chars().count(),
            200,
            "truncado a 200 chars (límite de char, no de byte)"
        );
        assert!(s.last_prompts[0].chars().all(|c| c == 'á'));
        assert_eq!(s.last_prompts[1], "segundo prompt");
    }

    #[test]
    fn pr_links_se_deduplican_por_numero() {
        let dir = tmpdir("pr_links");
        let recs = vec![
            pr_link_record(
                "s1",
                360,
                "https://github.com/o/r/pull/360",
                "2026-07-02T00:00:00Z",
            ),
            // Claude Code re-emite el mismo PR en turnos posteriores.
            pr_link_record(
                "s1",
                360,
                "https://github.com/o/r/pull/360",
                "2026-07-02T00:01:00Z",
            ),
            pr_link_record(
                "s1",
                361,
                "https://github.com/o/r/pull/361",
                "2026-07-02T00:02:00Z",
            ),
        ];
        write_top_level(&dir, "proj", "s1", &recs);

        let report = build_report(dir.to_str().unwrap());
        let s = &report.repos[0].sessions[0];
        assert_eq!(s.pr_links.len(), 2, "deduplicado por número");
        assert_eq!(s.pr_links[0].number, 360, "orden cronológico");
        assert_eq!(s.pr_links[1].number, 361);
        assert_eq!(s.pr_links[1].url, "https://github.com/o/r/pull/361");
    }

    #[test]
    fn retention_days_default_y_desde_settings() {
        let (claude, projects) = claude_layout("retention");
        let pd = projects.to_str().unwrap();

        // Sin settings.json → default 30.
        assert_eq!(build_report(pd).retention_days, 30);
        // cleanupPeriodDays null (así viene en instalaciones reales) → 30.
        fs::write(
            claude.join("settings.json"),
            r#"{"cleanupPeriodDays":null}"#,
        )
        .unwrap();
        assert_eq!(build_report(pd).retention_days, 30);
        // JSON roto → 30 (parsing defensivo).
        fs::write(claude.join("settings.json"), "{ rota").unwrap();
        assert_eq!(build_report(pd).retention_days, 30);
        // Valor real → se respeta.
        fs::write(claude.join("settings.json"), r#"{"cleanupPeriodDays":7}"#).unwrap();
        assert_eq!(build_report(pd).retention_days, 7);
    }

    #[test]
    fn trash_session_dry_run_localiza_artefactos_sin_borrar() {
        let (claude, projects) = claude_layout("trash_dry");
        let proj = projects.join("proj");
        fs::create_dir_all(&proj).unwrap();
        // Los 4 artefactos de la sesión "abc123".
        let jsonl = proj.join("abc123.jsonl");
        fs::write(&jsonl, "{}\n").unwrap();
        let sibling = proj.join("abc123");
        fs::create_dir_all(sibling.join("subagents")).unwrap();
        let fh = claude.join("file-history").join("abc123");
        fs::create_dir_all(&fh).unwrap();
        let se = claude.join("session-env").join("abc123");
        fs::create_dir_all(&se).unwrap();
        let pd = projects.to_str().unwrap();

        let out = trash_session(pd, "abc123", false).unwrap();
        assert!(out.dry_run);
        assert_eq!(out.session_id, "abc123");
        assert_eq!(out.trashed.len(), 4, "los 4 artefactos localizados");
        for p in [&jsonl, &sibling, &fh, &se] {
            let ps = p.to_string_lossy();
            assert!(
                out.trashed.iter().any(|t| t == ps.as_ref()),
                "falta {ps} en {:?}",
                out.trashed
            );
            assert!(p.exists(), "dry-run NO borra nada: {ps}");
        }

        // Un prefijo único también resuelve.
        let out2 = trash_session(pd, "abc", false).unwrap();
        assert_eq!(out2.session_id, "abc123");

        // Sesión inexistente → error claro, nada tocado.
        assert!(trash_session(pd, "zzz", false).is_err());

        // Prefijo ambiguo → error (borrar exige resolución única).
        fs::write(proj.join("abd999.jsonl"), "{}\n").unwrap();
        assert!(trash_session(pd, "ab", false).is_err());
    }

    #[test]
    fn trash_session_rejects_ids_that_could_escape() {
        let (_claude, projects) = claude_layout("trash_escape");
        let proj = projects.join("proj");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("abc123.jsonl"), "{}\n").unwrap();
        // A planted `...jsonl` (stem `..`) tries to turn the artifact paths
        // into `projects/..` and friends; the charset rule kills it up front.
        fs::write(proj.join("...jsonl"), "{}\n").unwrap();
        let pd = projects.to_str().unwrap();
        for bad in ["..", ".", "a/b", "abc123/", "a.b", "*"] {
            let err = trash_session(pd, bad, false).expect_err(bad);
            assert!(
                err.to_string().contains("invalid session id"),
                "{bad}: {err}"
            );
        }
        // The planted file is also invisible to prefix resolution: `abc123`
        // still resolves uniquely, and only to its own artifacts.
        let out = trash_session(pd, "abc123", false).unwrap();
        assert!(out.trashed.iter().all(|t| !t.ends_with("..")));
    }

    #[test]
    fn trash_session_ignores_a_directory_named_like_a_transcript() {
        let (_claude, projects) = claude_layout("trash_dirjsonl");
        let proj = projects.join("proj");
        fs::create_dir_all(proj.join("fakedir1.jsonl")).unwrap();
        let err = trash_session(projects.to_str().unwrap(), "fakedir1", false)
            .expect_err("a directory is not a transcript");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn trash_session_rehusa_sesion_live() {
        let (claude, projects) = claude_layout("trash_live");
        let proj = projects.join("proj");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("lively99.jsonl"), "{}\n").unwrap();
        // Registro de proceso vivo: NUESTRO propio pid (existe en /proc) y un
        // updatedAt fresco (fallback en plataformas sin /proc).
        let sessions = claude.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        fs::write(
            sessions.join("1234.json"),
            json!({ "pid": std::process::id(), "sessionId": "lively99", "updatedAt": now_ms })
                .to_string(),
        )
        .unwrap();

        let err = trash_session(projects.to_str().unwrap(), "lively99", false)
            .expect_err("una sesión live debe rehusarse");
        assert!(
            err.to_string().contains("live"),
            "el error explica que la sesión está live: {err}"
        );
        assert!(proj.join("lively99.jsonl").exists());
    }

    #[test]
    fn sesion_live_se_marca_en_el_report() {
        let (claude, projects) = claude_layout("live_flag");
        let proj = projects.join("proj");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("lively99.jsonl"), "").unwrap();
        fs::write(proj.join("deadses99.jsonl"), "").unwrap();
        let sessions = claude.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        // Vivo: nuestro pid + updatedAt fresco.
        fs::write(
            sessions.join("1.json"),
            json!({ "pid": std::process::id(), "sessionId": "lively99", "updatedAt": now_ms })
                .to_string(),
        )
        .unwrap();
        // Muerto: pid imposible + updatedAt viejo.
        fs::write(
            sessions.join("2.json"),
            json!({ "pid": 999_999_999u64, "sessionId": "deadses99", "updatedAt": 0 }).to_string(),
        )
        .unwrap();

        let report = build_report(projects.to_str().unwrap());
        let all: Vec<&SessionOut> = report
            .repos
            .iter()
            .flat_map(|r| r.sessions.iter())
            .collect();
        let lively = all.iter().find(|s| s.session_id == "lively99").unwrap();
        let dead = all.iter().find(|s| s.session_id == "deadses99").unwrap();
        assert!(lively.live, "proceso registrado y vivo ⇒ live");
        assert!(
            !dead.live,
            "registro con pid muerto/updatedAt viejo ⇒ no live"
        );
    }

    #[test]
    fn sesion_sin_cwd_se_ancla_al_nombre_decodificado() {
        // Transcript sin ningún record con cwd: el anclaje cae al nombre
        // codificado del directorio ("-home-user-proj" → "/home/user/proj").
        let dir = tmpdir("decode_anchor");
        let recs = vec![last_prompt_record("s1", "algo")];
        write_top_level(&dir, "-home-user-proj", "s1", &recs);

        let report = build_report(dir.to_str().unwrap());
        assert_eq!(report.repo_count, 1);
        assert_eq!(report.repos[0].cwd, "/home/user/proj");
        assert_eq!(report.repos[0].sessions[0].session_id, "s1");
    }
}
