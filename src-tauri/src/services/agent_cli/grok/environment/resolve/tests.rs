use super::*;
use crate::models::{
    AgentAssetInstallationOrigin, AgentAssetSourceKind, AgentCliKind, AgentConfigurationContext,
    AgentTrustState,
};
use crate::services::agent_cli::contracts::{
    AgentAssetDirectoryEntry, AgentAssetParseRequest, AgentAssetResolveSource, AgentAssetSnapshot,
    AgentDiagnosticEmission, AgentDiagnosticOutput, AgentOutputStop, AgentParseOutput,
};
use crate::services::agent_cli::environment::{source, SourceInput};
use std::ops::ControlFlow;

#[derive(Default)]
struct Captured {
    declarations: Vec<ParsedAgentAsset>,
    drafts: Vec<AgentAssetProjectedDraft>,
    diagnostics: Vec<AgentAssetDiagnostic>,
}
impl AgentDiagnosticOutput for Captured {
    fn has_regular_capacity(&self) -> bool {
        true
    }
    fn emit_diagnostic(&mut self, diagnostic: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.diagnostics.push(diagnostic);
        AgentDiagnosticEmission::Accepted
    }
}
impl AgentParseOutput for Captured {
    fn emit_declaration(&mut self, asset: ParsedAgentAsset) -> ControlFlow<AgentOutputStop> {
        self.declarations.push(asset);
        ControlFlow::Continue(())
    }
}
impl AgentResolveOutput for Captured {
    fn emit_draft(&mut self, draft: AgentAssetProjectedDraft) -> ControlFlow<AgentOutputStop> {
        self.drafts.push(draft);
        ControlFlow::Continue(())
    }
}

#[test]
fn native_proofs_keep_decisive_peers_without_promoting_plain_plugin_directories() {
    let root = std::env::temp_dir().join("balancehub-grok-proof-input");
    let workspace = root.join("workspace");
    let context = AgentConfigurationContext {
        id: "grok-proof-context".to_owned(),
        environment_id: "native".to_owned(),
        agent_kind: AgentCliKind::Grok,
        config_root: root.to_string_lossy().into_owned(),
        profile: "default".to_owned(),
        workspace_id: Some(workspace.to_string_lossy().into_owned()),
        trust_context: AgentTrustState::Unknown,
        parser_version: 2,
        schema_facts: BTreeMap::new(),
        compatible_installation_ids: Vec::new(),
    };
    let mut inputs = Vec::new();
    for (key, scope, precedence, config) in [
        ("config", AgentAssetScope::User, 10, "disabled_mcp_servers=['same']\nstatus_line=true\n[mcp_servers.same]\ncommand='low'\nenabled=false\n[mcp_servers.defaulted]\ncommand='runner'\n[plugins]\nenabled=['plugin']\ndisabled=['plugin']"),
        ("workspace-config", AgentAssetScope::Workspace, 20, "[mcp_servers.same]\nurl='https://example.invalid'\n[plugins]\ndisabled=['plugin']"),
    ] {
        let path = if scope == AgentAssetScope::Workspace {
            workspace.join(".grok/config.toml")
        } else {
            root.join("config.toml")
        };
        inputs.push((source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: key, label: key, path, allowed_root: &root,
            scope, precedence, sensitive: true, source_kind: AgentAssetSourceKind::File,
            categories: &[AgentAssetCategory::Mcp, AgentAssetCategory::Plugin, AgentAssetCategory::StatusUi],
        }), AgentAssetSnapshot::File { bytes: config.as_bytes().to_vec(), revision: Default::default() }));
    }
    for (key, scope, precedence, category, name) in [
        (
            "plugins",
            AgentAssetScope::User,
            10,
            AgentAssetCategory::Plugin,
            "plugin",
        ),
        (
            "workspace-plugins",
            AgentAssetScope::Workspace,
            20,
            AgentAssetCategory::Plugin,
            "plugin",
        ),
        (
            "hooks",
            AgentAssetScope::User,
            10,
            AgentAssetCategory::Hook,
            "hook.json",
        ),
        (
            "workspace-hooks",
            AgentAssetScope::Workspace,
            20,
            AgentAssetCategory::Hook,
            "hook.json",
        ),
        (
            "workspace-skills",
            AgentAssetScope::Workspace,
            20,
            AgentAssetCategory::Skill,
            "skill",
        ),
        (
            "workspace-shared-skills",
            AgentAssetScope::Workspace,
            20,
            AgentAssetCategory::Skill,
            "skill",
        ),
    ] {
        let (path, origin) = match key {
            "plugins" => (
                root.join("plugins"),
                AgentAssetInstallationOrigin::NativePackage,
            ),
            "workspace-plugins" => (
                workspace.join(".grok/plugins"),
                AgentAssetInstallationOrigin::NativePackage,
            ),
            "hooks" => (root.join("hooks"), AgentAssetInstallationOrigin::LocalFiles),
            "workspace-hooks" => (
                workspace.join(".grok/hooks"),
                AgentAssetInstallationOrigin::LocalFiles,
            ),
            "workspace-skills" => (
                workspace.join(".grok/skills"),
                AgentAssetInstallationOrigin::LocalFiles,
            ),
            "workspace-shared-skills" => (
                workspace.join(".agents/skills"),
                AgentAssetInstallationOrigin::SharedFiles,
            ),
            _ => unreachable!("fixed native source roles"),
        };
        if category == AgentAssetCategory::Skill {
            // The two valid native manifests deliberately share precedence to
            // exercise structural-conflict evidence independently of discovery.
            inputs.push((
                source(SourceInput {
                    origin,
                    native_source_key: &format!("grok-skill-manifest:{key}:{name}"),
                    label: key,
                    path: path.join(name).join("SKILL.md"),
                    allowed_root: &root,
                    scope,
                    precedence,
                    sensitive: false,
                    source_kind: AgentAssetSourceKind::File,
                    categories: &[category],
                }),
                AgentAssetSnapshot::File {
                    bytes: format!("---\nname: {name}\ndescription: fixture\n---\n").into_bytes(),
                    revision: Default::default(),
                },
            ));
            continue;
        }
        inputs.push((
            source(SourceInput {
                origin,
                native_source_key: key,
                label: key,
                path: path.clone(),
                allowed_root: &root,
                scope,
                precedence,
                sensitive: false,
                source_kind: AgentAssetSourceKind::Directory,
                categories: &[category],
            }),
            AgentAssetSnapshot::DirectoryManifest {
                entries: vec![AgentAssetDirectoryEntry {
                    name: name.to_owned(),
                    source_kind: if category == AgentAssetCategory::Hook {
                        AgentAssetSourceKind::File
                    } else {
                        AgentAssetSourceKind::Directory
                    },
                    is_symlink: false,
                }],
                revision: Default::default(),
                complete: true,
            },
        ));
        if category == AgentAssetCategory::Hook {
            inputs.push((
                source(SourceInput {
                    origin,
                    native_source_key: &format!("grok-hook-file:{key}:{name}"),
                    label: key,
                    path: path.join(name),
                    allowed_root: &root,
                    scope,
                    precedence,
                    sensitive: true,
                    source_kind: AgentAssetSourceKind::File,
                    categories: &[AgentAssetCategory::Hook],
                }),
                AgentAssetSnapshot::File {
                    bytes: br#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"bh-fixture-hook"}]}]}}"#.to_vec(),
                    revision: Default::default(),
                },
            ));
        }
    }
    let mut policy_source = source(SourceInput {
        origin: AgentAssetInstallationOrigin::ConfigEntry,
        native_source_key: "hook-disabled-state",
        label: "Hook disabled names",
        path: root.join("disabled-hooks"),
        allowed_root: &root,
        scope: AgentAssetScope::User,
        precedence: 10,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: &[AgentAssetCategory::Hook],
    });
    policy_source.hook_definition_source = false;
    inputs.push((
        policy_source,
        AgentAssetSnapshot::File {
            bytes: b"project/hook:session_start[0].hooks[0]\n".to_vec(),
            revision: Default::default(),
        },
    ));
    let mut capture = Captured::default();
    for (source, snapshot) in &inputs {
        let before = capture.declarations.len();
        super::super::parse::parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source,
                snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut capture,
        );
        if source.source_kind == AgentAssetSourceKind::Directory {
            assert_eq!(
                capture.declarations.len(),
                before,
                "directory names alone must not create Plugin or Hook assets"
            );
        }
    }
    assert_eq!(capture.declarations.len(), 13);
    assert_eq!(
        capture
            .declarations
            .iter()
            .fold(BTreeMap::new(), |mut counts, declaration| {
                *counts.entry(declaration.category).or_insert(0) += 1;
                counts
            }),
        BTreeMap::from([
            (AgentAssetCategory::Mcp, 4),
            (AgentAssetCategory::Plugin, 3),
            (AgentAssetCategory::StatusUi, 1),
            (AgentAssetCategory::Skill, 2),
            (AgentAssetCategory::Hook, 3),
        ])
    );
    assert!(
        capture.diagnostics.iter().all(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::DiscoveryIncomplete {
                category: AgentAssetCategory::Plugin,
                ..
            }
        )),
        "{:?}",
        capture.diagnostics
    );
    let declarations = capture.declarations;
    let resolve = |declarations: &[ParsedAgentAsset], reverse: bool| {
        let mut sources = inputs
            .iter()
            .map(|(spec, snapshot)| AgentAssetResolveSource { spec, snapshot })
            .collect::<Vec<_>>();
        if reverse {
            sources.reverse();
        }
        let mut result = Captured::default();
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations,
                sources: &sources,
            },
            &mut result,
        );
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        result
            .drafts
            .sort_by(|left, right| left.projection_key.cmp(&right.projection_key));
        result.drafts
    };
    let drafts = resolve(&declarations, false);
    assert_eq!(drafts.len(), 7);
    let row = |category, id: &str| {
        drafts
            .iter()
            .find(|draft| {
                draft.native_kind == category
                    && draft.native_id == id
                    && draft.resolution.relation != AgentAssetResolutionRelation::Replaced
            })
            .unwrap()
    };
    let defaulted = row(AgentAssetCategory::Mcp, "defaulted");
    assert!(matches!(
        defaulted.state_proof.declared,
        AgentAssetDeclaredStateProofDraft::NativeDefault {
            outcome: AgentAssetDeclaredState::Enabled,
            ..
        }
    ));
    assert!(drafts
        .iter()
        .all(|draft| draft.native_kind != AgentAssetCategory::Plugin));
    let hook_policy = declarations
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::Hook && asset.native_id == "hook-disabled-state"
        })
        .unwrap();
    assert_eq!(hook_policy.role, AgentAssetDeclarationRole::PolicyOverlay);
    assert_eq!(hook_policy.declared_state, AgentAssetDeclaredState::Unknown);
    for (native_id, declared_state, expected_state) in [
        (
            "global/hook:session_start[0].hooks[0]",
            AgentAssetDeclaredState::Enabled,
            AgentAssetState::Enabled,
        ),
        (
            "project/hook:session_start[0].hooks[0]",
            AgentAssetDeclaredState::Disabled,
            AgentAssetState::Disabled,
        ),
    ] {
        let hook = row(AgentAssetCategory::Hook, native_id);
        assert_eq!(
            hook.resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert_eq!(hook.declared_state, declared_state);
        assert_eq!(hook.effective_state, expected_state);
        assert!(hook.resolution.winner.is_none());
        let definition = declarations
            .iter()
            .find(|asset| {
                asset.category == AgentAssetCategory::Hook && asset.native_id == native_id
            })
            .unwrap();
        assert_eq!(
            hook.represented_declaration_ids,
            vec![definition.declaration_id.clone()]
        );
        assert_eq!(
            hook.contributor_ids,
            vec![definition.declaration_id.clone()]
        );
        if expected_state == AgentAssetState::Disabled {
            assert_eq!(
                hook.state_proof.declared,
                AgentAssetDeclaredStateProofDraft::Policy {
                    declaration_ids: vec![hook_policy.declaration_id.clone()],
                    outcome: AgentAssetDeclaredState::Disabled,
                }
            );
        } else {
            assert_eq!(
                hook.state_proof.declared,
                AgentAssetDeclaredStateProofDraft::Definition {
                    declaration_ids: vec![definition.declaration_id.clone()],
                    selected_id: definition.declaration_id.clone(),
                }
            );
        }
    }
    let expected = ids(&declarations
        .iter()
        .filter(|asset| {
            matches!(
                asset.native_payload,
                AgentAssetNativePayload::GrokStateControl(GrokStateControl::PluginDisabled)
            )
        })
        .collect::<Vec<_>>());
    assert_eq!(expected.len(), 2);
    assert!(declarations
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Plugin)
        .all(|asset| asset.declared_state == AgentAssetDeclaredState::Unknown));
    let skill = row(AgentAssetCategory::Skill, "skill");
    assert!(
        matches!(&skill.state_proof.declared, AgentAssetDeclaredStateProofDraft::Unknown { evidence, cause: AgentAssetDeclaredUnknownCauseDraft::StructuralConflict } if evidence.len() == 2)
    );
    assert!(matches!(
        skill.state_proof.effective,
        AgentAssetEffectiveStateProofDraft::Terminal {
            cause: AgentAssetTerminalCauseDraft::StructuralUnknown,
            ..
        }
    ));
    assert_eq!(
        drafts
            .iter()
            .filter(|draft| draft.resolution.relation == AgentAssetResolutionRelation::Replaced)
            .count(),
        1
    );
    for loser in drafts
        .iter()
        .filter(|draft| draft.resolution.relation == AgentAssetResolutionRelation::Replaced)
    {
        assert_eq!(loser.resolution.terminal, None);
        assert!(
            matches!(&loser.state_proof.effective, AgentAssetEffectiveStateProofDraft::Shadowed { input, .. } if **input == AgentAssetEffectiveStateProofDraft::Intrinsic)
        );
    }
    let mut reversed = declarations;
    reversed.reverse();
    assert_eq!(
        format!("{drafts:?}"),
        format!("{:?}", resolve(&reversed, true))
    );
}
