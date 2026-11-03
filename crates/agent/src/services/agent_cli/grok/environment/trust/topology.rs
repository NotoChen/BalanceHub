use super::{
    AgentAssetResolveSource, AgentAssetSnapshot, AgentAssetSourceKind, HOME_ROOT_KEY, REGISTRY_KEY,
    ROOT_PREFIX,
};
use std::path::{Component, Path, PathBuf};

pub(super) struct Topology<'a> {
    sources: &'a [AgentAssetResolveSource<'a>],
    home: Option<&'a Path>,
}

impl<'a> Topology<'a> {
    pub(super) fn new(sources: &'a [AgentAssetResolveSource<'a>]) -> Self {
        Self {
            sources,
            home: sources
                .iter()
                .find(|source| source.spec.native_source_key == HOME_ROOT_KEY)
                .map(|source| source.spec.path.as_path()),
        }
    }

    pub(super) fn unsafe_root(&self, path: &Path) -> bool {
        !path.is_absolute()
            || path.parent().is_none()
            || self.home == Some(path)
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
    }

    pub(super) fn workspace_key(&self, path: &Path) -> Option<PathBuf> {
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return None;
        }
        for directory in path.ancestors() {
            let root = self.sources.iter().find(|source| {
                source.spec.path == directory
                    && (source.spec.native_source_key.starts_with(ROOT_PREFIX)
                        || matches!(
                            source.spec.native_source_key.as_str(),
                            HOME_ROOT_KEY | REGISTRY_KEY
                        ))
            })?;
            let AgentAssetSnapshot::DirectoryManifest {
                entries,
                complete: true,
                ..
            } = root.snapshot
            else {
                return None;
            };
            // A bare repository can stop git2 discovery without a .git entry.
            if ["HEAD", "objects", "refs"].iter().all(|name| {
                entries
                    .iter()
                    .any(|entry| entry.name.eq_ignore_ascii_case(name))
            }) {
                return None;
            }
            let Some(git) = entries
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(".git"))
            else {
                continue;
            };
            if git.name != ".git"
                || git.is_symlink
                || git.source_kind != AgentAssetSourceKind::Directory
                || !self.conventional_git_directory(directory)
            {
                return None;
            }
            return Some(
                if self.unsafe_root(directory) {
                    path
                } else {
                    directory
                }
                .to_owned(),
            );
        }
        // Never assume absent Git ancestors outside the declared snapshot roots.
        None
    }

    fn snapshot(&self, path: &Path) -> Option<&AgentAssetSnapshot> {
        self.sources
            .iter()
            .find(|source| {
                super::is_authority_source(&source.spec.native_source_key)
                    && source.spec.path == path
            })
            .map(|source| source.snapshot)
    }

    fn conventional_git_directory(&self, root: &Path) -> bool {
        let git = root.join(".git");
        let Some(AgentAssetSnapshot::DirectoryManifest {
            entries,
            complete: true,
            ..
        }) = self.snapshot(&git)
        else {
            return false;
        };
        if entries.iter().any(|entry| {
            entry.name.eq_ignore_ascii_case("commondir")
                || entry.name.eq_ignore_ascii_case("gitdir")
        }) {
            return false;
        }
        for (name, kind) in [
            ("HEAD", AgentAssetSourceKind::File),
            ("config", AgentAssetSourceKind::File),
            ("objects", AgentAssetSourceKind::Directory),
            ("refs", AgentAssetSourceKind::Directory),
        ] {
            if !entries
                .iter()
                .any(|entry| entry.name == name && !entry.is_symlink && entry.source_kind == kind)
            {
                return false;
            }
        }
        let Some(AgentAssetSnapshot::File { bytes: head, .. }) = self.snapshot(&git.join("HEAD"))
        else {
            return false;
        };
        let Some(AgentAssetSnapshot::File { bytes: config, .. }) =
            self.snapshot(&git.join("config"))
        else {
            return false;
        };
        valid_head(head) && conventional_config(config)
    }
}

fn valid_head(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let text = text.trim_end_matches(['\r', '\n']);
    if let Some(reference) = text.strip_prefix("ref: refs/") {
        return !reference.is_empty()
            && !reference.ends_with('/')
            && !reference.bytes().any(|byte| byte <= b' ' || byte == 0x7f)
            && !reference.contains([' ', '\t', '\n', '\r', '\\', '~', '^', ':', '?', '*', '['])
            && !reference.contains("..")
            && !reference.contains("@{")
            && reference.split('/').all(|component| {
                !component.is_empty()
                    && !component.starts_with('.')
                    && !component.ends_with('.')
                    && !component.ends_with(".lock")
            });
    }
    text.len() == 40 && text.bytes().all(|value| value.is_ascii_hexdigit())
}

// This is a deliberately limited proof recognizer, not a second Git parser.
// Includes, extensions, escapes and worktree overrides need native resolution.
fn conventional_config(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut section = String::new();
    let mut format_zero = false;
    let mut non_bare = false;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.contains('\\')
            || line
                .chars()
                .any(|character| character.is_control() && character != '\t')
        {
            return false;
        }
        let mut quoted = false;
        let mut comment = line.len();
        for (index, character) in line.char_indices() {
            if character == '"' {
                quoted = !quoted;
            }
            if !quoted && matches!(character, '#' | ';') {
                comment = index;
                break;
            }
        }
        if quoted {
            return false;
        }
        let line = line[..comment].trim();
        if line.starts_with('[') {
            let Some(inner) = line
                .strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
            else {
                return false;
            };
            let name = inner
                .split([' ', '\t', '.'])
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                || matches!(name.as_str(), "include" | "includeif" | "extensions")
                || (name == "core" && inner != "core" && !inner.eq_ignore_ascii_case("core"))
                || inner.contains(['[', ']'])
            {
                return false;
            }
            let remainder = &inner[name.len()..];
            if !remainder.is_empty() {
                let subsection = remainder.trim();
                if subsection.len() < 2
                    || !subsection.starts_with('"')
                    || !subsection.ends_with('"')
                    || subsection[1..subsection.len() - 1].contains('"')
                {
                    return false;
                }
            }
            section = name;
            continue;
        }
        if section.is_empty() {
            return false;
        }
        let (name, value) = line
            .split_once('=')
            .map(|(name, value)| (name.trim(), value.trim()))
            .unwrap_or((line, "true"));
        if name.is_empty()
            || !name.as_bytes()[0].is_ascii_alphabetic()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return false;
        }
        let value = if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            &value[1..value.len() - 1]
        } else {
            value
        };
        if value.contains('"') {
            return false;
        }
        if section == "core" {
            match name.to_ascii_lowercase().as_str() {
                "worktree" => return false,
                "repositoryformatversion" => {
                    if value != "0" {
                        return false;
                    }
                    format_zero = true;
                }
                "bare" => {
                    if !value.eq_ignore_ascii_case("false") {
                        return false;
                    }
                    non_bare = true;
                }
                _ => {}
            }
        }
    }
    format_zero && non_bare
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_remote_config_is_supported_but_invalid_syntax_is_not_proof() {
        let config = b"[core]\nrepositoryformatversion=0\nbare=false\n[remote \"origin\"]\nurl = https://example.invalid/repo\nfetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\nremote = origin\nmerge = refs/heads/main\n";
        assert!(conventional_config(config));
        for suffix in [
            b"[broken".as_slice(),
            b"[remote]\ninvalid key=value\n",
            b"[includeIf \"gitdir:repo\"]\npath=elsewhere\n",
        ] {
            let mut invalid = config.to_vec();
            invalid.extend_from_slice(suffix);
            assert!(!conventional_config(&invalid));
        }
    }
}
