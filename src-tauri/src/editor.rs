//! "Open in editor" — detect installed editors and open a file at a line by
//! DELEGATING to the user's real editor. arrow never embeds an editor or a
//! language server; it just spawns the editor's CLI and exits. This is the
//! mirror image of Claude Code's `/ide`: instead of pulling the editor into the
//! tool, it sends you from the audit view into your editor at the exact spot.
//!
//! The editor list is a TABLE (data), so adding a fork later is a data change,
//! not new code — same isolation spirit as the frontend's `api.ts`/`zoom.ts`.
//!
//! Linux-first (where arrow ships today): editors are found on `$PATH`. macOS
//! (`open -a`/bundle ids) and Windows (`.cmd` shims) resolve binaries
//! differently and are left for when those platforms ship.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Serialize;

/// How an editor's CLI expresses "open at line:column".
#[derive(Clone, Copy)]
enum OpenStyle {
    /// VS Code family: `-g <file>:<line>:<col>` (the `--goto` flag).
    VsCodeGoto,
    /// Position appended to the path, no flag: `<file>:<line>:<col>` (Zed, Sublime).
    PathSuffix,
    /// Flags before the path: `--line <n> --column <c> <file>` (JetBrains, Kate).
    LineColumnFlags,
}

struct EditorDef {
    /// Stable id sent from the frontend and persisted as the user's choice.
    id: &'static str,
    /// Human label shown in the UI.
    name: &'static str,
    /// Binary names to probe on `$PATH`, in order (first found wins). Some
    /// editors ship under more than one name depending on packaging.
    binaries: &'static [&'static str],
    style: OpenStyle,
}

/// Known editors, in priority order (most common first). Detection only ever
/// surfaces the ones actually present, so listing extras here is free.
/// Terminal editors (vim/nvim/helix/nano) are intentionally omitted: a GUI app
/// can't spawn them without a controlling TTY (they'd need a terminal-emulator
/// host), so they're deferred.
///
/// The VS Code family and Zed are verified live; the JetBrains, Sublime and Kate
/// rows use their documented CLI contracts but are unverified on the dev machine
/// (best-effort until tested on a box that has them).
const EDITORS: &[EditorDef] = &[
    // VS Code family — all inherit upstream's `-g file:line:col`.
    EditorDef {
        id: "code",
        name: "VS Code",
        binaries: &["code"],
        style: OpenStyle::VsCodeGoto,
    },
    EditorDef {
        id: "code-insiders",
        name: "VS Code Insiders",
        binaries: &["code-insiders"],
        style: OpenStyle::VsCodeGoto,
    },
    EditorDef {
        id: "cursor",
        name: "Cursor",
        binaries: &["cursor"],
        style: OpenStyle::VsCodeGoto,
    },
    EditorDef {
        id: "windsurf",
        name: "Windsurf",
        binaries: &["windsurf"],
        style: OpenStyle::VsCodeGoto,
    },
    EditorDef {
        id: "kiro",
        name: "Kiro",
        binaries: &["kiro"],
        style: OpenStyle::VsCodeGoto,
    },
    EditorDef {
        id: "antigravity",
        name: "Antigravity",
        binaries: &["antigravity"],
        style: OpenStyle::VsCodeGoto,
    },
    EditorDef {
        id: "antigravity-ide",
        name: "Antigravity IDE",
        binaries: &["antigravity-ide"],
        style: OpenStyle::VsCodeGoto,
    },
    EditorDef {
        id: "codium",
        name: "VSCodium",
        binaries: &["codium", "vscodium"],
        style: OpenStyle::VsCodeGoto,
    },
    EditorDef {
        id: "code-oss",
        name: "Code - OSS",
        binaries: &["code-oss"],
        style: OpenStyle::VsCodeGoto,
    },
    EditorDef {
        id: "trae",
        name: "Trae",
        binaries: &["trae"],
        style: OpenStyle::VsCodeGoto,
    },
    // Zed — native editor (not a fork): positional `file:line:col`, no `-g`.
    EditorDef {
        id: "zed",
        name: "Zed",
        binaries: &["zed", "zeditor"],
        style: OpenStyle::PathSuffix,
    },
    // Sublime Text — positional `file:line:col`.
    EditorDef {
        id: "subl",
        name: "Sublime Text",
        binaries: &["subl"],
        style: OpenStyle::PathSuffix,
    },
    // JetBrains — one CLI contract: `--line N --column C file` (LightEdit mode,
    // so no pre-open project needed). `rustrover` is most relevant to arrow.
    EditorDef {
        id: "rustrover",
        name: "RustRover",
        binaries: &["rustrover"],
        style: OpenStyle::LineColumnFlags,
    },
    EditorDef {
        id: "idea",
        name: "IntelliJ IDEA",
        binaries: &["idea"],
        style: OpenStyle::LineColumnFlags,
    },
    EditorDef {
        id: "pycharm",
        name: "PyCharm",
        binaries: &["pycharm"],
        style: OpenStyle::LineColumnFlags,
    },
    EditorDef {
        id: "webstorm",
        name: "WebStorm",
        binaries: &["webstorm"],
        style: OpenStyle::LineColumnFlags,
    },
    EditorDef {
        id: "goland",
        name: "GoLand",
        binaries: &["goland"],
        style: OpenStyle::LineColumnFlags,
    },
    EditorDef {
        id: "clion",
        name: "CLion",
        binaries: &["clion"],
        style: OpenStyle::LineColumnFlags,
    },
    EditorDef {
        id: "phpstorm",
        name: "PhpStorm",
        binaries: &["phpstorm"],
        style: OpenStyle::LineColumnFlags,
    },
    // Kate (KDE) — same `--line/--column` flags as JetBrains.
    EditorDef {
        id: "kate",
        name: "Kate",
        binaries: &["kate"],
        style: OpenStyle::LineColumnFlags,
    },
];

/// An editor present on the machine, for the UI's picker.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorOut {
    pub id: String,
    pub name: String,
}

#[cfg(unix)]
pub(crate) fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub(crate) fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// Resolve a binary on `$PATH` (first directory that holds an executable of
/// that name). Pure filesystem lookup — no subprocess. Shared with
/// `terminal.rs` (the "Resume in terminal" emulator probe).
pub(crate) fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|cand| is_executable(cand))
}

/// First binary of an editor def that resolves on `$PATH`.
fn resolve(def: &EditorDef) -> Option<PathBuf> {
    def.binaries.iter().find_map(|b| which(b))
}

/// Editors detected on `$PATH`, in table (priority) order. The frontend shows
/// these in a picker; an empty result hides the "Open in editor" control.
pub fn detect_editors() -> Vec<EditorOut> {
    EDITORS
        .iter()
        .filter(|e| resolve(e).is_some())
        .map(|e| EditorOut {
            id: e.id.into(),
            name: e.name.into(),
        })
        .collect()
}

/// Open `file` at `line`:`col` in the editor identified by `id`. Fire-and-forget
/// (does not block the UI). Builds argv as SEPARATE arguments — never a shell
/// string — to dodge the documented `-g` breakage on paths with spaces
/// (microsoft/vscode#39891, cursor/cursor#3796). Opens the CURRENT on-disk file,
/// so `line` is the after-side `firstChangedLine`, not the reconstructed before.
/// Rejects a non-absolute path or one that no longer exists on disk — honest: it
/// never claims to open a file that isn't there (and never fabricates one).
pub fn open_in_editor(id: &str, file: &str, line: i64, col: i64) -> Result<(), String> {
    let def = EDITORS
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("unknown editor: {id}"))?;
    let bin = resolve(def).ok_or_else(|| format!("editor '{}' is not on PATH", def.name))?;

    // Honesty + safety: only open a real, current on-disk file. `is_absolute`
    // also closes any option-injection vector (an absolute path can't begin with
    // `-`); `exists` stops us from launching the editor on a deleted/moved path
    // (which would silently fabricate a phantom file in the VS Code family) and
    // from reporting success when there is nothing to open.
    let target = Path::new(file);
    if !target.is_absolute() {
        return Err(format!("refusing to open non-absolute path: {file}"));
    }
    if !target.exists() {
        return Err(format!("file no longer on disk: {file}"));
    }

    let line = line.max(1);
    let col = col.max(1);

    let mut cmd = std::process::Command::new(&bin);
    match def.style {
        OpenStyle::VsCodeGoto => {
            cmd.arg("-g").arg(format!("{file}:{line}:{col}"));
        }
        OpenStyle::PathSuffix => {
            cmd.arg(format!("{file}:{line}:{col}"));
        }
        OpenStyle::LineColumnFlags => {
            cmd.arg("--line")
                .arg(line.to_string())
                .arg("--column")
                .arg(col.to_string())
                .arg(file);
        }
    }
    // Detach the child's stdio so a chatty launcher can't write into arrow's
    // terminal (relevant when arrow is run from a shell / the AppImage).
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Fire-and-forget, but REAP the child. Most editor CLIs hand off to a running
    // instance and exit within milliseconds; if we dropped the Child it would
    // linger as a zombie (defunct) in arrow's process table — one per click, for
    // arrow's whole lifetime. Waiting on a detached thread collects it without
    // blocking the UI. (Dropping the Child does not kill the editor on Unix, so
    // this only changes reaping, not the editor's lifetime.)
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch {}: {e}", def.name))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
