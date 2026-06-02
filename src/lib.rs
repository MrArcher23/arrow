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

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use walkdir::WalkDir;

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
    pub title: Option<String>,
    pub last_prompt: Option<String>,
    pub first_activity: Option<String>,
    pub last_activity: Option<String>,
    pub file_count: usize,
    pub files: Vec<FileOut>,
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
/// snapshot (if Claude Code emitted it) and the patch hunks. Together these let
/// us reconstruct the "before" even when the first edit lacks an inline original.
struct EditRec {
    original: Option<String>,
    hunks: Vec<Hunk>,
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
        scan_transcript(path, &target, session, &mut scan);
    }

    let (after, after_available) = match std::fs::read_to_string(&target) {
        Ok(s) => (s, true),
        Err(_) => (String::new(), false),
    };

    let before = resolve_before(&scan.edits, scan.read_before.as_deref(), &after);
    let before_available = before.is_some();
    ContentOut {
        file: target,
        session: session.map(str::to_string),
        before_available,
        before: before.unwrap_or_default(),
        after,
        after_available,
        user_modified: scan.user_modified,
        ops: scan.ops,
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
    // Reads only back an edit within the SAME transcript (= same session), so a
    // pending snapshot must not leak across files when `session` is None.
    scan.pending_read = None;
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
/// `after`. Most authoritative source first; every reconstruction is
/// context-verified by [`reverse_apply`] and yields `None` on mismatch, so we
/// never show an invented "before". `None` overall means honestly "unavailable".
fn resolve_before(edits: &[EditRec], read_before: Option<&str>, after: &str) -> Option<String> {
    // 1. The first edit already carries the exact pre-edit content.
    if let Some(first) = edits.first() {
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
    // 3. A full Read snapshot captured before the first edit (≈ pre-edit state).
    if let Some(r) = read_before {
        return Some(r.to_string());
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

/// Reverse-applies `edits` (chronological — each transforms F_i → F_{i+1}) onto
/// `anchor` (the content AFTER the last edit in the slice, i.e. F_n), returning
/// the content BEFORE the first edit (F_0).
///
/// Each hunk is verified against the current reconstruction; any mismatch (drift,
/// stale disk, a non-tool edit in between) returns `None` so the caller can fall
/// back rather than show a wrong "before". Patch line prefixes: ' ' context, '+'
/// added (after only), '-' removed (before only); a leading '\' is the
/// "No newline at end of file" marker and is ignored.
fn reverse_apply(anchor: &str, edits: &[EditRec]) -> Option<String> {
    // `split('\n')` keeps a trailing "" when the content ends in a newline, and
    // `join("\n")` restores it exactly.
    let mut lines: Vec<String> = anchor.split('\n').map(str::to_string).collect();
    for edit in edits.iter().rev() {
        // Highest newStart first, so earlier splices don't shift later indices
        // within the same edit.
        let mut hunks: Vec<&Hunk> = edit.hunks.iter().collect();
        hunks.sort_by_key(|h| std::cmp::Reverse(h.new_start));
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
            if end > lines.len() || lines[start..end] != after_view[..] {
                return None; // the hunk doesn't line up: bail honestly
            }
            lines.splice(start..end, before_view);
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
                    if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
                        metas.entry(sid.to_string()).or_default().title = Some(t.to_string());
                    }
                }
                Some("last-prompt") => {
                    if let Some(p) = v.get("lastPrompt").and_then(Value::as_str) {
                        metas.entry(sid.to_string()).or_default().last_prompt = Some(p.to_string());
                    }
                }
                _ => {}
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
    let mut repos_out: Vec<RepoOut> = repos
        .iter()
        .map(|(cwd, repo)| {
            let mut sessions: Vec<SessionOut> = repo
                .sessions
                .iter()
                .map(|(sid, sess)| {
                    let m = metas.get(sid);
                    SessionOut {
                        session_id: sid.clone(),
                        title: m.and_then(|m| m.title.clone()),
                        last_prompt: m.and_then(|m| m.last_prompt.clone()),
                        first_activity: m.and_then(|m| m.first_ts.clone()),
                        last_activity: m.and_then(|m| m.last_ts.clone()),
                        file_count: sess.files.len(),
                        files: sess
                            .files
                            .iter()
                            .map(|(path, fc)| FileOut {
                                path: path.clone(),
                                write_type: fc.write_type.clone(),
                                user_modified: fc.user_modified,
                                ops: fc.ops,
                                added: fc.added,
                                removed: fc.removed,
                            })
                            .collect(),
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
        if dir.join(".git").exists() {
            root = dir.to_string_lossy().into_owned();
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    cache.insert(cwd.to_string(), root.clone());
    root
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

        // Read (full) then Edit with originalFile:null, in chronological order.
        let read = read_record(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:00:00Z",
            "line A\nline B\nline C\n",
            3,
            3,
        );
        let edit = edit_record_null_original(
            "s1",
            repo.to_str().unwrap(),
            fp,
            "2026-06-02T00:01:00Z",
            &["-line B", "+EDITED"],
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
}
