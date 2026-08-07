//! "Check for updates" against GitHub Releases.
//!
//! One anonymous GET to the release list, filtered to stable `gui-v` tags and
//! compared version-wise against `CARGO_PKG_VERSION`. The request runs on a detached thread; the About
//! dialog polls the receiver each frame. Every failure path degrades
//! gracefully: a 404 (nothing published yet) counts as up to date, network
//! errors surface as a gentle "couldn't check" line, and nothing panics.

use std::sync::mpsc::{Receiver, channel};

/// The human-facing releases page (fallback link target).
pub const RELEASES_URL: &str = "https://github.com/maelic13/colosseum/releases";
const API_URL: &str = "https://api.github.com/repos/maelic13/colosseum/releases?per_page=100";

/// Outcome of one update check.
pub enum UpdateStatus {
    UpToDate,
    UpdateAvailable {
        /// Version string without the `v` prefix, e.g. `1.1.0`.
        version: String,
        /// The release page to open in the browser.
        url: String,
    },
    /// The check itself failed (offline, GitHub unreachable…).
    Failed,
}

/// A running background check; poll it each frame until it yields.
pub struct UpdateCheck {
    rx: Receiver<UpdateStatus>,
}

impl UpdateCheck {
    /// Spawn the check on a detached thread and return the handle to poll.
    #[must_use]
    pub fn start() -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(check());
        });
        Self { rx }
    }

    /// The result, once the background thread has one.
    #[must_use]
    pub fn poll(&self) -> Option<UpdateStatus> {
        self.rx.try_recv().ok()
    }
}

fn check() -> UpdateStatus {
    match fetch_latest() {
        Ok(Some((tag, url))) => {
            let latest = gui_release_version(&tag);
            let current = parse_version(env!("CARGO_PKG_VERSION"));
            match (latest, current) {
                (Some(l), Some(c)) if l > c => UpdateStatus::UpdateAvailable {
                    version: tag
                        .strip_prefix("gui-v")
                        .or_else(|| tag.strip_prefix('v'))
                        .unwrap_or(&tag)
                        .to_string(),
                    url,
                },
                // Equal, older (dev build ahead of the release), or an
                // unparseable tag all read as "nothing newer out there".
                _ => UpdateStatus::UpToDate,
            }
        }
        // No release published yet — the current build is all there is.
        Ok(None) => UpdateStatus::UpToDate,
        Err(_) => UpdateStatus::Failed,
    }
}

/// `Ok(Some((tag_name, html_url)))` for the latest release, `Ok(None)` when
/// the repository has no releases (404).
fn fetch_latest() -> Result<Option<(String, String)>, Box<dyn std::error::Error>> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build();
    let agent: ureq::Agent = config.into();
    let response = agent
        .get(API_URL)
        .header(
            "User-Agent",
            concat!("colosseum/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/vnd.github+json")
        .call();
    let mut response = match response {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let body = response.body_mut().read_to_string()?;
    let releases: serde_json::Value = serde_json::from_str(&body)?;
    let releases = releases
        .as_array()
        .ok_or("release API did not return an array")?;
    Ok(select_latest_gui_release(releases))
}

fn select_latest_gui_release(releases: &[serde_json::Value]) -> Option<(String, String)> {
    releases
        .iter()
        .filter(|release| {
            !release
                .get("draft")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .filter_map(|release| {
            let tag = release.get("tag_name")?.as_str()?;
            let version = gui_release_version(tag)?;
            let url = release
                .get("html_url")
                .and_then(|value| value.as_str())
                .unwrap_or(RELEASES_URL);
            Some((version, tag.to_owned(), url.to_owned()))
        })
        .max_by_key(|(version, _, _)| *version)
        .map(|(_, tag, url)| (tag, url))
}

fn gui_release_version(tag: &str) -> Option<(u64, u64, u64)> {
    if let Some(version) = tag.strip_prefix("gui-v") {
        if version.contains('-') || version.contains('+') {
            return None;
        }
        return parse_version(version);
    }
    let legacy = tag.strip_prefix('v')?;
    let version = parse_version(legacy)?;
    (version <= (1, 0, 2)).then_some(version)
}

/// Parse `v1.2.3` / `1.2.3` (pre-release/build suffixes ignored) into a
/// comparable triple.
fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags_with_and_without_prefix() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.0.0"), Some((1, 0, 0)));
        assert_eq!(parse_version("v2.0.0-rc.1"), Some((2, 0, 0)));
        assert_eq!(parse_version("v1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("nightly"), None);
    }

    #[test]
    fn ordering_matches_semver() {
        assert!(parse_version("v1.0.1") > parse_version("v1.0.0"));
        assert!(parse_version("v1.10.0") > parse_version("v1.9.9"));
        assert!(parse_version("v1.0.0") == parse_version("1.0.0"));
    }

    #[test]
    fn selects_only_stable_gui_lane_with_bounded_legacy_fallback() {
        let releases = serde_json::json!([
            {"tag_name":"cli-v9.0.0","html_url":"cli"},
            {"tag_name":"gui-v1.2.0-rc.1","html_url":"rc"},
            {"tag_name":"v8.0.0","html_url":"unscoped"},
            {"tag_name":"gui-v1.1.0","html_url":"new"},
            {"tag_name":"v1.0.2","html_url":"legacy"},
            {"tag_name":"gui-v2.0.0","html_url":"draft","draft":true}
        ]);
        assert_eq!(
            select_latest_gui_release(releases.as_array().unwrap()),
            Some(("gui-v1.1.0".into(), "new".into()))
        );
    }
}
