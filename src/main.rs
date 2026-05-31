//! arrow — CLI de auditoría de Claude Code
//!
//! Capa fina sobre la librería `arrow` (`src/lib.rs`): parsea flags, llama a la
//! lib y presenta el resultado (terminal coloreado o JSON). TODA la lógica del
//! parser vive en la lib y se comparte con el backend de Tauri (`src-tauri/`).
//!
//! Flags: `--list`, `--json`, `--repo`, `--session`, `--content --file`,
//! `--projects-dir`. Su comportamiento es idéntico al de antes del refactor.

use anyhow::Result;
use clap::Parser;

use arrow::{Collected, Repo, SessionMeta};

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
        let target = cli
            .file
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--content requiere --file <ruta>"))?;
        let out = arrow::file_content(&projects_dir, target, cli.session.as_deref());
        println!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }

    let collected = arrow::collect(&projects_dir, cli.repo.as_deref(), cli.session.as_deref());

    if cli.json {
        let report = arrow::build_report_from(&projects_dir, &collected.repos, &collected.metas);
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    render_terminal(&projects_dir, &collected, cli.list);
    Ok(())
}

fn render_terminal(projects_dir: &str, collected: &Collected, list_only: bool) {
    let Collected {
        repos,
        metas,
        jsonl_files,
        lines_total,
        lines_skipped,
    } = collected;

    println!("{BOLD}arrow{RESET} {DIM}· fuente: {projects_dir}{RESET}");
    println!(
        "{DIM}{jsonl_files} sesiones · {lines_total} records · {lines_skipped} líneas ignoradas (parsing defensivo){RESET}",
    );

    if repos.is_empty() {
        println!("\n{YELLOW}Sin ediciones atribuibles a Claude (Edit/Write/MultiEdit) con los filtros dados.{RESET}");
        return;
    }

    for (cwd, repo) in repos {
        render_repo(cwd, repo, metas, list_only);
    }
}

fn render_repo(
    cwd: &str,
    repo: &Repo,
    metas: &std::collections::BTreeMap<String, SessionMeta>,
    list_only: bool,
) {
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
