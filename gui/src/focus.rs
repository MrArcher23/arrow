//! Time helpers ported from the web's `time.ts`: "live" detection, the active-session
//! focus window, relative times ("Nm ago"), and date buckets for the history view.
//!
//! All strings returned here are UI text → English (arrow is an English-branded app).
//! Timestamps come from the parser as RFC3339 ISO strings (`Option<String>`); we parse
//! them to epoch-millis so we can compare against a `now` the caller passes in. `now` is
//! a parameter (not read from the clock here) so the UI can drive it from a 30s tick:
//! that way "Nm ago" labels and the green dot keep aging — and expire after LIVE_WINDOW —
//! even when the report itself hasn't changed (see the clock tick in `main.rs`).

use arrow::RepoOut;
use chrono::{DateTime, Local, TimeZone, Utc};

const MIN: i64 = 60_000;
const HOUR: i64 = 3_600_000;
const DAY: i64 = 86_400_000;

/// Work is "live" (a recent edit) within this window. Drives the green dot and the
/// initial auto-open; past it, arrow shows the idle / "No active work" state.
pub const LIVE_WINDOW: i64 = 20 * MIN;
/// "Burst" window: a repo joins the focus if its last activity is within this of the
/// active session's. Anchored to the DATA, not the clock, so focus doesn't age between
/// refreshes. Also the active/stale cutoff for worktrees (recent EDIT activity on disk,
/// not a running process — same caveat as the green dot).
pub const BURST_WINDOW: i64 = 10 * MIN;
/// The active/stale cutoff for worktrees (recent edit activity on disk).
pub const STALE_AFTER: i64 = BURST_WINDOW;

/// Wall-clock now in epoch-millis (UTC). The UI samples this each frame / on a tick.
pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// Parse an RFC3339 ISO timestamp to epoch-millis; `None` if absent or unparseable.
fn parse_ms(iso: Option<&str>) -> Option<i64> {
    DateTime::parse_from_rfc3339(iso?)
        .ok()
        .map(|d| d.timestamp_millis())
}

pub fn is_live(iso: Option<&str>, now: i64) -> bool {
    parse_ms(iso).is_some_and(|t| now - t < LIVE_WINDOW)
}

/// A worktree is "active" if a file under it was edited within STALE_AFTER. Honest: recent
/// EDIT activity on disk, not a running process.
pub fn is_active(iso: Option<&str>, now: i64) -> bool {
    parse_ms(iso).is_some_and(|t| now - t < STALE_AFTER)
}

/// Indices (into `repos`) of the repos in focus: the active session's repo (globally most
/// recent activity = `repos[0]`, by the parser's ordering) plus any repo touched within
/// BURST_WINDOW of it. Returns indices instead of references so the caller can render
/// against the owned report without lifetime juggling.
///
/// IDLE: if the most recent activity is older than LIVE_WINDOW (e.g. opening arrow the next
/// day, not working), there is no active work → returns `[]` (no focus, no green dot, no
/// auto-open). Anchored to the data timestamp via the `now` passed in, so it crosses into
/// idle exactly when it should.
pub fn focus_repos(repos: &[RepoOut], now: i64) -> Vec<usize> {
    let top = repos
        .first()
        .and_then(|r| r.sessions.first())
        .and_then(|s| s.last_activity.as_deref());
    let t0 = match parse_ms(top) {
        Some(t) => t,
        None => return Vec::new(),
    };
    if now - t0 >= LIVE_WINDOW {
        return Vec::new(); // idle: nothing recent to focus
    }
    repos
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let la = r.sessions.first().and_then(|s| s.last_activity.as_deref());
            match parse_ms(la) {
                Some(t) if t0 - t <= BURST_WINDOW => Some(i),
                _ => None,
            }
        })
        .collect()
}

/// "now" / "5m ago" / "3h ago" / "2d ago", then a short calendar date past 30 days.
pub fn relative(iso: Option<&str>, now: i64) -> String {
    let t = match parse_ms(iso) {
        Some(t) => t,
        None => return String::new(),
    };
    let d = now - t;
    if d < MIN {
        return "now".into();
    }
    if d < HOUR {
        return format!("{}m ago", d / MIN);
    }
    if d < DAY {
        return format!("{}h ago", d / HOUR);
    }
    let days = d / DAY;
    if days < 30 {
        return format!("{days}d ago");
    }
    // Older than 30 days: a short local calendar date, e.g. "5 Jun".
    match Utc.timestamp_millis_opt(t).single() {
        Some(dt) => dt.with_timezone(&Local).format("%-d %b").to_string(),
        None => String::new(),
    }
}

/// History buckets, in display order. "No date" collects records without a timestamp.
pub const BUCKET_ORDER: [&str; 5] = ["Today", "Yesterday", "This week", "Older", "No date"];

/// Which history bucket a timestamp falls into, relative to local midnight today.
pub fn date_bucket(iso: Option<&str>) -> &'static str {
    let t = match parse_ms(iso) {
        Some(t) => t,
        None => return "No date",
    };
    let start_today = match Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| naive.and_local_timezone(Local).single())
    {
        Some(dt) => dt.timestamp_millis(),
        None => return "No date",
    };
    if t >= start_today {
        "Today"
    } else if t >= start_today - DAY {
        "Yesterday"
    } else if t >= start_today - 6 * DAY {
        "This week"
    } else {
        "Older"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A fixed "now" so the windows are deterministic.
    const NOW: i64 = 1_000_000_000_000; // arbitrary epoch-millis

    fn iso(ms_ago: i64) -> String {
        Utc.timestamp_millis_opt(NOW - ms_ago)
            .single()
            .unwrap()
            .to_rfc3339()
    }

    #[test]
    fn live_within_window_only() {
        assert!(is_live(Some(&iso(5 * MIN)), NOW));
        assert!(!is_live(Some(&iso(21 * MIN)), NOW));
        assert!(!is_live(None, NOW));
        assert!(!is_live(Some("not a date"), NOW));
    }

    #[test]
    fn active_uses_burst_window() {
        assert!(is_active(Some(&iso(5 * MIN)), NOW));
        assert!(!is_active(Some(&iso(11 * MIN)), NOW));
    }

    #[test]
    fn relative_buckets() {
        assert_eq!(relative(Some(&iso(30_000)), NOW), "now");
        assert_eq!(relative(Some(&iso(5 * MIN)), NOW), "5m ago");
        assert_eq!(relative(Some(&iso(3 * HOUR)), NOW), "3h ago");
        assert_eq!(relative(Some(&iso(2 * DAY)), NOW), "2d ago");
        assert_eq!(relative(None, NOW), "");
    }

    #[test]
    fn focus_empty_when_idle() {
        // Build a tiny report whose newest activity is older than LIVE_WINDOW.
        let repos = vec![mk_repo(iso(30 * MIN))];
        assert!(focus_repos(&repos, NOW).is_empty());
    }

    #[test]
    fn focus_includes_burst_excludes_old() {
        let repos = vec![
            mk_repo(iso(MIN)),      // active session (top)
            mk_repo(iso(8 * MIN)),  // within burst
            mk_repo(iso(30 * MIN)), // outside burst
        ];
        assert_eq!(focus_repos(&repos, NOW), vec![0, 1]);
    }

    fn mk_repo(last_activity: String) -> RepoOut {
        use arrow::SessionOut;
        RepoOut {
            cwd: "/tmp/repo".into(),
            git_branch: None,
            sessions: vec![SessionOut {
                session_id: "s".into(),
                title: None,
                last_prompt: None,
                first_activity: Some(last_activity.clone()),
                last_activity: Some(last_activity),
                file_count: 0,
                files: vec![],
            }],
        }
    }
}
