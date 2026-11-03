//! Homebrew owns the package even when its payload is an npm package or vendor binary.
use super::{
    filesystem::{changed, owned_writable_directory, FileStamp},
    npm,
    planning::{unavailable, LifecycleContext},
    service::map_mutation_error,
};
use crate::{models::*, services::agent_cli::environment::mutation::execution::ExactCliCommand};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub(super) struct HomebrewInstallation {
    pub prefix: PathBuf,
    pub package: String,
    pub cask: bool,
    package_root: PathBuf,
    launcher: PathBuf,
    linked_launcher: PathBuf,
    receipt: FileStamp,
    brew: FileStamp,
}

impl HomebrewInstallation {
    /// A canonical package root plus its install receipt establishes ownership.
    /// Merely finding a binary under /opt/homebrew/bin does not.
    pub fn inspect(installation: &AgentInstallation) -> Result<Option<Self>, AgentLifecycleError> {
        let Some(identity) = &installation.executable_identity else {
            return Ok(None);
        };
        let canonical = Path::new(&identity.canonical_path);
        let Some(store) = canonical.ancestors().find(|path| {
            path.file_name()
                .is_some_and(|name| name == "Cellar" || name == "Caskroom")
        }) else {
            return Ok(None);
        };
        let prefix = store.parent().ok_or_else(changed)?.to_path_buf();
        let relative = canonical.strip_prefix(store).map_err(|_| changed())?;
        let mut components = relative.components();
        let package = components
            .next()
            .and_then(|part| part.as_os_str().to_str())
            .filter(|name| {
                name.as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-_.@+".contains(&c))
            })
            .ok_or_else(changed)?
            .to_owned();
        let version = components.next().ok_or_else(changed)?.as_os_str();
        let package_root = store.join(&package);
        let cask = store.file_name().is_some_and(|name| name == "Caskroom");
        let receipt_path = if cask {
            package_root.join(".metadata/INSTALL_RECEIPT.json")
        } else {
            package_root.join(version).join("INSTALL_RECEIPT.json")
        };
        let (receipt, bytes) = FileStamp::read(&receipt_path, 1024 * 1024)?;
        let document: Value = serde_json::from_slice(&bytes).map_err(|_| changed())?;
        let expected_tap = if cask {
            "homebrew/cask"
        } else {
            "homebrew/core"
        };
        let tap = document.pointer("/source/tap").and_then(Value::as_str);
        if cask && document.pointer("/source/version").and_then(Value::as_str) != version.to_str() {
            return Err(changed());
        }
        if tap != Some(expected_tap) {
            return Err(unavailable(
                AgentLifecycleUnavailableReason::UnsupportedChannel,
            ));
        }
        if !owned_writable_directory(&prefix) {
            return Err(unavailable(
                AgentLifecycleUnavailableReason::PermissionRequired,
            ));
        }
        let brew_path = prefix.join("bin/brew");
        let (brew, _) = FileStamp::read(&brew_path, 1024 * 1024)?;
        let launcher = installation
            .executable_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(changed)?;
        let linked_launcher = prefix
            .join("bin")
            .join(crate::services::agent_cli::definition(installation.agent_kind).executable);
        Ok(Some(Self {
            prefix,
            package,
            cask,
            package_root,
            launcher,
            linked_launcher,
            receipt,
            brew,
        }))
    }

    pub fn signature(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.prefix.display(),
            self.package,
            self.cask,
            self.receipt.signature(),
            self.brew.signature()
        )
    }
    fn flag(&self) -> &str {
        if self.cask {
            "--cask"
        } else {
            "--formula"
        }
    }
    pub fn environment(&self, context: &LifecycleContext) -> Vec<(OsString, OsString)> {
        let mut environment = npm::environment(&context.home);
        for name in [
            "HOMEBREW_NO_AUTO_UPDATE",
            "HOMEBREW_NO_ANALYTICS",
            "HOMEBREW_NO_INSTALL_CLEANUP",
            "HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK",
            "HOMEBREW_NO_ENV_HINTS",
            "HOMEBREW_NO_ASK",
        ] {
            environment.push((name.into(), "1".into()));
        }
        environment
    }
    fn command(
        &self,
        context: &LifecycleContext,
        argv: Vec<String>,
    ) -> Result<ExactCliCommand, AgentLifecycleError> {
        ExactCliCommand::from_path(
            &self.prefix.join("bin/brew"),
            argv,
            &context.home,
            self.environment(context),
        )
        .map_err(map_mutation_error)
    }
    pub fn upgrade_command(
        &self,
        context: &LifecycleContext,
    ) -> Result<ExactCliCommand, AgentLifecycleError> {
        self.revalidate(context)?;
        let mut command = self.command(
            context,
            vec!["upgrade".into(), self.flag().into(), self.package.clone()],
        )?;
        // Read-only probes suppress auto-update; a confirmed upgrade follows
        // Homebrew's normal metadata refresh without upgrading other packages.
        command
            .environment
            .retain(|(key, _)| key != "HOMEBREW_NO_AUTO_UPDATE");
        Ok(command)
    }
    pub fn revalidate(&self, context: &LifecycleContext) -> Result<(), AgentLifecycleError> {
        self.receipt.revalidate()?;
        self.brew.revalidate()?;
        let info = self
            .command(
                context,
                vec![
                    "info".into(),
                    "--json=v2".into(),
                    self.flag().into(),
                    self.package.clone(),
                ],
            )?
            .probe_output(&context.settings, Duration::from_secs(10))
            .map_err(map_mutation_error)?;
        if info.timed_out
            || info.stdout_truncated
            || !info.status.is_some_and(|status| status.success())
        {
            return Err(changed());
        }
        let document: Value = serde_json::from_str(&info.stdout).map_err(|_| changed())?;
        let items = document
            .get(if self.cask { "casks" } else { "formulae" })
            .and_then(Value::as_array)
            .ok_or_else(changed)?;
        let [item] = items.as_slice() else {
            return Err(changed());
        };
        let name = item
            .get(if self.cask { "token" } else { "name" })
            .and_then(Value::as_str);
        let installed = if self.cask {
            item.get("installed").is_some_and(|value| {
                !value.is_null() && value.as_str().is_none_or(|value| !value.is_empty())
            })
        } else {
            item.get("installed")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
        };
        if name != Some(self.package.as_str())
            || !installed
            || item.get("pinned").and_then(Value::as_bool) == Some(true)
            || item.get("disabled").and_then(Value::as_bool) == Some(true)
        {
            let mut error = changed();
            error.message =
                "Homebrew 包身份或安装状态已变化，或该包已固定／停用；请检查后重试".into();
            return Err(error);
        }
        let tap = item.get("tap").and_then(Value::as_str);
        if tap
            != Some(if self.cask {
                "homebrew/cask"
            } else {
                "homebrew/core"
            })
        {
            return Err(changed());
        }
        Ok(())
    }
    pub fn verified_launcher(&self) -> Option<PathBuf> {
        [&self.linked_launcher, &self.launcher]
            .into_iter()
            .find(|path| {
                fs::canonicalize(path)
                    .ok()
                    .is_some_and(|selected| selected.starts_with(&self.package_root))
            })
            .cloned()
    }
    pub fn changed(&self) -> bool {
        self.receipt.revalidate().is_err()
    }
}
