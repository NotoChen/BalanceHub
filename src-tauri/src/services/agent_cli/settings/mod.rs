//! Native option descriptions and bindings share one source of truth.
mod apply;
mod display;
mod help;
mod model_catalog;
mod model_mappings;
mod models;
mod persistence;
mod reset;
mod sources;

pub(crate) use help::read_help;
pub(crate) use model_catalog::read as read_model_catalog;
pub(crate) use persistence::{render_form, supports_form};
mod claude;
mod codex;
mod gemini;
mod grok;

use crate::models::*;
use serde_json::{json, Value};
use sources::{NativeSnapshot, Observed};

pub(crate) use apply::{apply, json_config};

#[derive(Clone)]
enum Binding {
    Codex(&'static str, ValueType),
    CodexSubagents,
    CodexSubagentLimit,
    CodexApproval,
    CodexGranular,
    Flag(&'static str),
    Switch(&'static str),
    FlagList(&'static str),
    FlagRules(&'static str),
    Json(&'static str, ValueType),
    JsonAppend(&'static str),
    Env(&'static str),
    GeminiModel(&'static str),
    ModelMappings,
    Readonly,
}

#[derive(Clone, Copy)]
enum ValueType {
    Text,
    Integer,
    Boolean,
    Percent,
    List,
}

pub(crate) struct BoundSetting {
    pub(crate) ui: AgentNativeSetting,
    binding: Binding,
}

pub(crate) struct ResolvedOptions {
    pub(crate) fields: Vec<BoundSetting>,
    pub(crate) snapshot: NativeSnapshot,
    pub(crate) model: String,
    pub(crate) model_display: AgentSettingValueSummary,
    pub(crate) default_model: AgentSettingValueSummary,
    pub(crate) model_native_key: String,
    pub(crate) model_choices: Vec<AgentSettingChoice>,
    pub(crate) model_catalog: AgentModelCatalogContext,
}

pub(crate) fn resolve(
    request: &TemporaryCliSettingsRequest,
    help: &str,
) -> Result<ResolvedOptions, String> {
    let mut snapshot = sources::read(request)?;
    codex::load_features(&mut snapshot, &request.cli_path);
    let original_snapshot = snapshot.clone();
    model_mappings::overlay(&mut snapshot, &request.native_options)?;
    let model_native_key = match request.cli_kind {
        AgentCliKind::Codex => "--model / model",
        AgentCliKind::ClaudeCode => "--model / ANTHROPIC_MODEL / model",
        AgentCliKind::Gemini => "--model / model.name",
        AgentCliKind::Grok => "--model / models.default",
    };
    let default_model = if request.session_mode == TemporaryCliSessionMode::History {
        display::history_model(&request.inherited_model)
    } else {
        display::default_model(&snapshot)
    };
    let model_display = if !request.model.trim().is_empty() {
        display::selected_model(&request.model)
    } else {
        default_model.clone()
    };
    let model = model_display.value.clone().unwrap_or_default();
    let mut fields = fields_for(
        &snapshot,
        &model,
        help,
        &request.cli_path,
        false,
        Some(&request.native_options),
    );
    display::annotate(&mut fields, &snapshot);
    reset::launch_values(&original_snapshot, request, &model, help, &mut fields);
    let model_choices = models::choices(&snapshot);
    let model_catalog = models::context(&snapshot, &request.cli_path);
    Ok(ResolvedOptions {
        fields,
        snapshot,
        model,
        model_display,
        default_model,
        model_native_key: model_native_key.into(),
        model_choices,
        model_catalog,
    })
}

/// Previewing an unchanged launch needs only model configuration, not CLI help,
/// the remote model catalog, or an asset scan.
pub(crate) fn read_model_selection(
    request: &TemporaryCliSettingsRequest,
) -> AgentSettingValueSummary {
    if !request.model.trim().is_empty() {
        return display::selected_model(&request.model);
    }
    if request.session_mode == TemporaryCliSessionMode::History {
        return display::history_model(&request.inherited_model);
    }
    match sources::read(request) {
        Ok(snapshot) => display::default_model(&snapshot),
        Err(error) => AgentSettingValueSummary {
            value: None,
            label: "模型配置读取失败".into(),
            source: String::new(),
            state: AgentSettingValueState::ReadError,
            detail: error,
        },
    }
}

fn model_pointer(kind: AgentCliKind) -> &'static str {
    match kind {
        AgentCliKind::Codex | AgentCliKind::ClaudeCode => "/model",
        AgentCliKind::Gemini => "/model/name",
        AgentCliKind::Grok => "/models/default",
    }
}

fn model_observed(snapshot: &NativeSnapshot) -> Option<Observed> {
    let pointer = model_pointer(snapshot.kind);
    match snapshot.kind {
        AgentCliKind::ClaudeCode => snapshot
            .env("ANTHROPIC_MODEL", Some("/env/ANTHROPIC_MODEL"))
            .or_else(|| snapshot.get(pointer)),
        AgentCliKind::Grok => snapshot
            .env("GROK_DEFAULT_MODEL", None)
            .or_else(|| snapshot.get(pointer)),
        AgentCliKind::Gemini => snapshot
            .env("GEMINI_MODEL", None)
            .or_else(|| snapshot.get(pointer)),
        AgentCliKind::Codex => snapshot.get(pointer),
    }
}

impl BoundSetting {
    fn new(id: &str, section: &str, label: &str, key: &str, binding: Binding) -> Self {
        Self {
            ui: AgentNativeSetting {
                id: id.into(),
                section: section.into(),
                label: label.into(),
                native_key: key.into(),
                source_references: Vec::new(),
                description: String::new(),
                hint: String::new(),
                control: AgentSettingControl::Select,
                current_value: None,
                current_native_value: None,
                current_label: "未设置".into(),
                current_state: AgentSettingValueState::Unset,
                current_detail: String::new(),
                reset_value: None,
                source: String::new(),
                choices: Vec::new(),
                model_mappings: None,
                allow_custom: false,
                minimum: None,
                maximum: None,
                slider_maximum: None,
                percentage_base: None,
                minimum_items: 0,
                unit: String::new(),
                locked_reason: String::new(),
                when_option: None,
                exclusive_group: None,
            },
            binding,
        }
    }
    fn describe(mut self, description: &str) -> Self {
        self.ui.description = description.into();
        self
    }
    fn hint(mut self, hint: &str) -> Self {
        self.ui.hint = hint.into();
        self
    }
    /// Known native defaults are display values, never implicit file overrides.
    fn default(mut self, value: Value, source: &str) -> Self {
        if self.ui.current_value.is_none() {
            self = self.current(
                Some(Observed {
                    value,
                    source: source.into(),
                    managed: false,
                    state: AgentSettingValueState::Default,
                }),
                "",
            );
            self.ui.current_native_value = None;
        }
        self
    }
    fn percentage_base(mut self, base: Option<i64>) -> Self {
        self.ui.percentage_base = base.filter(|value| *value > 0);
        self
    }
    fn choices(mut self, choices: &[(&str, &str, &str)]) -> Self {
        self.ui.choices = choices
            .iter()
            .map(|(value, label, description)| choice(value, label, description))
            .collect();
        self
    }
    fn models(mut self, choices: Vec<AgentSettingChoice>) -> Self {
        self.ui.control = AgentSettingControl::Model;
        self.ui.choices = choices;
        self.ui.allow_custom = true;
        self
    }
    fn current(mut self, observed: Option<Observed>, fallback: &str) -> Self {
        if let Some(observed) = observed {
            self.ui.current_state = observed.state;
            self.ui.current_detail.clear();
            let text = observed.text();
            self.ui.current_native_value = Some(match &observed.value {
                Value::String(value) => value.clone(),
                _ => serde_json::to_string_pretty(&observed.value).unwrap_or_else(|_| text.clone()),
            });
            self.ui.current_label = self
                .ui
                .choices
                .iter()
                .find(|choice| choice.value == text)
                .map(|choice| choice.label.clone())
                .unwrap_or_else(|| {
                    if let Value::Array(values) = &observed.value {
                        format!("已配置 {} 项", values.len())
                    } else {
                        text.clone()
                    }
                });
            self.ui.current_value = Some(text);
            self.ui.source = observed.source;
        } else {
            self.ui.current_label = fallback.into();
            self.ui.current_detail = fallback.into();
        }
        self
    }
    fn unset_detail(mut self, label: &str, detail: &str) -> Self {
        if self.ui.current_value.is_none() {
            self.ui.current_state = AgentSettingValueState::Unset;
            self.ui.current_label = label.into();
            self.ui.current_detail = detail.into();
        }
        self
    }
    fn unset(mut self, label: &str) -> Self {
        if self.ui.current_value.is_none() {
            self.ui.current_state = AgentSettingValueState::Unset;
            self.ui.current_label = label.into();
            self.ui.current_detail = "已读取的配置中没有设置此项。".into();
        }
        self
    }
    fn custom(mut self) -> Self {
        self.ui.allow_custom = true;
        self
    }
    fn number(mut self, minimum: i64, maximum: Option<i64>, unit: &str) -> Self {
        self.ui.control = AgentSettingControl::Number;
        self.ui.minimum = Some(minimum);
        self.ui.maximum = maximum;
        self.ui.unit = unit.into();
        self
    }
    fn slider_maximum(mut self, maximum: Option<i64>) -> Self {
        self.ui.slider_maximum = maximum;
        self
    }
    fn list(mut self) -> Self {
        self.ui.control = AgentSettingControl::MultiSelect;
        self
    }
    fn nonempty(mut self) -> Self {
        self.ui.minimum_items = 1;
        self
    }
    fn rules(mut self) -> Self {
        self.ui.control = AgentSettingControl::Rules;
        self
    }
    fn readonly(mut self) -> Self {
        self.ui.control = AgentSettingControl::Readonly;
        self
    }
    fn locked(mut self, reason: &str) -> Self {
        self.ui.locked_reason = reason.into();
        self
    }
    fn when(mut self, id: &str, value: &str) -> Self {
        self.ui.when_option = Some((id.into(), value.into()));
        self
    }
    fn exclusive(mut self, group: &str) -> Self {
        self.ui.exclusive_group = Some(group.into());
        self
    }
    fn restrict(mut self, values: Option<&Value>) -> Self {
        if let Some(values) = values.and_then(Value::as_array) {
            self.ui.choices.retain(|choice| {
                values
                    .iter()
                    .any(|value| value.as_str() == Some(&choice.value))
            });
            if self.ui.choices.is_empty() {
                self.ui.locked_reason = "系统或组织策略未允许更改此项".into();
            }
        }
        self
    }

    fn validate(&self, value: &str) -> Result<(), String> {
        let invalid = |reason: &str| format!("{}：{reason}", self.ui.label);
        if !self.ui.locked_reason.is_empty() {
            return Err(invalid(&self.ui.locked_reason));
        }
        if value.len() > 64 * 1024 || value.contains('\0') {
            return Err(invalid("值过长或包含空字符"));
        }
        match self.ui.control {
            AgentSettingControl::ModelMappings => {
                model_mappings::validate(value, self.ui.model_mappings.as_ref())?;
            }
            AgentSettingControl::Readonly => {
                return Err(invalid("当前 CLI 不支持在本次启动时覆盖"))
            }
            AgentSettingControl::Number => {
                let number: i64 = value.parse().map_err(|_| invalid("请输入整数"))?;
                if self.ui.minimum.is_some_and(|min| number < min)
                    || self.ui.maximum.is_some_and(|max| number > max)
                {
                    return Err(invalid("数值超出当前原生选项支持的范围"));
                }
            }
            AgentSettingControl::Model | AgentSettingControl::Select => {
                if value.trim().is_empty()
                    || value.chars().any(char::is_control)
                    || (self.ui.control == AgentSettingControl::Model && value.len() > 512)
                    || (!self.ui.allow_custom
                        && !self.ui.choices.iter().any(|choice| choice.value == value))
                {
                    return Err(invalid("当前 Agent 或模型不支持该值，请重新选择"));
                }
            }
            AgentSettingControl::MultiSelect | AgentSettingControl::Rules => {
                let entries = entries(value);
                if entries.len() > 256 {
                    return Err(invalid("最多可设置 256 项"));
                }
                if entries.len() < self.ui.minimum_items {
                    return Err(invalid("请至少选择一项，或取消此项覆盖"));
                }
                if !self.ui.allow_custom
                    && self.ui.control == AgentSettingControl::MultiSelect
                    && entries
                        .iter()
                        .any(|item| !self.ui.choices.iter().any(|choice| choice.value == *item))
                {
                    return Err(invalid("包含当前原生设置未提供的选项"));
                }
                if self.ui.control == AgentSettingControl::MultiSelect
                    && entries.iter().any(|item| {
                        !item
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "_-.::*".contains(c))
                    })
                {
                    return Err(invalid(
                        "请输入原生工具名称，每项只包含字母、数字、下划线、连字符或通配符",
                    ));
                }
            }
        }
        Ok(())
    }
}

fn choice(value: &str, label: &str, description: &str) -> AgentSettingChoice {
    AgentSettingChoice {
        value: value.into(),
        label: label.into(),
        description: description.into(),
    }
}

fn boolean() -> [(&'static str, &'static str, &'static str); 2] {
    [("true", "启用", ""), ("false", "停用", "")]
}

fn help_option(help: &str, flag: &str) -> String {
    let mut lines = help.lines().skip_while(|line| {
        !line
            .split_whitespace()
            .any(|part| part.trim_end_matches(',') == flag)
    });
    let Some(first) = lines.next() else {
        return String::new();
    };
    std::iter::once(first)
        .chain(lines.take_while(|line| !line.trim_start().starts_with('-')))
        .collect::<Vec<_>>()
        .join("\n")
}

fn entries(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    for value in value
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !result.iter().any(|item| item == value) {
            result.push(value.into());
        }
    }
    result
}

fn typed(value: &str, kind: ValueType) -> Result<Value, String> {
    Ok(match kind {
        ValueType::Text => json!(value),
        ValueType::Integer => json!(value.parse::<i64>().map_err(|_| "原生选项需要整数")?),
        ValueType::Boolean => json!(value
            .parse::<bool>()
            .map_err(|_| "原生选项需要 true 或 false")?),
        ValueType::Percent => {
            json!(value.parse::<f64>().map_err(|_| "原生选项需要百分比")? / 100.0)
        }
        ValueType::List => json!(entries(value)),
    })
}

fn native_model_pointer(model: &str) -> String {
    model.replace('~', "~0").replace('/', "~1")
}

pub(crate) fn validate_options(
    settings: &TemporaryCliRunSettings,
    resolved: &ResolvedOptions,
) -> Result<(), String> {
    if settings.native_options.len() > 64 {
        return Err("本次原生启动选项过多".into());
    }
    for (id, value) in &settings.native_options {
        let field = resolved
            .fields
            .iter()
            .find(|field| &field.ui.id == id)
            .ok_or_else(|| format!("当前 Agent 版本不再提供启动选项 {id}，请调整方案"))?;
        field.validate(value)?;
        if let Some(group) = &field.ui.exclusive_group {
            if let Some(other) = resolved.fields.iter().find(|other| {
                other.ui.id != *id
                    && other.ui.exclusive_group.as_ref() == Some(group)
                    && settings.native_options.contains_key(&other.ui.id)
            }) {
                return Err(format!(
                    "{} 与 {} 只能覆盖一项，请取消其中一项覆盖",
                    field.ui.label, other.ui.label
                ));
            }
        }
        if let Some((parent, expected)) = &field.ui.when_option {
            let actual = settings.native_options.get(parent).or_else(|| {
                resolved
                    .fields
                    .iter()
                    .find(|field| &field.ui.id == parent)
                    .and_then(|field| field.ui.current_value.as_ref())
            });
            if actual != Some(expected) {
                return Err(format!("{} 的关联选项已改变，请重新设置", field.ui.label));
            }
        }
    }
    if let (Some(window), Some(compact)) = (
        settings.native_options.get("contextWindow"),
        settings.native_options.get("autoCompact"),
    ) {
        if compact.parse::<i64>().unwrap_or_default() > window.parse::<i64>().unwrap_or(i64::MAX) {
            return Err("自动压缩阈值不能大于本次设置的上下文窗口".into());
        }
    }
    Ok(())
}

fn fields_for(
    snapshot: &NativeSnapshot,
    model: &str,
    help: &str,
    cli_path: &str,
    persistent: bool,
    native_options: Option<&std::collections::BTreeMap<String, String>>,
) -> Vec<BoundSetting> {
    let mut fields = match snapshot.kind {
        AgentCliKind::Codex => {
            codex::fields(snapshot, model, help, cli_path, persistent, native_options)
        }
        AgentCliKind::ClaudeCode => claude::fields(snapshot, model, help, persistent),
        AgentCliKind::Gemini => gemini::fields(snapshot, model, help, persistent),
        AgentCliKind::Grok => grok::fields(snapshot, model, help, persistent),
    };
    if let Some(field) = model_mappings::field(snapshot, model, persistent) {
        fields.insert(0, field);
    }
    fields
}
