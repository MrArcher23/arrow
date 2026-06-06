//! "Worktrees inventory" — list and classify the git worktrees that Claude Code
//! creates per session (under `<repo>/.claude/worktrees/<name>/`), so the user
//! can spot stale or already-merged ones eating disk.
//!
//! Listing/classifying is READ-ONLY. The one exception is the opt-in "Clean"
//! action (`remove_worktree`/`prune_worktrees` below) — the only place in arrow
//! that mutates disk — gated behind a dry-run + confirmation in the UI and run
//! WITHOUT `--force`, so git itself refuses a locked or dirty worktree. Tauri-only
//! (the browser dev-server has no equivalent) and — exactly like `editor.rs` —
//! ALL the git shelling lives here, so the parser lib (`src/lib.rs`) stays
//! git-free (the product premise: "without depending on git or hooks").
//!
//! Honesty rules baked in:
//!   - "merged → safe to remove" is claimed ONLY when the worktree's branch tip
//!     is an ancestor of the repo's resolved default branch (proven). A squash or
//!     rebase merge leaves the branch a non-ancestor, so it shows as "can't tell"
//!     (with a commits-ahead count) — never a false green, never a confident
//!     "not merged".
//!   - the default branch is resolved per repo (never hardcoded master/main); if
//!     it can't be resolved, merge classification is disabled for that repo.
//!   - detached / locked / prunable / dirty are surfaced honestly; the main
//!     worktree is flagged and never offered for removal.
//!   - sizes are APPROXIMATE (apparent bytes) and computed only on demand.
//!
//! Safety: git is invoked argv-direct (no shell), with read-only subcommands
//! only (worktree list, symbolic-ref, rev-parse, merge-base, rev-list, status),
//! each bounded by a timeout so a wedged repo can't freeze the modal.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use walkdir::WalkDir;

/// One worktree of a repo (the main checkout, or a linked worktree).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeOut {
    /// Absolute path of the worktree's working directory.
    pub path: String,
    /// Short branch name, or `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Abbreviated HEAD commit (display / disambiguation).
    pub head: String,
    /// The repo's primary worktree. Never removable (git refuses it).
    pub is_main: bool,
    /// Proven merged: the branch tip is an ancestor of the resolved default
    /// branch. Only this state earns a green "safe to remove".
    pub is_merged: bool,
    /// Commits on the branch not yet in the default branch (`<default>..<branch>`).
    /// `0` for a proven-merged branch; `> 0` typically means unmerged OR
    /// squash/rebase-merged (indistinguishable locally). `None` for the main
    /// worktree, a detached HEAD, or when the default branch is unknown.
    pub ahead: Option<i64>,
    /// Working tree has uncommitted/untracked changes (status not clean).
    pub dirty: bool,
    /// git marks the worktree locked (remove refuses it without --force).
    pub locked: bool,
    /// git marks the worktree prunable (its dir is gone / pointer dangling).
    pub prunable: bool,
}

/// All worktrees of one main repo, plus the default branch they were judged
/// against (so the UI can show provenance honestly).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoWorktreesOut {
    /// The MAIN repo root (groups rows; used to build the remove command).
    pub repo_root: String,
    /// Resolved default branch ("main"/"master"/…), or `None` if undeterminable
    /// (in which case merge classification is suppressed for this repo).
    pub default_branch: Option<String>,
    /// Worktrees of this repo, main first.
    pub worktrees: Vec<WorktreeOut>,
}

/// List + classify the worktrees of each given repo root. One `git worktree list`
/// per root (cheap plumbing — it only reads `$GIT_DIR/worktrees/` metadata, never
/// scans a working tree). Only repos with at least one LINKED worktree are
/// returned, so the modal's empty state stays honest. Roots that aren't git
/// repos, or where git is unavailable, are silently skipped.
pub fn list_worktrees(repo_roots: &[String]) -> Vec<RepoWorktreesOut> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for root in repo_roots {
        // Dedup (two cwds can map to one repo) and guard against option-injection
        // (an absolute path can't be read by git as a flag).
        if !seen.insert(root.as_str()) || !Path::new(root).is_absolute() {
            continue;
        }
        if let Some(repo) = classify_repo(root) {
            if repo.worktrees.iter().any(|w| !w.is_main) {
                out.push(repo);
            }
        }
    }
    out
}

/// Sum the apparent byte size of each worktree path (recursive, not following
/// symlinks). APPROXIMATE: apparent bytes (`metadata().len()`), not on-disk
/// blocks. On demand only ("Calculate sizes"), since walking a full checkout
/// (node_modules, target/) is the one expensive part of the whole feature.
pub fn worktree_sizes(paths: &[String]) -> HashMap<String, u64> {
    let mut out = HashMap::new();
    for path in paths {
        if Path::new(path).is_absolute() {
            out.insert(path.clone(), dir_size(path));
        }
    }
    out
}

// --- internals -------------------------------------------------------------

const GIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Run a READ-ONLY git command in `repo` (argv-direct, no shell), bounded by a
/// timeout so a wedged repo or a stalled network FS can never freeze the modal.
/// Returns `(exit_ok, stdout)`. Defensive: a missing git binary, a spawn failure
/// or a timeout all map to `None` — never a panic.
fn run_git(repo: &str, args: &[&str]) -> Option<(bool, String)> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + GIT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut buf = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut buf);
                }
                return Some((status.success(), buf));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

/// stdout of a git command that exited 0, else `None`.
fn git_stdout(repo: &str, args: &[&str]) -> Option<String> {
    match run_git(repo, args) {
        Some((true, out)) => Some(out),
        _ => None,
    }
}

/// Whether a git command exited 0 (for predicates like `merge-base --is-ancestor`).
fn git_ok(repo: &str, args: &[&str]) -> bool {
    matches!(run_git(repo, args), Some((true, _)))
}

fn classify_repo(root: &str) -> Option<RepoWorktreesOut> {
    let porcelain = git_stdout(root, &["worktree", "list", "--porcelain"])?;
    let entries = parse_porcelain(&porcelain);
    if entries.is_empty() {
        return None;
    }
    // (display name, ref usable in merge-base/rev-list). `None` => can't classify.
    let default = resolve_default_branch(root);
    let default_ref = default.as_ref().map(|(_, r)| r.as_str());
    let worktrees = entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| classify_worktree(root, e, i == 0, default_ref))
        .collect();
    Some(RepoWorktreesOut {
        repo_root: root.to_string(),
        default_branch: default.map(|(d, _)| d),
        worktrees,
    })
}

/// A raw `git worktree list --porcelain` record, parsed defensively.
struct RawWt {
    path: String,
    head: String,
    branch: Option<String>,
    locked: bool,
    prunable: bool,
    bare: bool,
}

/// Parse `git worktree list --porcelain` into records. Defensive: a record
/// starts at each `worktree ` line; known field prefixes are matched and unknown
/// lines ignored, so a format quirk degrades to a partial record, never a crash.
/// (Plain `--porcelain`, blank-line separated; `-z` exists only on git ≥ 2.36.)
fn parse_porcelain(text: &str) -> Vec<RawWt> {
    let mut entries = Vec::new();
    let mut cur: Option<RawWt> = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(e) = cur.take() {
                entries.push(e);
            }
            cur = Some(RawWt {
                path: p.to_string(),
                head: String::new(),
                branch: None,
                locked: false,
                prunable: false,
                bare: false,
            });
        } else if let Some(e) = cur.as_mut() {
            if let Some(h) = line.strip_prefix("HEAD ") {
                e.head = h.to_string();
            } else if let Some(b) = line.strip_prefix("branch ") {
                e.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            } else if line == "bare" {
                e.bare = true;
            } else if line == "locked" || line.starts_with("locked ") {
                e.locked = true;
            } else if line == "prunable" || line.starts_with("prunable ") {
                e.prunable = true;
            }
            // "detached" and any other line: ignored (branch stays None for detached).
        }
    }
    if let Some(e) = cur.take() {
        entries.push(e);
    }
    // Drop bare entries (not normal worktrees).
    entries.into_iter().filter(|e| !e.bare).collect()
}

fn classify_worktree(
    root: &str,
    e: RawWt,
    is_main: bool,
    default_ref: Option<&str>,
) -> WorktreeOut {
    let dirty = match git_stdout(&e.path, &["status", "--porcelain"]) {
        Some(s) => !s.trim().is_empty(),
        None => false, // can't tell → don't cry wolf
    };

    // Merge classification: only for a linked worktree on a real branch, against a
    // resolved default. Proven merged = branch tip is an ancestor of default.
    let (is_merged, ahead) = match (&e.branch, default_ref, is_main) {
        (Some(branch), Some(def), false) => {
            let merged = git_ok(root, &["merge-base", "--is-ancestor", branch, def]);
            let ahead = git_stdout(root, &["rev-list", "--count", &format!("{def}..{branch}")])
                .and_then(|s| s.trim().parse::<i64>().ok());
            (merged, ahead)
        }
        _ => (false, None),
    };

    WorktreeOut {
        path: e.path,
        head: short_head(&e.head),
        branch: e.branch,
        is_main,
        is_merged,
        ahead,
        dirty,
        locked: e.locked,
        prunable: e.prunable,
    }
}

/// Resolve a repo's default branch, OFFLINE only (no `remote show`, which would
/// hit the network and could hang). Returns `(display, ref)` where `ref` is
/// usable in merge-base/rev-list. `None` when undeterminable — callers then
/// suppress merge classification rather than guess.
fn resolve_default_branch(root: &str) -> Option<(String, String)> {
    // 1. origin/HEAD symbolic ref (offline; present only if it was ever set).
    //    Compare against the remote-tracking ref, which exists locally.
    if let Some(s) = git_stdout(
        root,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        if let Some(b) = s.trim().strip_prefix("origin/") {
            if !b.is_empty() {
                return Some((b.to_string(), format!("origin/{b}")));
            }
        }
    }
    // 2. Local-branch heuristic among the common default names (offline).
    for cand in ["main", "master", "develop", "trunk"] {
        if git_ok(
            root,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{cand}"),
            ],
        ) {
            return Some((cand.to_string(), cand.to_string()));
        }
    }
    None
}

fn short_head(head: &str) -> String {
    head.chars().take(8).collect()
}

fn dir_size(path: &str) -> u64 {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

// --- Stage 2: removal (the ONLY part of arrow that mutates the disk) ---------
//
// READ-ONLY is the default everywhere else in arrow; this is the single, opt-in
// exception, reached only by an explicit user action (the "Clean" button) and
// only after a dry-run + confirmation in the UI. `remove` runs WITHOUT `--force`,
// so git itself refuses to drop a worktree that is locked or has uncommitted /
// untracked changes — a free safety net we deliberately keep (matching the
// honesty rules above: we never promise a removal git would refuse).

/// Outcome of a removal/prune attempt (or its dry-run preview).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    /// git exited 0 (for a dry-run, always `true` — nothing was attempted).
    pub ok: bool,
    /// This was a preview only; the disk was not touched.
    pub dry_run: bool,
    /// The exact command, shell-quoted, for display / copy / an audit log.
    pub command: String,
    /// What git said (stdout + stderr), trimmed. Surfaced verbatim so a refusal
    /// ("contains modified or untracked files") is shown honestly, not hidden.
    pub output: String,
}

/// Quote a path for the DISPLAYED command only (the real call is argv-direct, so
/// it never goes through a shell — this just makes the copy-paste string correct
/// when a path has spaces).
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Run `git -C <repo> <args>` capturing stdout+stderr and success. Unlike the
/// read-only `run_git`, this keeps stderr: a removal that git refuses must show
/// its reason. Bounded by the same `GIT_TIMEOUT` (and with stdin nulled) as the
/// read path, so a wedged git — a stalled network FS, lock contention, or a
/// stray credential/hook prompt reading stdin — can never freeze the UI stuck on
/// "Running…". On timeout the child is killed and an honest error is returned.
fn run_git_capture(repo: &str, args: &[&str]) -> (bool, String) {
    let mut child = match Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (false, format!("failed to run git: {e}")),
    };
    let deadline = Instant::now() + GIT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                let mut err = String::new();
                if let Some(mut o) = child.stdout.take() {
                    let _ = o.read_to_string(&mut out);
                }
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                let mut s = out.trim().to_string();
                let err = err.trim();
                if !err.is_empty() {
                    if !s.is_empty() {
                        s.push('\n');
                    }
                    s.push_str(err);
                }
                return (status.success(), s);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (false, "git timed out".to_string());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return (false, format!("failed to run git: {e}")),
        }
    }
}

/// Remove a linked worktree. No `--force` (git refuses on locked / uncommitted /
/// untracked changes). `git worktree remove` has no native dry-run, so `dry_run`
/// reports the command without touching anything.
pub fn remove_worktree(repo: &str, path: &str, dry_run: bool) -> CleanupResult {
    let command = format!(
        "git -C {} worktree remove {}",
        shell_quote(repo),
        shell_quote(path)
    );
    if dry_run {
        return CleanupResult {
            ok: true,
            dry_run: true,
            command,
            output: "dry run — not executed (git would refuse if the worktree is \
                     locked or has uncommitted/untracked changes)"
                .to_string(),
        };
    }
    let (ok, output) = run_git_capture(repo, &["worktree", "remove", path]);
    CleanupResult {
        ok,
        dry_run: false,
        command,
        output,
    }
}

/// Prune phantom worktree entries (their directories are gone). Uses git's native
/// dry-run (`-n`) to preview, so the dry-run is a real git report, not a guess.
pub fn prune_worktrees(repo: &str, dry_run: bool) -> CleanupResult {
    let args: &[&str] = if dry_run {
        &["worktree", "prune", "-n", "-v"]
    } else {
        &["worktree", "prune", "-v"]
    };
    let command = format!(
        "git -C {} worktree prune -v{}",
        shell_quote(repo),
        if dry_run { " -n" } else { "" }
    );
    let (ok, output) = run_git_capture(repo, args);
    CleanupResult {
        ok,
        dry_run,
        command,
        output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_parses_branch_detached_locked_prunable() {
        let text = "\
worktree /home/u/repo
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree /home/u/repo/.claude/worktrees/feat
HEAD 2222222222222222222222222222222222222222
branch refs/heads/fix/whatsapp-dlq-noise

worktree /home/u/repo/.claude/worktrees/pr-7
HEAD 3333333333333333333333333333333333333333
detached
locked needs network mount

worktree /home/u/repo/.claude/worktrees/gone
HEAD 4444444444444444444444444444444444444444
branch refs/heads/old
prunable gitdir file points to non-existent location
";
        let e = parse_porcelain(text);
        assert_eq!(e.len(), 4);
        assert_eq!(e[0].path, "/home/u/repo");
        assert_eq!(e[0].branch.as_deref(), Some("main"));
        // Short branch name keeps slashes.
        assert_eq!(e[1].branch.as_deref(), Some("fix/whatsapp-dlq-noise"));
        // Detached → no branch; locked flagged.
        assert_eq!(e[2].branch, None);
        assert!(e[2].locked);
        // Prunable flagged.
        assert!(e[3].prunable);
        assert_eq!(e[3].branch.as_deref(), Some("old"));
    }

    #[test]
    fn porcelain_drops_bare_and_tolerates_garbage() {
        let text = "\
worktree /home/u/bare
bare

worktree /home/u/repo
HEAD 5555555555555555555555555555555555555555
branch refs/heads/main
some-future-field whatever
";
        let e = parse_porcelain(text);
        assert_eq!(e.len(), 1, "the bare worktree is dropped");
        assert_eq!(e[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn remove_dry_run_quotes_and_does_not_touch_disk() {
        // Pure: a dry-run for `remove` never shells out, so it needs no git.
        let res = remove_worktree("/repo with space", "/repo with space/wt", true);
        assert!(res.ok && res.dry_run);
        assert_eq!(
            res.command, "git -C '/repo with space' worktree remove '/repo with space/wt'",
            "the displayed command is shell-quoted for safe copy-paste"
        );
    }

    #[test]
    fn remove_deletes_a_real_worktree_without_force() {
        // Hermetic integration: a real temp git repo + a linked worktree, then
        // remove it. Skips cleanly if git is unavailable in the test environment.
        if run_git(".", &["--version"]).is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("arrow-wt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git_in = |dir: &Path, args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap();
        };
        git_in(&repo, &["init", "-q", "-b", "main"]);
        git_in(&repo, &["config", "user.email", "t@t"]);
        git_in(&repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("a.txt"), "x\n").unwrap();
        git_in(&repo, &["add", "-A"]);
        git_in(&repo, &["commit", "-q", "-m", "init"]);
        let wt = base.join("wt-feature");
        git_in(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                wt.to_str().unwrap(),
                "-b",
                "feature",
            ],
        );

        let repo_s = repo.to_string_lossy().to_string();
        let repos = list_worktrees(std::slice::from_ref(&repo_s));
        assert_eq!(
            repos.len(),
            1,
            "the repo with one linked worktree is listed"
        );
        assert!(repos[0].worktrees.iter().any(|w| !w.is_main));

        // Dry-run does not touch disk.
        let dry = remove_worktree(&repo_s, wt.to_str().unwrap(), true);
        assert!(dry.ok && dry.dry_run);
        assert!(wt.exists(), "dry-run must NOT remove the worktree");

        // Real removal (no uncommitted changes → git allows it without --force).
        let res = remove_worktree(&repo_s, wt.to_str().unwrap(), false);
        assert!(res.ok, "remove should succeed: {}", res.output);
        assert!(!wt.exists(), "the worktree must be gone");

        // prune dry-run runs cleanly even with no phantoms.
        let pr = prune_worktrees(&repo_s, true);
        assert!(pr.ok && pr.dry_run);

        let _ = std::fs::remove_dir_all(&base);
    }
}
