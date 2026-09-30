//! Exact native destination regressions: package exclusion, shared Skill roots,
//! first project Hooks, and Claude project-local MCP nodes in a shared account file.
use super::{
    hook_tests::{assert_verified, edit_request, plan, run, write_json},
    tests::{fixture, fixture_definitions, mcp_request},
    *,
};
use crate::services::agent_cli::environment::{
    config_document::{self, ConfigDocumentFormat},
    mutation::native_test_support::catalog_fixture_inventory,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub(super) struct WorkspaceInspector {
    pub home: PathBuf,
    pub workspace: PathBuf,
    pub settings: AppSettings,
    pub discover_installations: bool,
}

pub(super) fn trust_workspace(home: &Path, workspace: &Path) {
    // Native Grok folder trust keys a real repository root, not an arbitrary
    // cwd whose unseen ancestors could select another root.
    let git = workspace.join(".git");
    fs::create_dir(&git).unwrap();
    fs::create_dir(git.join("objects")).unwrap();
    fs::create_dir_all(git.join("refs/heads")).unwrap();
    fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(
        git.join("config"),
        "[core]\nrepositoryformatversion = 0\nfilemode = true\nbare = false\nlogallrefupdates = true\n",
    )
    .unwrap();
    let workspace = workspace
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let read = |path: &Path, format| match fs::read(path) {
        Ok(bytes) => config_document::parse(&bytes, format).expect("valid native trust fixture"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => panic!("cannot read native trust fixture: {error}"),
    };
    // Populate native inputs once during fixture setup. Inspection must remain
    // read-only, and the production pipeline must derive trust for itself.
    let account = home.join(".claude.json");
    let mut claude = read(&account, ConfigDocumentFormat::Json);
    claude["projects"][&workspace]["hasTrustDialogAccepted"] = json!(true);
    write_json(&account, &claude);
    let config = home.join(".codex/config.toml");
    let mut codex = read(&config, ConfigDocumentFormat::Toml);
    codex["projects"][&workspace]["trust_level"] = json!("trusted");
    fs::write(
        &config,
        config_document::serialize(&codex, ConfigDocumentFormat::Toml).unwrap(),
    )
    .unwrap();
    let folders = home.join(".gemini/trustedFolders.json");
    let mut gemini = read(&folders, ConfigDocumentFormat::Json);
    gemini[&workspace] = json!("TRUST_FOLDER");
    write_json(&folders, &gemini);
    let folders = home.join(".grok/trusted_folders.toml");
    let mut grok = read(&folders, ConfigDocumentFormat::Toml);
    grok["folders"][&workspace]["trusted"] = json!(true);
    fs::write(
        &folders,
        config_document::serialize(&grok, ConfigDocumentFormat::Toml).unwrap(),
    )
    .unwrap();
}

impl MutationInspector for WorkspaceInspector {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError> {
        catalog_fixture_inventory(
            &self.home,
            Some(&self.workspace),
            self.discover_installations.then_some(&self.settings),
            &fixture_definitions(),
        )
        .map_err(|_| AgentAssetMutationError::new(AgentAssetMutationErrorKind::PreparationFailed))
    }
    fn home(&self) -> &Path {
        &self.home
    }
    fn workspace(&self) -> Option<&Path> {
        Some(&self.workspace)
    }
    fn settings(&self) -> &AppSettings {
        &self.settings
    }
}

#[test]
fn plugin_package_named_like_native_config_is_observed_but_never_a_distribution_target() {
    let (_temporary, inspector, service) = fixture();
    let package = inspector
        .home
        .join(".claude/plugins/cache/fixture/demo/1.0.0");
    write_json(
        &package.join(".claude-plugin/plugin.json"),
        &json!({"name":"demo","version":"1.0.0","mcpServers":"./.claude.json"}),
    );
    write_json(
        &package.join(".claude.json"),
        &json!({"mcpServers":{"plugin-tool":{"command":"fixture-runner"}}}),
    );
    write_json(
        &inspector
            .home
            .join(".claude/plugins/installed_plugins.json"),
        &json!({"version":2,"plugins":{"demo@fixture":[{"scope":"user","installPath":package,"version":"1.0.0"}]}}),
    );
    write_json(
        &inspector.home.join(".claude/settings.json"),
        &json!({"enabledPlugins":{"demo@fixture":true}}),
    );
    let snapshot = inspector.inspect().unwrap();
    let source = snapshot
        .inventory
        .sources
        .iter()
        .find(|source| Path::new(&source.path) == package.join(".claude.json"))
        .expect("the real native plugin follow-up must observe the MCP file");
    assert_eq!(source.origin, AgentAssetInstallationOrigin::NativePackage);
    assert!(!source.writable);
    let context = snapshot
        .inventory
        .contexts
        .iter()
        .find(|context| context.id == source.context_id)
        .unwrap();
    assert!(!native::CLAUDE
        .target_sources(&snapshot.inventory, context)
        .iter()
        .any(|candidate| candidate.id == source.id));
    let catalog = service.catalog(&snapshot).unwrap();
    assert!(catalog
        .targets
        .iter()
        .all(|target| !target.label.contains(package.to_string_lossy().as_ref())));
    // Changing a row's advertised writable bit is insufficient: the full native
    // role and path remain authority at both enumeration and plan preparation.
    let source_id = source.id.clone();
    let mut tampered = snapshot;
    tampered
        .inventory
        .sources
        .iter_mut()
        .find(|source| source.id == source_id)
        .unwrap()
        .writable = true;
    let context = tampered
        .inventory
        .contexts
        .iter()
        .find(|context| context.agent_kind == AgentCliKind::ClaudeCode)
        .unwrap();
    assert!(!native::CLAUDE
        .target_sources(&tampered.inventory, context)
        .iter()
        .any(|source| source.id == source_id));
}

fn write_skill(directory: &Path, name: &str) {
    fs::create_dir_all(directory.join("scripts")).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Native target fixture\n---\noriginal-body\n"),
    )
    .unwrap();
    fs::write(
        directory.join("scripts/runner.py"),
        "print('untouched-resource')\n",
    )
    .unwrap();
}

fn target_at(
    service: &CatalogService,
    inspector: &dyn MutationInspector,
    category: AgentAssetCategory,
    kind: AgentCliKind,
    scope: AgentAssetScope,
    directory: &Path,
) -> String {
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    catalog
        .targets
        .iter()
        .find(|target| {
            target.categories == [category]
                && target.agent_kind == kind
                && target.scope == scope
                && projection::resolve_target(&snapshot, &target.id)
                    .is_ok_and(|(_, source, _)| Path::new(&source.path) == directory)
        })
        .expect("native source must have its own destination")
        .id
        .clone()
}

#[test]
fn existing_shared_and_workspace_skills_apply_in_place_with_resources_and_other_directories_preserved(
) {
    for (kind, scope, relative) in [
        (
            AgentCliKind::Codex,
            AgentAssetScope::User,
            ".agents/skills/shared-fixture",
        ),
        (
            AgentCliKind::Gemini,
            AgentAssetScope::Workspace,
            "project/.agents/skills/shared-fixture",
        ),
        (
            AgentCliKind::Codex,
            AgentAssetScope::Workspace,
            "project/.codex/skills/shared-fixture",
        ),
        (
            AgentCliKind::ClaudeCode,
            AgentAssetScope::Workspace,
            "project/.claude/skills/shared-fixture",
        ),
    ] {
        let (_temporary, base, service) = fixture();
        let package = base.home.join(relative);
        write_skill(&package, "shared-fixture");
        let workspace = base.home.join("project");
        fs::create_dir_all(&workspace).unwrap();
        trust_workspace(&base.home, &workspace);
        let inspector = Arc::new(WorkspaceInspector {
            home: base.home.clone(),
            workspace,
            settings: base.settings.clone(),
            discover_installations: false,
        });
        let snapshot = inspector.inspect().unwrap();
        let native = snapshot
            .inventory
            .assets
            .iter()
            .find(|asset| {
                asset.agent_kind == kind
                    && asset.category == AgentAssetCategory::Skill
                    && asset.scope == scope
                    && asset.native_id == "shared-fixture"
            })
            .expect("production inventory must find the target Skill");
        let saved = hook_tests::adopt(&service, inspector.clone(), &native.stable_id);
        let mut update = edit_request(&saved);
        update.skill_markdown = update
            .skill_markdown
            .map(|body| body.replace("original-body", "updated-body"));
        service.save(update).unwrap();
        let target = target_at(
            &service,
            inspector.as_ref(),
            AgentAssetCategory::Skill,
            kind,
            scope,
            package.parent().unwrap(),
        );
        assert_verified(&run(
            &service,
            plan(
                &service,
                inspector.clone(),
                &saved.asset_id,
                AgentCatalogAction::ApplyDefinition,
                vec![target],
            ),
        ));
        assert!(fs::read_to_string(package.join("SKILL.md"))
            .unwrap()
            .contains("updated-body"));
        assert_eq!(
            fs::read_to_string(package.join("scripts/runner.py")).unwrap(),
            "print('untouched-resource')\n"
        );
        if relative.contains(".agents/") {
            assert!(!base.home.join(".codex/skills/shared-fixture").exists());
        }
    }
}

#[test]
fn grok_workspace_skills_apply_without_folder_trust_while_other_targets_remain_gated() {
    let (_temporary, base, service) = fixture();
    let workspace = base.home.join("project");
    let skills = [
        (".grok/skills", "grok-local"),
        (".agents/skills", "grok-shared"),
    ];
    for (directory, name) in skills {
        write_skill(&workspace.join(directory).join(name), name);
    }
    for directory in [".claude", ".codex", ".gemini"] {
        write_skill(
            &workspace.join(directory).join("skills/other-agent"),
            "other-agent",
        );
    }
    let mcp = workspace.join(".grok/config.toml");
    let mcp_before = "[mcp_servers.repo]\ncommand='fixture-runner'\n";
    fs::write(&mcp, mcp_before).unwrap();
    let trust_store = base.home.join(".grok/trusted_folders.toml");
    assert!(!trust_store.exists());
    let inspector = Arc::new(WorkspaceInspector {
        home: base.home.clone(),
        workspace,
        settings: base.settings.clone(),
        discover_installations: false,
    });
    let snapshot = inspector.inspect().unwrap();
    assert_eq!(snapshot.inventory.contexts.len(), 4);
    assert!(snapshot
        .inventory
        .contexts
        .iter()
        .all(|context| context.trust_context != AgentTrustState::Trusted));
    let catalog = service.catalog(&snapshot).unwrap();
    for (kind, expected) in [
        (AgentCliKind::ClaudeCode, 1),
        (AgentCliKind::Codex, 2),
        (AgentCliKind::Gemini, 2),
    ] {
        let targets = catalog
            .targets
            .iter()
            .filter(|target| {
                target.agent_kind == kind
                    && target.scope == AgentAssetScope::Workspace
                    && target.categories == [AgentAssetCategory::Skill]
            })
            .collect::<Vec<_>>();
        assert_eq!(targets.len(), expected, "{kind:?}");
        assert!(targets.iter().all(|target| {
            !target.available
                && target.reason.as_deref() == Some("请先在 Agent 原生工具中信任该项目")
        }));
    }
    let mcp_targets = catalog
        .targets
        .iter()
        .filter(|target| {
            target.agent_kind == AgentCliKind::Grok
                && target.scope == AgentAssetScope::Workspace
                && target.categories == [AgentAssetCategory::Mcp]
        })
        .collect::<Vec<_>>();
    assert_eq!(mcp_targets.len(), 1);
    assert!(!mcp_targets[0].available);
    assert_eq!(
        mcp_targets[0].reason.as_deref(),
        Some("请先在 Agent 原生工具中信任该项目")
    );
    let mcp_definition = service.save(mcp_request("guarded-mcp")).unwrap();
    let blocked = plan(
        &service,
        inspector.clone(),
        &mcp_definition.asset_id,
        AgentCatalogAction::ApplyDefinition,
        vec![mcp_targets[0].id.clone()],
    );
    assert_eq!(blocked.targets.len(), 1);
    assert!(!blocked.targets[0].available);
    assert!(blocked.targets[0].changes.is_empty());
    assert!(blocked.token.is_none() && blocked.plan_id.is_none());

    for (directory, name) in skills {
        let snapshot = inspector.inspect().unwrap();
        let native = snapshot
            .inventory
            .assets
            .iter()
            .find(|asset| {
                asset.agent_kind == AgentCliKind::Grok
                    && asset.category == AgentAssetCategory::Skill
                    && asset.scope == AgentAssetScope::Workspace
                    && asset.native_id == name
            })
            .expect("the production inventory must observe the ordinary project Skill");
        let saved = hook_tests::adopt(&service, inspector.clone(), &native.stable_id);
        let mut request = edit_request(&saved);
        request.skill_markdown = request
            .skill_markdown
            .map(|body| body.replace("original-body", "updated-without-folder-trust"));
        let saved = service.save(request).unwrap();
        let target = target_at(
            &service,
            inspector.as_ref(),
            AgentAssetCategory::Skill,
            AgentCliKind::Grok,
            AgentAssetScope::Workspace,
            &inspector.workspace.join(directory),
        );
        let confirmation = plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![target],
        );
        assert_eq!(confirmation.targets.len(), 1);
        assert_verified(&run(&service, confirmation));
        let package = inspector.workspace.join(directory).join(name);
        assert!(fs::read_to_string(package.join("SKILL.md"))
            .unwrap()
            .contains("updated-without-folder-trust"));
        assert_eq!(
            fs::read_to_string(package.join("scripts/runner.py")).unwrap(),
            "print('untouched-resource')\n"
        );
        let snapshot = inspector.inspect().unwrap();
        assert!(snapshot
            .inventory
            .contexts
            .iter()
            .all(|context| context.trust_context != AgentTrustState::Trusted));
        let catalog = service.catalog(&snapshot).unwrap();
        let item = catalog
            .assets
            .iter()
            .find(|item| item.id == saved.asset_id)
            .unwrap();
        let bindings = item
            .bindings
            .iter()
            .filter(|binding| {
                binding.native.agent_kind == AgentCliKind::Grok
                    && binding.native.scope == AgentAssetScope::Workspace
            })
            .collect::<Vec<_>>();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].applied_version, Some(saved.version));
        assert_eq!(bindings[0].drift, AgentCatalogDrift::InSync);
        assert!(!trust_store.exists());
        assert_eq!(fs::read_to_string(&mcp).unwrap(), mcp_before);
    }
    for directory in [".claude", ".codex", ".gemini"] {
        assert!(fs::read_to_string(
            inspector
                .workspace
                .join(directory)
                .join("skills/other-agent/SKILL.md")
        )
        .unwrap()
        .contains("original-body"));
    }
}

fn grok_hook_request() -> AgentCatalogSaveRequest {
    AgentCatalogSaveRequest {
        asset_id: None,
        expected_version: None,
        name: "项目工具前检查".to_owned(),
        category: AgentAssetCategory::Hook,
        mcp: None,
        skill_markdown: None,
        hook: Some(AgentCatalogHookInput {
            variants: vec![AgentCatalogHookVariantInput {
                agent_kind: AgentCliKind::Grok,
                event: "PreToolUse".to_owned(),
                group_json: json!({"matcher":".*","hooks":[{
                    "type":"command","command":"printf project-user-hook"
                }]})
                .to_string(),
            }],
        }),
    }
}

#[test]
fn grok_first_project_hook_creates_missing_native_file_without_duplicate_sources_or_targets() {
    for directory_exists in [false, true] {
        let (_temporary, base, service) = fixture();
        let workspace = base.home.join("project");
        fs::create_dir(&workspace).unwrap();
        trust_workspace(&base.home, &workspace);
        let directory = workspace.join(".grok/hooks");
        fs::create_dir(workspace.join(".grok")).unwrap();
        if directory_exists {
            fs::create_dir(&directory).unwrap();
        }
        let path = directory.join("hooks.json");
        let neighbor = directory.join("neighbor.json");
        let mcp = workspace.join(".grok/config.toml");
        let mcp_before =
            "# unrelated project configuration\n[mcp_servers.repo]\ncommand='fixture-runner'\n";
        fs::write(&mcp, mcp_before).unwrap();
        let inspector = Arc::new(WorkspaceInspector {
            home: base.home.clone(),
            workspace,
            settings: base.settings.clone(),
            discover_installations: false,
        });
        let destination = |snapshot: &MutationInventory, missing| {
            let sources = snapshot
                .inventory
                .sources
                .iter()
                .filter(|source| Path::new(&source.path) == path)
                .collect::<Vec<_>>();
            assert_eq!(sources.len(), 1, "the fixed file has exactly one source");
            let source = sources[0];
            assert_eq!(source.scope, AgentAssetScope::Workspace);
            assert_eq!(source.origin, AgentAssetInstallationOrigin::LocalFiles);
            assert_eq!(source.source_kind, AgentAssetSourceKind::File);
            assert_eq!(source.categories, [AgentAssetCategory::Hook]);
            assert_eq!(source.revision.is_missing, missing);
            let catalog = service.catalog(snapshot).unwrap();
            let targets = catalog
                .targets
                .iter()
                .filter(|target| {
                    target.agent_kind == AgentCliKind::Grok
                        && target.scope == AgentAssetScope::Workspace
                        && target.categories == [AgentAssetCategory::Hook]
                        && hook_targets::resolve(snapshot, &target.id, None).is_ok_and(|resolved| {
                            resolved.source.id == source.id && resolved.binding.is_none()
                        })
                })
                .collect::<Vec<_>>();
            assert_eq!(targets.len(), 1, "member and receipt targets are distinct");
            assert!(targets[0].available, "{:?}", targets[0].reason);
            (source.id.clone(), targets[0].id.clone())
        };
        let snapshot = inspector.inspect().unwrap();
        assert_eq!(
            snapshot
                .inventory
                .contexts
                .iter()
                .find(|context| context.agent_kind == AgentCliKind::Grok)
                .unwrap()
                .trust_context,
            AgentTrustState::Trusted
        );
        let (source_id, target_id) = destination(&snapshot, true);
        assert!(!snapshot.inventory.assets.iter().any(|asset| {
            asset.agent_kind == AgentCliKind::Grok && asset.category == AgentAssetCategory::Hook
        }));
        let mut saved = service.save(grok_hook_request()).unwrap();
        let mut command = "printf project-user-hook";
        let mut neighbor_before = None;
        for expected_rules in 1..=2 {
            let before = fs::read(&path).ok();
            if expected_rules == 2 {
                write_json(
                    &neighbor,
                    &json!({"unrelated":{"keep":true},"hooks":{"PreToolUse":[{
                        "matcher":".*","hooks":[{
                            "type":"command","command":"printf neighboring-project-hook"
                        }]
                    }]}}),
                );
                neighbor_before = Some(fs::read(&neighbor).unwrap());
                command = "printf updated-project-user-hook";
                saved = hook_tests::update_command(&service, &saved, AgentCliKind::Grok, command);
            }
            assert_eq!(
                fs::read(&path).ok(),
                before,
                "save must not write native files"
            );
            let confirmation = plan(
                &service,
                inspector.clone(),
                &saved.asset_id,
                AgentCatalogAction::ApplyDefinition,
                vec![target_id.clone()],
            );
            assert_eq!(confirmation.targets.len(), 1);
            assert_eq!(
                fs::read(&path).ok(),
                before,
                "plan must not write native files"
            );
            if expected_rules == 1 {
                assert!(!path.exists());
                assert_eq!(directory.exists(), directory_exists);
            }
            assert_verified(&run(&service, confirmation));
            let snapshot = inspector.inspect().unwrap();
            assert_eq!(
                destination(&snapshot, false),
                (source_id.clone(), target_id.clone()),
                "Missing to Present must preserve the source and unbound destination IDs"
            );
            assert_eq!(
                snapshot
                    .inventory
                    .assets
                    .iter()
                    .filter(|asset| {
                        asset.agent_kind == AgentCliKind::Grok
                            && asset.category == AgentAssetCategory::Hook
                            && asset.scope == AgentAssetScope::Workspace
                    })
                    .count(),
                expected_rules as usize
            );
            assert_eq!(
                snapshot
                    .inventory
                    .hook_rule_counts
                    .iter()
                    .find(|count| count.agent_kind == AgentCliKind::Grok)
                    .unwrap()
                    .rule_count,
                Some(expected_rules)
            );
            let (item, binding) =
                hook_tests::binding_for(&service, inspector.as_ref(), AgentCliKind::Grok, command);
            assert_eq!(item.id, saved.asset_id);
            assert_eq!(item.bindings.len(), 1);
            assert_eq!(binding.applied_version, Some(saved.version));
            assert_eq!(binding.drift, AgentCatalogDrift::InSync);
            assert_eq!(binding.native.inspection_source_id, source_id);
            assert_eq!(binding.native.scope, AgentAssetScope::Workspace);
            assert!(matches!(
                binding.native.details,
                AgentAssetDetails::Hook {
                    managed: false,
                    rule_count: Some(1),
                    ..
                }
            ));
            assert_eq!(binding.native.provenance.len(), 1);
            assert_eq!(
                binding.native.provenance[0].installation,
                AgentAssetInstallationOrigin::LocalFiles
            );
            assert_eq!(
                binding.native.provenance[0].scope,
                AgentAssetScope::Workspace
            );
            assert_eq!(fs::read_to_string(&mcp).unwrap(), mcp_before);
            assert_eq!(fs::read(&neighbor).ok(), neighbor_before);
        }
    }
}

#[test]
fn grok_first_project_hook_refuses_external_parent_or_leaf_before_commit() {
    use std::os::unix::fs::symlink;

    for external in ["directory", "symlink", "file"] {
        let (temporary, base, service) = fixture();
        let workspace = base.home.join("project");
        fs::create_dir(&workspace).unwrap();
        trust_workspace(&base.home, &workspace);
        fs::create_dir(workspace.join(".grok")).unwrap();
        let directory = workspace.join(".grok/hooks");
        let path = directory.join("hooks.json");
        let outside = temporary.path().canonicalize().unwrap().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"unselected-directory").unwrap();
        let inspector = Arc::new(WorkspaceInspector {
            home: base.home.clone(),
            workspace,
            settings: base.settings.clone(),
            discover_installations: false,
        });
        let saved = service.save(grok_hook_request()).unwrap();
        let snapshot = inspector.inspect().unwrap();
        let catalog = service.catalog(&snapshot).unwrap();
        let target = catalog
            .targets
            .iter()
            .find(|target| {
                target.agent_kind == AgentCliKind::Grok
                    && target.categories == [AgentAssetCategory::Hook]
                    && hook_targets::resolve(&snapshot, &target.id, None).is_ok_and(|resolved| {
                        resolved.binding.is_none() && Path::new(&resolved.source.path) == path
                    })
            })
            .unwrap();
        let confirmation = plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![target.id.clone()],
        );
        assert!(
            !directory.exists(),
            "planning must retain the missing parent"
        );
        let leaf_before = b"{\"hooks\":{}}\n";
        match external {
            "directory" => fs::create_dir(&directory).unwrap(),
            "symlink" => symlink(&outside, &directory).unwrap(),
            "file" => {
                fs::create_dir(&directory).unwrap();
                fs::write(&path, leaf_before).unwrap();
            }
            _ => unreachable!(),
        }
        let done = run(&service, confirmation);
        assert_eq!(
            done.targets[0].outcome,
            Some(AgentAssetOperationOutcome::UnchangedConflict),
            "{external}: {:?}",
            done.targets
        );
        if external == "file" {
            assert_eq!(fs::read(&path).unwrap(), leaf_before);
        } else {
            assert!(!path.exists());
        }
        assert_eq!(
            fs::symlink_metadata(&directory)
                .unwrap()
                .file_type()
                .is_symlink(),
            external == "symlink"
        );
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"unselected-directory"
        );
        assert!(!outside.join("hooks.json").exists());
        let library: Value = serde_json::from_slice(
            &fs::read(temporary.path().join("library/library.json")).unwrap(),
        )
        .unwrap();
        assert!(library["entries"][&saved.asset_id]["receipts"]
            .as_object()
            .unwrap()
            .is_empty());
    }
}

#[test]
fn shared_skill_batch_converges_for_three_agents_with_distinct_mcp_document_formats() {
    use std::os::unix::fs::PermissionsExt;

    for scope in [AgentAssetScope::User, AgentAssetScope::Workspace] {
        let (_temporary, base, service) = fixture();
        let workspace = base.home.join("project");
        fs::create_dir(&workspace).unwrap();
        trust_workspace(&base.home, &workspace);
        let root = if scope == AgentAssetScope::User {
            base.home.join(".agents/skills")
        } else {
            workspace.join(".agents/skills")
        };
        let package = root.join("shared-fixture");
        write_skill(&package, "shared-fixture");
        let resource = package.join("scripts/runner.py");
        fs::set_permissions(&resource, fs::Permissions::from_mode(0o755)).unwrap();
        let neighbor = root.join("unselected-neighbor");
        write_skill(&neighbor, "unselected-neighbor");
        let neighbor_before = fs::read(neighbor.join("SKILL.md")).unwrap();
        let inspector = Arc::new(WorkspaceInspector {
            home: base.home.clone(),
            workspace,
            settings: base.settings.clone(),
            discover_installations: false,
        });
        let snapshot = inspector.inspect().unwrap();
        let native = snapshot
            .inventory
            .assets
            .iter()
            .find(|asset| {
                asset.agent_kind == AgentCliKind::Codex
                    && asset.category == AgentAssetCategory::Skill
                    && asset.scope == scope
                    && asset.native_id == "shared-fixture"
            })
            .expect("shared fixture must be observed through the production inventory");
        let saved = hook_tests::adopt(&service, inspector.clone(), &native.stable_id);
        let mut request = edit_request(&saved);
        request.skill_markdown = request
            .skill_markdown
            .map(|body| body.replace("original-body", "batch-updated-body"));
        let saved = service.save(request).unwrap();
        let snapshot = inspector.inspect().unwrap();
        let catalog = service.catalog(&snapshot).unwrap();
        let targets = catalog
            .targets
            .iter()
            .filter(|target| {
                target.categories == [AgentAssetCategory::Skill]
                    && target.scope == scope
                    && matches!(
                        target.agent_kind,
                        AgentCliKind::Codex | AgentCliKind::Gemini | AgentCliKind::Grok
                    )
                    && projection::resolve_target(&snapshot, &target.id)
                        .is_ok_and(|(_, source, _)| Path::new(&source.path) == root)
            })
            .collect::<Vec<_>>();
        assert_eq!(targets.len(), 3);
        assert_eq!(
            targets
                .iter()
                .map(|target| target.agent_kind)
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
        let confirmation = plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            targets.iter().map(|target| target.id.clone()).collect(),
        );
        let done = run(&service, confirmation);
        assert_eq!(done.targets.len(), 3);
        assert_verified(&done);
        assert!(fs::read_to_string(package.join("SKILL.md"))
            .unwrap()
            .contains("batch-updated-body"));
        assert_eq!(
            fs::read_to_string(&resource).unwrap(),
            "print('untouched-resource')\n"
        );
        assert_ne!(
            fs::metadata(resource).unwrap().permissions().mode() & 0o100,
            0
        );
        assert_eq!(
            fs::read(neighbor.join("SKILL.md")).unwrap(),
            neighbor_before
        );
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        let item = catalog
            .assets
            .iter()
            .find(|item| item.id == saved.asset_id)
            .unwrap();
        assert_eq!(item.bindings.len(), 3);
        assert!(item.unresolved_targets.is_empty());
        assert!(item.bindings.iter().all(|binding| {
            binding.applied_version == Some(saved.version)
                && binding.drift == AgentCatalogDrift::InSync
                && binding.native.scope == scope
        }));
        for directory in [".codex", ".gemini", ".grok"] {
            assert!(!base
                .home
                .join(directory)
                .join("skills/shared-fixture")
                .exists());
            assert!(!inspector
                .workspace
                .join(directory)
                .join("skills/shared-fixture")
                .exists());
        }
    }
}

#[test]
fn claude_local_mcp_and_same_named_user_mcp_have_distinct_targets_and_composed_writeback() {
    let (_temporary, base, service) = fixture();
    let workspace = base.home.join("project");
    fs::create_dir_all(&workspace).unwrap();
    let account = base.home.join(".claude.json");
    let before = json!({"keep":{"opaque":"unchanged"},"mcpServers":{"same":{"command":"user-runner","args":["user-original"]}},
        "projects":{(workspace.to_string_lossy().as_ref()):{"hasTrustDialogAccepted":true,"keepLocal":7,"mcpServers":{"same":{"command":"local-runner","args":["local-original"]}}}}});
    write_json(&account, &before);
    trust_workspace(&base.home, &workspace);
    let inspector = Arc::new(WorkspaceInspector {
        home: base.home.clone(),
        workspace: workspace.clone(),
        settings: base.settings.clone(),
        discover_installations: false,
    });
    let snapshot = inspector.inspect().unwrap();
    let local = snapshot
        .inventory
        .assets
        .iter()
        .find(|asset| {
            asset.agent_kind == AgentCliKind::ClaudeCode
                && asset.category == AgentAssetCategory::Mcp
                && asset.scope == AgentAssetScope::Local
                && asset.native_id == "same"
        })
        .expect("Local MCP must be a real native record");
    let saved = hook_tests::adopt(&service, inspector.clone(), &local.stable_id);
    assert_eq!(
        saved.mcp.as_ref().unwrap().command.as_deref(),
        Some("local-runner")
    );
    let mut edit = edit_request(&saved);
    edit.mcp.as_mut().unwrap().args = vec!["local-edited".into()];
    service.save(edit).unwrap();
    let local_target = target_at(
        &service,
        inspector.as_ref(),
        AgentAssetCategory::Mcp,
        AgentCliKind::ClaudeCode,
        AgentAssetScope::Local,
        &account,
    );
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![local_target.clone()],
        ),
    ));
    let value: Value = serde_json::from_slice(&fs::read(&account).unwrap()).unwrap();
    assert_eq!(value["mcpServers"], before["mcpServers"]);
    assert_eq!(
        value["projects"][workspace.to_string_lossy().as_ref()]["mcpServers"]["same"]["args"],
        json!(["local-edited"])
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let user = catalog
        .assets
        .iter()
        .flat_map(|item| &item.bindings)
        .find(|binding| {
            binding.native.agent_kind == AgentCliKind::ClaudeCode
                && binding.native.category == AgentAssetCategory::Mcp
                && binding.native.scope == AgentAssetScope::User
                && binding.native.native_id == "same"
        })
        .unwrap();
    let source = catalog
        .assets
        .iter()
        .find(|item| item.bindings.iter().any(|binding| binding.id == user.id))
        .unwrap();
    let relation = service
        .preview_relation(
            "fixture",
            AgentCatalogRelationPreviewRequest {
                intent: AgentCatalogRelationIntent::Merge {
                    destination_asset_id: saved.asset_id.clone(),
                    source_asset_id: source.id.clone(),
                },
                expected_revision: catalog.revision,
                workspace: Some(workspace.to_string_lossy().into_owned()),
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(relation.available, "{:?}", relation.reason);
    service
        .commit_relation(
            "fixture",
            AgentCatalogRelationCommitRequest {
                plan_token: relation.token.unwrap(),
                relation_key: relation.relation_key,
                action: relation.action,
            },
        )
        .unwrap();
    let mut edit = edit_request(&service.definition(&saved.asset_id).unwrap());
    edit.mcp.as_mut().unwrap().args = vec!["both-selected".into()];
    service.save(edit).unwrap();
    let user_target = target_at(
        &service,
        inspector.as_ref(),
        AgentAssetCategory::Mcp,
        AgentCliKind::ClaudeCode,
        AgentAssetScope::User,
        &account,
    );
    assert_ne!(user_target, local_target);
    let operation = run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![user_target, local_target],
        ),
    );
    assert_eq!(operation.targets.len(), 2);
    assert_verified(&operation);
    let value: Value = serde_json::from_slice(&fs::read(&account).unwrap()).unwrap();
    assert_eq!(
        value["mcpServers"]["same"]["args"],
        json!(["both-selected"])
    );
    assert_eq!(
        value["projects"][workspace.to_string_lossy().as_ref()]["mcpServers"]["same"]["args"],
        json!(["both-selected"])
    );
    assert_eq!(value["keep"], before["keep"]);
    assert_eq!(
        value["projects"][workspace.to_string_lossy().as_ref()]["keepLocal"],
        7
    );
}

#[test]
fn malformed_mcp_container_is_unknown_and_not_a_missing_definition() {
    let (_temporary, inspector, service) = fixture();
    let saved = service.save(mcp_request("configured")).unwrap();
    let target = target_at(
        &service,
        inspector.as_ref(),
        AgentAssetCategory::Mcp,
        AgentCliKind::ClaudeCode,
        AgentAssetScope::User,
        &inspector.home.join(".claude.json"),
    );
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![target],
        ),
    ));
    write_json(
        &inspector.home.join(".claude.json"),
        &json!({"mcpServers":[]}),
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|item| item.id == saved.asset_id)
        .unwrap();
    assert_eq!(
        item.unresolved_targets[0].state,
        AgentCatalogUnresolvedState::Unknown
    );
}
