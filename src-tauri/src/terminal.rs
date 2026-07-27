//! "Resume in terminal" — open the user's terminal emulator running
//! `claude --resume <sessionId>` in the session's cwd. Mirrors `editor.rs`:
//! arrow never embeds a terminal (a GUI app has no TTY to offer), it DELEGATES
//! to a real emulator found on `$PATH` and exits. The inner command is wrapped
//! so the terminal stays open when claude exits (`; exec bash`), leaving the
//! user in a shell at the session's cwd instead of a vanishing window.
//!
//! Safety: the emulator itself is spawned argv-direct (no shell); the ONE
//! shell string we do build (`bash -lc '…'`) single-quotes both the cwd and
//! the session id — ids are UUIDs in practice, but we don't trust the caller.

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Stdio;

/// Single-quote `s` for POSIX shells: closes the quote, emits a literal `'`,
/// reopens. Inside single quotes NOTHING else is special, so this neutralizes
/// `$`, backticks, `;`, spaces, etc.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The command line run INSIDE the terminal. `cd` first (quoted), then claude;
/// the trailing `; exec bash` (Linux only) keeps the window alive both after
/// claude exits and when the `cd` itself fails (the user sees the error).
#[cfg(target_os = "linux")]
fn inner_script(session_id: &str, cwd: Option<&str>) -> String {
    match cwd {
        Some(dir) => format!(
            "cd {} && claude --resume {}; exec bash",
            sh_quote(dir),
            sh_quote(session_id)
        ),
        None => format!("claude --resume {}; exec bash", sh_quote(session_id)),
    }
}

/// Session ids are UUIDs (hex + hyphens). Enforcing that charset up front is
/// defense-in-depth on top of the quoting — a hostile id can't even reach the
/// shell string. `cwd` stays free-form (paths may hold anything); quoting and
/// the absolute-path check cover it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate(session_id: &str, cwd: Option<&str>) -> Result<(), String> {
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-')
    {
        return Err(format!("refusing non-UUID session id: {session_id}"));
    }
    if let Some(dir) = cwd {
        if !std::path::Path::new(dir).is_absolute() {
            return Err(format!("refusing non-absolute cwd: {dir}"));
        }
    }
    Ok(())
}

/// How an emulator's CLI says "run this program". The program is always passed
/// as SEPARATE argv items (`bash -lc <script>`), never one big string.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
enum ExecStyle {
    /// gnome-terminal: `-- <argv…>` (its `-e` is deprecated upstream).
    DoubleDash,
    /// kitty: the program is positional: `kitty <argv…>`.
    Positional,
    /// alacritty / konsole / xterm — and the de-facto convention for an
    /// unknown `$TERMINAL`: `-e <argv…>`.
    DashE,
    /// xfce4-terminal: `-x <argv…>` (its `-e` takes ONE string, not argv).
    DashX,
}

/// Known emulators, in probe order (same table-as-data spirit as `EDITORS` in
/// `editor.rs`). `$TERMINAL` — the user's explicit choice — is consulted first,
/// in `resume_in_terminal`.
#[cfg(target_os = "linux")]
const TERMINALS: &[(&str, ExecStyle)] = &[
    ("gnome-terminal", ExecStyle::DoubleDash),
    ("kitty", ExecStyle::Positional),
    ("alacritty", ExecStyle::DashE),
    ("konsole", ExecStyle::DashE),
    ("xfce4-terminal", ExecStyle::DashX),
    ("xterm", ExecStyle::DashE),
];

/// Style for a `$TERMINAL` the table doesn't know: `-e` is the closest thing
/// to a convention among emulators. Best-effort by design — if it fails to
/// spawn we fall through to the known table.
#[cfg(target_os = "linux")]
fn style_for(basename: &str) -> ExecStyle {
    TERMINALS
        .iter()
        .find(|(name, _)| *name == basename)
        .map(|(_, style)| *style)
        .unwrap_or(ExecStyle::DashE)
}

/// Spawn `bin` with the emulator-appropriate argv around `bash -lc <script>`.
/// Ok(()) means the emulator process LAUNCHED — arrow can't know whether
/// claude inside it succeeds (honest limit; the terminal itself shows that).
#[cfg(target_os = "linux")]
fn spawn_terminal(bin: &std::path::Path, style: ExecStyle, script: &str) -> Result<(), String> {
    let mut cmd = std::process::Command::new(bin);
    match style {
        ExecStyle::DoubleDash => {
            cmd.arg("--");
        }
        ExecStyle::Positional => {}
        ExecStyle::DashE => {
            cmd.arg("-e");
        }
        ExecStyle::DashX => {
            cmd.arg("-x");
        }
    }
    cmd.arg("bash").arg("-lc").arg(script);
    // Detach stdio (a chatty launcher must not write into arrow's terminal) —
    // same rationale as `open_in_editor`.
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    // Reap without blocking the UI: launcher-style emulators (gnome-terminal)
    // exit in milliseconds and would linger as zombies; process-style ones
    // (kitty/alacritty) keep the thread parked in wait() — cheap either way.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Open a terminal running `claude --resume <session_id>` in `cwd` (Linux).
/// Probe order: `$TERMINAL` (the user's explicit pick; a bare name resolves on
/// `$PATH`, a path is used as-is), then the known table. First successful spawn
/// wins. No emulator at all => a clear error, so the UI can fall back to its
/// `copy cmd` affordance.
#[cfg(target_os = "linux")]
pub fn resume_in_terminal(session_id: &str, cwd: Option<&str>) -> Result<(), String> {
    validate(session_id, cwd)?;
    let script = inner_script(session_id, cwd);

    // (1) $TERMINAL — respect the user's choice before guessing.
    if let Some(term) = std::env::var_os("TERMINAL").filter(|v| !v.is_empty()) {
        let term = std::path::PathBuf::from(term);
        let resolved = if term.components().count() > 1 {
            // A path: honor it verbatim (only if it actually is executable).
            crate::editor::is_executable(&term).then_some(term.clone())
        } else {
            term.to_str().and_then(crate::editor::which)
        };
        if let Some(bin) = resolved {
            let base = bin
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if spawn_terminal(&bin, style_for(&base), &script).is_ok() {
                return Ok(());
            }
        }
        // $TERMINAL missing or failed to spawn: fall through to the table.
    }

    // (2) Known emulators, in order.
    for (name, style) in TERMINALS {
        if let Some(bin) = crate::editor::which(name) {
            if spawn_terminal(&bin, *style, &script).is_ok() {
                return Ok(());
            }
        }
    }

    Err(
        "no terminal emulator found (tried $TERMINAL, gnome-terminal, kitty, alacritty, konsole, \
         xfce4-terminal, xterm) — use the copy cmd fallback"
            .to_string(),
    )
}

/// macOS: delegate to Terminal.app via `osascript`. `do script` opens a new
/// window whose shell RUNS the command and stays open afterwards, so no
/// `exec bash` tail is needed. The shell command is single-quote-escaped first
/// (same `sh_quote`), then AppleScript-escaped (`\` and `"`) to survive being
/// embedded in the `do script "…"` literal. UNVERIFIED on real Mac hardware —
/// same standing caveat as the rest of the repo's macOS support.
#[cfg(target_os = "macos")]
pub fn resume_in_terminal(session_id: &str, cwd: Option<&str>) -> Result<(), String> {
    validate(session_id, cwd)?;
    let shell_cmd = match cwd {
        Some(dir) => format!(
            "cd {} && claude --resume {}",
            sh_quote(dir),
            sh_quote(session_id)
        ),
        None => format!("claude --resume {}", sh_quote(session_id)),
    };
    let escaped = shell_cmd.replace('\\', "\\\\").replace('"', "\\\"");
    let mut child = std::process::Command::new("osascript")
        .arg("-e")
        .arg("tell application \"Terminal\" to activate")
        .arg("-e")
        .arg(format!(
            "tell application \"Terminal\" to do script \"{escaped}\""
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Other platforms (Windows): not wired yet — arrow doesn't ship there.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn resume_in_terminal(_session_id: &str, _cwd: Option<&str>) -> Result<(), String> {
    Err("resume in terminal is not supported on this platform yet".to_string())
}
