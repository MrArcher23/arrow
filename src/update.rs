//! Update check — a lightweight "is there a newer release?" probe.
//!
//! arrow ships **unsigned** (no Apple notarization yet), so a silent in-app
//! auto-updater (tauri-plugin-updater) would fight Gatekeeper; that's deferred.
//! What we CAN do honestly is *tell* the user a newer version exists and point
//! them at the release. This module does exactly that and nothing more — it
//! never downloads or installs anything.
//!
//! It is an OPT-IN, secondary network layer, kept out of the parser's hot path
//! (`build_report`/`file_content` never call it) — same separation as the git
//! worktree layer. To avoid pulling an HTTP client into the core lib, it shells
//! out to `curl` (present on macOS and the Linux targets), argv-direct, with a
//! hard `--max-time`, and degrades gracefully (a missing curl, no network, a
//! rate-limit, or unparseable JSON all yield `error: Some(..)`, never a panic).

use serde::Serialize;
use serde_json::Value;
use std::process::Command;

/// GitHub repo that publishes arrow's releases (the upstream, not a fork).
const RELEASES_REPO: &str = "MrArcher23/arrow";

/// Outcome of an update check. `update_available` is only ever `true` when a
/// `latest` was fetched AND parsed AND is strictly newer than `current`.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// The running version (e.g. "0.1.6"), without a leading "v".
    pub current: String,
    /// The latest published release tag, normalized without "v". `None` when the
    /// check failed or no release exists.
    pub latest: Option<String>,
    /// Strictly-newer latest than current.
    pub update_available: bool,
    /// Release page to open / share when an update is available.
    pub url: Option<String>,
    /// Human-readable reason the check couldn't complete (network, no release,
    /// curl missing). `None` on a clean check (whether or not an update exists).
    pub error: Option<String>,
}

impl UpdateStatus {
    fn failed(current: &str, error: impl Into<String>) -> Self {
        UpdateStatus {
            current: normalize(current).to_string(),
            latest: None,
            update_available: false,
            url: None,
            error: Some(error.into()),
        }
    }
}

/// Check whether a release newer than `current` exists. Network-bound but
/// time-boxed; safe to call from a button. Never downloads or installs.
pub fn check_update(current: &str) -> UpdateStatus {
    let url = format!("https://api.github.com/repos/{RELEASES_REPO}/releases/latest");
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "6",
            "-H",
            "Accept: application/vnd.github+json",
            // GitHub rejects requests without a User-Agent.
            "-A",
            "arrow-update-check",
            &url,
        ])
        .output();

    let body = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        // curl ran but the request failed (404 = no release yet, 403 = rate
        // limited, etc.). `-f` makes curl exit non-zero on HTTP errors.
        Ok(_) => {
            return UpdateStatus::failed(current, "no published release found (or rate-limited)")
        }
        Err(_) => return UpdateStatus::failed(current, "couldn't run curl to check for updates"),
    };

    let json: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return UpdateStatus::failed(current, "couldn't parse the release response"),
    };

    let latest_tag = match json.get("tag_name").and_then(Value::as_str) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return UpdateStatus::failed(current, "the release had no tag"),
    };
    let page = json
        .get("html_url")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://github.com/{RELEASES_REPO}/releases/latest"));

    let cur = normalize(current);
    let lat = normalize(&latest_tag);
    UpdateStatus {
        current: cur.to_string(),
        update_available: is_newer(lat, cur),
        latest: Some(lat.to_string()),
        url: Some(page),
        error: None,
    }
}

/// Strip a leading "v"/"V" so "v0.1.6" and "0.1.6" compare equal.
fn normalize(v: &str) -> &str {
    v.trim().strip_prefix(['v', 'V']).unwrap_or(v.trim())
}

/// Is `a` a strictly newer semver-ish version than `b`? Compares dot-separated
/// numeric components (missing components count as 0), so "0.2.0" > "0.1.9" and
/// "0.1.6" == "0.1.6". A non-numeric component stops the comparison conservatively
/// (returns false), so a weird tag never spuriously claims an update.
fn is_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Option<Vec<u64>> {
        // Drop any pre-release/build suffix (e.g. "0.2.0-rc1" -> "0.2.0").
        let core = s.split(['-', '+']).next().unwrap_or(s);
        core.split('.').map(|p| p.parse::<u64>().ok()).collect()
    };
    match (parse(a), parse(b)) {
        (Some(av), Some(bv)) => {
            let n = av.len().max(bv.len());
            for i in 0..n {
                let x = av.get(i).copied().unwrap_or(0);
                let y = bv.get(i).copied().unwrap_or(0);
                if x != y {
                    return x > y;
                }
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("0.1.7", "0.1.6"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.6", "0.1.6"));
        assert!(!is_newer("0.1.6", "0.2.0"));
        // Padding: "0.1" == "0.1.0".
        assert!(!is_newer("0.1", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1"));
        // Pre-release suffix dropped to the core.
        assert!(is_newer("0.2.0-rc1", "0.1.9"));
        // Non-numeric junk never claims an update.
        assert!(!is_newer("nightly", "0.1.6"));
    }

    #[test]
    fn normalize_strips_v() {
        assert_eq!(normalize("v0.1.6"), "0.1.6");
        assert_eq!(normalize("0.1.6"), "0.1.6");
        assert_eq!(normalize("  v0.1.6 "), "0.1.6");
    }

    #[test]
    fn failed_status_is_honest() {
        let s = UpdateStatus::failed("v0.1.6", "boom");
        assert_eq!(s.current, "0.1.6");
        assert!(!s.update_available);
        assert!(s.latest.is_none());
        assert_eq!(s.error.as_deref(), Some("boom"));
    }
}
