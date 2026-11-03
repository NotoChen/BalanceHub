//! Gemini 0.59 drops local admin objects before applying session-only policy.
use super::skill_state::inventory;
use super::*;

fn assert_local_rows(inventory: &AgentEnvironmentInventory) {
    assert_eq!(inventory.assets.len(), 2, "{:?}", inventory.assets);
    assert_eq!(inventory.declarations.len(), 2);
    assert_eq!(
        inventory
            .assets
            .iter()
            .map(|asset| (asset.category, asset.native_id.as_str()))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            (AgentAssetCategory::Mcp, "healthy"),
            (AgentAssetCategory::StatusUi, "footer"),
        ])
    );
    assert!(inventory
        .declarations
        .iter()
        .all(|declaration| declaration.role == AgentAssetDeclarationRole::Definition));
    for asset in &inventory.assets {
        assert_eq!(asset.declared_state, AgentAssetState::Enabled);
        assert_eq!(asset.effective_state, AgentAssetState::Enabled);
        assert_eq!(
            asset.resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert!(asset.resolution.control_source.is_none());
        assert!(asset.resolution.terminal.is_none());
    }
    let healthy = inventory
        .assets
        .iter()
        .find(|asset| asset.native_id == "healthy")
        .unwrap();
    assert!(matches!(
        healthy.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Stdio,
            effective_availability: AgentAssetEffectiveAvailability::Available,
            ..
        }
    ));
    assert!(inventory
        .diagnostics
        .iter()
        .chain(
            inventory
                .sources
                .iter()
                .flat_map(|source| &source.diagnostics)
        )
        .chain(inventory.assets.iter().flat_map(|asset| &asset.diagnostics))
        .all(|diagnostic| !matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. })));
}

#[test]
fn real_inventory_ignores_local_admin_in_every_settings_scope() {
    let home = inventory_test_root("local-admin-all-scopes");
    let workspace = home.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let home = fs::canonicalize(home).unwrap();
    let workspace = fs::canonicalize(workspace).unwrap();
    let admin = serde_json::json!({"mcp": {
        "enabled": false,
        "config": {
            "healthy": {"httpUrl": "https://private-admin-rewrite.invalid"},
            "config-only": {"command": "private-admin-command"}
        },
        "requiredConfig": {"phantom": {"command": "private-required-command"}}
    }});
    let settings_path = home.join(".gemini/settings.json");
    let documents = [
        (
            settings_path.clone(),
            serde_json::json!({
                "admin": admin.clone(),
                "mcpServers": {"healthy": {"command": "private-local-command"}},
                "ui": {"footer": true}
            }),
        ),
        (
            home.join(".gemini/managed-fixture/system-defaults.json"),
            serde_json::json!({"admin": admin.clone()}),
        ),
        (
            home.join(".gemini/managed-fixture/system-settings.json"),
            serde_json::json!({"admin": admin.clone()}),
        ),
        (
            workspace.join(".gemini/settings.json"),
            serde_json::json!({"admin": admin}),
        ),
    ];
    let mut original_bytes = Vec::new();
    for (path, document) in documents {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = serde_json::to_vec(&document).unwrap();
        fs::write(&path, &bytes).unwrap();
        original_bytes.push((path, bytes));
    }
    fs::write(
        home.join(".gemini/trustedFolders.json"),
        serde_json::to_vec(&BTreeMap::from([(
            workspace.to_string_lossy().into_owned(),
            "TRUST_FOLDER",
        )]))
        .unwrap(),
    )
    .unwrap();

    let inventory = inventory(&home, Some(&workspace));
    assert_local_rows(&inventory);
    assert!(inventory
        .contexts
        .iter()
        .all(|context| context.trust_context == AgentTrustState::Trusted));
    let healthy = inventory
        .assets
        .iter()
        .find(|asset| asset.native_id == "healthy")
        .unwrap();
    assert_eq!(
        healthy.path.as_deref().map(Path::new),
        Some(settings_path.as_path())
    );
    for (path, bytes) in original_bytes {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    let public = format!(
        "{}\n{inventory:?}",
        serde_json::to_string(&inventory).unwrap()
    );
    for sentinel in [
        "private-admin-rewrite",
        "private-admin-command",
        "private-required-command",
        "private-local-command",
        "phantom",
        "config-only",
    ] {
        assert!(
            !public.contains(sentinel),
            "local admin/private payload leaked: {sentinel}"
        );
    }
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn real_inventory_ignores_malformed_local_admin_without_poisoning_mcp() {
    let home = inventory_test_root("local-admin-malformed");
    fs::create_dir_all(home.join(".gemini")).unwrap();
    let home = fs::canonicalize(home).unwrap();
    for admin in [
        serde_json::Value::Null,
        serde_json::json!(false),
        serde_json::json!([]),
        serde_json::json!({"mcp": false}),
        serde_json::json!({"mcp": {"enabled": "invalid", "config": false, "requiredConfig": []}}),
    ] {
        fs::write(
            home.join(".gemini/settings.json"),
            serde_json::to_vec(&serde_json::json!({
                "admin": admin,
                "mcpServers": {"healthy": {"command": "fixture-runner"}},
                "ui": {"footer": true}
            }))
            .unwrap(),
        )
        .unwrap();
        assert_local_rows(&inventory(&home, None));
    }
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn local_admin_cannot_replace_a_real_local_policy_authority() {
    for enabled in [false, true] {
        let document = serde_json::to_vec(&serde_json::json!({
            "admin": {"mcp": {
                "enabled": enabled,
                "config": {"healthy": {"httpUrl": "https://ignored.invalid"}},
                "requiredConfig": {"phantom": {"command": "ignored"}}
            }},
            "mcp": {"allowed": []},
            "mcpServers": {"healthy": {"command": "fixture-runner"}},
            "footer": true
        }))
        .unwrap();
        let trace = run_d2_fixture(&document, AgentTrustState::Trusted);
        assert_eq!(trace.records.len(), 2);
        assert_eq!(trace.declarations.len(), 3);
        let policy = trace
            .declarations
            .iter()
            .find(|declaration| declaration.declaration_key == "mcp.allowed")
            .unwrap();
        let healthy = record(&trace, AgentAssetCategory::Mcp, "healthy");
        assert_eq!(healthy.declared_state, AgentAssetState::Enabled);
        assert_eq!(healthy.effective_state, AgentAssetState::Blocked);
        assert_eq!(
            healthy.resolution.control_source,
            Some(AgentAssetPolicyReference::Declaration {
                declaration_id: policy.declaration_id.clone(),
            })
        );
        assert!(matches!(
            healthy.details,
            AgentAssetDetails::Mcp {
                transport: AgentMcpTransport::Stdio,
                effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
                ..
            }
        ));
        let draft = trace
            .drafts
            .iter()
            .find(|draft| draft.native_id == "healthy")
            .unwrap();
        assert!(
            matches!(&draft.state_proof.effective, AgentAssetEffectiveStateProofDraft::Terminal { evidence, .. }
            if evidence == &vec![AgentAssetStateEvidenceRefDraft::Declaration { declaration_id: policy.declaration_id.clone() }])
        );
        assert_eq!(
            record(&trace, AgentAssetCategory::StatusUi, "footer").effective_state,
            AgentAssetState::Enabled
        );
        assert_no_projection_diagnostics(&trace);
    }
}
