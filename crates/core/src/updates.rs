//! "A new version is out": asks GitHub for LocalFlow's latest published release and
//! compares it with this build. One anonymous request to GitHub's public API; nothing
//! about the user or their automations is sent.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The public releases of LocalFlow.
pub const LATEST_RELEASE_API: &str = "https://api.github.com/repos/HoPMaLbHblu/LocalFlow/releases/latest";

/// This build's version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// "1.4.0" (without the "v").
    pub version: String,
    /// The release page, where the installers are.
    pub url: String,
    pub name: String,
    pub published_at: String,
}

/// "v1.3.0" / "1.3" / "1.3.0-beta" -> (1, 3, 0). Pre-release suffixes are ignored here;
/// GitHub's "latest" never points at a pre-release anyway.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let major = parts.next()??;
    let minor = parts.next().unwrap_or(Some(0))?;
    let patch = parts.next().unwrap_or(Some(0))?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Whether `latest` is a newer version than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Read GitHub's answer for `/releases/latest`.
pub fn parse_release(json: &serde_json::Value) -> Result<Release, String> {
    if json["draft"].as_bool() == Some(true) || json["prerelease"].as_bool() == Some(true) {
        return Err("the latest release is not final".into());
    }
    let tag = json["tag_name"].as_str().ok_or("no version in GitHub's answer")?;
    let (a, b, c) = parse_version(tag).ok_or_else(|| format!("\"{tag}\" is not a version"))?;
    let url = json["html_url"].as_str().unwrap_or("https://github.com/HoPMaLbHblu/LocalFlow/releases/latest");
    if !url.starts_with("https://github.com/") {
        return Err("unexpected release address".into());
    }
    Ok(Release {
        version: format!("{a}.{b}.{c}"),
        url: url.to_string(),
        name: json["name"].as_str().unwrap_or(tag).to_string(),
        published_at: json["published_at"].as_str().unwrap_or("").to_string(),
    })
}

/// The latest published release, from GitHub.
pub fn latest_release(timeout: Duration) -> Result<Release, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(timeout)
        .user_agent(concat!("LocalFlow/", env!("CARGO_PKG_VERSION")))
        .build();
    let response = agent
        .get(LATEST_RELEASE_API)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => "no release published yet".to_string(),
            ureq::Error::Status(403 | 429, _) => "GitHub asked to wait (rate limit); trying again later".to_string(),
            ureq::Error::Status(code, _) => format!("GitHub answered {code}"),
            ureq::Error::Transport(t) => format!("could not reach GitHub ({})", t.kind()),
        })?;
    let json: serde_json::Value = response.into_json().map_err(|e| e.to_string())?;
    parse_release(&json)
}

/// The latest release if it is newer than this build, otherwise `None`.
pub fn check(timeout: Duration) -> Result<Option<Release>, String> {
    let latest = latest_release(timeout)?;
    Ok(is_newer(&latest.version, CURRENT).then_some(latest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_not_text() {
        assert_eq!(parse_version("v1.3.0"), Some((1, 3, 0)));
        assert_eq!(parse_version("1.10"), Some((1, 10, 0)));
        assert_eq!(parse_version("2.0.1-beta.2"), Some((2, 0, 1)));
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert!(is_newer("v1.10.0", "1.9.9"));
        assert!(is_newer("1.3.1", "1.3.0"));
        assert!(!is_newer("1.3.0", "1.3.0"));
        assert!(!is_newer("1.2.9", "1.3.0"));
        assert!(!is_newer("nonsense", "1.3.0"));
    }

    #[test]
    fn github_answers_are_read_carefully() {
        let json = serde_json::json!({
            "tag_name": "v1.4.0", "name": "LocalFlow v1.4.0", "draft": false, "prerelease": false,
            "html_url": "https://github.com/HoPMaLbHblu/LocalFlow/releases/tag/v1.4.0", "published_at": "2026-10-10T10:00:00Z"
        });
        let r = parse_release(&json).unwrap();
        assert_eq!((r.version.as_str(), r.name.as_str()), ("1.4.0", "LocalFlow v1.4.0"));
        assert!(r.url.ends_with("/v1.4.0"));

        let mut pre = json.clone();
        pre["prerelease"] = true.into();
        assert!(parse_release(&pre).is_err());
        let mut odd = json.clone();
        odd["html_url"] = "https://example.com/evil".into();
        assert!(parse_release(&odd).is_err(), "only GitHub release pages are opened");
        assert!(parse_release(&serde_json::json!({})).is_err());
    }
}
