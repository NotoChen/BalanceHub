use std::{borrow::Cow, path::Path};

/// Use the ordinary Windows spelling only when it names the same file.
/// Keep canonical identities and handle-based access checks at their call sites.
/// This conversion performs no filesystem access, including for saved bookmarks.
pub(crate) fn compatible_path(path: &Path) -> Cow<'_, Path> {
    #[cfg(windows)]
    if let Some(path) = compatible_unc_path(path) {
        return Cow::Owned(path);
    }
    Cow::Borrowed(dunce::simplified(path))
}

/// Format public path fields without changing the in-memory paths used by
/// source identity, scope checks and verified snapshot access.
pub(crate) fn serialize_path<S>(path: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&compatible_path(Path::new(path)).to_string_lossy())
}

pub(crate) fn serialize_optional_path<S>(
    path: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match path {
        Some(path) => {
            serializer.serialize_some(&compatible_path(Path::new(path)).to_string_lossy())
        }
        None => serializer.serialize_none(),
    }
}

#[cfg(windows)]
fn compatible_unc_path(path: &Path) -> Option<std::path::PathBuf> {
    use std::path::{Component, PathBuf, Prefix};

    let Component::Prefix(prefix) = path.components().next()? else {
        return None;
    };
    let Prefix::VerbatimUNC(server, share) = prefix.kind() else {
        return None;
    };
    if server.is_empty() || share.is_empty() {
        return None;
    }
    let suffix = path.to_str()?.strip_prefix(r"\\?\UNC\")?;
    // dunce validates DOS names, trailing dots/spaces and length for disk
    // paths. Apply those same rules to the whole UNC suffix before changing
    // its prefix; this validation path is never used for filesystem access.
    let validation = PathBuf::from(format!(r"\\?\C:\{suffix}"));
    if dunce::simplified(&validation) == validation.as_path() {
        return None;
    }
    Some(PathBuf::from(format!(r"\\{suffix}")))
}

/// All Windows temporary launchers currently execute a CMD wrapper. Fail during
/// preparation if a path genuinely needs a namespace that CMD cannot address.
#[cfg(any(windows, test))]
pub(crate) fn windows_shell_path(path: &Path) -> Result<Cow<'_, Path>, String> {
    let compatible = compatible_path(path);
    let text = compatible
        .to_str()
        .ok_or_else(|| "Windows 终端无法使用包含无效字符的路径".to_owned())?;
    if text.starts_with(r"\\?\") || text.starts_with(r"\\.\") {
        return Err(format!(
            "Windows 终端无法使用此扩展路径，请使用普通目录或缩短目录层级：{text}"
        ));
    }
    Ok(compatible)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_paths_keep_their_spelling() {
        for path in ["/tmp/project with spaces", "./project", ""] {
            assert_eq!(compatible_path(Path::new(path)).as_ref(), Path::new(path));
        }
    }

    #[cfg(windows)]
    #[test]
    fn public_serialization_keeps_internal_path_identity_untouched() {
        #[derive(serde::Serialize)]
        struct PublicPaths {
            #[serde(serialize_with = "serialize_path")]
            path: String,
            #[serde(serialize_with = "serialize_optional_path")]
            optional: Option<String>,
        }
        for (native, public) in [
            (r"\\?\C:\Users\demo", r"C:\Users\demo"),
            (r"\\?\UNC\server\share\project", r"\\server\share\project"),
            (r"\\?\C:\project.", r"\\?\C:\project."),
        ] {
            let value = PublicPaths {
                path: native.to_owned(),
                optional: Some(native.to_owned()),
            };
            let json = serde_json::to_value(&value).unwrap();
            assert_eq!(json["path"], public);
            assert_eq!(json["optional"], public);
            assert_eq!(value.path, native);
            assert_eq!(value.optional.as_deref(), Some(native));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_disk_and_network_paths_keep_the_same_location() {
        for (input, expected) in [
            (r"\\?\C:\Users\demo", r"C:\Users\demo"),
            (r"\\?\D:\项目 (demo) & 100% !", r"D:\项目 (demo) & 100% !"),
            (r"\\?\C:\", r"C:\"),
            (r"\\?\UNC\server\share\project", r"\\server\share\project"),
            (r"\\server\share\project", r"\\server\share\project"),
        ] {
            assert_eq!(
                compatible_path(Path::new(input)).as_ref(),
                Path::new(expected)
            );
            assert_eq!(
                windows_shell_path(Path::new(input)).unwrap().as_ref(),
                Path::new(expected)
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_special_names_and_long_paths_are_never_retargeted() {
        let long_path = format!(r"\\?\C:\{}", "directory\\".repeat(35));
        for path in [
            r"\\?\C:\project.",
            r"\\?\C:\project ",
            r"\\?\C:\CON",
            r"\\?\UNC\server\share\project.",
            r"\\?\Volume{demo}\project",
            r"\\.\PhysicalDrive0",
            &long_path,
        ] {
            assert_eq!(compatible_path(Path::new(path)).as_ref(), Path::new(path));
            assert!(windows_shell_path(Path::new(path)).is_err());
        }
    }
}
