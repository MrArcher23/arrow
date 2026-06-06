# Contributing to arrow

Thanks for taking the time to contribute! arrow is a small, focused tool — a
lightweight desktop **audit viewer for Claude Code** that answers one question
reliably: *which files did Claude Code touch, in which repo, with what diff, in
which session?* No AI chat, no dependency on git or hooks.

Contributions of all kinds are welcome: bug reports, fixes, docs, and features
from the roadmap or backlog. Before you start, please skim this guide — it
covers the project layout, how to build and test, the code style, and one
non-negotiable constraint: **the honesty principle**.

By participating, you agree to abide by our
[Code of Conduct](CODE_OF_CONDUCT.md).

## Project layout

The Rust parser lives in a **library** so the CLI and the desktop app share the
same source of truth — zero duplicated logic. The app is native **egui/eframe**
(no webview, no Node): it consumes the parser's structs directly.

| Path | What it is |
|---|---|
| `src/lib.rs` | The parser **library**: pure functions `build_report(projects_dir)` and `file_content(projects_dir, file, session)` plus the structs. This is where the data-model logic lives — read it before touching the parser. |
| `src/main.rs` | The **CLI** (`arrow`), a thin frontend over the library (`--list`, `--repo`, `--session`, `--json`, `--content`). |
| `gui/` | The **egui/eframe desktop app**. It calls the library directly (no IPC, no JSON). **It is its own Cargo workspace root** — `cargo build` at the repo root does *not* pull it in. |
| `gui/src/{main,sidebar,diff,worktrees_modal}.rs` | The UI: app state + the sidebar tree, the side-by-side diff, and the worktrees modal. |
| `gui/src/{worker,focus,theme,editor,worktrees,sys}.rs` | The off-thread worker (parsing/IO), ported time/focus logic, themes, and the git/editor shelling (`editor.rs`/`worktrees.rs` — the parser stays git-free). |

The root crate `arrow` is pure Rust (serde, walkdir, clap, anyhow) with **no
system dependencies**; only the egui app needs the build libs below.

## Prerequisites

- **Rust stable** via [rustup](https://rustup.rs/). (If `cargo` isn't on your
  `PATH`, it's usually at `~/.cargo/bin/cargo`.)

To build or run the **desktop app** you additionally need egui/eframe's build
deps. On Linux (Debian/Ubuntu/Pop!\_OS), once — note: **no WebKitGTK or Node**:

```bash
sudo apt install -y libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev \
                    libxcb-xfixes0-dev libxkbcommon-dev libssl-dev
```

## Build, run, test

### Parser / CLI

```bash
cargo build --release          # binary at target/release/arrow
cargo test --release           # parser unit tests (in src/lib.rs, src/update.rs)
```

Try the CLI against your own data:

```bash
./target/release/arrow --list                  # all repos -> sessions -> files (+/-)
./target/release/arrow --repo my-project        # full diff for one repo
./target/release/arrow --session 14385fed       # one session by id prefix
./target/release/arrow --repo my-project --json # normalized JSON (the data contract)
```

### Desktop app (egui)

`gui/` is its own workspace, so run cargo inside it:

```bash
cargo run   --manifest-path gui/Cargo.toml            # dev run (native window)
cargo run   --release --manifest-path gui/Cargo.toml  # release run
cargo test  --release --manifest-path gui/Cargo.toml  # GUI unit tests
```

The app reuses the parser via `arrow = { path = ".." }`; editing `src/lib.rs`
is picked up automatically on the next `arrow-gui` build.

## Code style

**Rust** (run in **both** crates — root and `gui/`):

```bash
cargo fmt              # format
cargo clippy          # lint
```

Keep the tree `rustfmt`-clean and clippy-clean — **CI treats clippy warnings as
errors** (`-D warnings`). Maintainers also have a `/rust-review` Claude Code
skill (clippy + rustfmt + an arrow-tuned best-practices checklist).

**Language conventions** (going forward):

- **Code comments in English**, identifiers in English. Legacy Spanish comments
  migrate gradually — there's no need to rewrite them all at once.
- **All user-visible UI text in English** (labels, states, tooltips, banners) —
  arrow is an English-branded app. UI strings live in the `gui/src/`
  modules and in `gui/src/focus.rs` (relative times / date buckets).
- **Keep IO and git shelling off the parser.** Parsing/disk reads run on the
  worker thread (`gui/src/worker.rs`); all git/editor shelling lives in
  `gui/src/{worktrees,editor}.rs`, never in `src/lib.rs`.

## The honesty principle

**This is a core, non-negotiable design constraint.** arrow must never claim more
than the data actually knows. A PR that breaks this will be asked to change,
regardless of how nice the feature is otherwise.

- arrow only captures edits that go through **`Edit` / `Write` / `MultiEdit`**.
  Changes made via the session's **`Bash`** commands (sed, prettier, build,
  `mv`, `rm`) are **not** captured. The correct label is always *"edits via
  Claude's tools"*, **never** *"everything Claude did"*.
- The **`userModified`** flag signals drift (the user edited a file between
  Claude's read and write). Surface it with a ⚠.
- **"Active session" means most recent activity *on disk*, not a running
  process.** arrow cannot know whether a Claude process is alive; don't word
  anything as if it could.
- `~/.claude/` (the global Claude HOME) is **internal bookkeeping and is filtered
  out** — it is not the user's code. A `.claude/` *inside* a repo (project
  settings/skills) **is** shown. Filter by the HOME prefix, not by the substring
  `/.claude/`.
- `git diff` is only an optional **secondary** view (whole working tree, git
  repos only); it does not attribute changes by session or author.

If a wording or feature would mislead a user about what was captured, it doesn't
ship. When in doubt, under-claim.

## Verifying parser changes

The transcript format is **internal, undocumented, changes between Claude Code
versions, and auto-deletes at ~30 days** (`cleanupPeriodDays`). Two rules follow
from that:

1. **Defensive parsing is mandatory.** Parse to `serde_json::Value`, not rigid
   structs. An invalid line is **skipped, never allowed to crash** the run.
2. **Verify every parser change against real data before claiming it works.**
   - Run `cargo test --release` (the unit tests use transcript fixtures in a
     tempdir).
   - Run the CLI against your own `~/.claude/projects` (e.g.
     `./target/release/arrow --list`) and **confirm the output is correct** —
     report the actual output, not just an assertion that it works.

Maintainers also have a `/verify-parser` Claude Code skill that recompiles and
runs the parser against `~/.claude` as a pass/fail check. The skill complements,
but does not replace, the unit tests.

The data model has subtleties worth knowing before you touch the parser (touched
files come from `toolUseResult.filePath`; the diff from
`toolUseResult.structuredPatch`; a repo is the **git root** of the session's
`cwd`; only top-level transcripts count, nested subagent `.jsonl` files are
ignored). See `CLAUDE.md` and `SPEC.md` for the full picture.

## Branching model

arrow follows a simple **trunk-based / GitHub Flow** model — there are no long-lived
`develop`, `feature`, or `hotfix` branches kept around. `main` is always the source of
truth and is protected: changes land via pull requests with green CI.

For each change, branch off `main`, open a PR, and **delete the branch once it's merged**.
Name the branch with a type prefix so its intent is obvious:

| Prefix | For | Example |
|---|---|---|
| `feat/` | a new feature | `feat/sidebar-search` |
| `fix/` | a bug fix | `fix/diff-scroll` |
| `hotfix/` | an urgent fix on top of a release | `hotfix/crash-on-empty-session` |
| `docs/` | documentation only | `docs/readme-screenshots` |
| `refactor/` | internal cleanup, no behavior change | `refactor/extract-parser` |
| `chore/` or `ci/` | tooling, dependencies, CI | `ci/cache-cargo` |

Keep branches small and short-lived; if `main` moves under you, rebase onto it before
merging. (Maintainers may push small, low-risk changes straight to `main` thanks to the
admin bypass, but branch + PR is the default for everything non-trivial.)

## Pull request workflow

1. **Fork the repo**, or create a short-lived branch off `main` if you have push access
   (see the branch-naming convention above). Don't commit directly to `main`.
2. Make **small, focused commits** with messages in the **imperative mood**
   (e.g. "Add sidebar filter", not "Added" / "Adds").
3. Before opening the PR, make sure the checks below pass locally — they're the
   same ones CI runs:

   ```bash
   # root crate (parser/CLI)
   cargo fmt --all --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --release
   # egui app (its own workspace)
   (cd gui && cargo fmt --all --check && \
      cargo clippy --all-targets -- -D warnings && cargo test --release)
   ```

   CI runs the same two jobs (root crate + `arrow-gui` crate) on Rust stable.
4. **Open a PR against `main`** and fill out the PR template. Describe what
   changed and, for parser changes, paste the real-data output you verified.
5. **For non-trivial features, open an issue first** to discuss the approach —
   especially anything that touches the **data model** or the **honesty
   principle**. It saves everyone a round-trip.

## Where to start

- The roadmap and an unscoped backlog of ideas live in
  [ROADMAP.md](ROADMAP.md). Good candidates right now:
  - **Phase 3** — honesty + git: a "git diff working tree" toggle, `userModified`
    marking, a point-in-time timeline reusing `file-history`.
  - **Phase 4** (deferred) — real editing with save-to-disk, GitHub integration.
  - **Backlog (frontend-friendly)** — search/filter in the sidebar, change stats
    (`+/-` totals per repo/session/day), export (copy a diff, or a session
    summary to Markdown), and keyboard navigation of the tree.
- Check the issues labeled **`good first issue`** and **`help wanted`** on
  GitHub for scoped, ready-to-pick work.

Welcome aboard, and thanks again for helping make arrow better.
