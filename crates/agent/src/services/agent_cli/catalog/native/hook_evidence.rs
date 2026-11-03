#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn fixture_digest() -> String {
    super::super::digest(include_bytes!("hook-fixture.json"))
}

/// Acceptance producers must only use an isolated temporary directory.
pub(crate) fn with_isolated_fixture<T>(
    root: &std::path::Path,
    action: impl FnOnce() -> T,
) -> Result<T, String> {
    let root = root
        .canonicalize()
        .map_err(|_| "Hook fixture root is unavailable")?;
    let temporary = std::env::temp_dir()
        .canonicalize()
        .map_err(|_| "Temporary fixture parent unavailable")?;
    if root == temporary
        || !root.starts_with(&temporary)
        || root.components().count() <= temporary.components().count()
    {
        return Err(
            "Hook certification qualification requires an isolated temporary directory".to_owned(),
        );
    }
    Ok(action())
}
