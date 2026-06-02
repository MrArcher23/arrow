# Security Policy

arrow is a local, read-only audit viewer for Claude Code. This policy explains which
versions get security fixes, how to report a vulnerability privately, and what is in or
out of scope.

## Supported versions

arrow is pre-1.0 software (`0.x`). The API, data contract, and internals may change
between releases. Security fixes target only the **latest release** and the current
`main` branch. Older `0.x` tags do not receive backported fixes — please update to the
latest version before reporting.

| Version | Supported |
|---|---|
| Latest release / `main` | ✅ |
| Any earlier `0.x` | ❌ (please update) |

## Reporting a vulnerability

**Please do not open a public issue for security problems.** A public issue discloses the
problem before there is a fix.

Report privately through one of these channels:

1. **Preferred — GitHub Private Vulnerability Reporting.** On the repository, go to the
   **Security** tab → **Report a vulnerability**. This keeps the report private and
   tracked. (If the option is not visible, it isn't enabled yet — use email instead.)
2. **Email** — <rgh2309@gmail.com>. Please include `arrow security` in the subject.

When reporting, it helps to include:

- A description of the issue and why you believe it is a security problem.
- Steps to reproduce, or a minimal proof of concept.
- The arrow version (or commit) and your OS.

### What to expect

arrow is a hobby project maintained in spare time, so responses are **best-effort**. A
realistic expectation is an initial acknowledgment within about a week, with follow-up as
time allows. Please report privately first and give a reasonable window to address the
issue before any public disclosure. Thank you for helping keep the project safe.

## Scope and threat model

Knowing what arrow does — and does not do — helps frame what counts as a vulnerability.

arrow is a **local, read-only** desktop viewer. It:

- Reads Claude Code's native transcripts from `~/.claude/projects` (and, where used,
  `~/.claude/file-history`). These files can contain **sensitive data** — source code,
  prompts, file paths, and possibly secrets that appeared in your sessions.
- Has **no network backend** and does not phone home. There is no telemetry and no remote
  server: the Tauri app talks only to the local Rust parser.
- Does **not write to your files**. As of the current release (Phase 2) arrow is
  read-only; the data it reads on disk is owned by Claude Code, not by arrow. Editing with
  save-to-disk is a planned future phase (see [ROADMAP.md](ROADMAP.md)); this policy will be
  updated when that ships.

Because of that, the relevant concerns are mostly **local**:

- **Defensive parsing of untrusted JSONL.** The transcript format is internal,
  undocumented, and changes between versions. Parsing it is already a design rule: an
  invalid or malformed line is skipped, never crashing the parser. A parsing flaw that
  breaks this (e.g. a crafted transcript causing a crash, hang, or unsafe behavior) is
  in scope.
- **Rendering file contents in a webview.** The desktop app displays file and diff
  contents (which originate from your transcripts) inside a system webview. Issues where
  that rendering path could be abused are in scope.

### Out of scope

- Vulnerabilities in **Claude Code itself**, or in the transcript data it writes —
  report those to Anthropic.
- The contents of `~/.claude` data that you do not control, or secrets that ended up in
  your own transcripts. arrow only reads what is already on your disk; protecting that
  directory is the responsibility of the OS user account and of Claude Code. (arrow
  filters out the global `~/.claude/` bookkeeping but still reads transcripts that may
  contain sensitive content.)
- Issues that require an attacker who already has full local access to your user account
  (at that point they can read `~/.claude` directly, without arrow).
- Vulnerabilities in third-party dependencies that have no exploitable impact in arrow —
  please point to a concrete attack path within arrow.

If you are unsure whether something is in scope, report it privately anyway and we'll
figure it out together.
