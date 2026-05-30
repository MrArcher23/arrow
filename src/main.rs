//! arrow — parser de auditoría de Claude Code
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

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;
use serde::Serialize;
use serde_json::Value;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(
    name = "arrow",
    version,
    about = "Audita qué archivos tocó Claude Code y con qué diff (datos nativos: ~/.claude/projects, sin hooks ni git)"
)]
struct Cli {
    /// Directorio de proyectos de Claude Code (por defecto ~/.claude/projects)
    #[arg(long)]
    projects_dir: Option<String>,

    /// Filtra repos cuyo cwd contenga este texto
    #[arg(long)]
    repo: Option<String>,

    /// Filtra por sessionId (acepta prefijo)
    #[arg(long)]
    session: Option<String>,

    /// Solo resumen (repos -> sesiones -> archivos), sin cuerpos de diff
    #[arg(long)]
    list: bool,

    /// Emite el modelo normalizado como JSON (el contrato para la UI)
    #[arg(long)]
    json: bool,

    /// Modo contenido: emite JSON {before, after} de UN archivo (requiere --file)
    #[arg(long)]
    content: bool,

    /// Ruta exacta del archivo (para --content)
    #[arg(long)]
    file: Option<String>,
}

// ---------------------------------------------------------------------------
// Modelo interno (acumulación)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FileChange {
    write_type: Option<String>,
    user_modified: bool,
    ops: usize,
    added: usize,
    removed: usize,
    hunks: Vec<Hunk>,
}

struct Hunk {
    old_start: i64,
    old_lines: i64,
    new_start: i64,
    new_lines: i64,
    lines: Vec<String>,
}

#[derive(Default)]
struct Session {
    files: BTreeMap<String, FileChange>,
}

#[derive(Default)]
struct Repo {
    git_branch: Option<String>,
    sessions: BTreeMap<String, Session>,
}

/// Metadatos por sesión, recogidos de cualquier record con `sessionId`.
#[derive(Default)]
struct SessionMeta {
    title: Option<String>,
    last_prompt: Option<String>,
    first_ts: Option<String>,
    last_ts: Option<String>,
}

// ---------------------------------------------------------------------------
// Salida JSON (contrato hacia la UI)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportOut {
    projects_dir: String,
    repo_count: usize,
    repos: Vec<RepoOut>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoOut {
    cwd: String,
    git_branch: Option<String>,
    sessions: Vec<SessionOut>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionOut {
    session_id: String,
    title: Option<String>,
    last_prompt: Option<String>,
    first_activity: Option<String>,
    last_activity: Option<String>,
    file_count: usize,
    files: Vec<FileOut>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileOut {
    path: String,
    write_type: Option<String>,
    user_modified: bool,
    ops: usize,
    added: usize,
    removed: usize,
}

/// Salida del modo `--content`: el "antes" (primer originalFile de la sesión)
/// y el "después" (archivo actual en disco), para alimentar @codemirror/merge.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ContentOut {
    file: String,
    session: Option<String>,
    before: String,
    after: String,
    before_available: bool,
    after_available: bool,
    user_modified: bool,
    ops: usize,
}

// ---------------------------------------------------------------------------

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";

fn main() -> Result<()> {
    let cli = Cli::parse();
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let projects_dir = cli
        .projects_dir
        .clone()
        .unwrap_or_else(|| format!("{home}/.claude/projects"));

    if cli.content {
        return run_content(&cli, &projects_dir);
    }

    let projects_path = Path::new(&projects_dir);
    let mut repos: BTreeMap<String, Repo> = BTreeMap::new();
    let mut metas: BTreeMap<String, SessionMeta> = BTreeMap::new();
    let mut roots: HashMap<String, String> = HashMap::new(); // cache cwd -> raíz git
    let mut jsonl_files = 0usize;
    let mut lines_total = 0usize;
    let mut lines_skipped = 0usize;

    for entry in WalkDir::new(&projects_dir).into_iter().filter_map(|e| e.ok()) {
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

        jsonl_files += 1;
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            lines_total += 1;
            match serde_json::from_str::<Value>(&line) {
                Ok(v) => ingest(&v, &cli, &mut repos, &mut metas, &mut roots),
                Err(_) => lines_skipped += 1, // parsing defensivo: formato no documentado
            }
        }
    }

    if cli.json {
        let report = build_report(&projects_dir, &repos, &metas);
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    render_terminal(
        &projects_dir,
        &repos,
        &metas,
        jsonl_files,
        lines_total,
        lines_skipped,
        cli.list,
    );
    Ok(())
}

/// Procesa un record JSONL: actualiza metadatos de sesión y, si representa una
/// edición de archivo, la acumula.
fn ingest(
    v: &Value,
    cli: &Cli,
    repos: &mut BTreeMap<String, Repo>,
    metas: &mut BTreeMap<String, SessionMeta>,
    roots: &mut HashMap<String, String>,
) {
    // --- metadatos: cualquier record con sessionId ---
    if let Some(sid) = v.get("sessionId").and_then(Value::as_str) {
        let pass = cli
            .session
            .as_ref()
            .map(|sf| sid.starts_with(sf))
            .unwrap_or(true);
        if pass {
            match v.get("type").and_then(Value::as_str) {
                Some("ai-title") => {
                    if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
                        metas.entry(sid.to_string()).or_default().title = Some(t.to_string());
                    }
                }
                Some("last-prompt") => {
                    if let Some(p) = v.get("lastPrompt").and_then(Value::as_str) {
                        metas.entry(sid.to_string()).or_default().last_prompt =
                            Some(p.to_string());
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
    // Archivos internos de Claude (memoria, historial…) NO son cambios de tu código.
    if file_path.contains("/.claude/") {
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

    if let Some(rf) = &cli.repo {
        if !repo_key.contains(rf) {
            return;
        }
    }
    if let Some(sf) = &cli.session {
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
    if tur.get("userModified").and_then(Value::as_bool).unwrap_or(false) {
        fc.user_modified = true;
    }

    if let Some(arr) = tur.get("structuredPatch").and_then(Value::as_array) {
        for h in arr {
            let lines: Vec<String> = h
                .get("lines")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect())
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
fn build_report(
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

/// Reúne el "antes" (primer `originalFile` de la sesión para ese archivo) y el
/// "después" (archivo actual en disco) de UN archivo, para la vista de diff.
fn run_content(cli: &Cli, projects_dir: &str) -> Result<()> {
    let target = cli
        .file
        .clone()
        .ok_or_else(|| anyhow::anyhow!("--content requiere --file <ruta>"))?;

    let mut before: Option<String> = None;
    let mut ops = 0usize;
    let mut user_modified = false;

    for entry in WalkDir::new(projects_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.extension().map(|e| e == "jsonl").unwrap_or(false) {
            continue;
        }
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
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
            if tur.get("filePath").and_then(Value::as_str) != Some(target.as_str()) {
                continue;
            }
            if let Some(sf) = &cli.session {
                let sid = v.get("sessionId").and_then(Value::as_str).unwrap_or("");
                if !sid.starts_with(sf) {
                    continue;
                }
            }
            ops += 1;
            if before.is_none() {
                before = Some(
                    tur.get("originalFile")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                );
            }
            if tur.get("userModified").and_then(Value::as_bool).unwrap_or(false) {
                user_modified = true;
            }
        }
    }

    let (after, after_available) = match std::fs::read_to_string(&target) {
        Ok(s) => (s, true),
        Err(_) => (String::new(), false),
    };

    let out = ContentOut {
        file: target,
        session: cli.session.clone(),
        before_available: before.is_some(),
        before: before.unwrap_or_default(),
        after,
        after_available,
        user_modified,
        ops,
    };
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

fn render_terminal(
    projects_dir: &str,
    repos: &BTreeMap<String, Repo>,
    metas: &BTreeMap<String, SessionMeta>,
    jsonl_files: usize,
    lines_total: usize,
    lines_skipped: usize,
    list_only: bool,
) {
    println!("{BOLD}arrow{RESET} {DIM}· fuente: {projects_dir}{RESET}");
    println!(
        "{DIM}{jsonl_files} sesiones · {lines_total} records · {lines_skipped} líneas ignoradas (parsing defensivo){RESET}",
    );

    if repos.is_empty() {
        println!("\n{YELLOW}Sin ediciones atribuibles a Claude (Edit/Write/MultiEdit) con los filtros dados.{RESET}");
        return;
    }

    for (cwd, repo) in repos {
        let branch = repo.git_branch.as_deref().unwrap_or("—");
        println!("\n{CYAN}{BOLD}repo {cwd}{RESET}  {DIM}[{branch}]{RESET}");

        for (sid, sess) in &repo.sessions {
            let short: String = sid.chars().take(8).collect();
            let title = metas
                .get(sid)
                .and_then(|m| m.title.as_deref())
                .unwrap_or("(sin título)");
            let (mut add, mut rem) = (0usize, 0usize);
            for fc in sess.files.values() {
                add += fc.added;
                rem += fc.removed;
            }
            println!(
                "  {BOLD}{title}{RESET} {DIM}{short} · {} archivo(s) · {GREEN}+{add}{RESET}{DIM} {RED}-{rem}{RESET}",
                sess.files.len()
            );

            for (path, fc) in &sess.files {
                let tag = match fc.write_type.as_deref() {
                    Some("create") => " (nuevo)",
                    Some("update") => " (write)",
                    _ => "",
                };
                let warn = if fc.user_modified {
                    format!("  {YELLOW}⚠ modificado también fuera de Claude{RESET}")
                } else {
                    String::new()
                };
                println!(
                    "    {path}{DIM}{tag}{RESET}  {GREEN}+{}{RESET} {RED}-{}{RESET}{warn}",
                    fc.added, fc.removed
                );

                if list_only {
                    continue;
                }
                for h in &fc.hunks {
                    println!(
                        "      {CYAN}@@ -{},{} +{},{} @@{RESET}",
                        h.old_start, h.old_lines, h.new_start, h.new_lines
                    );
                    for line in &h.lines {
                        match line.as_bytes().first() {
                            Some(b'+') => println!("      {GREEN}{line}{RESET}"),
                            Some(b'-') => println!("      {RED}{line}{RESET}"),
                            _ => println!("      {DIM}{line}{RESET}"),
                        }
                    }
                }
            }
        }
    }
}
