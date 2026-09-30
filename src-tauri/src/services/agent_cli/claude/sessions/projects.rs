use super::{encode_project_path, path_key};
use crate::services::agent_cli::contracts::SessionReadBudget;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const MAX_PROJECT_NAME_LENGTH: usize = 200;

#[cfg(test)]
thread_local! {
    static NEXT_PROJECT_ENTRY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn with_next_project_entry<T>(
    on_entry: impl FnOnce() + 'static,
    run: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<Box<dyn FnOnce()>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            NEXT_PROJECT_ENTRY.with(|entry| entry.replace(self.0.take()));
        }
    }
    let _restore =
        Restore(NEXT_PROJECT_ENTRY.with(|entry| entry.replace(Some(Box::new(on_entry)))));
    run()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProjectMatch {
    Exact,
    Prefix,
}

pub(super) struct ProjectSelector {
    names: BTreeMap<String, ProjectMatch>,
    origins: BTreeSet<String>,
}

impl ProjectSelector {
    pub(super) fn new(workdir: &Path) -> Self {
        let mut names = BTreeMap::new();
        let mut origins = BTreeSet::new();
        for path in [
            workdir.to_path_buf(),
            workdir
                .canonicalize()
                .unwrap_or_else(|_| workdir.to_path_buf()),
        ] {
            origins.insert(path_key(&path));
            let encoded = encode_project_path(&path);
            if encoded.len() > MAX_PROJECT_NAME_LENGTH {
                names.insert(
                    format!("{}-", &encoded[..MAX_PROJECT_NAME_LENGTH]),
                    ProjectMatch::Prefix,
                );
            } else {
                names.insert(encoded, ProjectMatch::Exact);
            }
        }
        Self { names, origins }
    }

    fn matches_name(&self, name: &str) -> Option<ProjectMatch> {
        if self.names.get(name) == Some(&ProjectMatch::Exact) {
            return Some(ProjectMatch::Exact);
        }
        self.names.iter().find_map(|(prefix, kind)| {
            (*kind == ProjectMatch::Prefix
                && name
                    .strip_prefix(prefix.as_str())
                    .is_some_and(|suffix| !suffix.is_empty()))
            .then_some(ProjectMatch::Prefix)
        })
    }

    pub(super) fn proves_origin(&self, origin: Option<&Path>) -> Option<bool> {
        origin.map(|origin| self.origins.contains(&path_key(origin)))
    }
}

pub(super) struct ProjectCandidate {
    pub path: PathBuf,
    pub kind: ProjectMatch,
}

pub(super) enum ProjectCandidateState {
    Ready(ProjectCandidate),
    Missing,
    Invalid,
}

pub(super) struct ProjectLookup {
    projects: PathBuf,
    projects_exist: bool,
}

fn canonical_with_missing_tail(path: &Path, budget: &SessionReadBudget) -> Result<PathBuf, String> {
    let mut ancestor = path;
    let mut missing = Vec::new();
    loop {
        budget.check()?;
        match ancestor.canonicalize() {
            Ok(mut canonical) => {
                for name in missing.into_iter().rev() {
                    canonical.push(name);
                }
                budget.check()?;
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = ancestor.file_name().ok_or_else(|| error.to_string())?;
                missing.push(name.to_os_string());
                ancestor = ancestor.parent().ok_or_else(|| error.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

impl ProjectLookup {
    pub(super) fn new(root: &Path, budget: &SessionReadBudget) -> Result<Self, String> {
        budget.check()?;
        let projects = root.join("projects");
        let projects_exist = match fs::symlink_metadata(&projects) {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err("Claude 项目根不是可信的普通目录".into());
                }
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(format!("读取 Claude 项目根失败：{error}")),
        };
        let source = canonical_with_missing_tail(root, budget)?;
        let projects = canonical_with_missing_tail(&projects, budget)?;
        if projects.parent() != Some(source.as_path()) {
            return Err("Claude 项目目录已离开选定原生来源".into());
        }
        budget.check()?;
        Ok(Self {
            projects,
            projects_exist,
        })
    }

    pub(super) fn candidate(
        &self,
        path: &Path,
        selector: &ProjectSelector,
        budget: &SessionReadBudget,
    ) -> Result<ProjectCandidateState, String> {
        budget.check()?;
        let Some(kind) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| selector.matches_name(name))
        else {
            return Ok(ProjectCandidateState::Invalid);
        };
        let Some(parent) = path.parent() else {
            return Ok(ProjectCandidateState::Invalid);
        };
        if canonical_with_missing_tail(parent, budget)? != self.projects {
            return Ok(ProjectCandidateState::Invalid);
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                budget.check()?;
                return Ok(ProjectCandidateState::Missing);
            }
            Err(error) => return Err(format!("读取 Claude 项目目录失败：{error}")),
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Ok(ProjectCandidateState::Invalid);
        }
        let path = path.canonicalize().map_err(|error| error.to_string())?;
        if path.parent() != Some(self.projects.as_path()) {
            return Ok(ProjectCandidateState::Invalid);
        }
        budget.check()?;
        Ok(ProjectCandidateState::Ready(ProjectCandidate {
            path,
            kind,
        }))
    }

    pub(super) fn discover(
        &self,
        selectors: &[ProjectSelector],
        budget: &SessionReadBudget,
    ) -> Result<Vec<Vec<ProjectCandidate>>, String> {
        budget.check()?;
        let mut results = vec![BTreeMap::<PathBuf, ProjectMatch>::new(); selectors.len()];
        if !self.projects_exist {
            return Ok(results.into_iter().map(|_| Vec::new()).collect());
        }
        let projects = &self.projects;
        for (selector, found) in selectors.iter().zip(&mut results) {
            for (name, kind) in &selector.names {
                budget.check()?;
                if *kind != ProjectMatch::Exact {
                    continue;
                }
                if let ProjectCandidateState::Ready(candidate) =
                    self.candidate(&projects.join(name), selector, budget)?
                {
                    found.insert(candidate.path, candidate.kind);
                }
            }
        }
        if selectors.iter().any(|selector| {
            selector
                .names
                .values()
                .any(|kind| *kind == ProjectMatch::Prefix)
        }) {
            for entry in fs::read_dir(projects)
                .map_err(|error| format!("枚举 Claude 项目目录失败：{error}"))?
            {
                #[cfg(test)]
                if let Some(on_entry) = NEXT_PROJECT_ENTRY.with(|entry| entry.borrow_mut().take()) {
                    on_entry();
                }
                budget.check()?;
                let entry = entry.map_err(|error| error.to_string())?;
                for (selector, found) in selectors.iter().zip(&mut results) {
                    if let ProjectCandidateState::Ready(candidate) =
                        self.candidate(&entry.path(), selector, budget)?
                    {
                        found.entry(candidate.path).or_insert(candidate.kind);
                    }
                }
            }
        }
        budget.check()?;
        Ok(results
            .into_iter()
            .map(|found| {
                found
                    .into_iter()
                    .map(|(path, kind)| ProjectCandidate { path, kind })
                    .collect()
            })
            .collect())
    }
}
