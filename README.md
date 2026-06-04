# arrow

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-000?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white)](https://tauri.app/)
[![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte&logoColor=white)](https://svelte.dev/)
[![CI](https://github.com/MrArcher23/arrow/actions/workflows/ci.yml/badge.svg)](https://github.com/MrArcher23/arrow/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/MrArcher23/arrow?sort=semver)](https://github.com/MrArcher23/arrow/releases/latest)
[![Platform: Linux](https://img.shields.io/badge/platform-Linux-FCC624?logo=linux&logoColor=black)](#install-linux)
[![Platform: macOS](https://img.shields.io/badge/platform-macOS-000?logo=apple&logoColor=white)](#install-macos)

**An audit viewer for Claude Code.** It answers a single question, reliably:
*which files did Claude touch, in which repo, with what diff, and in which session?* — without
opening an IDE, without AI chat, and **without depending on git or hooks**.

> Status: **Phase 2 complete + polish — released on Linux
> ([v0.1.4](https://github.com/MrArcher23/arrow/releases/latest))**. A desktop app (Tauri 2.x) with
> the Rust parser as its native backend, **already usable day to day** on Linux (`.deb` + AppImage).
> Phases 0 (parser/CLI), 1 (web UI) and 2 (packaging) are complete; since then: 20 parser tests, a
> resilient watcher, zoom, a custom titlebar, active-session focus, live diff-panel refresh,
> open-in-editor (jump from a diff straight into your editor at the first change), files
> ordered by most-recent edit (with relative times that age in place via a clock tick), macOS
> adaptation, and a robust diff-"before" reconstruction (shows a real diff even on Claude Code's
> `originalFile: null` edits, instead of mislabeling an edited file as new). Phases 3 (honesty + git)
> and 4 (editing) are pending — details in [ROADMAP.md](ROADMAP.md).

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
> **uninstall**, `rm -rf /Applications/arrow.app`. (There is no auto-update / package manager yet —
> a Homebrew Cask is on the [roadmap](ROADMAP.md).)

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
```

Options: `--projects-dir <path>` (defaults to `~/.claude/projects`), `--repo`, `--session`,
`--list`, `--json`, and `--content --file <path> [--session <id>]` (emits `{before, after}` for a
file, for the UI's diff view).

## Web UI (Phase 1)

A Vite + Svelte 5 app that consumes the parser through a local dev-server which runs the `arrow`
binary (in `web/vite.config.ts`). The diff view uses **CodeMirror 6 + `@codemirror/merge`**.

```bash
cargo build --release          # the dev-server runs target/release/arrow
cd web && npm install
npm run dev                     # http://localhost:5173
```

Layout: a sidebar `repo → session → touched files` (with `+/-` and ⚠ `userModified`), and a central
panel with the before/after diff of the selected file. Total frontend weight: ~93 KB gzip.

## Desktop app (Phase 2)

The same UI, packaged in **Tauri 2.x**: the Rust parser is the **native backend** (no sidecar, no
HTTP server). The Svelte UI is reused as-is; only its transport layer (`web/src/lib/api.ts`) detects
the environment and uses Tauri `invoke()` inside the app or `fetch` in the browser — so `npm run
dev` keeps working for fast iteration.

```bash
# system requirements (Linux/Debian/Ubuntu/Pop!_OS), once:
sudo apt install -y libwebkit2gtk-4.1-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev
cargo install tauri-cli --version "^2"     # Tauri CLI

# development: native window with frontend hot-reload
cargo tauri dev      # (from the repo root)

# installer: .deb + AppImage in src-tauri/target/release/bundle/
cargo tauri build
```

> **macOS:** the app adapts to the OS on its own (native titlebar with traffic lights + correct font
> weight). To build the `.app`/`.dmg` — which can only be done on a Mac — see [MACOS.md](MACOS.md).

- **Native backend** (`src-tauri/`): two `invoke` commands — `report()` and `content(file, session)`
  — that wrap the parser library (`arrow = { path = ".." }`, see Architecture). The AppImage runs
  standalone, reading `~/.claude/projects` directly from Rust.
- **Lightweight footprint**: Tauri uses the **system's native webview** (WebKitGTK on Linux), it
  does not bundle a Chromium like Electron — so the app lives on the order of **~200 MB of RAM (PSS)
  in use**, a fraction of an equivalent Electron desktop client (which usually runs into several
  GB). The installer weighs **~77 MB** (AppImage) / **~2 MB** (`.deb`).
- **Native live refresh**: a `notify` watcher over `~/.claude/projects` emits a `report-changed`
  event (debounced) and the UI refreshes instantly; a slow polling fallback is kept as a backstop.
- **Active-session focus**: the repo(s) of the **active session** (the one with the most recent
  activity) are shown at the top, plus any repo touched in the same burst (~10 min, `BURST_WINDOW`
  in `web/src/lib/time.ts`); the rest sink into *Other repos* as they age relative to the active
  one. The **green dot** marks those focused repos with recent activity (the redundant `live` text
  badge was removed). Honest: "active session" = *most recent activity on disk*, not *a running
  process* (arrow cannot know the latter).
- **Open in editor**: a button in the file bar opens the selected file in your own editor at the
  first changed line. It detects installed editors (the VS Code family — incl. Cursor, Windsurf,
  Kiro, Antigravity — plus Zed, JetBrains, Sublime, Kate) and **delegates to the editor's CLI**;
  arrow never embeds an editor or a language server (the mirror image of Claude Code's `/ide`).
  Honest: it opens the **current on-disk file** (and is disabled when that file is gone), never the
  reconstructed snapshot.
- **Window and zoom**: a custom titlebar (`decorations:false`) with minimize/maximize/close buttons,
  drag, and double-click to maximize — guaranteeing *cross-distro* controls (on GNOME/Pop!_OS the WM
  doesn't paint them reliably). VSCode-style UI zoom with `Ctrl +/−/0`: native to the webview
  (`setZoom`, so it doesn't throw off CodeMirror), persistent across sessions.
- **Linux/WebKitGTK**: the app sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` in its `main()` (avoids the
  white screen caused by DMABUF/NVIDIA); the `font-weight` (+100) bug is already compensated in
  `web/src/app.css` (`font-weight: 350`).

### Architecture (shared parser, no duplication)

The parser lives in a **library** (`src/lib.rs`): pure functions `build_report(projects_dir)` and
`file_content(projects_dir, file, session)` + the serializable structs. Two frontends consume it:
`src/main.rs` (the CLI, with flags intact) and `src-tauri/` (the desktop backend). Zero duplicated
logic; the same source of truth for terminal, web, and native app. The parser ships **20 unit tests**
(`cargo test`) over fixture transcripts in a tempdir, covering the non-obvious parts: defensive
parsing, top-level transcripts only, grouping by git root, `+/−` counting, filtering of
`~/.claude/`, recency ordering (repos, sessions, and files within a session), and the diff-"before" reconstruction cascade (inline `originalFile`,
reverse-applied patches, full-`Read` snapshot, create → new file, and drift → honestly unavailable).

## Roadmap

- [x] **Phase 0** — native parser `JSONL → repo/session/file/diff`, terminal output and `--json`.
- [x] **Phase 1** — local web UI (Vite + Svelte 5) consuming the parser: sidebar
      `repo → session → files` (Antigravity-style) + per-file diff with
      **CodeMirror 6 + `@codemirror/merge`**. (Read-only; editing is Phase 4.)
- [x] **Phase 2** — packaging in **Tauri 2.x** (`.deb` + AppImage). The Rust parser was extracted
      into a **library** (`src/lib.rs`) and is the native backend (no sidecar); the CLI and the app
      share it. Dual-mode frontend (`invoke`/`fetch`), a `notify` watcher → `report-changed` event
      for live refresh, and WebKitGTK mitigation. (The sidebar focus was later refined to
      "active session + ~10 min burst" with the green dot as the sole indicator; see ROADMAP.)
- [ ] **Phase 3** — honesty + git: a "git diff working tree" toggle, `userModified` marking, a
      point-in-time timeline reusing `file-history`.
- [ ] **Phase 4** (deferred) — real editing with save-to-disk, GitHub integration (PRs/commits).

Operational tracking, technical debt, and a backlog of ideas (search, stats, export, shortcuts):
see [ROADMAP.md](ROADMAP.md). The data contract lives in [SPEC.md](SPEC.md).

## Stack

Rust (parser/backend) · CodeMirror 6 (UI, Phase 1+) · Tauri 2.x (packaging, Phase 2+).
On Linux/WebKitGTK you'll need to apply `WEBKIT_DISABLE_DMABUF_RENDERER=1` and compensate for the
`font-weight` (+100) bug — both documented for Phase 2.

## Contributing

Contributions are welcome — bug reports, ideas, and pull requests alike. Please read
[CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test, and submit changes, and our
[Code of Conduct](CODE_OF_CONDUCT.md). To report a security issue, see our
[Security Policy](SECURITY.md). When in doubt, keep the product honest: arrow shows *edits via
Claude's tools*, never *everything Claude did*.

## License

MIT.
