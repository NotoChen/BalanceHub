//! Visual editing prepares native document drafts. The configuration service
//! remains the only owner of file authorization, diff plans and writes.
mod bindings;
pub(super) mod document;
mod gemini_model;
mod mapping_resets;
mod navigation;

use super::{sources, *};
use crate::services::agent_cli::configuration::{
    contracts::AgentConfigurationSourceSpec, sources::ConfigurationSourceAuthority,
};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn supports_form(kind: AgentCliKind, spec: &AgentConfigurationSourceSpec) -> bool {
    if !matches!(
        spec.format,
        AgentConfigurationFormat::Toml
            | AgentConfigurationFormat::Json
            | AgentConfigurationFormat::Jsonc
    ) {
        return false;
    }
    let role = spec.native.native_source_key.as_str();
    match kind {
        AgentCliKind::Codex => {
            matches!(role, "config" | "workspace-config" | "system-config")
                || role.starts_with("ancestor-config:")
                || role.starts_with("profile:")
        }
        AgentCliKind::ClaudeCode => matches!(
            role,
            "settings"
                | "workspace-settings"
                | "workspace-local-settings"
                | "workspace-root-local-settings"
                | "managed-settings"
        ),
        AgentCliKind::Gemini => matches!(
            role,
            "settings" | "workspace-settings" | "system-defaults" | "system-settings"
        ),
        AgentCliKind::Grok => matches!(role, "config" | "workspace-config"),
    }
}

#[derive(Clone)]
struct FormContext {
    inherited: NativeSnapshot,
    draft: NativeSnapshot,
    effective: NativeSnapshot,
    help: String,
    cli_path: String,
    active_layer: bool,
}

fn context(
    home: &Path,
    source: &ConfigurationSourceAuthority,
    cli_path: &str,
) -> Result<FormContext, String> {
    let mut snapshot = sources::read_context(
        home,
        &source.context,
        source.source.workspace.as_deref().map(Path::new),
        &[],
    )?;
    if matches!(source.source.runtime_scope, AgentRuntimeScope::Native) {
        codex::load_features(&mut snapshot, cli_path);
    }
    let path = &source.spec.native.path;
    let active_layer = (snapshot.layers.iter().any(|layer| layer.path == *path)
        || (!matches!(
            source.source.scope,
            AgentAssetScope::Workspace | AgentAssetScope::Local
        ) || source.context.trust_context == AgentTrustState::Trusted))
        && source
            .source
            .profile
            .as_ref()
            .is_none_or(|profile| *profile == source.context.profile);
    snapshot.layers.retain(|layer| layer.path != *path);
    for failure in snapshot
        .read_failures
        .iter()
        .filter(|failure| failure.path == *path)
    {
        snapshot.notices.retain(|notice| notice != &failure.message);
    }
    snapshot
        .read_failures
        .retain(|failure| failure.path != *path);
    let cli_path = if matches!(source.source.runtime_scope, AgentRuntimeScope::Native) {
        cli_path
    } else {
        ""
    };
    let help = if cli_path.is_empty() {
        String::new()
    } else {
        read_help(cli_path).unwrap_or_default()
    };
    Ok(FormContext {
        inherited: snapshot.clone(),
        draft: snapshot.clone(),
        effective: snapshot,
        help,
        cli_path: cli_path.into(),
        active_layer,
    })
}

fn target_model(root: &Value, snapshot: &NativeSnapshot) -> String {
    (snapshot.kind == AgentCliKind::ClaudeCode)
        .then(|| root.pointer("/env/ANTHROPIC_MODEL"))
        .flatten()
        .or_else(|| root.pointer(model_pointer(snapshot.kind)))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| model_observed(snapshot).map(|value| value.text()))
        .unwrap_or_default()
}

fn bound_fields(
    snapshot: &NativeSnapshot,
    model: &str,
    context: &FormContext,
) -> Vec<BoundSetting> {
    let mut fields = fields_for(
        snapshot,
        model,
        &context.help,
        &context.cli_path,
        true,
        None,
    );
    let mut model_field = BoundSetting::new(
            "model",
            "model",
            "默认模型",
            model_pointer(snapshot.kind),
            Binding::Readonly,
        )
        .custom()
        .describe("优先列出原生配置和本机模型目录中的模型；也可以输入网关提供的模型 ID。模型是否可用仍由当前账号、服务端与 Agent 管理规则决定。")
        .current(model_observed(snapshot), "未设置");
    display::set_summary(&mut model_field.ui, display::default_model(snapshot));
    fields.insert(0, model_field);
    bindings::additional_fields(snapshot, &mut fields);
    display::annotate(&mut fields, snapshot);
    fields
}

impl FormContext {
    fn overlay(&mut self, source: &ConfigurationSourceAuthority, root: &Value) {
        self.effective = self.inherited.clone();
        self.draft = self.inherited.clone();
        self.draft.layers.push(sources::Layer {
            value: root.clone(),
            path: source.spec.native.path.clone(),
            scope: source.source.scope,
            role: source.source.native_role.clone(),
            requirements: false,
            precedence: source.spec.native.precedence,
        });
        self.draft
            .layers
            .sort_by_key(|layer| (layer.precedence, layer.path.components().count()));
        if self.active_layer {
            self.effective = self.draft.clone();
        } else if source.source.profile.is_none() {
            self.effective
                .notices
                .push("项目配置将在 Agent 信任此目录后加载；当前生效值按已加载的来源显示。".into());
        }
        if source.source.profile.is_some() {
            self.effective
                .notices
                .push("此配置方案在 Agent 选择对应 profile 时生效。".into());
        }
    }
}

fn render_read(
    source: &ConfigurationSourceAuthority,
    text: &str,
    root: &Value,
    mut context: FormContext,
) -> Result<AgentConfigurationForm, String> {
    let model = target_model(root, &context.inherited);
    context.overlay(source, root);
    let effective_model = model_observed(&context.effective)
        .map(|value| value.text())
        .unwrap_or_default();
    let effective_fields = (!context.active_layer || effective_model != model)
        .then(|| bound_fields(&context.effective, &effective_model, &context));
    let mut values = BTreeMap::new();
    let mut effective = BTreeMap::new();
    let mut inherited = BTreeMap::new();
    let mut fields = Vec::new();
    let mut source_paths = Vec::new();
    for mut field in bound_fields(&context.draft, &model, &context) {
        let Some(storage) = bindings::storage(&mut field, source, &model) else {
            continue;
        };
        if let Some(value) = storage.read(root, &model) {
            if field.ui.control == AgentSettingControl::ModelMappings {
                mapping_resets::fill(&mut field.ui, &value, source, root, &context);
            }
            values.insert(field.ui.id.clone(), value);
            // Resetting one setting keeps all other values in this file. In
            // particular, removing autoCompact must still use this draft's
            // model/contextWindow, and removing model must preserve env keys.
            let mut removed = root.clone();
            storage.write(&mut removed, &model, None, &context.draft)?;
            let mut fallback = context.clone();
            fallback.overlay(source, &removed);
            let fallback_model = target_model(&removed, &context.inherited);
            if let Some(fallback_field) = bound_fields(&fallback.draft, &fallback_model, &fallback)
                .into_iter()
                .find(|item| item.ui.id == field.ui.id)
            {
                inherited.insert(field.ui.id.clone(), display::effective(&fallback_field.ui));
            }
        } else {
            inherited.insert(field.ui.id.clone(), display::effective(&field.ui));
        }
        let observed = effective_fields
            .as_ref()
            .and_then(|fields| fields.iter().find(|item| item.ui.id == field.ui.id))
            .unwrap_or(&field);
        effective.insert(field.ui.id.clone(), display::effective(&observed.ui));
        if field.ui.id == "model" {
            field.ui.choices = models::choices(&context.draft);
        }
        source_paths.push(navigation::paths(&storage, &field.ui, root, &model));
        fields.push(field.ui);
    }
    navigation::attach(&mut fields, source_paths, text, root, source.spec.format)?;
    let model_catalog = models::context(&context.draft, &context.cli_path);
    let mut notices = context.effective.notices;
    notices.sort();
    notices.dedup();
    Ok(AgentConfigurationForm {
        source_id: source.source.source_id.clone(),
        scope: source.source.scope,
        profile: source.source.profile.clone(),
        text: text.into(),
        fields,
        model_catalog,
        values,
        effective,
        inherited,
        notices,
    })
}

pub(crate) fn read_form(
    home: &Path,
    source: &ConfigurationSourceAuthority,
    cli_path: &str,
    text: &str,
) -> Result<AgentConfigurationForm, String> {
    let root = document::parse(text, source.spec.format)?;
    render_read(source, text, &root, context(home, source, cli_path)?)
}

pub(crate) fn render_form(
    home: &Path,
    source: &ConfigurationSourceAuthority,
    cli_path: &str,
    text: &str,
    changes: &BTreeMap<String, Option<String>>,
) -> Result<AgentConfigurationForm, String> {
    if changes.is_empty() {
        return read_form(home, source, cli_path, text);
    }
    let mut root = document::parse(text, source.spec.format)?;
    let mut context = context(home, source, cli_path)?;
    context.overlay(source, &root);
    // Model-dependent controls are resolved against the model selected in this draft.
    if let Some(value) = changes.get("model") {
        let mut field = BoundSetting::new(
            "model",
            "model",
            "默认模型",
            model_pointer(source.source.agent_kind),
            Binding::Readonly,
        )
        .custom();
        let storage = bindings::storage(&mut field, source, "")
            .ok_or("此配置范围不支持设置默认模型，请使用用户配置")?;
        if let Some(value) = value {
            if value.trim().is_empty() || value.chars().any(char::is_control) || value.len() > 512 {
                return Err("请输入有效的模型名称，或移除此项设置。".into());
            }
        }
        storage.write(&mut root, "", value.as_deref(), &context.draft)?;
        context.overlay(source, &root);
    }
    let model = target_model(&root, &context.inherited);
    // These choices determine the native concurrency binding and the child
    // model's allowed reasoning levels. Resolve them before dependent values.
    let dependency_ids = ["modelMappings", "subagents", "subagentModel"];
    for id in dependency_ids {
        let Some(value) = changes.get(id) else {
            continue;
        };
        let mut field = bound_fields(&context.draft, &model, &context)
            .into_iter()
            .find(|field| field.ui.id == id)
            .ok_or("当前 Agent 不支持此可视化配置项，请重新读取")?;
        let storage = bindings::storage(&mut field, source, &model)
            .ok_or_else(|| format!("{} 不支持保存在此配置文件中", field.ui.label))?;
        if let Some(value) = value {
            field.validate(value)?;
        }
        storage.write(&mut root, &model, value.as_deref(), &context.draft)?;
        context.overlay(source, &root);
        if id == "modelMappings" {
            model_mappings::validate_snapshot(&context.draft)?;
        }
    }
    let mut fields = bound_fields(&context.draft, &model, &context);
    for field in &fields {
        if changes.get(&field.ui.id).is_some_and(Option::is_some) {
            if let Some(group) = &field.ui.exclusive_group {
                if fields.iter().any(|other| {
                    other.ui.id != field.ui.id
                        && other.ui.exclusive_group.as_ref() == Some(group)
                        && changes.get(&other.ui.id).is_some_and(Option::is_some)
                }) {
                    return Err("推理级别与思考 token 预算只能设置一项，请移除其中一项设置".into());
                }
            }
        }
    }
    for (id, value) in changes
        .iter()
        .filter(|(id, _)| id.as_str() != "model" && !dependency_ids.contains(&id.as_str()))
    {
        let field = fields
            .iter_mut()
            .find(|field| field.ui.id == *id)
            .ok_or("当前 Agent 不支持此可视化配置项，请重新读取")?;
        let storage = bindings::storage(field, source, &model)
            .ok_or_else(|| format!("{} 不支持保存在此配置文件中", field.ui.label))?;
        if let Some(value) = value {
            field.validate(value)?;
        }
        storage.write(&mut root, &model, value.as_deref(), &context.draft)?;
    }
    // Preserve untouched options, including keys this version of the form does not know.
    let rendered = document::rewrite(text, &root, source.spec.format)?;
    let form = render_read(source, &rendered, &root, context)?;
    let parsed_value = |id: &str| {
        form.values.get(id).or_else(|| {
            form.fields
                .iter()
                .find(|field| field.id == id)
                .and_then(|field| field.current_value.as_ref())
        })
    };
    if let (Some(window), Some(compact)) =
        (parsed_value("contextWindow"), parsed_value("autoCompact"))
    {
        if (changes.contains_key("contextWindow") || changes.contains_key("autoCompact"))
            && compact
                .parse::<i64>()
                .ok()
                .zip(window.parse::<i64>().ok())
                .is_some_and(|(compact, window)| compact > window)
        {
            return Err("自动压缩阈值不能大于上下文窗口".into());
        }
    }
    Ok(form)
}
