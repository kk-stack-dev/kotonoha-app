//! Asks GitHub which release is the latest. Called only when a person presses "check for updates".
//! The request carries no identifiers and nothing about the user's data; only the response is read.

use std::time::Duration;

use kotonoha_core::update::{self, UpdateStatus};

const LATEST_API: &str = "https://api.github.com/repos/kk-stack-dev/kotonoha-app/releases/latest";

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("request failed: {0}")]
    Http(#[from] ureq::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected response from the release server")]
    BadResponse,
}

/// Pulls `tag_name` out of a GitHub "latest release" response.
fn tag_of(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json.get("tag_name")?.as_str().map(str::to_owned)
}

/// Compares `current` with the latest published release. Blocking.
pub fn check(current: &str) -> Result<UpdateStatus, UpdateError> {
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into();
    let mut response = agent
        .get(LATEST_API)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "kotonoha-update-check")
        .call()?;
    let body = response.body_mut().read_to_string()?;
    let tag = tag_of(&body).ok_or(UpdateError::BadResponse)?;
    update::judge(current, &tag).ok_or(UpdateError::BadResponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_tag() {
        assert_eq!(tag_of(r#"{"tag_name":"v1.2.0","name":"x"}"#).as_deref(), Some("v1.2.0"));
    }

    #[test]
    fn rejects_other_bodies() {
        assert_eq!(tag_of("not json"), None);
        assert_eq!(tag_of(r#"{"message":"Not Found"}"#), None);
    }
}
