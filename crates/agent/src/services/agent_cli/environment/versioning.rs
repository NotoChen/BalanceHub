pub(crate) mod releases;

use crate::models::{
    AgentInstallationChannel, AgentLifecycleVersionState, AgentSemanticVersion,
    AgentVersionIdentifier,
};

pub(super) fn latest_stable_version(payload: &serde_json::Value) -> Result<String, String> {
    let value = payload
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| is_stable_version(value))
        .ok_or_else(|| "npm 响应缺少有效的最新稳定版本".to_string())?;
    Ok(value.to_string())
}

fn is_stable_version(value: &str) -> bool {
    semver::Version::parse(value).is_ok_and(|version| version.pre.is_empty())
}

pub(crate) fn version_state(
    installed: Option<&str>,
    latest: Option<&str>,
) -> AgentLifecycleVersionState {
    let (Some(installed), Some(latest)) =
        (installed.filter(|value| !value.trim().is_empty()), latest)
    else {
        return AgentLifecycleVersionState::Unknown;
    };
    let (Some(installed), Some(latest)) = (
        parse_semantic_version(installed),
        parse_semantic_version(latest),
    ) else {
        return AgentLifecycleVersionState::Unknown;
    };
    match installed.cmp(&latest) {
        std::cmp::Ordering::Equal => AgentLifecycleVersionState::UpToDate,
        std::cmp::Ordering::Less => AgentLifecycleVersionState::UpdateAvailable,
        std::cmp::Ordering::Greater => AgentLifecycleVersionState::AheadOfLatest,
    }
}

/// One parser feeds version display and exact native mechanism eligibility.
pub(crate) fn parse_semantic_version(value: &str) -> Option<AgentSemanticVersion> {
    let value = value.trim();
    let start = value
        .char_indices()
        .find_map(|(index, character)| character.is_ascii_digit().then_some(index))?;
    let token = value[start..]
        .split_whitespace()
        .next()?
        .trim_end_matches([',', ')', ']']);
    let (without_build, build) = token
        .split_once('+')
        .map_or((token, None), |(core, build)| (core, Some(build)));
    if build.is_some_and(|value| !valid_version_identifiers(value)) {
        return None;
    }
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(core, pre)| (core, Some(pre)));
    let mut parts = core.split('.');
    let parse_number = |value: &str| {
        (!value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && (value == "0" || !value.starts_with('0')))
        .then(|| value.parse::<u64>().ok())
        .flatten()
    };
    let major = parse_number(parts.next()?)?;
    let minor = parse_number(parts.next()?)?;
    let patch = parse_number(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    let prerelease = match prerelease {
        None => Vec::new(),
        Some(value) => {
            if !valid_version_identifiers(value) {
                return None;
            }
            value
                .split('.')
                .map(|part| {
                    if part.bytes().all(|byte| byte.is_ascii_digit()) {
                        parse_number(part).map(AgentVersionIdentifier::Numeric)
                    } else {
                        Some(AgentVersionIdentifier::Text(part.to_owned()))
                    }
                })
                .collect::<Option<Vec<_>>>()?
        }
    };
    Some(AgentSemanticVersion {
        major,
        minor,
        patch,
        prerelease,
    })
}

fn valid_version_identifiers(value: &str) -> bool {
    value.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

pub(crate) fn version_channel(value: &str) -> AgentInstallationChannel {
    let Some(version) = parse_semantic_version(value) else {
        return AgentInstallationChannel::Unknown;
    };
    if version.prerelease.is_empty() {
        AgentInstallationChannel::Stable
    } else if version.prerelease.iter().any(|part| {
        matches!(part,
        AgentVersionIdentifier::Text(text) if text.to_ascii_lowercase().contains("nightly"))
    }) {
        AgentInstallationChannel::Nightly
    } else {
        AgentInstallationChannel::Preview
    }
}

#[cfg(test)]
mod semantic_version_tests {
    use super::*;

    #[test]
    fn one_parser_rejects_invalid_authorization_versions_and_orders_identifiers() {
        for value in ["1.02.3", "1.2.3-alpha..1", "1.2.3-01", "1.2.3+", "1.2.3.4"] {
            assert!(parse_semantic_version(value).is_none(), "{value}");
        }
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(
                parse_semantic_version(pair[0]).unwrap() < parse_semantic_version(pair[1]).unwrap()
            );
        }
        assert_eq!(
            parse_semantic_version("codex-cli 0.154.0"),
            parse_semantic_version("0.154.0+build.1")
        );
    }
}
