# arrow

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-000?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![egui](https://img.shields.io/badge/egui-0.34-blue)](https://github.com/emilk/egui)
[![CI](https://github.com/MrArcher23/arrow/actions/workflows/ci.yml/badge.svg)](https://github.com/MrArcher23/arrow/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/MrArcher23/arrow?sort=semver)](https://github.com/MrArcher23/arrow/releases/latest)
[![Platform: Linux](https://img.shields.io/badge/platform-Linux-FCC624?logo=linux&logoColor=black)](#install-linux)
[![Platform: macOS](https://img.shields.io/badge/platform-macOS-000?logo=apple&logoColor=white)](#install-macos)

**An audit viewer for Claude Code.** It answers a single question, reliably:
*which files did Claude touch, in which repo, with what diff, and in which session?* — without
opening an IDE, without AI chat, and **without depending on git or hooks**.

> Status: **Native rewrite — the desktop app is now a single Rust binary built on
> [egui/eframe](https://github.com/emilk/egui)** (no webview, no Node/Vite, no WebKitGTK). It
> replaces the previous Tauri 2.x + Svelte/CodeMirror stack, reusing the same Rust parser library
> directly (no IPC, no JSON contract — the UI consumes the parser's structs as-is). It keeps the
> feature set: sidebar `repo → session → files`, a side-by-side diff with syntax highlighting,
> active-session focus with a live `notify` watcher, open-in-editor, the worktrees inventory +
> cleanup, themes, zoom, and the release-update check. Phases 3 (honesty + git) and 4 (editing) are
> still pending — details in [ROADMAP.md](ROADMAP.md).

## Install (Linux)

> **Linux (x86-64) has pre-built bundles.** macOS (Apple Silicon + Intel) bundles are built and
> published by CI for each tagged release — see [Install (macOS)](#install-macos). Windows is **not
> built yet**; build from source there (see *Desktop app* below). The macOS `.dmg` is **unsigned**
> (no Apple notarization yet — see [MACOS.md](MACOS.md) and the [roadmap](ROADMAP.md)), so the first
> launch needs one extra step (covered in [Install (macOS)](#install-macos)).

Grab the latest from the [**Releases**](https://github.com/MrArcher23/arrow/releases/latest) page:

```bash
# Debian / Ubuntu / Pop!_OS — the .deb
sudo dpkg -i arrow_*_amd64.deb
sudo apt-get install -f      # only if dpkg reports missing dependencies

# Any distro — the AppImage (no install)
chmod +x arrow_*_amd64.AppImage
./arrow_*_amd64.AppImage
```

The bundles are built on Ubuntu 22.04, so they run on **glibc ≥ 2.35** (Ubuntu/Pop!_OS 22.04+,
Debian 12+, Fedora 37+, and newer). On older systems, build from source.

## Install (macOS)

> Each tagged release publishes an **unsigned** `.dmg` for both Apple Silicon and Intel, built by
> CI (no Apple notarization yet), so macOS Gatekeeper would normally block the first launch — the
> one-liner below handles that for you. If no macOS release has been published yet, the installer
> says so and exits cleanly; meanwhile, build from source (see *Desktop app* below).

**One-liner (recommended):** downloads the right `.dmg` for your chip from the latest release,
installs `arrow.app` into `/Applications`, and clears the Gatekeeper quarantine flag:

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/MrArcher23/arrow/main/install.sh)"
```

> The installer strips the quarantine attribute of an **unsigned** binary so it opens without
> friction — only run it if you trust this source. To **update**, re-run the one-liner; to
> **uninstall**, `rm -rf /Applications/arrow.app`. arrow **tells you when a newer release exists**
> (a dot on the version badge, or `arrow --check-update` from the CLI) but does not auto-install it
> yet — being unsigned, a silent in-app updater would fight Gatekeeper. A Homebrew Cask and a signed
> auto-updater are on the [roadmap](ROADMAP.md).

**Manual:** grab the `.dmg` for your chip (`_aarch64` = Apple Silicon, `_x64` = Intel) from the
[**Releases**](https://github.com/MrArcher23/arrow/releases/latest) page, open it, and drag arrow to
**Applications**. Because the app is unsigned, the first launch needs **right-click → Open** (once),
or clear the flag manually:

```bash
xattr -dr com.apple.quarantine /Applications/arrow.app
```

## Why it exists

When Claude Code becomes **the one doing** most of the changes, your job shifts to **steering and
reviewing**: the IDE stops being where you write and is relegated to *"show me what it touched."*
But keeping an IDE — or a heavy Electron-style Git client — open just to review diffs wastes
anywhere from hundreds of MB to several GB of RAM. arrow does exactly that one part — seeing what
Claude changed, repo by repo and session by session — in a lightweight, focused app.

And while the tooling space around Claude Code is crowded, nobody covers exactly this: a **graphical
UI, no chat**, with the `repository → touched files → diff/editor` hierarchy as a navigable audit
trail. The closest options are a terminal TUI (`claude-file-recovery`), a web chat client
(`claude-code-viewer`), or large GUIs oriented toward *running* agents (`opcode`, AGPL). Anthropic
closed the "recoverable edit history" feature request (#36542) as *not planned* — so the gap is
real, even if the margin is narrow.

## The key idea: the right data source

There is no need to install a `session-log` hook. Claude Code **already persists** everything
needed, natively and in a structured form (verified on Claude Code v2.1.x):

| Data | Native source |
|---|---|
| **Repos** | The `cwd` field of each record (the real path; we don't decode the directory name, which is ambiguous). |
| **Sessions** | Each `~/.claude/projects/<encoded-cwd>/<sessionId>.jsonl`. The file name **is** the `sessionId`. |
| **Touched files** | `type:"user"` records with `toolUseResult.filePath` (only `Edit`/`Write`/`MultiEdit`). |
| **Diff (before/after)** | `toolUseResult.structuredPatch` — the exact hunks `{oldStart, oldLines, newStart, newLines, lines}`. This is already the per-session diff. |
| **"Before" / point-in-time** | `~/.claude/file-history/<sessionId>/<hash>@v<n>` — snapshots of the prior content. |

`git diff` is kept as an optional **secondary** view (the full working tree), and only when the repo
is a git repo: it does not attribute by author or by session, and many repos aren't git at all.

## Honest limits (the product must NOT lie)

- Only what goes through `Edit`/`Write`/`MultiEdit` is captured. Changes made via the session's
  `Bash` commands (sed, prettier, build, `mv`, `rm`) **do not** appear. The correct label is always
  *"edits via Claude's tools"*, never *"everything Claude did"*.
- The `userModified` flag signals drift (the user edited between the read and the write): it's
  marked with ⚠.
- The JSONL format is **internal, undocumented, changes between versions, and auto-deletes at ~30
  days** (`cleanupPeriodDays`). Hence the defensive parsing: an invalid line is skipped, never
  crashing.

## Usage

```bash
cargo build --release

# Summary: all repos -> sessions -> files (+/-)
./target/release/arrow --list

# Full diff of a repo (filters by a substring of the cwd)
./target/release/arrow --repo my-project

# A specific session (prefix of the sessionId)
./target/release/arrow --session 14385fed

# Normalized JSON (the contract the UI consumes)
./target/release/arrow --repo my-project --json

# Is there a newer arrow release? (network; read-only, never installs)
./target/release/arrow --check-update
```

Options: `--projects-dir <path>` (defaults to `~/.claude/projects`), `--repo`, `--session`,
`--list`, `--json`, `--check-update`, and `--content --file <path> [--session <id>]` (emits `{before, after}` for a
file, for the UI's diff view).

## Desktop app (egui)

A single native Rust binary built on **egui/eframe** — no webview, no Node/Vite, no WebKitGTK. The
parser library *is* the backend: the app calls `arrow::build_report` / `arrow::file_content`
directly, so there is no IPC and no JSON contract between a frontend and a backend.

```bash
# Linux build deps (Debian/Ubuntu/Pop!_OS), once — note: no WebKitGTK/Node anymore:
sudo apt install -y libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
                    libxkbcommon-dev libssl-dev

# run the app (gui/ is its own cargo workspace)
cargo run --release --manifest-path gui/Cargo.toml
```

- **Single binary, no webview**: the release binary is ~13 MB and self-contained, reading
  `~/.claude/projects` directly from Rust. It uses **~150 MB of RAM (PSS)** in use (down from
  ~200 MB on the old Tauri/WebKitGTK build, and a fraction of an Electron client).
- **Side-by-side diff**: a hand-built diff view (egui has no merge widget) — lines aligned with
  `similar`, syntax-highlighted with `syntect` (themes/syntaxes bundled by `two-face`), rendered in
  one virtualized scroll area so both columns scroll together. Created/deleted/unknown files get an
  honest banner and a single column (a missing "before" is never shown as a new file).
- **Native live refresh**: a `notify` watcher over `~/.claude/projects` (debounced, resilient) feeds
  a background worker thread that re-parses off the UI thread; the open diff refreshes in place.
- **Active-session focus**: the repo(s) of the **active session** (most recent activity) sit at the
  top, plus any repo touched in the same ~10-min burst; the rest fold into *Other repos*, and an
  idle launch shows a quiet "No active work" with collapsed history. A **green dot** marks focused
  repos with recent activity. Honest: "active" = *most recent activity on disk*, not a running process.
- **Open in editor**: a picker in the file bar opens the selected file at the first changed line by
  **delegating to the editor's CLI** (the VS Code family, Zed, JetBrains, Sublime, Kate); arrow never
  embeds an editor. Honest: it opens the **current on-disk file** (disabled when it's gone).
- **Worktrees inventory + cleanup**: a `Worktrees` button opens an inventory of the git worktrees
  Claude Code creates per session, flagged **active / stale / merged → safe to remove**, with
  on-demand disk sizes and a `copy cmd` per row. A **`Clean`/`Prune`** button runs `git worktree
  remove` **without `--force`** after a dry-run + confirmation — the only action in arrow that mutates
  disk, offered only on rows git would actually clean. "merged" shows only when provably an ancestor of
  the (dynamically resolved) default branch; a squash/rebase reads as *can't confirm*. All git shelling
  stays in `gui/`, so the parser library remains git-free.
- **Themes, zoom, persistence**: a curated set of themes (egui visuals + a syntect code theme), VS
  Code-style zoom (`Ctrl +/−/0`), and a version badge with the release-update check. Theme, zoom,
  remembered editor and sidebar width persist across runs via eframe's on-disk storage.

> **macOS:** eframe creates a real native window; the `.app`/`.dmg` are built in CI (`cargo bundle`),
> ad-hoc signed and not notarized — see [MACOS.md](MACOS.md).

### Architecture (shared parser, no duplication)

The parser lives in a **library** (`src/lib.rs`): pure functions `build_report(projects_dir)` and
`file_content(projects_dir, file, session)` + the structs. Two consumers reuse it with zero
duplicated logic: `src/main.rs` (the CLI, flags intact) and `gui/` (the egui desktop app, via
`arrow = { path = ".." }`). The parser ships **24 unit tests** (`cargo test`) over fixture
transcripts in a tempdir — defensive parsing, top-level transcripts only, grouping by git root,
`+/−` counting, filtering of `~/.claude/`, recency ordering, and the diff-"before" reconstruction
cascade. `gui/` adds **9 more** (`cargo test` inside it) over the ported time/focus logic and
the worktrees git plumbing. The parser **never invokes git**; all git/editor shelling lives in
`gui/src/{editor,worktrees}.rs`.

## Roadmap

- [x] **Phase 0** — native parser `JSONL → repo/session/file/diff`, terminal output and `--json`.
- [x] **Phase 1** — local UI consuming the parser: sidebar `repo → session → files` + per-file diff.
      (Originally a Vite + Svelte 5 web UI with CodeMirror; now native egui — see below.)
- [x] **Phase 2** — desktop packaging (`.deb` + AppImage + `.dmg`). The Rust parser was extracted
      into a **library** (`src/lib.rs`) shared by the CLI and the app; a `notify` watcher drives live
      refresh.
- [x] **egui rewrite** — replaced the Tauri 2.x + Svelte/CodeMirror stack with a single native egui
      binary (no webview/Node), reusing the parser's structs directly (no IPC). `web/` and `src-tauri/`
      were removed.
- [ ] **Phase 3** — honesty + git: a "git diff working tree" toggle, `userModified` marking, a
      point-in-time timeline reusing `file-history`.
- [ ] **Phase 4** (deferred) — real editing with save-to-disk, GitHub integration (PRs/commits).

Operational tracking, technical debt, and a backlog of ideas (search, stats, export, shortcuts):
see [ROADMAP.md](ROADMAP.md). The data contract lives in [SPEC.md](SPEC.md).

## Stack

Rust · [egui/eframe](https://github.com/emilk/egui) (native UI) · `syntect` + `two-face` (syntax
highlighting) · `similar` (diff) · `notify` (live refresh). No webview, no Node — one binary.

## Contributing

Contributions are welcome — bug reports, ideas, and pull requests alike. Please read
[CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test, and submit changes, and our
[Code of Conduct](CODE_OF_CONDUCT.md). To report a security issue, see our
[Security Policy](SECURITY.md). When in doubt, keep the product honest: arrow shows *edits via
Claude's tools*, never *everything Claude did*.

## License

MIT.
