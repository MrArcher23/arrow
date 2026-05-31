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

/// Salida del modo `--content`: el "antes" (primer originalFile de la sesión)
/// y el "después" (archivo actual en disco), para alimentar @codemirror/merge.
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

/// Reúne el "antes" (primer `originalFile` de la sesión para ese archivo) y el
/// "después" (archivo actual en disco) de UN archivo, para la vista de diff.
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

    let mut before: Option<String> = None;
    let mut ops = 0usize;
    let mut user_modified = false;

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
        scan_transcript(
            path,
            &target,
            session,
            &mut before,
            &mut ops,
            &mut user_modified,
        );
    }

    let (after, after_available) = match std::fs::read_to_string(&target) {
        Ok(s) => (s, true),
        Err(_) => (String::new(), false),
    };

    ContentOut {
        file: target,
        session: session.map(str::to_string),
        before_available: before.is_some(),
        before: before.unwrap_or_default(),
        after,
        after_available,
        user_modified,
        ops,
    }
}

/// Escanea un transcript acumulando el "antes" (primer `originalFile`), el nº de
/// `ops` y el flag `userModified` para `target`. Filtra por `session` (prefijo) si
/// se da. La lógica de extracción es idéntica a la del barrido original.
fn scan_transcript(
    path: &Path,
    target: &str,
    session: Option<&str>,
    before: &mut Option<String>,
    ops: &mut usize,
    user_modified: &mut bool,
) {
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
        if tur.get("filePath").and_then(Value::as_str) != Some(target) {
            continue;
        }
        if let Some(sf) = session {
            let sid = v.get("sessionId").and_then(Value::as_str).unwrap_or("");
            if !sid.starts_with(sf) {
                continue;
            }
        }
        *ops += 1;
        if before.is_none() {
            *before = Some(
                tur.get("originalFile")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            );
        }
        if tur
            .get("userModified")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            *user_modified = true;
        }
    }
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

    if let Some(arr) = tur.get("structuredPatch").and_then(Value::as_array) {
        for h in arr {
            let lines: Vec<String> = h
                .get("lines")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|l| l.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            for l in &lines {
                match l.as_bytes().first() {
                    Some(b'+') => fc.added += 1,
                    Some(b'-') => fc.removed += 1,
                    _ => {}
                }
            }
            fc.hunks.push(Hunk {
                old_start: h.get("oldStart").and_then(Value::as_i64).unwrap_or(0),
                old_lines: h.get("oldLines").and_then(Value::as_i64).unwrap_or(0),
                new_start: h.get("newStart").and_then(Value::as_i64).unwrap_or(0),
                new_lines: h.get("newLines").and_then(Value::as_i64).unwrap_or(0),
                lines,
            });
        }
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
