//! arrow — Fase 0
//!
//! Reconstruye "repo -> sesión -> archivo -> diff" leyendo los transcripts
//! NATIVOS de Claude Code en `~/.claude/projects/**/*.jsonl`, sin hooks y sin git.
//!
//! Fuente de verdad (verificada en disco, Claude Code v2.1.x):
//!   - Cada `Edit`/`Write`/`MultiEdit` emite, en un record `type:"user"`, un campo
//!     top-level `toolUseResult` con: filePath, structuredPatch (hunks exactos),
//!     userModified y, para Write, `type` = "create"/"update".
//!   - El `cwd` de cada record es la ruta real del repo (no decodificamos el nombre
//!     del directorio, que es ambiguo cuando la ruta ya contiene guiones).
//!
//! Límite honesto: solo captura lo que pasa por Edit/Write/MultiEdit. Cambios vía
//! comandos Bash de la sesión (sed, prettier, build, mv, rm) NO aparecen aquí.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

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

    /// Emite el modelo normalizado como JSON (el contrato para la futura UI)
    #[arg(long)]
    json: bool,
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

// ---------------------------------------------------------------------------
// Salida JSON (contrato estable hacia la UI de la Fase 1)
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
    hunks: Vec<HunkOut>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HunkOut {
    old_start: i64,
    old_lines: i64,
    new_start: i64,
    new_lines: i64,
    lines: Vec<String>,
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

    let mut repos: BTreeMap<String, Repo> = BTreeMap::new();
    let mut jsonl_files = 0usize;
    let mut lines_total = 0usize;
    let mut lines_skipped = 0usize;

    for entry in WalkDir::new(&projects_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
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
                    Ok(v) => ingest(&v, &cli, &mut repos),
                    Err(_) => lines_skipped += 1, // parsing defensivo: formato no documentado
                }
            }
        }
    }

    if cli.json {
        let report = build_report(&projects_dir, &repos);
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    render_terminal(&projects_dir, &repos, jsonl_files, lines_total, lines_skipped, cli.list);
    Ok(())
}

/// Procesa un record JSONL y, si representa una edición de archivo, la acumula.
fn ingest(v: &Value, cli: &Cli, repos: &mut BTreeMap<String, Repo>) {
    let tur = match v.get("toolUseResult") {
        Some(t) if t.is_object() => t,
        _ => return,
    };
    // Solo edits/writes traen filePath; Read/Bash/Glob/Grep no.
    let file_path = match tur.get("filePath").and_then(Value::as_str) {
        Some(s) => s.to_string(),
        None => return,
    };
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

    if let Some(rf) = &cli.repo {
        if !cwd.contains(rf) {
            return;
        }
    }
    if let Some(sf) = &cli.session {
        if !session.starts_with(sf) {
            return;
        }
    }

    let repo = repos.entry(cwd).or_default();
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

fn build_report(projects_dir: &str, repos: &BTreeMap<String, Repo>) -> ReportOut {
    let repos_out = repos
        .iter()
        .map(|(cwd, repo)| RepoOut {
            cwd: cwd.clone(),
            git_branch: repo.git_branch.clone(),
            sessions: repo
                .sessions
                .iter()
                .map(|(sid, sess)| SessionOut {
                    session_id: sid.clone(),
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
                            hunks: fc
                                .hunks
                                .iter()
                                .map(|h| HunkOut {
                                    old_start: h.old_start,
                                    old_lines: h.old_lines,
                                    new_start: h.new_start,
                                    new_lines: h.new_lines,
                                    lines: h.lines.clone(),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();

    ReportOut {
        projects_dir: projects_dir.to_string(),
        repo_count: repos.len(),
        repos: repos_out,
    }
}

fn render_terminal(
    projects_dir: &str,
    repos: &BTreeMap<String, Repo>,
    jsonl_files: usize,
    lines_total: usize,
    lines_skipped: usize,
    list_only: bool,
) {
    println!(
        "{BOLD}arrow{RESET} {DIM}· fuente: {projects_dir}{RESET}",
    );
    println!(
        "{DIM}{jsonl_files} transcripts · {lines_total} records · {lines_skipped} líneas ignoradas (parsing defensivo){RESET}",
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
            let (mut add, mut rem) = (0usize, 0usize);
            for fc in sess.files.values() {
                add += fc.added;
                rem += fc.removed;
            }
            println!(
                "  {BOLD}sesión {short}{RESET}  {DIM}· {} archivo(s) · {GREEN}+{add}{RESET}{DIM} {RED}-{rem}{RESET}",
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
