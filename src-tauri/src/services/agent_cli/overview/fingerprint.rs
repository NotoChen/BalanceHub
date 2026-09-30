use super::super::{cache, discovery};
use crate::models::{AgentCliKind, AppSettings};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::PathBuf};

pub(super) fn signature(
    kind: AgentCliKind,
    settings: &AppSettings,
    paths: &BTreeSet<PathBuf>,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"agent-overview-v1");
    digest.update(env!("CARGO_PKG_VERSION"));
    digest.update(settings.agent_cli_path(kind));
    // Environment overrides may change the documented config roots without a file edit.
    cache::observe_environment(&mut digest, kind);
    discovery::observe_candidates(
        &mut digest,
        settings.agent_cli_path(kind),
        super::super::definition(kind),
    );
    for path in paths {
        cache::observe_path(&mut digest, path);
    }
    format!("{:x}", digest.finalize())
}
