//! Guard the finalization boundary in addition to the behavioral attack tests.
use regex_lite::Regex;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn sources_under(path: &Path, sources: &mut Vec<PathBuf>) {
    if path.is_dir() {
        if path.file_name().is_some_and(|name| name == "tests") {
            return;
        }
        for entry in fs::read_dir(path).expect("read repository source directory") {
            sources_under(&entry.expect("source entry").path(), sources);
        }
    } else if path.extension().is_some_and(|extension| extension == "rs")
        && path.file_name().is_some_and(|name| name != "tests.rs")
    {
        sources.push(path.to_path_buf());
    }
}

fn production_text(path: &Path) -> String {
    let source = fs::read_to_string(path).expect("read repository Rust source");
    // Inline test modules are at the end of these production source files;
    // candidate corruption inside those tests is intentional.
    let tests = Regex::new(r"#\[cfg\(test\)\]\s*mod\s+tests\s*\{").unwrap();
    match tests.find(&source) {
        Some(start) => source[..start.start()].to_owned(),
        None => source,
    }
}

#[test]
fn production_drafts_use_one_finalizer_without_later_state_mutation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/services/agent_cli");
    let constructor = Regex::new(r"\bAgentAssetProjectedDraft\s*\{").unwrap();
    let finalizer = production_text(&root.join("environment/adapter_support.rs"));
    assert_eq!(constructor.find_iter(&finalizer).count(), 1);

    let mutation = Regex::new(
        r"\b(?:draft|candidate)(?:\.[a-z_][a-z_0-9]*)+\s*=[^=]|\b(?:let\s+mut|mut)\s+(?:draft|candidate)\b|&\s*mut\s+AgentAssetProjectedDraft",
    ).unwrap();
    let mut sources = Vec::new();
    for agent in ["codex", "grok", "claude", "gemini"] {
        let directory = root.join(agent);
        let entrypoint = directory.join("environment.rs");
        if entrypoint.is_file() {
            sources.push(entrypoint);
        }
        sources_under(&directory.join("environment"), &mut sources);
    }
    sources.sort();
    assert!(
        sources.len() >= 20,
        "native source coverage unexpectedly empty"
    );
    for path in sources {
        let source = production_text(&path);
        assert!(
            !constructor.is_match(&source),
            "draft bypasses finalizer: {path:?}"
        );
        assert!(
            !mutation.is_match(&source),
            "native output is mutable after construction: {path:?}"
        );
        let is_resolver = path.file_name().is_some_and(|name| name == "resolve.rs")
            || path
                .components()
                .any(|component| component.as_os_str() == "resolve");
        if is_resolver {
            for forbidden in [
                "AgentAssetSnapshot",
                ".snapshot",
                ".facts",
                "std::fs",
                "fs::",
                "std::process",
                "Command::",
                "reqwest",
                "serde_json",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "native decision reads {forbidden}: {path:?}"
                );
            }
        }
    }

    let contracts = production_text(&root.join("contracts.rs"));
    let projection = production_text(&root.join("environment/projection.rs"));
    for source in [contracts, projection] {
        for forbidden in [
            "Option<AgentAssetStateAssessor>",
            "with_state_assessor",
            "AssessorNotMigrated",
        ] {
            assert!(
                !source.contains(forbidden),
                "optional native assessment returned: {forbidden}"
            );
        }
    }
}
