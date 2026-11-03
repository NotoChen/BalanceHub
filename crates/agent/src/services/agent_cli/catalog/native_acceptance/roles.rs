//! Additional native destination locations reuse the existing catalog producer.
use super::{copy_stage_directory, *};
use crate::services::agent_cli::catalog::{
    hook_tests::{adopt, assert_verified, edit_request, plan, run, write_json},
    target_regressions::{trust_workspace, WorkspaceInspector},
};
use serde_json::{json, Value};

struct Case {
    kind: AgentCliKind,
    category: AgentAssetCategory,
    role: &'static str,
    scope: AgentAssetScope,
}

impl Case {
    fn directory(&self) -> &'static str {
        match self.kind {
            AgentCliKind::ClaudeCode => ".claude",
            AgentCliKind::Codex => ".codex",
            AgentCliKind::Gemini => ".gemini",
            AgentCliKind::Grok => ".grok",
        }
    }

    fn source_path(&self, home: &Path, workspace: &Path) -> PathBuf {
        match (self.category, self.role) {
            (AgentAssetCategory::Skill, "userShared") => home.join(".agents/skills"),
            (AgentAssetCategory::Skill, "workspaceShared") => workspace.join(".agents/skills"),
            (AgentAssetCategory::Skill, _) => workspace.join(self.directory()).join("skills"),
            (AgentAssetCategory::Mcp, "local") => home.join(".claude.json"),
            (AgentAssetCategory::Mcp, _) if self.kind == AgentCliKind::ClaudeCode => {
                workspace.join(".mcp.json")
            }
            (AgentAssetCategory::Mcp, _) => {
                workspace
                    .join(self.directory())
                    .join(if self.kind == AgentCliKind::Gemini {
                        "settings.json"
                    } else {
                        "config.toml"
                    })
            }
            _ => unreachable!("only Skill and MCP native locations are certified here"),
        }
    }

    fn key(&self) -> String {
        format!(
            "{}-{}-{}",
            self.kind.key(),
            if self.category == AgentAssetCategory::Skill {
                "skill"
            } else {
                "mcp"
            },
            self.role
        )
    }
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for kind in [
        AgentCliKind::ClaudeCode,
        AgentCliKind::Codex,
        AgentCliKind::Gemini,
        AgentCliKind::Grok,
    ] {
        for category in [AgentAssetCategory::Skill, AgentAssetCategory::Mcp] {
            cases.push(Case {
                kind,
                category,
                role: "workspaceNative",
                scope: AgentAssetScope::Workspace,
            });
        }
        if kind != AgentCliKind::ClaudeCode {
            cases.push(Case {
                kind,
                category: AgentAssetCategory::Skill,
                role: "userShared",
                scope: AgentAssetScope::User,
            });
            cases.push(Case {
                kind,
                category: AgentAssetCategory::Skill,
                role: "workspaceShared",
                scope: AgentAssetScope::Workspace,
            });
        }
    }
    cases.push(Case {
        kind: AgentCliKind::ClaudeCode,
        category: AgentAssetCategory::Mcp,
        role: "local",
        scope: AgentAssetScope::Local,
    });
    cases
}

fn request(case: &Case, name: &str) -> AgentCatalogSaveRequest {
    AgentCatalogSaveRequest {
        asset_id: None, expected_version: None, name: name.to_owned(), category: case.category, hook: None,
        skill_markdown: (case.category == AgentAssetCategory::Skill).then(|| format!("---\nname: {name}\ndescription: User role fixture created\n---\nrole-created-body\n")),
        mcp: (case.category == AgentAssetCategory::Mcp).then(|| AgentCatalogMcpInput { transport: Some(AgentMcpTransport::Stdio),
            command: Some("/usr/bin/true".to_owned()), args: vec!["role-created".to_owned()], url: None, cwd: None, environment: BTreeMap::new(), headers: BTreeMap::new(), connection_options: BTreeMap::new() }),
    }
}

fn expected(case: &Case, name: &str, native_path: &Path, updated: bool) -> Value {
    if case.category == AgentAssetCategory::Skill {
        json!({"name":name,"nativePath":native_path,"description":if updated {"User role fixture updated"} else {"User role fixture created"},
            "bodyMarker":if updated {"role-updated-body"} else {"role-created-body"},
            "resources": if updated { vec![json!({"relativePath":"scripts/user-resource.py","content":"print('user-resource-preserved')\n"})] } else {Vec::new()} })
    } else {
        let mut value = json!({"name":name,"nativePath":native_path,"command":"/usr/bin/true","args":[if updated {"role-updated"} else {"role-created"}]});
        if case.role == "local" {
            value["unselectedUserMcp"] =
                json!({"command":"/usr/bin/true","args":["unselected-user"]});
        }
        value
    }
}

fn stage(
    stage_root: &Path,
    inspector: &WorkspaceInspector,
    action: &str,
    expected: Value,
) -> Value {
    let root = stage_root.join(action);
    fs::create_dir(&root).unwrap();
    let home = root.join("home");
    let workspace = root.join("workspace");
    copy_stage_directory(&inspector.home, &home);
    copy_stage_directory(&inspector.workspace, &workspace);
    json!({"action":action,"home":home,"workspace":workspace,"originalHome":inspector.home,"originalWorkspace":inspector.workspace,"expected":expected})
}

pub(super) fn produce(root: &Path, cli_paths: &BTreeMap<AgentCliKind, String>) -> Vec<Value> {
    let work_root = root.join("catalog-role-work");
    let stages_root = root.join("catalog-role-stages");
    fs::create_dir(&work_root).unwrap();
    fs::create_dir(&stages_root).unwrap();
    let mut records = Vec::new();
    for case in cases() {
        let work = work_root.join(case.key());
        let stage_root = stages_root.join(case.key());
        fs::create_dir(&work).unwrap();
        fs::create_dir(&stage_root).unwrap();
        let home = work.join("home");
        let workspace = work.join("workspace");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&workspace).unwrap();
        for directory in [".claude", ".codex", ".gemini", ".grok"] {
            fs::create_dir(home.join(directory)).unwrap();
        }
        let source_path = case.source_path(&home, &workspace);
        fs::create_dir_all(if case.category == AgentAssetCategory::Skill {
            &source_path
        } else {
            source_path.parent().unwrap()
        })
        .unwrap();
        let name = format!("user-{}-role-fixture", case.kind.key());
        if case.role == "local" {
            write_json(
                &source_path,
                &json!({"mcpServers":{(&name):{"command":"/usr/bin/true","args":["unselected-user"]}},
                "projects":{(workspace.to_string_lossy().as_ref()):{"hasTrustDialogAccepted":true,"roleSentinel":"keep"}}}),
            );
        }
        trust_workspace(&home, &workspace);
        let mut settings = AppSettings::default();
        for (&kind, path) in cli_paths {
            settings.set_agent_cli_path(kind, path.clone());
        }
        let inspector = Arc::new(WorkspaceInspector {
            home,
            workspace,
            settings,
            discover_installations: true,
        });
        let service =
            CatalogService::new(work.join("library"), Arc::new(MutationService::default()));

        let saved = service.save(request(&case, &name)).unwrap();
        let snapshot = inspector.inspect().unwrap();
        let catalog = service.catalog(&snapshot).unwrap();
        let target = catalog
            .targets
            .iter()
            .find(|target| {
                target.agent_kind == case.kind
                    && target.scope == case.scope
                    && target.categories == [case.category]
                    && projection::resolve_target(&snapshot, &target.id)
                        .is_ok_and(|(_, source, _)| Path::new(&source.path) == source_path)
            })
            .expect("the exact native role must be a real target");
        assert!(target.available);
        let target_id = target.id.clone();
        let context = snapshot
            .inventory
            .contexts
            .iter()
            .find(|context| context.id == target.context_id)
            .unwrap();
        let version = snapshot
            .inventory
            .installations
            .iter()
            .find(|installation| {
                installation.agent_kind == case.kind
                    && installation.availability == AgentInstallationAvailability::Available
                    && context
                        .compatible_installation_ids
                        .contains(&installation.id)
            })
            .and_then(|installation| installation.installed_version.clone())
            .expect("exact native CLI version is required");
        assert!(!version.starts_with("source:"));
        assert_verified(&run(
            &service,
            plan(
                &service,
                inspector.clone(),
                &saved.asset_id,
                AgentCatalogAction::ApplyDefinition,
                vec![target_id.clone()],
            ),
        ));
        let native_path = if case.category == AgentAssetCategory::Skill {
            source_path.join(&name).join("SKILL.md")
        } else {
            source_path.clone()
        };
        let created = stage(
            &stage_root,
            &inspector,
            "created",
            expected(&case, &name, &native_path, false),
        );
        let current = if case.category == AgentAssetCategory::Skill {
            let script = native_path
                .parent()
                .unwrap()
                .join("scripts/user-resource.py");
            fs::create_dir_all(script.parent().unwrap()).unwrap();
            fs::write(script, "print('user-resource-preserved')\n").unwrap();
            let snapshot = inspector.inspect().unwrap();
            let catalog = service.catalog(&snapshot).unwrap();
            let binding = catalog
                .assets
                .iter()
                .find(|item| item.id == saved.asset_id)
                .unwrap()
                .bindings
                .iter()
                .find(|binding| {
                    binding.native.agent_kind == case.kind && binding.native.scope == case.scope
                })
                .unwrap();
            adopt(&service, inspector.clone(), &binding.id)
        } else {
            saved.clone()
        };
        let mut edit = edit_request(&current);
        if let Some(markdown) = &mut edit.skill_markdown {
            *markdown = markdown
                .replace("role-created-body", "role-updated-body")
                .replace("User role fixture created", "User role fixture updated");
        }
        if let Some(mcp) = &mut edit.mcp {
            mcp.args = vec!["role-updated".to_owned()];
        }
        service.save(edit).unwrap();
        assert_verified(&run(
            &service,
            plan(
                &service,
                inspector.clone(),
                &saved.asset_id,
                AgentCatalogAction::ApplyDefinition,
                vec![target_id],
            ),
        ));
        let updated = stage(
            &stage_root,
            &inspector,
            "updated",
            expected(&case, &name, &native_path, true),
        );
        if case.role == "local" {
            let account: Value = serde_json::from_slice(&fs::read(&source_path).unwrap()).unwrap();
            assert_eq!(
                account["mcpServers"][&name]["args"],
                json!(["unselected-user"])
            );
        }
        if case.category == AgentAssetCategory::Skill {
            assert!(fs::read_to_string(&native_path)
                .unwrap()
                .contains("role-updated-body"));
            assert_eq!(
                fs::read_to_string(
                    native_path
                        .parent()
                        .unwrap()
                        .join("scripts/user-resource.py")
                )
                .unwrap(),
                "print('user-resource-preserved')\n"
            );
        }
        records.push(json!({"agentKind":case.kind,"category":case.category,"destinationRole":case.role,"scope":case.scope,"version":version,"stages":[created,updated]}));
    }
    records
}
