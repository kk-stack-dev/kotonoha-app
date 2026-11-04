//! Update check rules: comparing the running version with the latest published release.
//!
//! The app never updates itself. A person asks for the check, and a newer version only leads them to the
//! release page. The network call lives in the platform crate; this module is pure.

use serde::Serialize;

/// Where releases are published, and the page a person is sent to for a new version.
pub const RELEASES_PAGE: &str = "https://github.com/kk-stack-dev/kotonoha-app/releases/latest";

/// A `major.minor.patch` version. Anything after `-` or `+` (pre-release, build) is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u64, u64, u64);

impl Version {
    /// Parses `1.2.3`, `v1.2.3`, `1.2` and `1.2.3-rc1`. Returns `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let core = text.split(['-', '+']).next()?;
        let numbers = core
            .split('.')
            .map(|p| if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) { p.parse::<u64>().ok() } else { None })
            .collect::<Option<Vec<_>>>()?;
        match numbers[..] {
            [major] => Some(Self(major, 0, 0)),
            [major, minor] => Some(Self(major, minor, 0)),
            [major, minor, patch] => Some(Self(major, minor, patch)),
            _ => None,
        }
    }
}

/// What the check found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateStatus {
    UpToDate,
    Available { latest: String },
}

/// Compares the running version with a release tag such as `v1.0.2`. `None` when either is unreadable.
pub fn judge(current: &str, latest_tag: &str) -> Option<UpdateStatus> {
    let current = Version::parse(current)?;
    let latest = Version::parse(latest_tag)?;
    Some(if latest > current {
        UpdateStatus::Available { latest: latest_tag.trim().trim_start_matches('v').to_string() }
    } else {
        UpdateStatus::UpToDate
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_forms() {
        assert_eq!(Version::parse("1.0.1"), Some(Version(1, 0, 1)));
        assert_eq!(Version::parse("v1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse("1.2"), Some(Version(1, 2, 0)));
        assert_eq!(Version::parse("1.2.3-rc1"), Some(Version(1, 2, 3)));
    }

    #[test]
    fn rejects_garbage() {
        for text in ["", "v", "abc", "1.x.3", "1..3", "1.2.3.4", "tools-2024"] {
            assert_eq!(Version::parse(text), None, "{text}");
        }
    }

    #[test]
    fn compares_numerically_not_textually() {
        assert!(Version::parse("1.10.0") > Version::parse("1.9.9"));
    }

    #[test]
    fn newer_release_is_available() {
        assert_eq!(judge("1.0.1", "v1.0.2"), Some(UpdateStatus::Available { latest: "1.0.2".into() }));
        assert_eq!(judge("1.0.1", "v2.0.0"), Some(UpdateStatus::Available { latest: "2.0.0".into() }));
    }

    #[test]
    fn same_or_older_is_up_to_date() {
        assert_eq!(judge("1.0.1", "v1.0.1"), Some(UpdateStatus::UpToDate));
        assert_eq!(judge("1.0.1", "v1.0.0"), Some(UpdateStatus::UpToDate));
    }

    #[test]
    fn unreadable_tag_is_unknown() {
        assert_eq!(judge("1.0.1", "tools-abc"), None);
        assert_eq!(judge("x", "v1.0.2"), None);
    }
}
